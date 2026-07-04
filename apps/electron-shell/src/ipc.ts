import { app, dialog, ipcMain } from "electron";
import { appScheme } from "./protocol";
import type { UpdaterCheckOptions } from "./updater";

export type OAuthProvider = "google" | "github";

export type RendererDiagnostic = {
  window?: string;
  kind: string;
  message?: string;
  stack?: string;
  source?: string;
  line?: number;
  column?: number;
  componentStack?: string;
  href?: string;
  userAgent?: string;
  details?: Record<string, unknown>;
};

export type IpcHandlers = {
  invokeCommand: (command: string, args: Record<string, unknown>) => unknown;
  getAppVersion: () => string;
  checkForUpdate: () => Promise<unknown>;
  runUpdateCheck: (options?: UpdaterCheckOptions) => Promise<boolean>;
  getUpdaterState: () => unknown;
  collectUpdaterDiagnostics: () => unknown;
  startUpdateDownload: () => Promise<void>;
  installUpdate: () => Promise<void>;
  loadStore: (storePath: string) => Record<string, unknown>;
  markStoreDirty: (storePath: string) => void;
  saveStore: (storePath: string) => void;
  getSecureToken: (key: string) => string | null | undefined;
  setSecureToken: (key: string, value: string | null) => void;
  startOAuthLogin: (provider: OAuthProvider) => Promise<void>;
  writeRendererDiagnostic: (entry: RendererDiagnostic) => void;
};

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

export function registerIpcHandlers(handlers: IpcHandlers) {
  ipcMain.handle("cerul:invoke", async (event, command: string, args?: Record<string, unknown>) => {
    assertTrustedIpcSender(event);
    return handlers.invokeCommand(command, args ?? {});
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
    return handlers.getAppVersion();
  });
  ipcMain.handle("cerul:check-update", async (event) => {
    assertTrustedIpcSender(event);
    return handlers.checkForUpdate();
  });
  ipcMain.handle("cerul:updater-check", async (event, options?: UpdaterCheckOptions) => {
    assertTrustedIpcSender(event);
    const reached = await handlers.runUpdateCheck(options);
    if (!reached) {
      // Reject so the renderer treats this as a transient failure (retry soon,
      // don't advance the throttle) rather than a successful "no update" result.
      throw new Error("update-check-failed");
    }
    return handlers.getUpdaterState();
  });
  ipcMain.handle("cerul:updater-get-state", async (event) => {
    assertTrustedIpcSender(event);
    return handlers.getUpdaterState();
  });
  ipcMain.handle("cerul:updater-diagnostics", async (event) => {
    assertTrustedIpcSender(event);
    return handlers.collectUpdaterDiagnostics();
  });
  ipcMain.handle("cerul:updater-download", async (event) => {
    assertTrustedIpcSender(event);
    await handlers.startUpdateDownload();
    return handlers.getUpdaterState();
  });
  ipcMain.handle("cerul:updater-install", async (event) => {
    assertTrustedIpcSender(event);
    await handlers.installUpdate();
  });
  ipcMain.handle("cerul:store-get", async (event, storePath: string, key: string) => {
    assertTrustedIpcSender(event);
    return handlers.loadStore(storePath)[key];
  });
  ipcMain.handle("cerul:store-set", async (event, storePath: string, key: string, value: unknown) => {
    assertTrustedIpcSender(event);
    handlers.loadStore(storePath)[key] = value;
    handlers.markStoreDirty(storePath);
  });
  ipcMain.handle("cerul:store-save", async (event, storePath: string) => {
    assertTrustedIpcSender(event);
    handlers.saveStore(storePath);
  });
  ipcMain.handle("cerul:secure-token-get", async (event, key: string) => {
    assertTrustedIpcSender(event);
    return handlers.getSecureToken(key);
  });
  ipcMain.handle("cerul:secure-token-set", async (event, key: string, value: string | null) => {
    assertTrustedIpcSender(event);
    handlers.setSecureToken(key, value);
  });
  ipcMain.handle("cerul:oauth-start", async (event, provider: OAuthProvider) => {
    assertTrustedIpcSender(event);
    await handlers.startOAuthLogin(provider);
  });
  ipcMain.handle("cerul:renderer-error", async (event, payload: RendererDiagnostic) => {
    assertTrustedIpcSender(event);
    handlers.writeRendererDiagnostic({
      ...payload,
      window: payload.window ?? "renderer",
    });
  });
}
