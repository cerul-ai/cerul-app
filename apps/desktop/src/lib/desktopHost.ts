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

export type DesktopStore = {
  get<T>(key: string): Promise<T | undefined>;
  set<T>(key: string, value: T): Promise<void>;
  save(): Promise<void>;
};

type ElectronDesktopHost = {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  openDialog(options: OpenDialogOptions): Promise<string | string[] | null>;
  checkForUpdate(): Promise<DesktopUpdate>;
  storeGet<T>(path: string, key: string): Promise<T | undefined>;
  storeSet<T>(path: string, key: string, value: T): Promise<void>;
  storeSave(path: string): Promise<void>;
  secureTokenGet(key: string): Promise<string | undefined>;
  secureTokenSet(key: string, value: string | null): Promise<void>;
  startOAuth(provider: "google" | "github"): Promise<void>;
};

declare global {
  interface Window {
    cerulDesktop?: ElectronDesktopHost;
  }
}

export function hasDesktopHost() {
  return Boolean(window.cerulDesktop);
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
