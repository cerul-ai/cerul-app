import { app, net, shell, type BrowserWindow } from "electron";
import type { AppUpdater } from "electron-updater";

const defaultUpdateRepository = "cerul-ai/cerul-app";

type GitHubRelease = {
  tag_name?: string;
  name?: string | null;
  html_url?: string;
  body?: string | null;
  draft?: boolean;
  prerelease?: boolean;
  published_at?: string | null;
};

export type DesktopReleaseNotes = {
  publishedAt?: string;
  sections: Array<{
    title?: string;
    items: string[];
  }>;
};

export type DesktopUpdateInfo = {
  version: string;
  url: string;
  name?: string;
  prerelease: boolean;
  publishedAt?: string;
  releaseNotes?: DesktopReleaseNotes;
};

// Drives the rail "Update" pill. `available` always works (GitHub-release
// detection, signing-independent); later phases only occur once releases ship
// signed + a latest-mac.yml that electron-updater can apply.
export type UpdaterState =
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

export type UpdaterProgress = {
  percent?: number;
  bytesPerSecond?: number;
  transferred?: number;
  total?: number;
};

export type UpdaterCheckOptions = {
  installWhenDownloaded?: boolean;
};

export type UpdaterControllerOptions = {
  checkForReleaseUpdate?: () => Promise<DesktopUpdateInfo | null>;
  clearPreparedUpdate: (version?: string) => void;
  getMainWindow: () => BrowserWindow | null;
  isPackaged?: () => boolean;
  resolveAutoUpdater?: () => AppUpdater | null;
  markInstallWhenPrepared: () => void;
  prepareDownloadedUpdateForRestart: (
    version: string,
    updater: AppUpdater,
    installWhenReady?: boolean,
  ) => void;
  installUpdate: (version?: string) => Promise<void>;
};

export type UpdaterController = {
  checkForUpdate: () => Promise<DesktopUpdateInfo | null>;
  clearInstallRequests: () => void;
  currentReleaseNotes: () => DesktopReleaseNotes | undefined;
  getAutoUpdater: () => AppUpdater | null;
  getState: () => UpdaterState;
  runCheck: (options?: UpdaterCheckOptions) => Promise<boolean>;
  setError: (error: unknown, version?: string) => void;
  setState: (next: UpdaterState) => void;
  startDownload: () => Promise<void>;
};

export function createUpdaterController(options: UpdaterControllerOptions): UpdaterController {
  let autoUpdaterInstance: AppUpdater | null = null;
  let autoUpdaterWired = false;
  let updateInstallRequested = false;
  let updaterCheckInstallRequested = false;
  let latestUpdaterState: UpdaterState = { phase: "idle" };
  const checkForReleaseUpdate = options.checkForReleaseUpdate ?? checkForGitHubReleaseUpdate;

  function isPackaged() {
    return options.isPackaged?.() ?? app.isPackaged;
  }

  function setState(next: UpdaterState) {
    latestUpdaterState = next;
    // The renderer also pulls the current state on mount (cerul:updater-get-state)
    // in case it subscribes after the first check emits.
    const mainWindow = options.getMainWindow();
    if (mainWindow && !mainWindow.isDestroyed()) {
      mainWindow.webContents.send("cerul:updater-event", next);
    }
  }

  function currentReleaseNotes(): DesktopReleaseNotes | undefined {
    return "releaseNotes" in latestUpdaterState ? latestUpdaterState.releaseNotes : undefined;
  }

  function setError(error: unknown, version?: string) {
    const message = error instanceof Error ? error.message : String(error);
    console.error("desktop updater error", error);
    setState({
      phase: "error",
      version,
      message,
      releaseUrl: releasesPageUrl(),
      releaseNotes: currentReleaseNotes(),
    });
  }

  type AvailableUpdaterState = Extract<UpdaterState, { phase: "available" }>;

  function newerReleasePageUpdateForVersion(version: string): AvailableUpdaterState | null {
    if (
      latestUpdaterState.phase !== "available" ||
      latestUpdaterState.canAutoInstall ||
      compareVersions(latestUpdaterState.version, version) <= 0
    ) {
      return null;
    }
    return latestUpdaterState;
  }

  function keepNewerReleasePageUpdate(version: string, source: string) {
    const newerUpdate = newerReleasePageUpdateForVersion(version);
    if (!newerUpdate) {
      return false;
    }
    updaterCheckInstallRequested = false;
    updateInstallRequested = false;
    console.warn(
      `electron-updater ${source} ignored stale update metadata version=${version}; newer GitHub release=${newerUpdate.version}`,
    );
    setState(newerUpdate);
    return true;
  }

  function startAutoUpdaterDownload(updater: AppUpdater, version: string) {
    void updater.downloadUpdate().catch((error) => {
      console.error("electron-updater auto download failed; release-page fallback active", error);
      updaterCheckInstallRequested = false;
      updateInstallRequested = false;
      options.clearPreparedUpdate(version);
      setState({
        phase: "error",
        version,
        message: error instanceof Error ? error.message : String(error),
        releaseUrl: releasesPageUrl(),
        releaseNotes: currentReleaseNotes(),
      });
    });
  }

  function getAutoUpdater(): AppUpdater | null {
    if (autoUpdaterInstance) {
      return autoUpdaterInstance;
    }
    try {
      if (options.resolveAutoUpdater) {
        autoUpdaterInstance = options.resolveAutoUpdater();
        return autoUpdaterInstance;
      }
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
    // Keep downloads behind our own version arbitration so stale generic-provider
    // metadata cannot replace a newer GitHub release-page update with an older
    // auto-installable build.
    updater.autoDownload = false;
    // On macOS, electron-updater emits its own update-downloaded event before
    // native Squirrel has necessarily finished its handoff. Keep the Squirrel
    // fetch/install tied to explicit quitAndInstall so a fallback app.quit cannot
    // strand a staged update.
    updater.autoInstallOnAppQuit = process.platform !== "darwin";
    updater.on("update-available", (info) => {
      const version = normalizeVersion(info.version);
      if (keepNewerReleasePageUpdate(version, "update-available")) {
        return;
      }
      options.clearPreparedUpdate();
      if (updaterCheckInstallRequested) {
        updateInstallRequested = true;
        updaterCheckInstallRequested = false;
      }
      setState(updateDownloadState(version, {}, currentReleaseNotes()));
      startAutoUpdaterDownload(updater, version);
    });
    updater.on("update-not-available", () => {
      updaterCheckInstallRequested = false;
      updateInstallRequested = false;
      options.clearPreparedUpdate();
      // A successful, definitive check must recover from an earlier failure.
      // Without this transition, an error pill survives for the lifetime of the
      // process even after the update server confirms that this build is current.
      if (latestUpdaterState.phase === "error") {
        setState({ phase: "idle" });
      }
    });
    updater.on("download-progress", (progress) => {
      const version =
        latestUpdaterState.phase === "available" || latestUpdaterState.phase === "downloading"
          ? latestUpdaterState.version
          : normalizeVersion(app.getVersion());
      setState(updateDownloadState(version, progress, currentReleaseNotes()));
    });
    updater.on("update-downloaded", (info) => {
      const version = normalizeVersion(info.version);
      if (keepNewerReleasePageUpdate(version, "update-downloaded")) {
        return;
      }
      const installWhenReady = updateInstallRequested || updaterCheckInstallRequested;
      updateInstallRequested = false;
      updaterCheckInstallRequested = false;
      options.prepareDownloadedUpdateForRestart(version, updater, installWhenReady);
    });
    updater.on("error", (error) => {
      // No latest-mac.yml, a signature mismatch on ad-hoc builds, or a network
      // failure. Degrade to the GitHub-release fallback so the pill still lets the
      // user grab the new version from the download page.
      console.error("electron-updater error", error);
      const fallbackUrl =
        latestUpdaterState.phase === "available" ? latestUpdaterState.releaseUrl : releasesPageUrl();
      updaterCheckInstallRequested = false;
      if (updateInstallRequested) {
        updateInstallRequested = false;
      }
      options.clearPreparedUpdate();
      // A provider check can fail while no update exists at all (for example,
      // Chromium reports ERR_NETWORK_CHANGED during Wi-Fi/VPN or wake routing
      // changes). Keep background probe failures quiet, and preserve a GitHub
      // release fallback if that independent probe already found a newer build.
      // Download/preparation/install failures remain user-visible below.
      if (
        latestUpdaterState.phase === "idle" ||
        (latestUpdaterState.phase === "available" && !latestUpdaterState.canAutoInstall)
      ) {
        console.warn("electron-updater provider check failed; keeping current update state", error);
        return;
      }
      setState({
        phase: "error",
        version:
          latestUpdaterState.phase === "available" ||
          latestUpdaterState.phase === "downloading" ||
          latestUpdaterState.phase === "preparing" ||
          latestUpdaterState.phase === "installing" ||
          latestUpdaterState.phase === "downloaded"
            ? latestUpdaterState.version
            : undefined,
        message: error instanceof Error ? error.message : String(error),
        releaseUrl: fallbackUrl,
        releaseNotes: currentReleaseNotes(),
      });
    });
  }

  // Signing-independent detection (GitHub releases API) that works on today's
  // ad-hoc builds. Drives the "available" pill; never clobbers an in-flight
  // download/installed state.
  // Returns false when the release probe could not reach a conclusion (network or
  // server error). Callers use this to avoid reporting a false "up to date" and to
  // retry soon instead of advancing the check throttle.
  async function refreshManualUpdateState(): Promise<boolean> {
    let info: DesktopUpdateInfo | null = null;
    try {
      info = await checkForReleaseUpdate();
    } catch (error) {
      console.error("github update check failed", error);
      return false;
    }
    if (info) {
      if (
        latestUpdaterState.phase === "idle" ||
        latestUpdaterState.phase === "available" ||
        latestUpdaterState.phase === "error"
      ) {
        setState({
          phase: "available",
          version: info.version,
          releaseUrl: info.url,
          canAutoInstall: false,
          releaseNotes: info.releaseNotes,
        });
      }
    } else if (
      latestUpdaterState.phase === "available" ||
      latestUpdaterState.phase === "error"
    ) {
      setState({ phase: "idle" });
    }
    return true;
  }

  // Resolves true when the check reached a definitive answer (update found or
  // confirmed up to date), false when it failed to reach the update server. The
  // IPC handler rejects on false so the renderer's automatic-check retry path and
  // the About page surface the failure instead of a misleading "up to date".
  async function runCheck(checkOptions: UpdaterCheckOptions = {}): Promise<boolean> {
    const installWhenDownloaded = checkOptions.installWhenDownloaded === true;

    // Dev demo hook: CERUL_FAKE_UPDATE=<version> renders the pill without a real
    // release so the flow is reviewable before signed releases exist.
    const fake = process.env.CERUL_FAKE_UPDATE;
    if (fake && !isPackaged()) {
      setState({
        phase: "available",
        version: normalizeVersion(fake),
        releaseUrl: releasesPageUrl(),
        canAutoInstall: false,
        releaseNotes: {
          publishedAt: new Date().toISOString(),
          sections: [
            {
              title: "Improved",
              items: [
                "Show release notes from the update button before opening the download page.",
                "Keep update status visible while the app checks, downloads, and prepares a restart.",
                "Use GitHub release notes generated by the existing release workflow.",
              ],
            },
            {
              title: "Fixed",
              items: ["Avoid showing an empty update card when release notes are missing."],
            },
          ],
        },
      });
      return true;
    }

    if (
      latestUpdaterState.phase === "downloading" ||
      latestUpdaterState.phase === "preparing" ||
      latestUpdaterState.phase === "downloaded" ||
      latestUpdaterState.phase === "installing"
    ) {
      if (installWhenDownloaded) {
        if (latestUpdaterState.phase === "downloading") {
          updateInstallRequested = true;
        } else if (latestUpdaterState.phase === "preparing") {
          options.markInstallWhenPrepared();
        } else if (latestUpdaterState.phase === "downloaded") {
          await options.installUpdate(latestUpdaterState.version);
        }
      }
      return true;
    }

    const githubOk = await refreshManualUpdateState();

    // Opportunistic in-place updater — dormant until releases ship signed +
    // notarized with a latest-mac.yml that Squirrel.Mac can apply. When that
    // lands, these events upgrade the pill from "open download page" to a
    // one-click download followed by an automatic restart-to-install.
    if (!isPackaged()) {
      return githubOk;
    }
    const updater = getAutoUpdater();
    if (!updater) {
      return githubOk;
    }
    try {
      wireAutoUpdater(updater);
      if (installWhenDownloaded) {
        updaterCheckInstallRequested = true;
      }
      await updater.checkForUpdates();
      return true;
    } catch (error) {
      if (installWhenDownloaded) {
        updaterCheckInstallRequested = false;
        updateInstallRequested = false;
      }
      console.error("electron-updater check failed; release-page fallback active", error);
      return githubOk;
    }
  }

  async function startDownload() {
    if (latestUpdaterState.phase !== "available") {
      return;
    }
    const { releaseNotes, releaseUrl, canAutoInstall, version } = latestUpdaterState;
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
      setState(updateDownloadState(version, {}, releaseNotes));
      await updater.downloadUpdate();
    } catch (error) {
      console.error("electron-updater download failed; opening release page", error);
      updateInstallRequested = false;
      setState({
        phase: "error",
        version,
        message: error instanceof Error ? error.message : String(error),
        releaseUrl,
        releaseNotes,
      });
    }
  }

  return {
    checkForUpdate: checkForReleaseUpdate,
    clearInstallRequests: () => {
      updaterCheckInstallRequested = false;
      updateInstallRequested = false;
    },
    currentReleaseNotes,
    getAutoUpdater,
    getState: () => latestUpdaterState,
    runCheck,
    setError,
    setState,
    startDownload,
  };
}

export async function checkForGitHubReleaseUpdate(): Promise<DesktopUpdateInfo | null> {
  const repository = updateRepository();
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
        releaseNotes: releaseNotesFromMarkdown(release.body, release.published_at),
      };
    }
  }
  return bestUpdate;
}

export function releasesPageUrl() {
  return `https://github.com/${updateRepository()}/releases`;
}

export function normalizeVersion(version: string) {
  return version.trim().replace(/^v/i, "");
}

export function isPrereleaseVersion(version: string) {
  return normalizeVersion(version).split("+", 1)[0].includes("-");
}

export function compareVersions(left: string, right: string) {
  const a = parseVersion(left);
  const b = parseVersion(right);
  for (let index = 0; index < 3; index += 1) {
    if (a.core[index] !== b.core[index]) {
      return a.core[index] > b.core[index] ? 1 : -1;
    }
  }
  return comparePrerelease(a.prerelease, b.prerelease);
}

function positiveFiniteNumber(value: unknown): number | undefined {
  const number = typeof value === "number" ? value : Number(value);
  return Number.isFinite(number) && number > 0 ? number : undefined;
}

function updateDownloadState(
  version: string,
  progress: UpdaterProgress = {},
  releaseNotes: DesktopReleaseNotes | undefined = undefined,
): UpdaterState {
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
    releaseNotes,
  };
}

function updateRepository() {
  return process.env.CERUL_UPDATE_REPOSITORY ?? defaultUpdateRepository;
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

function releaseNotesFromMarkdown(
  markdown: string | null | undefined,
  publishedAt: string | null | undefined,
): DesktopReleaseNotes | undefined {
  const sections = releaseNoteSections(markdown ?? "");
  if (sections.length === 0) {
    return undefined;
  }
  return {
    publishedAt: publishedAt ?? undefined,
    sections,
  };
}

function releaseNoteSections(markdown: string): DesktopReleaseNotes["sections"] {
  const mainBody = markdown.split(/\n---\n/, 1)[0] ?? "";
  const sections: DesktopReleaseNotes["sections"] = [];
  let current: { title?: string; items: string[] } = { items: [] };

  function pushCurrent() {
    if (current.items.length > 0) {
      sections.push({
        title: current.title,
        items: current.items.slice(0, 8),
      });
    }
  }

  for (const rawLine of mainBody.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("<!--")) {
      continue;
    }
    const heading = line.match(/^#{1,6}\s+(.+)$/);
    if (heading) {
      pushCurrent();
      current = { title: cleanReleaseNoteText(heading[1]), items: [] };
      continue;
    }
    const bullet = line.match(/^[-*]\s+(.+)$/);
    if (bullet) {
      const item = cleanReleaseNoteText(bullet[1]);
      if (item) {
        current.items.push(item);
      }
      continue;
    }
    if (sections.length === 0 && current.items.length === 0) {
      const item = cleanReleaseNoteText(line);
      if (item && !/^download:/i.test(item) && !/^github:/i.test(item)) {
        current.items.push(item);
      }
    }
  }
  pushCurrent();
  return sections.slice(0, 4);
}

function cleanReleaseNoteText(value: string) {
  return value
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/[*_`~]/g, "")
    .replace(/\s+/g, " ")
    .trim();
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
