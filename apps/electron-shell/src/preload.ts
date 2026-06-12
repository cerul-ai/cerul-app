import { contextBridge, ipcRenderer } from "electron";

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
  storeGet: <T>(path: string, key: string) =>
    ipcRenderer.invoke("cerul:store-get", path, key) as Promise<T | undefined>,
  storeSet: <T>(path: string, key: string, value: T) =>
    ipcRenderer.invoke("cerul:store-set", path, key, value),
  storeSave: (path: string) => ipcRenderer.invoke("cerul:store-save", path),
  secureTokenGet: (key: string) => ipcRenderer.invoke("cerul:secure-token-get", key),
  secureTokenSet: (key: string, value: string | null) => ipcRenderer.invoke("cerul:secure-token-set", key, value),
});
