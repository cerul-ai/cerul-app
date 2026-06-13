import { contextBridge, ipcRenderer, type IpcRendererEvent } from "electron";

type OpenDialogOptions = {
  directory?: boolean;
  multiple?: boolean;
  filters?: Array<{ name: string; extensions: string[] }>;
};

contextBridge.exposeInMainWorld("cerulDesktop", {
  invoke: <T>(command: string, args?: Record<string, unknown>) =>
    ipcRenderer.invoke("cerul:invoke", command, args) as Promise<T>,
  openDialog: (options: OpenDialogOptions) => ipcRenderer.invoke("cerul:open-dialog", options),
  checkForUpdate: () => ipcRenderer.invoke("cerul:check-update"),
  updaterCheck: () => ipcRenderer.invoke("cerul:updater-check"),
  updaterGetState: () => ipcRenderer.invoke("cerul:updater-get-state"),
  updaterDownload: () => ipcRenderer.invoke("cerul:updater-download"),
  updaterInstall: () => ipcRenderer.invoke("cerul:updater-install"),
  onUpdaterEvent: (callback: (state: unknown) => void) => {
    const listener = (_event: IpcRendererEvent, state: unknown) => callback(state);
    ipcRenderer.on("cerul:updater-event", listener);
    return () => ipcRenderer.removeListener("cerul:updater-event", listener);
  },
  storeGet: <T>(path: string, key: string) =>
    ipcRenderer.invoke("cerul:store-get", path, key) as Promise<T | undefined>,
  storeSet: <T>(path: string, key: string, value: T) =>
    ipcRenderer.invoke("cerul:store-set", path, key, value),
  storeSave: (path: string) => ipcRenderer.invoke("cerul:store-save", path),
  secureTokenGet: (key: string) => ipcRenderer.invoke("cerul:secure-token-get", key),
  secureTokenSet: (key: string, value: string | null) => ipcRenderer.invoke("cerul:secure-token-set", key, value),
  startOAuth: (provider: "google" | "github") => ipcRenderer.invoke("cerul:oauth-start", provider),
});
