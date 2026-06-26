export type OpenDialogOptions = {
  directory?: boolean;
  multiple?: boolean;
  filters?: Array<{ name: string; extensions: string[] }>;
};

export type DesktopUpdate = {
  version: string;
  url: string;
  name?: string;
  prerelease?: boolean;
  publishedAt?: string;
} | null;

// Drives the rail "Update" pill. Mirrors UpdaterState in the electron shell.
// `available` works on any build (GitHub-release detection); later phases only
// occur once releases ship signed + a latest-mac.yml.
export type DesktopUpdaterState =
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
  | { phase: "preparing"; version: string }
  | { phase: "installing"; version: string }
  | { phase: "downloaded"; version: string }
  | { phase: "error"; version?: string; message: string; releaseUrl: string };

export type DesktopStore = {
  get<T>(key: string): Promise<T | undefined>;
  set<T>(key: string, value: T): Promise<void>;
  save(): Promise<void>;
};

export type DesktopUpdaterCheckOptions = {
  installWhenDownloaded?: boolean;
};

type ElectronDesktopHost = {
  apiBaseUrl?: string;
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  openDialog(options: OpenDialogOptions): Promise<string | string[] | null>;
  appVersion(): Promise<string>;
  checkForUpdate(): Promise<DesktopUpdate>;
  updaterCheck(options?: DesktopUpdaterCheckOptions): Promise<DesktopUpdaterState>;
  updaterGetState(): Promise<DesktopUpdaterState>;
  updaterDiagnostics(): Promise<string>;
  updaterDownload(): Promise<DesktopUpdaterState>;
  updaterInstall(): Promise<void>;
  onUpdaterEvent(callback: (state: DesktopUpdaterState) => void): () => void;
  storeGet<T>(path: string, key: string): Promise<T | undefined>;
  storeSet<T>(path: string, key: string, value: T): Promise<void>;
  storeSave(path: string): Promise<void>;
  secureTokenGet(key: string): Promise<string | undefined>;
  secureTokenSet(key: string, value: string | null): Promise<void>;
  startOAuth(provider: "google" | "github"): Promise<void>;
  reportRendererError(payload: Record<string, unknown>): Promise<void>;
};

declare global {
  interface Window {
    cerulDesktop?: ElectronDesktopHost;
  }
}

export function hasDesktopHost() {
  return Boolean(window.cerulDesktop);
}

export function localApiBaseUrl() {
  return window.cerulDesktop?.apiBaseUrl ?? "http://127.0.0.1:23785";
}

export async function invokeHostCommand<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (window.cerulDesktop) {
    return window.cerulDesktop.invoke<T>(command, args);
  }
  throw new Error(`desktop command is unavailable outside the desktop shell: ${command}`);
}

export async function openDialog(
  options: OpenDialogOptions,
): Promise<string | string[] | null> {
  if (window.cerulDesktop) {
    return window.cerulDesktop.openDialog(options);
  }
  return null;
}

export async function checkForDesktopUpdate(): Promise<DesktopUpdate> {
  if (window.cerulDesktop) {
    return window.cerulDesktop.checkForUpdate();
  }
  return null;
}

export async function getDesktopAppVersion(): Promise<string | null> {
  if (window.cerulDesktop) {
    return window.cerulDesktop.appVersion();
  }
  return null;
}

export async function runDesktopUpdaterCheck(
  options?: DesktopUpdaterCheckOptions,
): Promise<DesktopUpdaterState> {
  if (window.cerulDesktop) {
    return window.cerulDesktop.updaterCheck(options);
  }
  return { phase: "idle" };
}

export async function getDesktopUpdaterState(): Promise<DesktopUpdaterState> {
  if (window.cerulDesktop) {
    return window.cerulDesktop.updaterGetState();
  }
  return { phase: "idle" };
}

export async function getDesktopUpdaterDiagnostics(): Promise<string | null> {
  if (window.cerulDesktop) {
    return window.cerulDesktop.updaterDiagnostics();
  }
  return null;
}

export async function downloadDesktopUpdate(): Promise<DesktopUpdaterState> {
  if (window.cerulDesktop) {
    return window.cerulDesktop.updaterDownload();
  }
  return { phase: "idle" };
}

export async function installDesktopUpdate(): Promise<void> {
  await window.cerulDesktop?.updaterInstall();
}

export function subscribeDesktopUpdater(
  callback: (state: DesktopUpdaterState) => void,
): () => void {
  if (window.cerulDesktop) {
    return window.cerulDesktop.onUpdaterEvent(callback);
  }
  return () => undefined;
}

export async function loadDesktopStore(path: string): Promise<DesktopStore | null> {
  if (window.cerulDesktop) {
    return {
      get: (key) => window.cerulDesktop!.storeGet(path, key),
      set: (key, value) => window.cerulDesktop!.storeSet(path, key, value),
      save: () => window.cerulDesktop!.storeSave(path),
    };
  }
  return null;
}

export async function getSecureToken(key: string): Promise<string | undefined> {
  return window.cerulDesktop?.secureTokenGet(key);
}

export async function setSecureToken(key: string, value: string | null): Promise<void> {
  await window.cerulDesktop?.secureTokenSet(key, value);
}

export async function startDesktopOAuth(provider: "google" | "github"): Promise<boolean> {
  if (!window.cerulDesktop) {
    return false;
  }
  await window.cerulDesktop.startOAuth(provider);
  return true;
}

export async function reportRendererError(payload: Record<string, unknown>): Promise<void> {
  await window.cerulDesktop?.reportRendererError(payload);
}
