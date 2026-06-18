import {
  BrowserWindow,
  Menu,
  Notification,
  Tray,
  app,
  dialog,
  globalShortcut,
  ipcMain,
  nativeImage,
  net,
  protocol,
  safeStorage,
  screen,
  session,
  shell,
} from "electron";
import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import http, { type Server } from "node:http";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
// Type-only: erased at runtime. The implementation is lazy-required in
// getAutoUpdater() so a missing/mis-packaged electron-updater degrades to the
// GitHub-release fallback instead of crashing the main process at load time.
import type { AppUpdater } from "electron-updater";

const apiBaseUrl = "http://127.0.0.1:7777";
const appScheme = "app";
const appHost = "cerul";
const deepLinkSchemes = ["cerul", "cerul-app"];
const defaultHotkey = "Alt+Space";
const cloudAccountOrigin = "https://accounts.cerul.ai";
const defaultUpdateRepository = "cerul-ai/cerul-app";
const macBundleIdentifier = "ai.cerul.desktop";
const packagedCoreBinaryName = "cerul-core";
const devCoreBinaryName = "cerul-api";
const packagedMlxRuntimeArchiveName = "mlx-runtime.tar.gz";
const packagedMlxRuntimeManifestName = "mlx-runtime-manifest.json";
const packagedMlxRuntimeReadyMarker = ".cerul-mlx-runtime-ready.json";
const apiStartupTimeoutMs = positiveIntegerEnv("CERUL_API_STARTUP_TIMEOUT_MS", 90_000);
const apiOutputTailBytes = 32 * 1024;
const contentSecurityPolicy = [
  "default-src 'self'",
  "script-src 'self'",
  "style-src 'self' 'unsafe-inline'",
  "font-src 'self' data:",
  "img-src 'self' app: http://127.0.0.1:7777 data: blob:",
  "media-src 'self' http://127.0.0.1:7777 blob:",
  `connect-src 'self' http://127.0.0.1:7777 ${cloudAccountOrigin}`,
  "object-src 'none'",
  "base-uri 'self'",
  "form-action 'none'",
  "frame-ancestors 'none'",
].join("; ");

let mainWindow: BrowserWindow | null = null;
let overlayWindow: BrowserWindow | null = null;
let menuBarWindow: BrowserWindow | null = null;
let tray: Tray | null = null;
let apiProcess: ChildProcessWithoutNullStreams | null = null;
let ownsApiProcess = false;
let isQuitting = false;
let apiRestartAttempts = 0;
let lastApiExit: ApiExitInfo | null = null;
let mainWindowLoaded = false;
let pendingDeepLink = firstDeepLinkArg(process.argv);
let queuedMainRoute: string | null = null;
let registeredGlobalHotkey: string | null = null;
let statusMonitor: NodeJS.Timeout | null = null;
let hadActiveIndexing = false;
let lastFailedJobCount: number | null = null;
const loginItemCliCommand = firstLoginItemCliCommand(process.argv);
const stores = new Map<string, Record<string, unknown>>();
const dirtyStores = new Set<string>();
const secureTokenStorePath = "secure-tokens.json";
let oauthCallbackServer: Server | null = null;
let oauthCallbackPort: number | null = null;
let autoUpdaterInstance: AppUpdater | null = null;
let autoUpdaterWired = false;
let updateInstallRequested = false;
let updateInstallFallbackTimer: NodeJS.Timeout | null = null;
let updateInstallForceExitTimer: NodeJS.Timeout | null = null;
let latestUpdaterState: UpdaterState = { phase: "idle" };

type OAuthProvider = "google" | "github";

type GitHubRelease = {
  tag_name?: string;
  name?: string | null;
  html_url?: string;
  draft?: boolean;
  prerelease?: boolean;
  published_at?: string | null;
};

type DesktopUpdateInfo = {
  version: string;
  url: string;
  name?: string;
  prerelease: boolean;
  publishedAt?: string;
};

// Drives the rail "Update" pill. `available` always works (GitHub-release
// detection, signing-independent); later phases only occur once releases ship
// signed + a latest-mac.yml that electron-updater can apply.
type UpdaterState =
  | { phase: "idle" }
  | { phase: "available"; version: string; releaseUrl: string; canAutoInstall: boolean }
  | {
      phase: "downloading";
      version: string;
      percent: number;
      bytesPerSecond?: number;
      etaSeconds?: number;
      transferredBytes?: number;
      totalBytes?: number;
    }
  | { phase: "installing"; version: string }
  | { phase: "downloaded"; version: string }
  | { phase: "error"; version?: string; message: string; releaseUrl: string };

type UpdaterProgress = {
  percent?: number;
  bytesPerSecond?: number;
  transferred?: number;
  total?: number;
};

type ApiOutputTail = {
  stdout: string;
  stderr: string;
};

type ApiExitInfo = {
  pid: number | undefined;
  code: number | null;
  signal: string | null;
  elapsedMs: number;
};

type BundleProcessHolder = {
  pid: number;
  command: string;
  paths: string[];
};

protocol.registerSchemesAsPrivileged([
  {
    scheme: appScheme,
    privileges: {
      standard: true,
      secure: true,
      supportFetchAPI: true,
      corsEnabled: true,
      stream: true,
    },
  },
]);

if (!loginItemCliCommand) {
  const gotSingleInstanceLock = app.requestSingleInstanceLock();
  if (!gotSingleInstanceLock) {
    app.quit();
  }

  app.on("second-instance", (_event, argv) => {
    focusMainWindow();
    routeDeepLink(firstDeepLinkArg(argv));
  });
}

registerDeepLinkProtocols();

app.on("open-url", (event, url) => {
  event.preventDefault();
  if (app.isReady()) {
    routeDeepLink(url);
  } else {
    pendingDeepLink = url;
  }
});

app
  .whenReady()
  .then(async () => {
    if (loginItemCliCommand) {
      runLoginItemCliCommand(loginItemCliCommand);
      app.exit(0);
      return;
    }
    setDockIcon();
    registerAppProtocol();
    // The app:// renderer is content-hashed, but a stale index.html cached in
    // the userData partition would keep pointing at old asset hashes across
    // restarts (a rebuild then appears to "not take effect"). Clear the HTTP
    // cache on launch — cheap for a local file-backed app.
    await session.defaultSession.clearCache();
    // Electron grants permission requests (camera, mic, geolocation, ...) by
    // default; deny everything except the two benign permissions the app
    // genuinely uses — clipboard *write* (copy citation / timestamp / Markdown)
    // and player fullscreen. clipboard-read stays denied (reading the clipboard
    // is the sensitive direction). A blanket deny here previously broke every
    // copy-to-clipboard action, since navigator.clipboard.writeText needs the
    // clipboard-sanitized-write permission.
    const allowedPermissions = new Set(["clipboard-sanitized-write", "fullscreen"]);
    session.defaultSession.setPermissionRequestHandler((_webContents, permission, callback) => {
      callback(allowedPermissions.has(permission));
    });
    session.defaultSession.setPermissionCheckHandler((_webContents, permission) =>
      allowedPermissions.has(permission),
    );
    registerIpcHandlers();
    await startRustCore();
    createMainWindow();
    createOverlayWindow();
    setupTray();
    startStatusMonitor();
    registerGlobalHotkey(await initialGlobalHotkey(), { throwOnFailure: false });
    routeDeepLink(pendingDeepLink);
    pendingDeepLink = undefined;
  })
  .catch((error) => {
    console.error("Failed to start Cerul Electron shell", error);
    // A packaged app has no visible stderr; without a dialog a startup
    // failure looks like the icon bouncing once and vanishing.
    dialog.showErrorBox(
      "Cerul failed to start",
      error instanceof Error ? `${error.message}\n\n${error.stack ?? ""}` : String(error),
    );
    app.quit();
  });

app.on("before-quit", () => {
  isQuitting = true;
});

let coreShutdownComplete = false;

app.on("will-quit", (event) => {
  globalShortcut.unregisterAll();
  stopOAuthCallbackServer();
  stopStatusMonitor();
  flushDirtyStores();
  if (!coreShutdownComplete && apiProcess && ownsApiProcess) {
    // Wait for the backend (which in turn owns qdrant) to exit, escalating
    // to SIGKILL after a grace period — fire-and-forget SIGTERM used to
    // leave orphans whenever the process needed longer than the app.
    event.preventDefault();
    void stopRustCoreGracefully().finally(() => {
      coreShutdownComplete = true;
      app.quit();
    });
  }
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") {
    app.quit();
  }
});

app.on("activate", () => {
  if (!mainWindow) {
    createMainWindow();
  } else {
    focusMainWindow();
  }
});

function repoRoot() {
  return app.isPackaged ? process.resourcesPath : path.resolve(__dirname, "../../..");
}

function desktopDistDir() {
  return app.isPackaged
    ? path.join(process.resourcesPath, "desktop-dist")
    : path.join(repoRoot(), "apps", "desktop", "dist");
}

function preloadPath() {
  return path.join(__dirname, "preload.js");
}

function desktopBrandResourcePath(relativePath: string) {
  const brandRoot = app.isPackaged
    ? path.join(process.resourcesPath, "desktop-dist")
    : path.join(repoRoot(), "apps", "desktop", "public");
  return path.join(
    brandRoot,
    relativePath,
  );
}

function desktopAppIconPath() {
  return desktopBrandResourcePath("brand/app-store-icon-1024.png");
}

// macOS ignores BrowserWindow.icon for the Dock; in dev (running the Electron
// binary directly) that leaves the default Electron icon. Set it explicitly so
// the Dock shows the Cerul mark. Packaged builds use the bundled .icns.
function setDockIcon() {
  if (process.platform !== "darwin" || !app.dock) {
    return;
  }
  // Use the margin-padded macOS icon (824/1024 grid) so the Dock icon matches
  // the visual size of other apps, instead of the full-bleed app-store image.
  const image = nativeImage.createFromPath(
    desktopBrandResourcePath("brand/cerul-icon-mac-1024.png"),
  );
  if (!image.isEmpty()) {
    app.dock.setIcon(image);
  }
}

function trayIconPath() {
  return desktopBrandResourcePath(
    process.platform === "darwin" ? "brand/cerul-menubarTemplate.png" : "brand/icon-192.png",
  );
}

function registerAppProtocol() {
  protocol.handle(appScheme, async (request) => {
    const url = new URL(request.url);
    if (url.hostname !== appHost) {
      return new Response("unknown app host", { status: 404 });
    }

    const dist = path.resolve(desktopDistDir());
    const pathname = decodeURIComponent(url.pathname === "/" ? "/index.html" : url.pathname);
    const filePath = path.resolve(dist, pathname.replace(/^\/+/, ""));
    if (!isPathInsideDirectory(filePath, dist)) {
      return new Response("invalid app path", { status: 403 });
    }
    if (!fs.existsSync(filePath) || fs.statSync(filePath).isDirectory()) {
      return new Response("not found", { status: 404 });
    }

    const response = await net.fetch(pathToFileURL(filePath).toString());
    return withAppSecurityHeaders(response, filePath);
  });
}

function withAppSecurityHeaders(response: Response, filePath: string) {
  if (!filePath.endsWith(".html")) {
    return response;
  }
  const headers = new Headers(response.headers);
  headers.set("Content-Security-Policy", contentSecurityPolicy);
  // Never cache index.html so it always references the current (content-hashed)
  // assets after a rebuild; the hashed assets themselves remain cacheable.
  headers.set("Cache-Control", "no-store");
  return new Response(response.body, {
    status: response.status,
    statusText: response.statusText,
    headers,
  });
}

const WINDOW_STATE_STORE = "window-state";

function savedMainWindowBounds(): Partial<Electron.Rectangle> {
  const stored = loadStore(WINDOW_STATE_STORE)["mainBounds"];
  if (!stored || typeof stored !== "object") {
    return {};
  }
  const bounds = stored as Partial<Electron.Rectangle>;
  if (
    typeof bounds.width !== "number" ||
    typeof bounds.height !== "number" ||
    bounds.width < 600 ||
    bounds.height < 400
  ) {
    return {};
  }
  // Only restore a position that is still on a connected display.
  if (typeof bounds.x === "number" && typeof bounds.y === "number") {
    const visible = screen.getAllDisplays().some((display) => {
      const area = display.workArea;
      return (
        bounds.x! >= area.x - 50 &&
        bounds.y! >= area.y - 50 &&
        bounds.x! < area.x + area.width &&
        bounds.y! < area.y + area.height
      );
    });
    if (!visible) {
      return { width: bounds.width, height: bounds.height };
    }
  }
  return bounds;
}

function persistMainWindowBounds() {
  if (!mainWindow || mainWindow.isDestroyed() || mainWindow.isMinimized()) {
    return;
  }
  loadStore(WINDOW_STATE_STORE)["mainBounds"] = mainWindow.getNormalBounds();
  dirtyStores.add(WINDOW_STATE_STORE);
  saveStore(WINDOW_STATE_STORE);
}

function createMainWindow() {
  const saved = savedMainWindowBounds();
  mainWindow = new BrowserWindow({
    width: saved.width ?? 1440,
    height: saved.height ?? 920,
    x: saved.x,
    y: saved.y,
    minWidth: 1080,
    minHeight: 720,
    title: "Cerul",
    ...(process.platform === "darwin"
      ? { titleBarStyle: "hiddenInset" as const, trafficLightPosition: { x: 19, y: 13 } }
      : {}),
    icon: desktopAppIconPath(),
    show: false,
    webPreferences: {
      preload: preloadPath(),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });

  secureDesktopWindow(mainWindow);
  mainWindow.on("close", () => persistMainWindowBounds());
  mainWindow.on("hide", () => persistMainWindowBounds());
  mainWindow.on("close", (event) => {
    if (isQuitting) {
      return;
    }
    event.preventDefault();
    void shouldCloseToTray().then((enabled) => {
      if (enabled) {
        mainWindow?.hide();
        return;
      }
      quitFromMainWindowClose();
    });
  });
  mainWindow.once("ready-to-show", () => {
    if (shouldShowMainWindowAtLaunch()) {
      mainWindow?.show();
      mainWindow?.focus();
    }
  });
  mainWindow.webContents.once("did-finish-load", () => {
    console.log("cerul_electron_main_window_loaded");
    mainWindowLoaded = true;
    flushQueuedMainRoute();
    maybeRunRendererVideoSmoke();
  });
  mainWindow.webContents.on("did-fail-load", (_event, code, description, url) => {
    console.error(`Cerul main window failed to load code=${code} url=${url}: ${description}`);
  });
  mainWindow.webContents.on("render-process-gone", (_event, details) => {
    console.error(`Cerul main window renderer exited reason=${details.reason}`);
  });
  mainWindow.on("closed", () => {
    mainWindow = null;
    mainWindowLoaded = false;
  });
  void mainWindow.loadURL(`${appScheme}://${appHost}/index.html`);
}

function createOverlayWindow() {
  const isMac = process.platform === "darwin";
  overlayWindow = new BrowserWindow({
    width: OVERLAY_WIDTH,
    height: overlayMeasuredHeight,
    title: "",
    icon: desktopAppIconPath(),
    show: false,
    frame: false,
    transparent: true,
    alwaysOnTop: true,
    skipTaskbar: true,
    resizable: false,
    hasShadow: true,
    roundedCorners: true,
    // Real frosted glass on macOS: the OS compositor blurs whatever is behind
    // the overlay window. (CSS backdrop-filter can't blur across OS windows, so
    // a translucent panel alone just lets the page behind bleed through.)
    vibrancy: isMac ? "under-window" : undefined,
    visualEffectState: "active",
    webPreferences: {
      preload: preloadPath(),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });

  secureDesktopWindow(overlayWindow);
  overlayWindow.on("closed", () => {
    overlayWindow = null;
  });
  overlayWindow.webContents.once("did-finish-load", () => {
    console.log("cerul_electron_overlay_window_loaded");
  });
  overlayWindow.webContents.on("did-fail-load", (_event, code, description, url) => {
    console.error(`Cerul overlay window failed to load code=${code} url=${url}: ${description}`);
  });
  void overlayWindow.loadURL(`${appScheme}://${appHost}/overlay.html`);
}

function createMenuBarWindow() {
  if (menuBarWindow) {
    return menuBarWindow;
  }
  const isMac = process.platform === "darwin";
  menuBarWindow = new BrowserWindow({
    width: 332,
    height: 312,
    title: "Cerul",
    icon: desktopAppIconPath(),
    show: false,
    frame: false,
    transparent: true,
    alwaysOnTop: true,
    skipTaskbar: true,
    resizable: false,
    movable: true,
    hasShadow: true,
    roundedCorners: true,
    vibrancy: isMac ? "popover" : undefined,
    visualEffectState: "active",
    webPreferences: {
      preload: preloadPath(),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });

  secureDesktopWindow(menuBarWindow);
  menuBarWindow.on("blur", () => {
    menuBarWindow?.hide();
  });
  menuBarWindow.on("closed", () => {
    menuBarWindow = null;
  });
  menuBarWindow.webContents.once("did-finish-load", () => {
    console.log("cerul_electron_menubar_window_loaded");
  });
  menuBarWindow.webContents.on("did-fail-load", (_event, code, description, url) => {
    console.error(`Cerul menu bar window failed to load code=${code} url=${url}: ${description}`);
  });
  void menuBarWindow.loadURL(`${appScheme}://${appHost}/menubar.html`);
  return menuBarWindow;
}

function secureDesktopWindow(window: BrowserWindow) {
  window.webContents.setWindowOpenHandler(({ url }) => {
    if (isExternalUrl(url)) {
      void shell.openExternal(url);
    }
    return { action: "deny" };
  });
  window.webContents.on("will-navigate", (event, url) => {
    if (isAppUrl(url)) {
      return;
    }
    event.preventDefault();
    if (isExternalUrl(url)) {
      void shell.openExternal(url);
    }
  });
}

function setupTray() {
  const iconPath = trayIconPath();
  const image = nativeImage.createFromPath(iconPath);
  const trayImage = image.isEmpty() ? nativeImage.createEmpty() : image.resize({ width: 18, height: 18 });
  if (!trayImage.isEmpty() && process.platform === "darwin") {
    trayImage.setTemplateImage(true);
  }
  tray = new Tray(trayImage);
  tray.setToolTip("Cerul");
  tray.on("click", () => toggleMenuBarWindow());
  tray.setContextMenu(
    Menu.buildFromTemplate([
      { label: "Mini Window", click: () => toggleMenuBarWindow({ forceShow: true }) },
      { label: "Open Cerul", click: () => focusMainWindow() },
      { label: "Search Overlay", click: () => showOverlay() },
      { type: "separator" },
      { label: "Quit", click: () => app.quit() },
    ]),
  );
}

function toggleMenuBarWindow(options: { forceShow?: boolean } = {}) {
  if (!tray) {
    return;
  }
  const window = createMenuBarWindow();
  if (!options.forceShow && window.isVisible()) {
    window.hide();
    return;
  }
  positionMenuBarWindow(window);
  window.show();
  window.focus();
}

function positionMenuBarWindow(window: BrowserWindow) {
  if (!tray) {
    return;
  }
  const trayBounds = tray.getBounds();
  const windowBounds = window.getBounds();
  const display = screen.getDisplayNearestPoint({
    x: Math.round(trayBounds.x + trayBounds.width / 2),
    y: Math.round(trayBounds.y + trayBounds.height / 2),
  });
  const workArea = display.workArea;
  const centeredX = Math.round(trayBounds.x + trayBounds.width / 2 - windowBounds.width / 2);
  const belowTray = Math.round(trayBounds.y + trayBounds.height + 8);
  const aboveTray = Math.round(trayBounds.y - windowBounds.height - 8);
  const x = Math.max(workArea.x + 8, Math.min(centeredX, workArea.x + workArea.width - windowBounds.width - 8));
  const y =
    belowTray + windowBounds.height <= workArea.y + workArea.height
      ? belowTray
      : Math.max(workArea.y + 8, aboveTray);
  window.setBounds({ x, y, width: windowBounds.width, height: windowBounds.height });
}

function startStatusMonitor() {
  if (statusMonitor) {
    return;
  }
  void refreshDesktopStatus();
  statusMonitor = setInterval(() => {
    void refreshDesktopStatus();
  }, 5_000);
}

function stopStatusMonitor() {
  if (!statusMonitor) {
    return;
  }
  clearInterval(statusMonitor);
  statusMonitor = null;
}

async function refreshDesktopStatus() {
  try {
    const [jobs, items] = await Promise.all([
      fetchApiJson("/jobs"),
      fetchApiJson("/items"),
    ]);
    if (!Array.isArray(jobs) || !Array.isArray(items)) {
      return;
    }

    const total = items.length;
    const indexed = items.filter((item) => recordStatus(item) === "indexed").length;
    const active = jobs.filter((job) => activeJobStatuses.has(recordStatus(job))).length;
    const failed = jobs.filter((job) => recordStatus(job) === "failed").length;

    if (active > 0) {
      hadActiveIndexing = true;
      tray?.setToolTip(`Cerul · indexing ${indexed}/${total}`);
    } else {
      tray?.setToolTip(`Cerul · ${indexed} indexed`);
      if (hadActiveIndexing) {
        hadActiveIndexing = false;
        if (total > 0 && indexed >= total) {
          showNotification("Indexing complete", `All ${indexed} indexed items are searchable.`);
        }
      }
    }

    if (lastFailedJobCount === null) {
      lastFailedJobCount = failed;
    } else if (failed > lastFailedJobCount) {
      const newlyFailed = failed - lastFailedJobCount;
      showNotification(`${newlyFailed} items failed`, "View details in jobs panel.");
      lastFailedJobCount = failed;
    } else {
      lastFailedJobCount = failed;
    }
  } catch {
    return;
  }
}

const activeJobStatuses = new Set(["queued", "running", "processing", "indexing"]);

async function fetchApiJson(pathname: string) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 1_500);
  try {
    const response = await fetch(`${apiBaseUrl}${pathname}`, { signal: controller.signal });
    if (!response.ok) {
      return null;
    }
    return await response.json();
  } finally {
    clearTimeout(timer);
  }
}

function recordStatus(record: unknown) {
  if (!record || typeof record !== "object") {
    return "";
  }
  const status = (record as { status?: unknown }).status;
  return typeof status === "string" ? status : "";
}

function registerGlobalHotkey(
  label: string,
  options: { throwOnFailure?: boolean } = { throwOnFailure: true },
) {
  const accelerator = normalizeAccelerator(label);
  if (registeredGlobalHotkey === accelerator && globalShortcut.isRegistered(accelerator)) {
    return true;
  }
  if (globalShortcut.register(accelerator, showOverlay)) {
    if (registeredGlobalHotkey && registeredGlobalHotkey !== accelerator) {
      globalShortcut.unregister(registeredGlobalHotkey);
    }
    registeredGlobalHotkey = accelerator;
    return true;
  }
  const message = `failed to register global shortcut: ${accelerator}`;
  if (options.throwOnFailure) {
    throw new Error(message);
  }
  console.warn(message);
  return false;
}

function normalizeAccelerator(label: string) {
  let accelerator = label.replace(/\s*\+\s*/g, "+").replace(/^Alt Space$/i, "Alt+Space");
  if (process.platform !== "darwin") {
    accelerator = accelerator.replace(/\b(Command|Cmd)\b/gi, "Super");
  }
  return accelerator;
}

function showOverlay() {
  if (!overlayWindow) {
    createOverlayWindow();
  }
  const display = mainWindow?.getBounds() ?? { x: 0, y: 0, width: 1440, height: 920 };
  // Open compact and top-anchored; the renderer measures the panel and grows the
  // window to fit it via "resize_overlay", so there's no transparent dead-zone
  // below the panel showing the app underneath.
  overlayWindow?.setBounds({
    x: Math.round(display.x + display.width / 2 - OVERLAY_WIDTH / 2),
    y: Math.round(display.y + display.height * 0.16),
    width: OVERLAY_WIDTH,
    height: overlayMeasuredHeight,
  });
  overlayWindow?.show();
  overlayWindow?.focus();
}

const OVERLAY_WIDTH = 560; // window hugs the panel; OS shadow provides the float
const OVERLAY_MIN_HEIGHT = 120;
const OVERLAY_INITIAL_HEIGHT = 200;
const OVERLAY_MAX_HEIGHT = 640;
let overlayMeasuredHeight = OVERLAY_INITIAL_HEIGHT;

function resizeOverlay(requestedHeight: number) {
  if (!overlayWindow || !Number.isFinite(requestedHeight)) {
    return;
  }
  const height = Math.max(OVERLAY_MIN_HEIGHT, Math.min(OVERLAY_MAX_HEIGHT, Math.round(requestedHeight)));
  overlayMeasuredHeight = height;
  const bounds = overlayWindow.getBounds();
  if (bounds.height === height) {
    return;
  }
  // Keep the top edge anchored — grow downward.
  overlayWindow.setBounds({ x: bounds.x, y: bounds.y, width: OVERLAY_WIDTH, height });
}

function focusMainWindow() {
  if (!mainWindow) {
    createMainWindow();
    return;
  }
  mainWindow.show();
  mainWindow.focus();
}

function shouldShowMainWindowAtLaunch() {
  return !isHiddenLaunch();
}

function isHiddenLaunch() {
  if (process.argv.includes("--hidden") || process.argv.includes("--background") || process.argv.includes("--daemon")) {
    return true;
  }
  try {
    return app.getLoginItemSettings({ args: loginItemArgs() }).wasOpenedAsHidden;
  } catch {
    return false;
  }
}

function quitFromMainWindowClose() {
  isQuitting = true;
  app.quit();
}

function routeDeepLink(url?: string) {
  if (!url) {
    return;
  }
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return;
  }
  const scheme = parsed.protocol.replace(/:$/, "");
  if (!deepLinkSchemes.includes(scheme)) {
    return;
  }
  if (parsed.hostname === "item") {
    const itemId = decodeURIComponent(parsed.pathname.replace(/^\//, ""));
    const timestamp = parsed.searchParams.get("t") ?? "";
    openMainRoute(
      `item-detail?itemId=${encodeURIComponent(itemId)}&t=${encodeURIComponent(timestamp)}`,
    );
  } else if (parsed.hostname === "settings") {
    const section = parsed.searchParams.get("section");
    openMainRoute(section ? `settings?section=${encodeURIComponent(section)}` : "settings");
  } else if (parsed.hostname === "auth" && parsed.pathname === "/callback") {
    if (!oauthFlowPending()) {
      return;
    }
    oauthFlowPendingUntil = 0;
    const params = new URLSearchParams({ section: "Usage" });
    for (const key of ["provider", "code", "state", "error"]) {
      const value = parsed.searchParams.get(key);
      if (value) {
        params.set(key, value);
      }
    }
    openMainRoute(`settings?${params.toString()}`);
  }
}

// Only accept OAuth callbacks while a sign-in the user actually started is
// in flight; the localhost listener and the cerul:// deep link are otherwise
// open to any local process / website forging a login-CSRF callback.
let oauthFlowPendingUntil = 0;
const OAUTH_FLOW_WINDOW_MS = 10 * 60 * 1000;

function oauthFlowPending() {
  return Date.now() <= oauthFlowPendingUntil;
}

async function startOAuthLogin(provider: OAuthProvider) {
  if (provider !== "google" && provider !== "github") {
    throw new Error("unsupported OAuth provider");
  }
  oauthFlowPendingUntil = Date.now() + OAUTH_FLOW_WINDOW_MS;
  const redirectUri = await ensureOAuthCallbackServer();
  const startUrl = new URL(`/v1/auth/oauth/${provider}/start`, cloudAccountOrigin);
  startUrl.searchParams.set("redirect_uri", redirectUri);
  await shell.openExternal(startUrl.toString());
}

async function ensureOAuthCallbackServer() {
  if (oauthCallbackServer && oauthCallbackPort) {
    return oauthCallbackRedirectUri(oauthCallbackPort);
  }
  const server = http.createServer((request, response) => {
    handleOAuthCallbackRequest(request.url ?? "/", response);
  });
  await new Promise<void>((resolve, reject) => {
    const onError = (error: Error) => {
      oauthCallbackServer = null;
      oauthCallbackPort = null;
      reject(error);
    };
    server.once("error", onError);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", onError);
      const address = server.address();
      if (!address || typeof address === "string") {
        server.close();
        reject(new Error("OAuth callback server did not bind a TCP port"));
        return;
      }
      oauthCallbackServer = server;
      oauthCallbackPort = address.port;
      resolve();
    });
  });
  return oauthCallbackRedirectUri(oauthCallbackPort!);
}

function handleOAuthCallbackRequest(rawUrl: string, response: http.ServerResponse) {
  let url: URL;
  try {
    url = new URL(rawUrl, "http://127.0.0.1");
  } catch {
    writeOAuthCallbackResponse(response, 400, "Invalid OAuth callback URL.");
    return;
  }
  if (url.pathname !== "/auth/callback") {
    writeOAuthCallbackResponse(response, 404, "Not found.");
    return;
  }
  if (!oauthFlowPending()) {
    writeOAuthCallbackResponse(response, 403, "No Cerul sign-in is in progress.");
    return;
  }
  const params = new URLSearchParams({ section: "Usage" });
  for (const key of ["provider", "code", "state", "error"]) {
    const value = url.searchParams.get(key);
    if (value) {
      params.set(key, value);
    }
  }
  oauthFlowPendingUntil = 0;
  openMainRoute(`settings?${params.toString()}`);
  focusMainWindow();
  writeOAuthCallbackResponse(response, 200, "Cerul sign-in is complete. You can return to the app.");
  // One-shot: the flow is over, stop listening.
  setImmediate(() => stopOAuthCallbackServer());
}

function writeOAuthCallbackResponse(response: http.ServerResponse, statusCode: number, message: string) {
  response.writeHead(statusCode, {
    "content-type": "text/html; charset=utf-8",
    "cache-control": "no-store",
  });
  response.end(`<!doctype html><meta charset="utf-8"><title>Cerul</title><p>${escapeHtml(message)}</p>`);
}

function oauthCallbackRedirectUri(port: number) {
  return `http://127.0.0.1:${port}/auth/callback`;
}

function stopOAuthCallbackServer() {
  oauthCallbackServer?.close();
  oauthCallbackServer = null;
  oauthCallbackPort = null;
}

function escapeHtml(value: string) {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function maybeRunRendererVideoSmoke() {
  if (process.env.CERUL_ELECTRON_VIDEO_SMOKE !== "1") {
    return;
  }
  const itemId = process.env.CERUL_ELECTRON_VIDEO_SMOKE_ITEM_ID;
  if (!itemId || !mainWindow) {
    console.error("electron_video_playback_smoke status=failed reason=missing_item_id");
    app.exit(1);
    return;
  }
  const timeoutMs = Number(process.env.CERUL_ELECTRON_VIDEO_SMOKE_TIMEOUT_MS ?? "60000");
  openMainRoute(`item-detail?itemId=${encodeURIComponent(itemId)}`);
  void mainWindow.webContents
    .executeJavaScript(
      rendererVideoSmokeScript(Number.isFinite(timeoutMs) ? timeoutMs : 60_000),
      true,
    )
    .then((result: unknown) => {
      const smoke = rendererSmokeResult(result);
      console.log(
        [
          "electron_video_playback_smoke status=ok",
          `item=${itemId}`,
          `duration=${smoke.duration}`,
          `currentTime=${smoke.currentTime}`,
          `readyState=${smoke.readyState}`,
          `src=${smoke.src}`,
        ].join(" "),
      );
      app.quit();
    })
    .catch((error) => {
      const message = error instanceof Error ? error.stack ?? error.message : String(error);
      console.error(`electron_video_playback_smoke status=failed item=${itemId} ${message}`);
      app.exit(1);
    });
}

function rendererSmokeResult(value: unknown) {
  const result = value as Partial<{
    duration: number;
    currentTime: number;
    readyState: number;
    src: string;
  }>;
  return {
    duration: formatSmokeNumber(result.duration),
    currentTime: formatSmokeNumber(result.currentTime),
    readyState: String(result.readyState ?? "unknown"),
    src: JSON.stringify(result.src ?? ""),
  };
}

function formatSmokeNumber(value: unknown) {
  return typeof value === "number" && Number.isFinite(value) ? value.toFixed(3) : "unknown";
}

function rendererVideoSmokeScript(timeoutMs: number) {
  return `
    (async () => {
      const timeoutMs = ${JSON.stringify(timeoutMs)};
      const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
      const deadline = Date.now() + timeoutMs;
      let video = null;
      while (Date.now() < deadline) {
        video = document.querySelector("video");
        if (video) break;
        await delay(250);
      }
      if (!video) throw new Error("video element not found");

      await new Promise((resolve, reject) => {
        if (video.readyState >= 1) {
          resolve(null);
          return;
        }
        const timer = setTimeout(() => reject(new Error("video metadata timeout")), timeoutMs);
        const cleanup = () => {
          clearTimeout(timer);
          video.removeEventListener("loadedmetadata", onMetadata);
          video.removeEventListener("error", onError);
        };
        const onMetadata = () => {
          cleanup();
          resolve(null);
        };
        const onError = () => {
          cleanup();
          reject(new Error("video metadata error"));
        };
        video.addEventListener("loadedmetadata", onMetadata, { once: true });
        video.addEventListener("error", onError, { once: true });
      });

      const duration = video.duration;
      if (!Number.isFinite(duration) || duration <= 0) {
        throw new Error("video duration is not available");
      }

      const seekTarget = duration > 1 ? Math.min(1, duration - 0.1) : Math.max(0, duration / 2);
      if (seekTarget > 0.01) {
        await new Promise((resolve, reject) => {
          const timer = setTimeout(
            () => reject(new Error("video seek timeout")),
            Math.min(timeoutMs, 30000),
          );
          const cleanup = () => {
            clearTimeout(timer);
            video.removeEventListener("seeked", onSeeked);
            video.removeEventListener("error", onError);
          };
          const onSeeked = () => {
            cleanup();
            resolve(null);
          };
          const onError = () => {
            cleanup();
            reject(new Error("video seek error"));
          };
          video.addEventListener("seeked", onSeeked, { once: true });
          video.addEventListener("error", onError, { once: true });
          video.currentTime = seekTarget;
        });
      }

      return {
        duration,
        currentTime: video.currentTime,
        readyState: video.readyState,
        src: video.currentSrc || video.src,
      };
    })()
  `;
}

async function startRustCore() {
  if (await apiIsHealthy(400)) {
    ownsApiProcess = false;
    return;
  }

  const env = { ...process.env, ...runtimeEnv(), CERUL_ELECTRON: "1" };
  const outputTail: ApiOutputTail = { stdout: "", stderr: "" };
  const startedAt = Date.now();
  lastApiExit = null;
  let binary: string;
  if (app.isPackaged) {
    binary = path.join(process.resourcesPath, "bin", executableName(packagedCoreBinaryName));
    if (!fs.existsSync(binary)) {
      throw new Error(`packaged Cerul Core binary is missing: ${binary}`);
    }
    apiProcess = spawnApiProcess(binary, env);
  } else {
    binary = path.join(repoRoot(), "target", "debug", executableName(devCoreBinaryName));
    if (!fs.existsSync(binary)) {
      buildDevApiBinary(binary, env);
    }
    apiProcess = spawnApiProcess(binary, env, repoRoot());
  }

  ownsApiProcess = true;
  const launchedApiProcess = apiProcess;
  apiProcess.stdout.on("data", (chunk) => {
    outputTail.stdout = appendOutputTail(outputTail.stdout, chunk, apiOutputTailBytes);
    process.stdout.write(`[cerul-core] ${chunk}`);
  });
  apiProcess.stderr.on("data", (chunk) => {
    outputTail.stderr = appendOutputTail(outputTail.stderr, chunk, apiOutputTailBytes);
    process.stderr.write(`[cerul-core] ${chunk}`);
  });
  apiProcess.on("error", (error) => {
    console.error("failed to start Cerul Core", error);
  });
  apiProcess.on("exit", (code, signal) => {
    lastApiExit = {
      pid: launchedApiProcess.pid,
      code,
      signal,
      elapsedMs: Date.now() - startedAt,
    };
    if (!isQuitting) {
      console.warn(
        `Cerul Core exited pid=${launchedApiProcess.pid ?? "unknown"} code=${code} signal=${signal} elapsed_ms=${lastApiExit.elapsedMs}`,
      );
    }
    apiProcess = null;
    ownsApiProcess = false;
    if (!isQuitting) {
      // Restart with capped backoff: a dead backend used to leave the app
      // running as a shell that could never search again.
      const delay = Math.min(1000 * 2 ** apiRestartAttempts, 30000);
      apiRestartAttempts += 1;
      setTimeout(() => {
        if (!isQuitting && !apiProcess) {
          void startRustCore().catch((restartError) => {
            console.error("Cerul Core restart failed", restartError);
          });
        }
      }, delay);
    }
  });

  try {
    await waitForApi(apiStartupTimeoutMs, () => lastApiExit);
    apiRestartAttempts = 0;
  } catch (error) {
    console.error(
      collectApiStartupDiagnostics({
        child: launchedApiProcess,
        binary,
        startedAt,
        outputTail,
        exitInfo: lastApiExit,
      }),
    );
    throw error;
  }
}

async function stopRustCoreGracefully(timeoutMs = 4000) {
  const child = apiProcess;
  if (!child || !ownsApiProcess) {
    return;
  }
  apiProcess = null;
  ownsApiProcess = false;
  await new Promise<void>((resolve) => {
    let settled = false;
    let termTimer: NodeJS.Timeout | null = null;
    let resolveTimer: NodeJS.Timeout | null = null;
    const finish = () => {
      if (settled) {
        return;
      }
      settled = true;
      if (termTimer) {
        clearTimeout(termTimer);
      }
      if (resolveTimer) {
        clearTimeout(resolveTimer);
      }
      resolve();
    };
    termTimer = setTimeout(() => {
      try {
        child.kill("SIGKILL");
      } catch {
        // already gone
      }
    }, timeoutMs);
    resolveTimer = setTimeout(finish, timeoutMs + 2_000);
    child.once("exit", finish);
    try {
      child.kill("SIGTERM");
    } catch {
      finish();
    }
  });
}

function spawnApiProcess(binary: string, env: NodeJS.ProcessEnv, cwd?: string) {
  const options = { cwd, env, stdio: "pipe" as const };
  const nofileLimit = positiveIntegerValue(env.CERUL_API_NOFILE_LIMIT, 8192);
  if (
    process.platform === "darwin" &&
    env.CERUL_API_RAISE_NOFILE !== "0" &&
    nofileLimit > 0
  ) {
    return spawn(
      "/bin/zsh",
      [
        "-lc",
        `ulimit -n ${nofileLimit} >/dev/null 2>&1 || true; exec "$0"`,
        binary,
      ],
      options,
    );
  }
  return spawn(binary, [], options);
}

function appendOutputTail(current: string, chunk: Buffer | string, maxChars: number) {
  const next = current + (Buffer.isBuffer(chunk) ? chunk.toString("utf8") : String(chunk));
  return next.length > maxChars ? next.slice(-maxChars) : next;
}

function collectApiStartupDiagnostics({
  child,
  binary,
  startedAt,
  outputTail,
  exitInfo,
}: {
  child: ChildProcessWithoutNullStreams;
  binary: string;
  startedAt: number;
  outputTail: ApiOutputTail;
  exitInfo: ApiExitInfo | null;
}) {
  const pid = child.pid;
  const lines = [
    "Cerul Core startup diagnostics:",
    `  health_url=${apiBaseUrl}/health`,
    `  startup_timeout_ms=${apiStartupTimeoutMs}`,
    `  pid=${pid ?? "unknown"}`,
    `  binary=${binary}`,
    `  elapsed_ms=${Date.now() - startedAt}`,
    `  exit=${formatApiExit(exitInfo)}`,
  ];

  if (pid && processAlive(pid)) {
    lines.push(diagnosticCommand("ps", ["-p", String(pid), "-o", "pid,ppid,stat,etime,rss,command"]));
    lines.push(diagnosticCommand("lsof", ["-p", String(pid)]));
    if (process.platform === "darwin" && process.env.CERUL_API_STARTUP_SAMPLE !== "0") {
      lines.push(sampleProcessDiagnostic(pid));
    }
  } else {
    lines.push("  process_alive=false");
  }

  lines.push(formatOutputTail("stdout", outputTail.stdout));
  lines.push(formatOutputTail("stderr", outputTail.stderr));
  return lines.join("\n");
}

function formatApiExit(exitInfo: ApiExitInfo | null) {
  if (!exitInfo) {
    return "not_observed";
  }
  return `pid=${exitInfo.pid ?? "unknown"} code=${exitInfo.code} signal=${exitInfo.signal} elapsed_ms=${exitInfo.elapsedMs}`;
}

function processAlive(pid: number) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

function diagnosticCommand(command: string, args: string[]) {
  const result = spawnSync(command, args, {
    encoding: "utf8",
    maxBuffer: 64 * 1024,
    timeout: 3_000,
  });
  const output = `${result.stdout ?? ""}${result.stderr ?? ""}`.trimEnd();
  if (result.error) {
    return `$ ${command} ${args.join(" ")}\n${result.error.message}`;
  }
  return `$ ${command} ${args.join(" ")}\n${output || "<empty>"}`;
}

function readBundleProcessHolders(bundlePath: string) {
  const result = spawnSync("lsof", ["-F", "pcn", "+D", bundlePath], {
    encoding: "utf8",
    maxBuffer: 256 * 1024,
    timeout: 5_000,
  });
  if (result.error) {
    return {
      holders: [] as BundleProcessHolder[],
      error: result.error.message,
    };
  }

  const stdout = typeof result.stdout === "string" ? result.stdout : "";
  const holdersByPid = new Map<number, BundleProcessHolder>();
  let currentPid: number | null = null;
  for (const rawLine of stdout.split(/\r?\n/)) {
    if (!rawLine) {
      continue;
    }
    const field = rawLine[0];
    const value = rawLine.slice(1);
    if (field === "p") {
      const pid = Number(value);
      currentPid = Number.isFinite(pid) ? pid : null;
      if (currentPid !== null && !holdersByPid.has(currentPid)) {
        holdersByPid.set(currentPid, { pid: currentPid, command: "", paths: [] });
      }
      continue;
    }
    if (currentPid === null) {
      continue;
    }
    const holder = holdersByPid.get(currentPid);
    if (!holder) {
      continue;
    }
    if (field === "c") {
      holder.command = value;
    } else if (field === "n") {
      holder.paths.push(value);
    }
  }

  return {
    holders: Array.from(holdersByPid.values()).sort((left, right) => left.pid - right.pid),
    error: null,
  };
}

function shouldTerminateUpdateInstallHolder(holder: BundleProcessHolder) {
  if (holder.pid === process.pid) {
    return false;
  }
  const command = path.basename(holder.command).toLowerCase();
  return (
    command === packagedCoreBinaryName ||
    command === devCoreBinaryName ||
    command === "qdrant" ||
    command === "python" ||
    command === "python3" ||
    command.startsWith("python3.")
  );
}

function formatBundleProcessHolders(holders: BundleProcessHolder[]) {
  if (holders.length === 0) {
    return "<empty>";
  }
  return holders
    .map((holder) => {
      const paths = holder.paths.slice(0, 4).join(", ");
      const suffix = holder.paths.length > 4 ? `, ... +${holder.paths.length - 4}` : "";
      return `pid=${holder.pid} command=${holder.command || "<unknown>"} paths=${paths}${suffix}`;
    })
    .join("\n");
}

async function waitForPidsToExit(pids: number[], timeoutMs: number) {
  const deadline = Date.now() + timeoutMs;
  let alive = pids.filter(processAlive);
  while (alive.length > 0 && Date.now() < deadline) {
    await delay(Math.min(250, Math.max(25, deadline - Date.now())));
    alive = pids.filter(processAlive);
  }
  return alive;
}

async function stopUpdateInstallBundleSidecars(bundlePath: string, lines: string[]) {
  const before = readBundleProcessHolders(bundlePath);
  if (before.error) {
    lines.push(`bundle_holder_scan_error=${before.error}`);
  }
  lines.push("== bundle holders before sidecar cleanup ==");
  lines.push(formatBundleProcessHolders(before.holders));

  const targets = before.holders.filter(shouldTerminateUpdateInstallHolder);
  if (targets.length === 0) {
    lines.push("sidecar_cleanup_targets=<empty>");
    return;
  }

  const targetPids = targets.map((target) => target.pid);
  lines.push("== sidecar cleanup targets ==");
  lines.push(formatBundleProcessHolders(targets));
  for (const pid of targetPids) {
    try {
      process.kill(pid, "SIGTERM");
    } catch (error) {
      lines.push(`sigterm_failed pid=${pid} error=${error instanceof Error ? error.message : String(error)}`);
    }
  }

  let remaining = await waitForPidsToExit(targetPids, 3_000);
  if (remaining.length > 0) {
    lines.push(`sidecar_sigkill_pids=${remaining.join(",")}`);
    for (const pid of remaining) {
      try {
        process.kill(pid, "SIGKILL");
      } catch (error) {
        lines.push(`sigkill_failed pid=${pid} error=${error instanceof Error ? error.message : String(error)}`);
      }
    }
    remaining = await waitForPidsToExit(remaining, 2_000);
  }
  lines.push(`sidecar_cleanup_remaining_pids=${remaining.length > 0 ? remaining.join(",") : "<empty>"}`);

  const after = readBundleProcessHolders(bundlePath);
  if (after.error) {
    lines.push(`bundle_holder_rescan_error=${after.error}`);
  }
  lines.push("== bundle holders after sidecar cleanup ==");
  lines.push(formatBundleProcessHolders(after.holders));
}

function sampleProcessDiagnostic(pid: number) {
  const samplePath = path.join(os.tmpdir(), `cerul-core-${pid}-${Date.now()}.sample.txt`);
  const result = spawnSync("sample", [String(pid), "1", "-file", samplePath], {
    encoding: "utf8",
    maxBuffer: 64 * 1024,
    timeout: 5_000,
  });
  let sample = "";
  try {
    sample = fs.readFileSync(samplePath, "utf8");
  } catch {
    sample = `${result.stdout ?? ""}${result.stderr ?? ""}`;
  } finally {
    try {
      fs.unlinkSync(samplePath);
    } catch {
      // Best-effort diagnostic cleanup.
    }
  }

  const excerpt = sample
    .split(/\r?\n/)
    .slice(0, 120)
    .join("\n")
    .trimEnd();
  return [
    `$ sample ${pid} 1`,
    `dyld_start_observed=${sample.includes("_dyld_start")}`,
    excerpt || "<empty>",
  ].join("\n");
}

function formatOutputTail(label: string, text: string) {
  const trimmed = text.trimEnd();
  return `--- cerul-core ${label} tail ---\n${trimmed || "<empty>"}`;
}

function buildDevApiBinary(binary: string, env: NodeJS.ProcessEnv) {
  const jobs = devCargoBuildJobs(env);
  const attempts = devCargoBuildAttempts(env);
  const args = ["build", "-p", "cerul-api", "-j", jobs];
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    if (attempt > 1) {
      console.warn(`Retrying Cerul Core build (${attempt}/${attempts}) after transient Cargo failure.`);
    }
    const result = spawnSync("cargo", args, {
      cwd: repoRoot(),
      env,
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
    });
    const stdout = String(result.stdout ?? "");
    const stderr = String(result.stderr ?? "");
    if (stdout) {
      process.stdout.write(stdout);
    }
    if (stderr) {
      process.stderr.write(stderr);
    }
    if (result.status === 0 && !result.signal) {
      break;
    }
    if (result.error) {
      throw result.error;
    }
    const output = `${stdout}\n${stderr}`;
    const wasSigkill =
      result.signal === "SIGKILL" ||
      result.status === 137 ||
      /SIGKILL|signal:\s*9|Killed:\s*9/.test(output);
    const wasIncompleteArtifact = /error\[E0463\]|can't find crate for/.test(output);
    if ((!wasSigkill && !wasIncompleteArtifact) || attempt === attempts) {
      const status = result.signal ?? result.status ?? "unknown";
      throw new Error(`failed to build Cerul Core binary (status ${status})`);
    }
    sleepSync(2_000);
  }
  if (!fs.existsSync(binary)) {
    throw new Error(`Cerul Core binary was not produced: ${binary}`);
  }
}

function devCargoBuildJobs(env: NodeJS.ProcessEnv) {
  const configured = env.CERUL_DEV_CARGO_JOBS ?? env.CARGO_BUILD_JOBS;
  if (configured && /^\d+$/.test(configured) && Number.parseInt(configured, 10) > 0) {
    return configured;
  }
  return "1";
}

function devCargoBuildAttempts(env: NodeJS.ProcessEnv) {
  const configured = env.CERUL_DEV_CARGO_RETRIES ?? env.CERUL_REBUILD_CARGO_RETRIES;
  if (configured && /^\d+$/.test(configured) && Number.parseInt(configured, 10) > 0) {
    return Number.parseInt(configured, 10);
  }
  return 16;
}

function sleepSync(ms: number) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

async function waitForApi(timeoutMs: number, exitInfo?: () => ApiExitInfo | null) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    const observedExit = exitInfo?.();
    if (observedExit) {
      throw new Error(
        `Cerul Core exited before becoming healthy at ${apiBaseUrl} (${formatApiExit(observedExit)})`,
      );
    }
    if (await apiIsHealthy(750)) {
      return;
    }
    await delay(250);
  }
  throw new Error(
    `Cerul Core did not become healthy at ${apiBaseUrl} within ${timeoutMs}ms`,
  );
}

async function apiIsHealthy(timeoutMs: number) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetch(`${apiBaseUrl}/health`, { signal: controller.signal });
    return response.ok;
  } catch {
    return false;
  } finally {
    clearTimeout(timer);
  }
}

async function initialGlobalHotkey() {
  if (process.env.CERUL_GLOBAL_HOTKEY) {
    return process.env.CERUL_GLOBAL_HOTKEY;
  }
  return settingString(await readApiSettings(), "global_hotkey", defaultHotkey);
}

async function shouldCloseToTray() {
  return settingBoolean(await readApiSettings(), "close_to_tray", true);
}

async function readApiSettings() {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 1_500);
  try {
    const response = await fetch(`${apiBaseUrl}/settings`, { signal: controller.signal });
    if (!response.ok) {
      return {};
    }
    return (await response.json()) as Record<string, unknown>;
  } catch {
    return {};
  } finally {
    clearTimeout(timer);
  }
}

function settingString(settings: Record<string, unknown>, key: string, fallback: string) {
  const value = settings[key];
  return typeof value === "string" && value.trim() ? value : fallback;
}

function settingBoolean(settings: Record<string, unknown>, key: string, fallback: boolean) {
  const value = settings[key];
  return typeof value === "boolean" ? value : fallback;
}

function positiveIntegerEnv(key: string, fallback: number) {
  return positiveIntegerValue(process.env[key], fallback);
}

function positiveIntegerValue(value: string | undefined, fallback: number) {
  if (!value) {
    return fallback;
  }
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
}

function runtimeEnv() {
  const root = repoRoot();
  const thirdParty = app.isPackaged
    ? path.join(process.resourcesPath, "third-party")
    : path.join(root, "third-party");
  const target = targetTriple();
  const suffix = process.platform === "win32" ? ".exe" : "";
  const env: NodeJS.ProcessEnv = {};

  const ffmpeg = path.join(thirdParty, target, `ffmpeg${suffix}`);
  const ytdlp = path.join(thirdParty, target, `yt-dlp${suffix}`);
  const qdrant = path.join(thirdParty, target, `qdrant${suffix}`);
  setBundledBinaryEnv(env, "CERUL_FFMPEG_PATH", ffmpeg, ["-version"]);
  setBundledExecutableEnv(env, "CERUL_YTDLP_PATH", ytdlp);
  setBundledBinaryEnv(env, "CERUL_QDRANT_BIN", qdrant, ["--version"]);

  const mlxSidecar = path.join(
    app.isPackaged ? process.resourcesPath : root,
    "mlx-sidecar",
    "cerul_mlx_sidecar.py",
  );
  if (fs.existsSync(mlxSidecar)) env.CERUL_MLX_SIDECAR = mlxSidecar;

  const bundledModels = path.join(app.isPackaged ? process.resourcesPath : root, "bundled-models");
  if (fs.existsSync(bundledModels)) env.CERUL_BUNDLED_MODELS_DIR = bundledModels;

  // Packaged builds ship a signed MLX Python runtime as a single archive. We
  // extract it into user data on first launch so Gatekeeper does not recursively
  // scan hundreds of nested mach-O files inside the .app bundle.
  if (app.isPackaged) {
    const mlxRuntimeManifest = path.join(process.resourcesPath, packagedMlxRuntimeManifestName);
    if (fs.existsSync(mlxRuntimeManifest)) {
      env.CERUL_MLX_RUNTIME_MANIFEST = mlxRuntimeManifest;
    }
    const mlxPython = preparePackagedMlxRuntime();
    if (mlxPython) env.CERUL_MLX_PYTHON = mlxPython;
  }
  return env;
}

function preparePackagedMlxRuntime() {
  if (!app.isPackaged || process.platform !== "darwin") {
    return null;
  }

  const archive = path.join(process.resourcesPath, packagedMlxRuntimeArchiveName);
  if (!fs.existsSync(archive)) {
    return preparedExternalMlxRuntime();
  }

  const digest = packagedMlxRuntimeDigest(archive);
  const runtimesRoot = path.join(appPaths().data_dir, "runtimes", "mlx");
  const runtimeDir = path.join(runtimesRoot, digest.slice(0, 16));
  const python = path.join(runtimeDir, "bin", "python3");
  const marker = path.join(runtimeDir, packagedMlxRuntimeReadyMarker);
  if (packagedMlxRuntimeReady(marker, digest, python)) {
    return python;
  }

  const tmpDir = `${runtimeDir}.tmp-${process.pid}-${Date.now()}`;
  fs.rmSync(runtimeDir, { recursive: true, force: true });
  fs.rmSync(tmpDir, { recursive: true, force: true });
  fs.mkdirSync(tmpDir, { recursive: true });

  try {
    const tar = spawnSync("/usr/bin/tar", ["-xzf", archive, "-C", tmpDir], {
      encoding: "utf8",
    });
    if (tar.status !== 0) {
      throw new Error(
        `failed to extract MLX runtime archive: ${tar.stderr || tar.stdout || `status ${tar.status}`}`,
      );
    }
    stripQuarantineXattrs(tmpDir);
    fs.accessSync(path.join(tmpDir, "bin", "python3"), fs.constants.X_OK);
    fs.writeFileSync(
      path.join(tmpDir, packagedMlxRuntimeReadyMarker),
      `${JSON.stringify({ archive_sha256: digest, created_at: new Date().toISOString() })}\n`,
    );
    fs.mkdirSync(runtimesRoot, { recursive: true });
    fs.renameSync(tmpDir, runtimeDir);
    pruneOldPackagedMlxRuntimes(runtimesRoot, path.basename(runtimeDir));
    console.log(`Prepared packaged MLX runtime at ${runtimeDir}`);
    return python;
  } catch (error) {
    fs.rmSync(tmpDir, { recursive: true, force: true });
    throw error;
  }
}

function preparedExternalMlxRuntime() {
  const manifestPath = path.join(process.resourcesPath, packagedMlxRuntimeManifestName);
  if (!fs.existsSync(manifestPath)) {
    return null;
  }

  let digest: string | null = null;
  try {
    const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8")) as { sha256?: unknown };
    if (typeof manifest.sha256 === "string" && /^[a-fA-F0-9]{64}$/.test(manifest.sha256)) {
      digest = manifest.sha256.toLowerCase();
    }
  } catch (error) {
    console.warn(`Unable to read external MLX runtime manifest: ${(error as Error).message}`);
  }
  if (!digest) {
    return null;
  }

  const runtimeDir = mlxRuntimeDirForDigest(digest);
  const python = path.join(runtimeDir, "bin", "python3");
  const marker = path.join(runtimeDir, packagedMlxRuntimeReadyMarker);
  return packagedMlxRuntimeReady(marker, digest, python) ? python : null;
}

function mlxRuntimeDirForDigest(digest: string) {
  const runtimesRoot = path.join(appPaths().data_dir, "runtimes", "mlx");
  return path.join(runtimesRoot, digest.slice(0, 16));
}

function packagedMlxRuntimeReady(marker: string, digest: string, python: string) {
  try {
    fs.accessSync(python, fs.constants.X_OK);
    const state = JSON.parse(fs.readFileSync(marker, "utf8")) as { archive_sha256?: string };
    return state.archive_sha256 === digest;
  } catch {
    return false;
  }
}

function packagedMlxRuntimeDigest(archive: string) {
  try {
    const text = fs.readFileSync(`${archive}.sha256`, "utf8");
    const match = text.match(/\b[a-fA-F0-9]{64}\b/);
    if (match) {
      return match[0].toLowerCase();
    }
  } catch {
    // Older development packages may not include the sidecar digest file.
  }
  return fileSha256(archive);
}

function fileSha256(file: string) {
  const hash = createHash("sha256");
  const fd = fs.openSync(file, "r");
  const buffer = Buffer.allocUnsafe(1024 * 1024);
  try {
    for (;;) {
      const bytesRead = fs.readSync(fd, buffer, 0, buffer.length, null);
      if (bytesRead === 0) break;
      hash.update(buffer.subarray(0, bytesRead));
    }
  } finally {
    fs.closeSync(fd);
  }
  return hash.digest("hex");
}

function stripQuarantineXattrs(dir: string) {
  const result = spawnSync("/usr/bin/xattr", ["-dr", "com.apple.quarantine", dir], {
    encoding: "utf8",
  });
  if (result.error && (result.error as NodeJS.ErrnoException).code !== "ENOENT") {
    console.warn(`Unable to strip quarantine xattrs from MLX runtime: ${result.error.message}`);
  }
}

function pruneOldPackagedMlxRuntimes(runtimesRoot: string, keepName: string) {
  try {
    for (const entry of fs.readdirSync(runtimesRoot, { withFileTypes: true })) {
      if (!entry.isDirectory() || entry.name === keepName) continue;
      fs.rmSync(path.join(runtimesRoot, entry.name), { recursive: true, force: true });
    }
  } catch (error) {
    console.warn(`Unable to prune old packaged MLX runtimes: ${(error as Error).message}`);
  }
}

function setBundledBinaryEnv(
  env: NodeJS.ProcessEnv,
  key: string,
  binaryPath: string,
  probeArgs: string[],
) {
  if (!fs.existsSync(binaryPath)) {
    return;
  }
  if (isRunnableBinary(binaryPath, probeArgs)) {
    env[key] = binaryPath;
    return;
  }
  console.warn(`Ignoring bundled binary for ${key}; it is not runnable: ${binaryPath}`);
}

function setBundledExecutableEnv(env: NodeJS.ProcessEnv, key: string, binaryPath: string) {
  try {
    fs.accessSync(binaryPath, fs.constants.X_OK);
    env[key] = binaryPath;
  } catch {
    return;
  }
}

// Probe results are cached per (path, mtime): the synchronous spawn happens
// on the startup path and a slow/hung binary used to block it for up to 8s
// per probe, serially.
const runnableBinaryCache = new Map<string, boolean>();

function isRunnableBinary(binaryPath: string, probeArgs: string[]) {
  if (!fs.existsSync(binaryPath)) {
    return false;
  }
  let cacheKey = binaryPath;
  try {
    cacheKey = `${binaryPath}:${fs.statSync(binaryPath).mtimeMs}`;
  } catch {
    // fall back to path-only key
  }
  const cached = runnableBinaryCache.get(cacheKey);
  if (cached !== undefined) {
    return cached;
  }
  const result = spawnSync(binaryPath, probeArgs, {
    stdio: "ignore",
    timeout: 3_000,
  });
  const runnable = result.status === 0;
  runnableBinaryCache.set(cacheKey, runnable);
  return runnable;
}

function targetTriple() {
  const arch = process.arch === "arm64" ? "aarch64" : "x86_64";
  if (process.platform === "darwin") return `${arch}-apple-darwin`;
  if (process.platform === "linux") return `${arch}-unknown-linux-gnu`;
  if (process.platform === "win32") return `${arch}-pc-windows-msvc`;
  return `${arch}-${process.platform}`;
}

function executableName(name: string) {
  return process.platform === "win32" ? `${name}.exe` : name;
}

function registerDeepLinkProtocols() {
  for (const scheme of deepLinkSchemes) {
    if (app.isPackaged) {
      app.setAsDefaultProtocolClient(scheme);
    } else {
      app.setAsDefaultProtocolClient(scheme, process.execPath, [process.argv[1]].filter(Boolean));
    }
  }
}

function firstDeepLinkArg(argv: string[]) {
  return argv.find((arg) => deepLinkSchemes.some((scheme) => arg.startsWith(`${scheme}://`)));
}

function isAppUrl(rawUrl: string) {
  try {
    const url = new URL(rawUrl);
    return url.protocol === `${appScheme}:` && url.hostname === appHost;
  } catch {
    return false;
  }
}

function isExternalUrl(rawUrl: string) {
  try {
    const protocol = new URL(rawUrl).protocol;
    return protocol === "http:" || protocol === "https:" || protocol === "mailto:";
  } catch {
    return false;
  }
}

function isPathInsideDirectory(filePath: string, directory: string) {
  const relative = path.relative(directory, filePath);
  return relative === "" || (relative !== "" && !relative.startsWith("..") && !path.isAbsolute(relative));
}

// Only frames belonging to the app shell (app:// in production, the vite
// dev server in development) may call privileged IPC — secure-token-get
// returns plaintext tokens and open-dialog/oauth-start act on the user's
// behalf.
function assertTrustedIpcSender(event: Electron.IpcMainInvokeEvent) {
  const url = event.senderFrame?.url ?? "";
  const trustedAppFrame = url.startsWith(`${appScheme}://`);
  const trustedDevFrame =
    !app.isPackaged &&
    (url.startsWith("http://127.0.0.1:1420") || url.startsWith("http://localhost:1420"));
  const trusted = trustedAppFrame || trustedDevFrame;
  if (!trusted) {
    throw new Error(`IPC call from untrusted sender: ${url || "<unknown>"}`);
  }
}

function registerIpcHandlers() {
  ipcMain.handle("cerul:invoke", async (event, command: string, args?: Record<string, unknown>) => {
    assertTrustedIpcSender(event);
    return handleCommand(command, args ?? {});
  });
  ipcMain.handle("cerul:open-dialog", async (event, options) => {
    assertTrustedIpcSender(event);
    const result = await dialog.showOpenDialog({
      properties: [
        options?.directory ? "openDirectory" : "openFile",
        options?.multiple ? "multiSelections" : undefined,
      ].filter(Boolean) as Electron.OpenDialogOptions["properties"],
      filters: options?.filters,
    });
    if (result.canceled) return null;
    return options?.multiple ? result.filePaths : result.filePaths[0] ?? null;
  });
  ipcMain.handle("cerul:app-version", async (event) => {
    assertTrustedIpcSender(event);
    return app.getVersion();
  });
  ipcMain.handle("cerul:check-update", async (event) => {
    assertTrustedIpcSender(event);
    return checkForGitHubReleaseUpdate();
  });
  ipcMain.handle("cerul:updater-check", async (event) => {
    assertTrustedIpcSender(event);
    await runDesktopUpdateCheck();
    return latestUpdaterState;
  });
  ipcMain.handle("cerul:updater-get-state", async (event) => {
    assertTrustedIpcSender(event);
    return latestUpdaterState;
  });
  ipcMain.handle("cerul:updater-diagnostics", async (event) => {
    assertTrustedIpcSender(event);
    return collectUpdaterDiagnostics();
  });
  ipcMain.handle("cerul:updater-download", async (event) => {
    assertTrustedIpcSender(event);
    await startDesktopUpdateDownload();
    return latestUpdaterState;
  });
  ipcMain.handle("cerul:updater-install", async (event) => {
    assertTrustedIpcSender(event);
    await installDesktopUpdate();
  });
  ipcMain.handle("cerul:store-get", async (event, storePath: string, key: string) => {
    assertTrustedIpcSender(event);
    return loadStore(storePath)[key];
  });
  ipcMain.handle("cerul:store-set", async (event, storePath: string, key: string, value: unknown) => {
    assertTrustedIpcSender(event);
    loadStore(storePath)[key] = value;
    dirtyStores.add(storePath);
  });
  ipcMain.handle("cerul:store-save", async (event, storePath: string) => {
    assertTrustedIpcSender(event);
    saveStore(storePath);
  });
  ipcMain.handle("cerul:secure-token-get", async (event, key: string) => {
    assertTrustedIpcSender(event);
    return getSecureToken(key);
  });
  ipcMain.handle("cerul:secure-token-set", async (event, key: string, value: string | null) => {
    assertTrustedIpcSender(event);
    setSecureToken(key, value);
  });
  ipcMain.handle("cerul:oauth-start", async (event, provider: OAuthProvider) => {
    assertTrustedIpcSender(event);
    await startOAuthLogin(provider);
  });
}

async function checkForGitHubReleaseUpdate(): Promise<DesktopUpdateInfo | null> {
  const repository = process.env.CERUL_UPDATE_REPOSITORY ?? defaultUpdateRepository;
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository)) {
    throw new Error(`Invalid update repository: ${repository}`);
  }

  const currentVersion = normalizeVersion(app.getVersion());
  const updateChannel = process.env.CERUL_UPDATE_CHANNEL ?? "";
  const allowPrerelease = updateChannel === "alpha" || isPrereleaseVersion(currentVersion);
  const response = await net.fetch(`https://api.github.com/repos/${repository}/releases?per_page=20`, {
    headers: {
      Accept: "application/vnd.github+json",
      "User-Agent": `Cerul/${currentVersion}`,
    },
  });
  if (!response.ok) {
    throw new Error(`GitHub release check failed with HTTP ${response.status}`);
  }

  const releases = (await response.json()) as GitHubRelease[];
  let bestUpdate: DesktopUpdateInfo | null = null;
  for (const release of releases) {
    if (release.draft) {
      continue;
    }
    if (release.prerelease && !allowPrerelease) {
      continue;
    }
    const version = releaseVersionFromTag(release.tag_name);
    if (!version || !release.html_url || compareVersions(version, currentVersion) <= 0) {
      continue;
    }
    if (!bestUpdate || compareVersions(version, bestUpdate.version) > 0) {
      bestUpdate = {
        version,
        url: release.html_url,
        name: release.name ?? undefined,
        prerelease: Boolean(release.prerelease),
        publishedAt: release.published_at ?? undefined,
      };
    }
  }
  return bestUpdate;
}

function updateRepository() {
  return process.env.CERUL_UPDATE_REPOSITORY ?? defaultUpdateRepository;
}

function releasesPageUrl() {
  return `https://github.com/${updateRepository()}/releases`;
}

function updaterInstallCleanupLogPath() {
  return path.join(app.getPath("userData"), "updater-install-cleanup.log");
}

function writeUpdaterInstallCleanupLog(lines: string[]) {
  try {
    fs.writeFileSync(updaterInstallCleanupLogPath(), `${lines.join("\n")}\n`, "utf8");
  } catch (error) {
    console.warn("failed to write updater install cleanup log", error);
  }
}

function readTextIfExists(filePath: string, maxBytes = 64 * 1024) {
  try {
    if (!fs.existsSync(filePath) || !fs.statSync(filePath).isFile()) {
      return null;
    }
    const file = fs.openSync(filePath, "r");
    try {
      const buffer = Buffer.alloc(Math.min(maxBytes, fs.statSync(filePath).size));
      fs.readSync(file, buffer, 0, buffer.length, 0);
      return buffer.toString("utf8");
    } finally {
      fs.closeSync(file);
    }
  } catch (error) {
    return `[[read failed: ${error instanceof Error ? error.message : String(error)}]]`;
  }
}

function listTree(root: string, maxEntries = 160) {
  const output: string[] = [];
  function walk(current: string, depth: number) {
    if (output.length >= maxEntries || depth > 4) {
      return;
    }
    let stat: fs.Stats;
    try {
      stat = fs.lstatSync(current);
    } catch (error) {
      output.push(`${current} [[stat failed: ${error instanceof Error ? error.message : String(error)}]]`);
      return;
    }
    const kind = stat.isDirectory() ? "dir" : stat.isSymbolicLink() ? "link" : "file";
    output.push(`${current} ${kind} ${stat.size} ${stat.mtime.toISOString()}`);
    if (!stat.isDirectory()) {
      return;
    }
    let entries: string[];
    try {
      entries = fs.readdirSync(current).sort();
    } catch (error) {
      output.push(`${current} [[readdir failed: ${error instanceof Error ? error.message : String(error)}]]`);
      return;
    }
    for (const entry of entries) {
      walk(path.join(current, entry), depth + 1);
      if (output.length >= maxEntries) {
        break;
      }
    }
  }
  walk(root, 0);
  return output;
}

function collectUpdaterDiagnostics() {
  const userData = app.getPath("userData");
  const appCache = path.join(os.homedir(), "Library", "Caches", macBundleIdentifier);
  const updaterCache = path.join(os.homedir(), "Library", "Caches", "@cerulelectron-shell-updater");
  const shipItCache = path.join(os.homedir(), "Library", "Caches", `${macBundleIdentifier}.ShipIt`);
  const bundlePath = macAppBundlePath();
  const appUpdateYml = path.join(process.resourcesPath, "app-update.yml");
  const shipItState = path.join(shipItCache, "ShipItState.plist");
  const pendingInfo = path.join(updaterCache, "pending", "update-info.json");
  const installCleanupLog = updaterInstallCleanupLogPath();
  const lines = [
    "== Cerul updater diagnostics ==",
    `createdAt=${new Date().toISOString()}`,
    `platform=${process.platform}`,
    `arch=${process.arch}`,
    `appVersion=${app.getVersion()}`,
    `isPackaged=${app.isPackaged}`,
    `appPath=${app.getAppPath()}`,
    `bundlePath=${bundlePath ?? ""}`,
    `resourcesPath=${process.resourcesPath}`,
    `userData=${userData}`,
    `cache=${appCache}`,
    `latestUpdaterState=${JSON.stringify(latestUpdaterState)}`,
    "",
    "== app-update.yml ==",
    readTextIfExists(appUpdateYml) ?? "[[missing]]",
    "",
    "== pending update-info.json ==",
    readTextIfExists(pendingInfo) ?? "[[missing]]",
    "",
    "== ShipItState.plist raw ==",
    readTextIfExists(shipItState) ?? "[[missing]]",
    "",
    "== last updater install cleanup ==",
    readTextIfExists(installCleanupLog) ?? "[[missing]]",
    "",
    "== updater cache tree ==",
    ...listTree(updaterCache),
    "",
    "== ShipIt cache tree ==",
    ...listTree(shipItCache),
    "",
    "== open files under app bundle ==",
    bundlePath ? diagnosticCommand("lsof", ["+D", bundlePath]) : "[[not a macOS app bundle]]",
    "",
    "== last core exit ==",
    JSON.stringify(lastApiExit),
  ];
  return lines.join("\n");
}

function positiveFiniteNumber(value: unknown): number | undefined {
  const number = typeof value === "number" ? value : Number(value);
  return Number.isFinite(number) && number > 0 ? number : undefined;
}

function updateDownloadState(version: string, progress: UpdaterProgress = {}): UpdaterState {
  const rawPercent = Number.isFinite(progress.percent) ? Number(progress.percent) : 0;
  const percent = Math.max(0, Math.min(100, Math.round(rawPercent)));
  const bytesPerSecond = positiveFiniteNumber(progress.bytesPerSecond);
  const transferredBytes = positiveFiniteNumber(progress.transferred);
  const totalBytes = positiveFiniteNumber(progress.total);
  const remainingBytes =
    totalBytes !== undefined && transferredBytes !== undefined
      ? Math.max(0, totalBytes - transferredBytes)
      : undefined;
  const etaSeconds =
    bytesPerSecond !== undefined && remainingBytes !== undefined
      ? Math.ceil(remainingBytes / bytesPerSecond)
      : undefined;
  return {
    phase: "downloading",
    version,
    percent,
    bytesPerSecond,
    etaSeconds,
    transferredBytes,
    totalBytes,
  };
}

function setUpdaterState(next: UpdaterState) {
  latestUpdaterState = next;
  // The renderer also pulls the current state on mount (cerul:updater-get-state)
  // in case it subscribes after the first check emits.
  if (mainWindow && !mainWindow.isDestroyed()) {
    mainWindow.webContents.send("cerul:updater-event", next);
  }
}

function setUpdaterError(error: unknown, version?: string) {
  const message = error instanceof Error ? error.message : String(error);
  console.error("desktop updater error", error);
  setUpdaterState({
    phase: "error",
    version,
    message,
    releaseUrl: releasesPageUrl(),
  });
}

function getAutoUpdater(): AppUpdater | null {
  if (autoUpdaterInstance) {
    return autoUpdaterInstance;
  }
  try {
    const mod = require("electron-updater") as typeof import("electron-updater");
    autoUpdaterInstance = mod.autoUpdater;
    return autoUpdaterInstance;
  } catch (error) {
    console.error("electron-updater unavailable; using release-page fallback", error);
    return null;
  }
}

function wireAutoUpdater(updater: AppUpdater) {
  if (autoUpdaterWired) {
    return;
  }
  autoUpdaterWired = true;
  // Codex-like flow: signed update assets download in the background after a
  // successful check. The rail button then installs an already prepared update.
  updater.autoDownload = true;
  updater.autoInstallOnAppQuit = true;
  updater.on("update-available", (info) => {
    setUpdaterState(updateDownloadState(normalizeVersion(info.version)));
  });
  updater.on("download-progress", (progress) => {
    const version =
      latestUpdaterState.phase === "available" || latestUpdaterState.phase === "downloading"
        ? latestUpdaterState.version
        : normalizeVersion(app.getVersion());
    setUpdaterState(updateDownloadState(version, progress));
  });
  updater.on("update-downloaded", (info) => {
    const version = normalizeVersion(info.version);
    const shouldAutoInstall = updateInstallRequested;
    updateInstallRequested = false;
    if (!shouldAutoInstall) {
      setUpdaterState({ phase: "downloaded", version });
      return;
    }
    setUpdaterState({ phase: "installing", version });
    setTimeout(() => void installDesktopUpdate(version), 500);
  });
  updater.on("error", (error) => {
    // No latest-mac.yml, a signature mismatch on ad-hoc builds, or a network
    // failure. Degrade to the GitHub-release fallback so the pill still lets the
    // user grab the new version from the download page.
    console.error("electron-updater error", error);
    const fallbackUrl =
      latestUpdaterState.phase === "available" ? latestUpdaterState.releaseUrl : releasesPageUrl();
    if (updateInstallRequested) {
      updateInstallRequested = false;
    }
    setUpdaterState({
      phase: "error",
      version:
        latestUpdaterState.phase === "available" ||
        latestUpdaterState.phase === "downloading" ||
        latestUpdaterState.phase === "installing" ||
        latestUpdaterState.phase === "downloaded"
          ? latestUpdaterState.version
          : undefined,
      message: error instanceof Error ? error.message : String(error),
      releaseUrl: fallbackUrl,
    });
  });
}

// Signing-independent detection (GitHub releases API) that works on today's
// ad-hoc builds. Drives the "available" pill; never clobbers an in-flight
// download/installed state.
async function refreshManualUpdateState() {
  let info: DesktopUpdateInfo | null = null;
  try {
    info = await checkForGitHubReleaseUpdate();
  } catch (error) {
    console.error("github update check failed", error);
    return;
  }
  if (info) {
    if (latestUpdaterState.phase === "idle" || latestUpdaterState.phase === "available") {
      setUpdaterState({
        phase: "available",
        version: info.version,
        releaseUrl: info.url,
        canAutoInstall: false,
      });
    }
  } else if (latestUpdaterState.phase === "available") {
    setUpdaterState({ phase: "idle" });
  }
}

async function runDesktopUpdateCheck() {
  // Dev demo hook: CERUL_FAKE_UPDATE=<version> renders the pill without a real
  // release so the flow is reviewable before signed releases exist.
  const fake = process.env.CERUL_FAKE_UPDATE;
  if (fake && !app.isPackaged) {
    setUpdaterState({
      phase: "available",
      version: normalizeVersion(fake),
      releaseUrl: releasesPageUrl(),
      canAutoInstall: false,
    });
    return;
  }

  if (
    latestUpdaterState.phase === "downloading" ||
    latestUpdaterState.phase === "downloaded" ||
    latestUpdaterState.phase === "installing"
  ) {
    return;
  }

  await refreshManualUpdateState();

  // Opportunistic in-place updater — dormant until releases ship signed +
  // notarized with a latest-mac.yml that Squirrel.Mac can apply. When that
  // lands, these events upgrade the pill from "open download page" to a
  // one-click download followed by an automatic restart-to-install.
  if (!app.isPackaged) {
    return;
  }
  const updater = getAutoUpdater();
  if (!updater) {
    return;
  }
  try {
    wireAutoUpdater(updater);
    await updater.checkForUpdates();
  } catch (error) {
    console.error("electron-updater check failed; release-page fallback active", error);
  }
}

async function startDesktopUpdateDownload() {
  if (latestUpdaterState.phase !== "available") {
    return;
  }
  const { releaseUrl, canAutoInstall, version } = latestUpdaterState;
  // Without a working in-place updater, "update" means open the download page.
  if (!canAutoInstall) {
    await shell.openExternal(releaseUrl);
    return;
  }
  const updater = getAutoUpdater();
  if (!updater) {
    await shell.openExternal(releaseUrl);
    return;
  }
  updateInstallRequested = true;
  try {
    setUpdaterState(updateDownloadState(version));
    await updater.downloadUpdate();
  } catch (error) {
    console.error("electron-updater download failed; opening release page", error);
    updateInstallRequested = false;
    setUpdaterState({
      phase: "error",
      version,
      message: error instanceof Error ? error.message : String(error),
      releaseUrl,
    });
  }
}

function clearUpdateInstallFallbackTimers() {
  if (updateInstallFallbackTimer) {
    clearTimeout(updateInstallFallbackTimer);
    updateInstallFallbackTimer = null;
  }
  if (updateInstallForceExitTimer) {
    clearTimeout(updateInstallForceExitTimer);
    updateInstallForceExitTimer = null;
  }
}

function scheduleUpdateInstallExitFallback() {
  clearUpdateInstallFallbackTimers();
  updateInstallFallbackTimer = setTimeout(() => {
    if (!isQuitting) {
      isQuitting = true;
    }
    app.quit();
  }, 1500);
  updateInstallForceExitTimer = setTimeout(() => {
    app.exit(0);
  }, 9000);
}

async function prepareDesktopUpdateInstall(version: string) {
  const bundlePath = macAppBundlePath();
  const lines = [
    "== Cerul updater install cleanup ==",
    `createdAt=${new Date().toISOString()}`,
    `version=${version}`,
    `pid=${process.pid}`,
    `bundlePath=${bundlePath ?? ""}`,
    `apiProcessPid=${apiProcess?.pid ?? ""}`,
    `ownsApiProcess=${ownsApiProcess}`,
  ];

  stopStatusMonitor();
  stopOAuthCallbackServer();
  await stopRustCoreGracefully(10_000);

  if (bundlePath) {
    await stopUpdateInstallBundleSidecars(bundlePath, lines);
    lines.push("== open files under app bundle before quitAndInstall ==");
    lines.push(diagnosticCommand("lsof", ["+D", bundlePath]));
  } else {
    lines.push("bundle_holder_cleanup=skipped_not_macos_bundle");
  }

  writeUpdaterInstallCleanupLog(lines);
}

async function installDesktopUpdate(version?: string) {
  const updater = getAutoUpdater();
  if (!updater) {
    setUpdaterError(new Error("electron-updater is unavailable"), version);
    return;
  }
  const installingVersion =
    version ??
    (latestUpdaterState.phase === "downloaded" || latestUpdaterState.phase === "installing"
      ? latestUpdaterState.version
      : app.getVersion());
  setUpdaterState({ phase: "installing", version: installingVersion });
  try {
    await prepareDesktopUpdateInstall(installingVersion);
    // Electron's autoUpdater closes windows before `before-quit` fires. Mark the
    // app as quitting up front so our close-to-tray handler does not hide the
    // main window and leave ShipIt blocked on a still-running app instance.
    isQuitting = true;
    updater.quitAndInstall(false, true);
    scheduleUpdateInstallExitFallback();
  } catch (error) {
    clearUpdateInstallFallbackTimers();
    setUpdaterError(error, installingVersion);
  }
}

function releaseVersionFromTag(tag: string | undefined) {
  if (!tag) {
    return null;
  }
  const version = normalizeVersion(tag);
  return /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(version)
    ? version
    : null;
}

function normalizeVersion(version: string) {
  return version.trim().replace(/^v/i, "");
}

function isPrereleaseVersion(version: string) {
  return normalizeVersion(version).split("+", 1)[0].includes("-");
}

function compareVersions(left: string, right: string) {
  const a = parseVersion(left);
  const b = parseVersion(right);
  for (let index = 0; index < 3; index += 1) {
    if (a.core[index] !== b.core[index]) {
      return a.core[index] > b.core[index] ? 1 : -1;
    }
  }
  return comparePrerelease(a.prerelease, b.prerelease);
}

function parseVersion(version: string) {
  const withoutBuild = normalizeVersion(version).split("+", 1)[0];
  const prereleaseStart = withoutBuild.indexOf("-");
  const coreVersion = prereleaseStart === -1 ? withoutBuild : withoutBuild.slice(0, prereleaseStart);
  const prerelease = prereleaseStart === -1 ? "" : withoutBuild.slice(prereleaseStart + 1);
  const core = coreVersion.split(".").map((part) => Number.parseInt(part, 10));
  return {
    core: [core[0] ?? 0, core[1] ?? 0, core[2] ?? 0],
    prerelease: prerelease ? prerelease.split(".") : [],
  };
}

function comparePrerelease(left: string[], right: string[]) {
  if (left.length === 0 && right.length === 0) {
    return 0;
  }
  if (left.length === 0) {
    return 1;
  }
  if (right.length === 0) {
    return -1;
  }
  const length = Math.max(left.length, right.length);
  for (let index = 0; index < length; index += 1) {
    const a = left[index];
    const b = right[index];
    if (a === undefined) {
      return -1;
    }
    if (b === undefined) {
      return 1;
    }
    const numericA = /^\d+$/.test(a);
    const numericB = /^\d+$/.test(b);
    if (numericA && numericB) {
      const numberA = Number.parseInt(a, 10);
      const numberB = Number.parseInt(b, 10);
      if (numberA !== numberB) {
        return numberA > numberB ? 1 : -1;
      }
      continue;
    }
    if (numericA !== numericB) {
      return numericA ? -1 : 1;
    }
    if (a !== b) {
      return a > b ? 1 : -1;
    }
  }
  return 0;
}

async function handleCommand(command: string, args: Record<string, unknown>) {
  switch (command) {
    case "daemon_status":
      return loginItemResult();
    case "install_daemon":
      return installLoginItem();
    case "uninstall_daemon":
      return uninstallLoginItem();
    case "open_accessibility_settings":
      if (process.platform === "darwin") {
        await shell.openExternal(
          "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
        );
      }
      return null;
    case "reveal_data_directory":
      await openPathOrThrow(appPaths().data_dir);
      return null;
    case "reveal_logs_directory": {
      const logs = path.join(appPaths().data_dir, "logs");
      fs.mkdirSync(logs, { recursive: true });
      await openPathOrThrow(logs);
      return null;
    }
    case "reveal_source_path":
      await revealSource(String(args.path ?? ""));
      return null;
    case "storage_locations":
      return appPaths();
    case "clear_cache":
      return clearCache();
    case "reset_local_data":
      return scheduleLocalDataReset();
    case "set_global_hotkey":
      registerGlobalHotkey(String(args.label ?? defaultHotkey));
      return null;
    case "hide_overlay":
      overlayWindow?.hide();
      return null;
    case "hide_menubar":
      menuBarWindow?.hide();
      return null;
    case "open_main_window":
      menuBarWindow?.hide();
      focusMainWindow();
      return null;
    case "show_search_overlay":
      menuBarWindow?.hide();
      showOverlay();
      return null;
    case "resize_overlay":
      resizeOverlay(Number(args.height ?? 0));
      return null;
    case "open_main_result":
      openMainRoute(
        `result-detail?itemId=${encodeURIComponent(String(args.itemId ?? ""))}&t=${encodeURIComponent(
          String(args.timestamp ?? ""),
        )}`,
      );
      return null;
    case "open_main_settings":
      openMainRoute(
        args.section ? `settings?section=${encodeURIComponent(String(args.section))}` : "settings",
      );
      return null;
    case "notify_first_items_indexed":
      showNotification("Cerul is ready", `Your first ${args.count ?? 0} videos are searchable.`);
      return null;
    case "notify_indexing_complete":
      showNotification("Indexing complete", `All ${args.total ?? 0} items are now searchable.`);
      return null;
    case "notify_update_available":
      showNotification("Update available", `Cerul ${args.version ?? ""} is ready.`);
      return null;
    case "notify_items_failed":
      showNotification(`${args.failed ?? 0} items failed`, "View details in jobs panel.");
      return null;
    case "notify_folder_source_missing":
      showNotification("Folder source unavailable", `Cerul can't find ${args.source ?? ""}.`);
      return null;
    case "update_tray_idle_status":
      tray?.setToolTip(`Cerul · ${args.indexed ?? 0} indexed`);
      return null;
    case "update_tray_indexing_status":
      tray?.setToolTip(`Cerul · indexing ${args.indexed ?? 0}/${args.total ?? 0}`);
      return null;
    default:
      throw new Error(`unsupported Electron desktop command: ${command}`);
  }
}

function loginItemResult(message?: string) {
  const smokeFile = loginItemSmokeFile();
  if (smokeFile) {
    return {
      platform: process.platform,
      installed: fs.existsSync(smokeFile),
      path: smokeFile,
      message,
    };
  }
  if (process.platform === "linux") {
    const autostartPath = linuxAutostartPath();
    return {
      platform: process.platform,
      installed: fs.existsSync(autostartPath),
      path: autostartPath,
      message,
    };
  }
  const settings = app.getLoginItemSettings({ args: loginItemArgs() });
  return {
    platform: process.platform,
    installed: settings.openAtLogin,
    path: null,
    message,
  };
}

function installLoginItem() {
  const smokeFile = loginItemSmokeFile();
  if (smokeFile) {
    fs.mkdirSync(path.dirname(smokeFile), { recursive: true });
    fs.writeFileSync(smokeFile, JSON.stringify({ installed: true }));
    return loginItemResult("Start at login is enabled");
  }
  if (process.platform === "linux") {
    installLinuxAutostart();
    return loginItemResult("Start at login is enabled");
  }
  app.setLoginItemSettings({
    openAtLogin: true,
    openAsHidden: true,
    args: loginItemArgs(),
  });
  return loginItemResult("Start at login is enabled");
}

function uninstallLoginItem() {
  const smokeFile = loginItemSmokeFile();
  if (smokeFile) {
    fs.rmSync(smokeFile, { force: true });
    return loginItemResult("Start at login is disabled");
  }
  if (process.platform === "linux") {
    uninstallLinuxAutostart();
    return loginItemResult("Start at login is disabled");
  }
  app.setLoginItemSettings({
    openAtLogin: false,
    args: loginItemArgs(),
  });
  return loginItemResult("Start at login is disabled");
}

function firstLoginItemCliCommand(argv: string[]) {
  if (argv.includes("--daemon-status")) return "daemon_status";
  if (argv.includes("--install-daemon")) return "install_daemon";
  if (argv.includes("--uninstall-daemon")) return "uninstall_daemon";
  return null;
}

function runLoginItemCliCommand(command: "daemon_status" | "install_daemon" | "uninstall_daemon") {
  const result =
    command === "install_daemon"
      ? installLoginItem()
      : command === "uninstall_daemon"
        ? uninstallLoginItem()
        : loginItemResult();
  process.stdout.write(`${JSON.stringify(result)}\n`);
}

function loginItemSmokeFile() {
  const value = process.env.CERUL_LOGIN_ITEM_SMOKE_FILE?.trim();
  return value ? value : null;
}

function loginItemArgs() {
  return ["--hidden"];
}

function installLinuxAutostart() {
  const autostartPath = linuxAutostartPath();
  fs.mkdirSync(path.dirname(autostartPath), { recursive: true });
  fs.writeFileSync(
    autostartPath,
    [
      "[Desktop Entry]",
      "Type=Application",
      "Name=Cerul",
      `Exec=${desktopExec(process.env.APPIMAGE ?? process.execPath, loginItemArgs())}`,
      "Terminal=false",
      "X-GNOME-Autostart-enabled=true",
      "",
    ].join("\n"),
  );
}

function uninstallLinuxAutostart() {
  fs.rmSync(linuxAutostartPath(), { force: true });
}

function linuxAutostartPath() {
  const configHome = process.env.XDG_CONFIG_HOME ?? path.join(os.homedir(), ".config");
  return path.join(configHome, "autostart", "cerul.desktop");
}

function desktopExec(binary: string, args: string[]) {
  return [desktopExecQuote(binary), ...args.map(desktopExecQuote)].join(" ");
}

function desktopExecQuote(value: string) {
  return `"${value.replace(/["\\$`]/g, "\\$&")}"`;
}

function openMainRoute(route: string) {
  overlayWindow?.hide();
  queuedMainRoute = route;
  focusMainWindow();
  flushQueuedMainRoute();
}

function flushQueuedMainRoute() {
  if (!mainWindow || !mainWindowLoaded || !queuedMainRoute) {
    return;
  }
  const route = queuedMainRoute;
  queuedMainRoute = null;
  void mainWindow.webContents.executeJavaScript(`window.location.hash = ${JSON.stringify(route)};`);
}

function showNotification(title: string, body: string) {
  if (Notification.isSupported()) {
    new Notification({ title, body }).show();
  }
}

function appPaths() {
  const data = process.env.CERUL_DATA_DIR ?? path.join(dataBaseDir(), "Cerul");
  return {
    data_dir: data,
    cache_dir: path.join(data, "cache"),
    models_dir: path.join(data, "models"),
    index_dir: path.join(data, "indexes", "qdrant"),
  };
}

function dataBaseDir() {
  if (process.platform === "darwin") {
    return path.join(os.homedir(), "Library", "Application Support");
  }
  if (process.platform === "win32") {
    return process.env.APPDATA ?? path.join(os.homedir(), "AppData", "Roaming");
  }
  return process.env.XDG_DATA_HOME ?? path.join(os.homedir(), ".local", "share");
}

async function openPathOrThrow(targetPath: string) {
  const error = await shell.openPath(targetPath);
  if (error) {
    throw new Error(error);
  }
}

async function revealSource(rawPath: string) {
  const source = expandHome(rawPath.trim());
  if (!source || !fs.existsSync(source)) {
    throw new Error(`source path not found: ${rawPath}`);
  }
  if (fs.statSync(source).isFile()) {
    shell.showItemInFolder(source);
  } else {
    await openPathOrThrow(source);
  }
}

function clearCache() {
  const paths = appPaths();
  const bytesRemoved = directorySize(paths.cache_dir);
  fs.rmSync(paths.cache_dir, { recursive: true, force: true });
  fs.mkdirSync(paths.cache_dir, { recursive: true });
  return {
    cache_dir: paths.cache_dir,
    bytes_removed: bytesRemoved,
  };
}

function scheduleLocalDataReset() {
  const targets = resetLocalDataTargets();
  const scriptPath = path.join(os.tmpdir(), `cerul-reset-${process.pid}-${Date.now()}.sh`);
  const relaunchLine = relaunchShellLine();
  const lines = [
    "#!/bin/sh",
    "set -u",
    `PARENT_PID=${process.pid}`,
    'while kill -0 "$PARENT_PID" 2>/dev/null; do sleep 0.2; done',
    "sleep 0.5",
    ...targets.map((target) => `rm -rf -- ${shellQuote(target.path)}`),
    relaunchLine,
    `rm -f -- ${shellQuote(scriptPath)}`,
    "",
  ];

  fs.writeFileSync(scriptPath, lines.join("\n"), { mode: 0o700 });
  const child = spawn("/bin/sh", [scriptPath], {
    detached: true,
    stdio: "ignore",
  });
  child.unref();
  isQuitting = true;
  app.quit();

  return {
    scheduled: true,
    targets,
  };
}

function resetLocalDataTargets() {
  const paths = appPaths();
  const userData = app.getPath("userData");
  const targets = [
    { label: "data", path: paths.data_dir },
    {
      label: app.isPackaged ? "userData" : "devStores",
      path: app.isPackaged ? userData : path.join(userData, "stores"),
    },
  ];
  const seen = new Set<string>();
  return targets
    .map((target) => ({ ...target, path: path.resolve(target.path) }))
    .filter((target) => {
      if (seen.has(target.path)) {
        return false;
      }
      seen.add(target.path);
      return true;
    })
    .map((target) => {
      assertSafeResetTarget(target.path);
      return target;
    });
}

function assertSafeResetTarget(targetPath: string) {
  const resolved = path.resolve(targetPath);
  const forbidden = [
    path.parse(resolved).root,
    os.homedir(),
    dataBaseDir(),
    path.dirname(dataBaseDir()),
  ].map((value) => path.resolve(value));
  if (forbidden.includes(resolved)) {
    throw new Error(`refusing to reset unsafe path: ${resolved}`);
  }
  const depth = resolved.split(path.sep).filter(Boolean).length;
  if (depth < 3) {
    throw new Error(`refusing to reset shallow path: ${resolved}`);
  }
}

function relaunchShellLine() {
  if (process.env.CERUL_RESET_SKIP_RELAUNCH === "1") {
    return "true";
  }
  const bundle = macAppBundlePath();
  if (bundle) {
    return `open -n ${shellQuote(bundle)} >/dev/null 2>&1 || true`;
  }
  return `${shellQuote(process.execPath)} >/dev/null 2>&1 &`;
}

function macAppBundlePath() {
  if (process.platform !== "darwin") {
    return null;
  }
  const marker = ".app/Contents/MacOS";
  const index = process.execPath.indexOf(marker);
  if (index === -1) {
    return null;
  }
  return process.execPath.slice(0, index + ".app".length);
}

function shellQuote(value: string) {
  return `'${value.replace(/'/g, "'\\''")}'`;
}

function directorySize(root: string) {
  if (!fs.existsSync(root)) return 0;
  let total = 0;
  const stack = [root];
  while (stack.length > 0) {
    const current = stack.pop()!;
    for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
      const fullPath = path.join(current, entry.name);
      if (entry.isDirectory()) {
        stack.push(fullPath);
      } else if (entry.isFile()) {
        total += fs.statSync(fullPath).size;
      }
    }
  }
  return total;
}

function expandHome(value: string) {
  if (value === "~") return os.homedir();
  if (value.startsWith("~/")) return path.join(os.homedir(), value.slice(2));
  return value;
}

function storeFilePath(storePath: string) {
  const safeName = storePath.replace(/[/\\:]/g, "_");
  return path.join(app.getPath("userData"), "stores", safeName);
}

function loadStore(storePath: string) {
  if (stores.has(storePath)) {
    return stores.get(storePath)!;
  }
  const file = storeFilePath(storePath);
  let value: Record<string, unknown> = {};
  try {
    value = JSON.parse(fs.readFileSync(file, "utf8")) as Record<string, unknown>;
  } catch (error) {
    // Keep the corrupt file for forensics instead of silently wiping
    // everything (this store may hold cloud login tokens).
    if (fs.existsSync(file)) {
      console.error(`store file is unreadable, moving aside: ${file}`, error);
      try {
        fs.renameSync(file, `${file}.corrupt`);
      } catch {
        // best effort
      }
    }
    value = {};
  }
  stores.set(storePath, value);
  return value;
}

function saveStore(storePath: string) {
  if (!dirtyStores.has(storePath)) return;
  const file = storeFilePath(storePath);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  // temp + rename: a crash mid-write must not truncate the store.
  const tmp = `${file}.tmp`;
  fs.writeFileSync(tmp, JSON.stringify(loadStore(storePath), null, 2));
  fs.renameSync(tmp, file);
  dirtyStores.delete(storePath);
}

function flushDirtyStores() {
  for (const storePath of [...dirtyStores]) {
    try {
      saveStore(storePath);
    } catch (error) {
      console.error(`failed to flush store ${storePath} on quit`, error);
    }
  }
}

function getSecureToken(key: string) {
  const tokenKey = normalizeSecureTokenKey(key);
  const store = loadStore(secureTokenStorePath);
  const record = store[tokenKey];
  if (!record || typeof record !== "object") {
    return undefined;
  }
  const encrypted = (record as { scheme?: unknown; value?: unknown }).value;
  const scheme = (record as { scheme?: unknown }).scheme;
  if (scheme !== "safeStorage:v1" || typeof encrypted !== "string") {
    delete store[tokenKey];
    dirtyStores.add(secureTokenStorePath);
    saveStore(secureTokenStorePath);
    return undefined;
  }
  try {
    return safeStorage.decryptString(Buffer.from(encrypted, "base64"));
  } catch {
    delete store[tokenKey];
    dirtyStores.add(secureTokenStorePath);
    saveStore(secureTokenStorePath);
    return undefined;
  }
}

function setSecureToken(key: string, value: string | null) {
  const tokenKey = normalizeSecureTokenKey(key);
  const store = loadStore(secureTokenStorePath);
  if (!value) {
    delete store[tokenKey];
    dirtyStores.add(secureTokenStorePath);
    saveStore(secureTokenStorePath);
    return;
  }
  if (!safeStorage.isEncryptionAvailable()) {
    console.warn("secure token storage is unavailable; token will not be persisted");
    delete store[tokenKey];
    dirtyStores.add(secureTokenStorePath);
    saveStore(secureTokenStorePath);
    return;
  }
  store[tokenKey] = {
    scheme: "safeStorage:v1",
    value: safeStorage.encryptString(value).toString("base64"),
  };
  dirtyStores.add(secureTokenStorePath);
  saveStore(secureTokenStorePath);
}

function normalizeSecureTokenKey(key: string) {
  if (!/^[A-Za-z0-9_.-]{1,80}$/.test(key)) {
    throw new Error("invalid secure token key");
  }
  return key;
}

function delay(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
