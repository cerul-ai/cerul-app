import { BrowserWindow } from "electron";
import { secureDesktopWindow, secureRendererWebPreferences } from "./window-security";

export type MainWindowOptions = {
  url: string;
  preloadPath: string;
  iconPath: string;
  savedBounds: Partial<Electron.Rectangle>;
  persistBounds: () => void;
  isQuitting: () => boolean;
  shouldCloseToTray: () => Promise<boolean>;
  quitFromClose: () => void;
  shouldShowAtLaunch: () => boolean;
  onDidFinishLoad: () => void;
  onClosed: () => void;
  wireDiagnostics: (window: BrowserWindow, reloadUrl: string) => void;
};

export type OverlayWindowOptions = {
  url: string;
  width: number;
  height: number;
  preloadPath: string;
  iconPath: string;
  onClosed: () => void;
};

export type MenuBarWindowOptions = {
  url: string;
  preloadPath: string;
  iconPath: string;
  onClosed: () => void;
};

export function createMainBrowserWindow(options: MainWindowOptions) {
  const window = new BrowserWindow({
    width: options.savedBounds.width ?? 1440,
    height: options.savedBounds.height ?? 920,
    x: options.savedBounds.x,
    y: options.savedBounds.y,
    minWidth: 1080,
    minHeight: 720,
    title: "Cerul",
    ...(process.platform === "darwin"
      ? { titleBarStyle: "hiddenInset" as const, trafficLightPosition: { x: 19, y: 13 } }
      : {}),
    icon: options.iconPath,
    show: false,
    webPreferences: secureRendererWebPreferences(options.preloadPath),
  });

  secureDesktopWindow(window);
  options.wireDiagnostics(window, options.url);
  window.on("close", () => options.persistBounds());
  window.on("hide", () => options.persistBounds());
  window.on("close", (event) => {
    if (options.isQuitting()) {
      return;
    }
    event.preventDefault();
    void options.shouldCloseToTray().then((enabled) => {
      if (enabled) {
        if (!window.isDestroyed()) {
          window.hide();
        }
        return;
      }
      options.quitFromClose();
    });
  });
  window.once("ready-to-show", () => {
    if (options.shouldShowAtLaunch()) {
      window.show();
      window.focus();
    }
  });
  window.webContents.once("did-finish-load", options.onDidFinishLoad);
  window.on("closed", options.onClosed);
  void window.loadURL(options.url);
  return window;
}

export function createOverlayBrowserWindow(options: OverlayWindowOptions) {
  const isMac = process.platform === "darwin";
  const window = new BrowserWindow({
    width: options.width,
    height: options.height,
    title: "",
    icon: options.iconPath,
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
    webPreferences: secureRendererWebPreferences(options.preloadPath),
  });

  secureDesktopWindow(window);
  // Dismiss like a normal spotlight: when the overlay loses focus, hide it.
  window.on("blur", () => {
    window.hide();
  });
  window.on("closed", options.onClosed);
  window.webContents.once("did-finish-load", () => {
    console.log("cerul_electron_overlay_window_loaded");
  });
  window.webContents.on("did-fail-load", (_event, code, description, url) => {
    console.error(`Cerul overlay window failed to load code=${code} url=${url}: ${description}`);
  });
  void window.loadURL(options.url);
  return window;
}

export function createMenuBarBrowserWindow(options: MenuBarWindowOptions) {
  const isMac = process.platform === "darwin";
  const window = new BrowserWindow({
    width: 332,
    height: 312,
    title: "Cerul",
    icon: options.iconPath,
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
    webPreferences: secureRendererWebPreferences(options.preloadPath),
  });

  secureDesktopWindow(window);
  window.on("blur", () => {
    window.hide();
  });
  window.on("closed", options.onClosed);
  window.webContents.once("did-finish-load", () => {
    console.log("cerul_electron_menubar_window_loaded");
  });
  window.webContents.on("did-fail-load", (_event, code, description, url) => {
    console.error(`Cerul menu bar window failed to load code=${code} url=${url}: ${description}`);
  });
  void window.loadURL(options.url);
  return window;
}
