import { shell, type BrowserWindow, type BrowserWindowConstructorOptions } from "electron";
import { isAppUrl, isExternalUrl } from "./protocol";

type SecureWebPreferences = NonNullable<BrowserWindowConstructorOptions["webPreferences"]>;

export function secureRendererWebPreferences(preload: string): SecureWebPreferences {
  const webPreferences: SecureWebPreferences = {
    preload,
    contextIsolation: true,
    nodeIntegration: false,
    sandbox: true,
  };
  assertSecureRendererWebPreferences(webPreferences);
  return webPreferences;
}

export function assertSecureRendererWebPreferences(webPreferences: SecureWebPreferences) {
  const violations = [
    webPreferences.contextIsolation === true ? null : "contextIsolation must be true",
    webPreferences.nodeIntegration === false ? null : "nodeIntegration must be false",
    webPreferences.sandbox === true ? null : "sandbox must be true",
  ].filter((violation): violation is string => violation !== null);

  if (violations.length > 0) {
    throw new Error(`Unsafe BrowserWindow webPreferences: ${violations.join("; ")}`);
  }
}

export function secureDesktopWindow(window: BrowserWindow) {
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
