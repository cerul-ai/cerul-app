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
  screen,
  shell,
} from "electron";
import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";

const apiBaseUrl = "http://127.0.0.1:7777";
const appScheme = "app";
const appHost = "cerul";
const deepLinkSchemes = ["cerul", "cerul-app"];
const defaultHotkey = "Alt+Space";
const cloudAccountOrigin = "https://accounts.cerul.ai";
const defaultUpdateRepository = "cerul-ai/cerul-app";
const contentSecurityPolicy = [
  "default-src 'self'",
  "script-src 'self'",
  "style-src 'self' 'unsafe-inline'",
  "font-src 'self'",
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
    app.quit();
  });

app.on("before-quit", () => {
  isQuitting = true;
});

app.on("will-quit", () => {
  globalShortcut.unregisterAll();
  stopStatusMonitor();
  stopRustCore();
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
  return new Response(response.body, {
    status: response.status,
    statusText: response.statusText,
    headers,
  });
}

function createMainWindow() {
  mainWindow = new BrowserWindow({
    width: 1440,
    height: 920,
    minWidth: 1080,
    minHeight: 720,
    title: "Cerul",
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
    width: 320,
    height: 260,
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
  if (!image.isEmpty() && process.platform === "darwin") {
    image.setTemplateImage(true);
  }
  tray = new Tray(image.isEmpty() ? nativeImage.createEmpty() : image.resize({ width: 18, height: 18 }));
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
  }
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
  if (app.isPackaged) {
    const binary = path.join(process.resourcesPath, "bin", executableName("cerul-api"));
    if (!fs.existsSync(binary)) {
      throw new Error(`packaged Cerul API binary is missing: ${binary}`);
    }
    apiProcess = spawn(binary, [], { env, stdio: "pipe" });
  } else {
    const devBinary = path.join(repoRoot(), "target", "debug", executableName("cerul-api"));
    if (!fs.existsSync(devBinary)) {
      buildDevApiBinary(devBinary, env);
    }
    apiProcess = spawn(devBinary, [], { cwd: repoRoot(), env, stdio: "pipe" });
  }

  ownsApiProcess = true;
  apiProcess.stdout.on("data", (chunk) => process.stdout.write(`[cerul-api] ${chunk}`));
  apiProcess.stderr.on("data", (chunk) => process.stderr.write(`[cerul-api] ${chunk}`));
  apiProcess.on("error", (error) => {
    console.error("failed to start Cerul local API", error);
  });
  apiProcess.on("exit", (code, signal) => {
    if (!isQuitting) {
      console.warn(`Cerul local API exited code=${code} signal=${signal}`);
    }
    apiProcess = null;
    ownsApiProcess = false;
  });

  await waitForApi(30_000);
}

function stopRustCore() {
  if (!apiProcess || !ownsApiProcess) {
    return;
  }
  apiProcess.kill("SIGTERM");
  apiProcess = null;
  ownsApiProcess = false;
}

function buildDevApiBinary(binary: string, env: NodeJS.ProcessEnv) {
  const jobs = devCargoBuildJobs(env);
  const attempts = devCargoBuildAttempts(env);
  const args = ["build", "-p", "cerul-api", "-j", jobs];
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    if (attempt > 1) {
      console.warn(`Retrying Cerul local API build (${attempt}/${attempts}) after transient Cargo failure.`);
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
      throw new Error(`failed to build Cerul local API binary (status ${status})`);
    }
    sleepSync(2_000);
  }
  if (!fs.existsSync(binary)) {
    throw new Error(`Cerul local API binary was not produced: ${binary}`);
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

async function waitForApi(timeoutMs: number) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    if (await apiIsHealthy(750)) {
      return;
    }
    await delay(250);
  }
  throw new Error(`Cerul local API did not become healthy at ${apiBaseUrl}`);
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
  return env;
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

function isRunnableBinary(binaryPath: string, probeArgs: string[]) {
  if (!fs.existsSync(binaryPath)) {
    return false;
  }
  const result = spawnSync(binaryPath, probeArgs, {
    stdio: "ignore",
    timeout: 8_000,
  });
  return result.status === 0;
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

function registerIpcHandlers() {
  ipcMain.handle("cerul:invoke", async (_event, command: string, args?: Record<string, unknown>) =>
    handleCommand(command, args ?? {}),
  );
  ipcMain.handle("cerul:open-dialog", async (_event, options) => {
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
  ipcMain.handle("cerul:check-update", async () => {
    return checkForGitHubReleaseUpdate();
  });
  ipcMain.handle("cerul:store-get", async (_event, storePath: string, key: string) => {
    return loadStore(storePath)[key];
  });
  ipcMain.handle("cerul:store-set", async (_event, storePath: string, key: string, value: unknown) => {
    loadStore(storePath)[key] = value;
    dirtyStores.add(storePath);
  });
  ipcMain.handle("cerul:store-save", async (_event, storePath: string) => saveStore(storePath));
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
  } catch {
    value = {};
  }
  stores.set(storePath, value);
  return value;
}

function saveStore(storePath: string) {
  if (!dirtyStores.has(storePath)) return;
  const file = storeFilePath(storePath);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, JSON.stringify(loadStore(storePath), null, 2));
  dirtyStores.delete(storePath);
}

function delay(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
