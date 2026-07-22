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
  releaseNotes?: DesktopReleaseNotes;
} | null;

export type DesktopReleaseNotes = {
  publishedAt?: string;
  sections: Array<{
    title?: string;
    items: string[];
  }>;
};

// Drives the rail "Update" pill. Mirrors UpdaterState in the electron shell.
// `available` works on any build (GitHub-release detection); later phases only
// occur once releases ship signed + a latest-mac.yml.
export type DesktopUpdaterState =
  | { phase: "idle" }
  | {
      phase: "available";
      version: string;
      releaseUrl: string;
      canAutoInstall: boolean;
      releaseNotes?: DesktopReleaseNotes;
    }
  | {
      phase: "downloading";
      version: string;
      percent: number;
      bytesPerSecond?: number;
      etaSeconds?: number;
      transferredBytes?: number;
      totalBytes?: number;
      releaseNotes?: DesktopReleaseNotes;
    }
  | { phase: "preparing"; version: string; releaseNotes?: DesktopReleaseNotes }
  | { phase: "installing"; version: string; releaseNotes?: DesktopReleaseNotes }
  | { phase: "downloaded"; version: string; releaseNotes?: DesktopReleaseNotes }
  | {
      phase: "error";
      version?: string;
      message: string;
      releaseUrl: string;
      releaseNotes?: DesktopReleaseNotes;
    };

export type DesktopStore = {
  get<T>(key: string): Promise<T | undefined>;
  set<T>(key: string, value: T): Promise<void>;
  save(): Promise<void>;
};

export type DesktopUpdaterCheckOptions = {
  installWhenDownloaded?: boolean;
};

export type DesktopMenuCommand =
  | { type: "new_source"; triggeredByAccelerator: boolean }
  | { type: "find"; triggeredByAccelerator: boolean };

export type AgentConnectTargetId = "claude-code" | "codex";

export type AgentSkillFilePayload = {
  path: string;
  content: string;
};

export type AgentConnectSkillState = {
  installed: boolean;
  version?: string;
};

export type AgentConnectDetection = {
  id: AgentConnectTargetId;
  detected: boolean;
  skillsDir: string;
  skill: AgentConnectSkillState;
};

export type AgentConnectInstallPayload = {
  target?: AgentConnectTargetId;
  baseDir?: string;
  files: AgentSkillFilePayload[];
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
  onMenuCommand(callback: (command: DesktopMenuCommand) => void): () => void;
  storeGet<T>(path: string, key: string): Promise<T | undefined>;
  storeSet<T>(path: string, key: string, value: T): Promise<void>;
  storeSave(path: string): Promise<void>;
  secureTokenGet(key: string): Promise<string | undefined>;
  secureTokenSet(key: string, value: string | null): Promise<void>;
  startOAuth(provider: "google" | "github"): Promise<void>;
  reportRendererError(payload: Record<string, unknown>): Promise<void>;
  agentConnectDetect(): Promise<AgentConnectDetection[]>;
  agentConnectInstall(payload: AgentConnectInstallPayload): Promise<AgentConnectSkillState>;
  agentConnectUninstall(payload: {
    target?: AgentConnectTargetId;
    baseDir?: string;
  }): Promise<AgentConnectSkillState>;
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

export function subscribeDesktopMenuCommand(
  callback: (command: DesktopMenuCommand) => void,
): () => void {
  if (window.cerulDesktop) {
    return window.cerulDesktop.onMenuCommand((command) => {
      if (
        command &&
        typeof command === "object" &&
        (command.type === "new_source" || command.type === "find")
      ) {
        callback(command);
      }
    });
  }
  return () => undefined;
}

export async function validateDesktopApplicationMenuShortcut(accelerator: string): Promise<void> {
  if (window.cerulDesktop) {
    await window.cerulDesktop.invoke("validate_application_menu_shortcut", { accelerator });
  }
}

export async function syncDesktopApplicationMenu(): Promise<void> {
  if (window.cerulDesktop) {
    await window.cerulDesktop.invoke("sync_application_menu");
  }
}

export async function detectAgentConnectTargets(): Promise<AgentConnectDetection[] | null> {
  if (window.cerulDesktop?.agentConnectDetect) {
    return window.cerulDesktop.agentConnectDetect();
  }
  return null;
}

export async function installAgentConnectSkill(
  payload: AgentConnectInstallPayload,
): Promise<AgentConnectSkillState> {
  if (window.cerulDesktop?.agentConnectInstall) {
    return window.cerulDesktop.agentConnectInstall(payload);
  }
  throw new Error("skill install is only available in the desktop app");
}

export async function uninstallAgentConnectSkill(payload: {
  target?: AgentConnectTargetId;
  baseDir?: string;
}): Promise<AgentConnectSkillState> {
  if (window.cerulDesktop?.agentConnectUninstall) {
    return window.cerulDesktop.agentConnectUninstall(payload);
  }
  throw new Error("skill uninstall is only available in the desktop app");
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
