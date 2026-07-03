// Cerul Desktop — main application shell.
//
// NOTE on size: this file still hosts every screen, dialog, and helper.
// Splitting it into smaller modules is a tracked follow-up. Phase A
// of that split is done (formatters and settings helpers moved into
// ./lib/formatters.ts and ./lib/settings-helpers.ts). The remaining
// phases are tracked in this comment so the next contributor can pick
// up cleanly:
//
//   Phase B — done: leaf components live in ./components/
//     InlineNotice, EmptyState, Metric, CoreBanner, ResultCard,
//     ItemCard, ItemModalityIcon, DetailIssuePanel, SettingsQuietNotice.
//   Phase C — extract screens into ./screens/
//     Done: Onboarding, SourcesScreen, MomentsScreen, ResultsScreen,
//     HomeScreen, LibraryScreen, ResultDetail, ItemDetail, SettingsScreen.
//     Remaining: none for Phase C.
//   Phase D — done: dialogs live in ./dialogs/
//     ConfirmDialog, JobsSheet, AddSourceDialog (+ tabs).
//   Phase E — done: mappers/helpers live in ./lib/
//     items.ts, sources.ts, jobs.ts, results.ts, route.ts.
//
// Each phase should land as its own PR so the diff stays reviewable.

import {
  AlertTriangle,
  ArrowRight,
  Check,
  ChevronDown,
  ArrowLeft,
  ChevronRight,
  Clock,
  Command,
  Copy,
  Cloud,
  Cpu,
  Database,
  Download,
  ExternalLink,
  FileAudio,
  FileVideo,
  Folder,
  HardDrive,
  Info,
  Library,
  ListChecks,
  Loader2,
  Lock,
  MoreHorizontal,
  Pause,
  Play,
  Plus,
  Podcast,
  RefreshCcw,
  ReceiptText,
  Search,
  Settings,
  ShieldCheck,
  SlidersHorizontal,
  Sparkles,
  Star,
  Trash2,
  Video,
  Wrench,
  Wallet,
  X,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useCallback, useEffect, useRef, useState, useMemo } from "react";
import type { FormEvent, ReactNode } from "react";
import * as api from "./lib/api";
import { useAuthStore } from "./lib/cloud/authStore";
import { appLocaleTag, LangProvider, useLang, useT, type TFunction } from "./lib/i18n";
import {
  errorMessage,
  formatDuration,
  formatSpeed,
  uniqueStrings,
  formatHotkeyLabel,
} from "./lib/formatters";
import {
  resolveThemePreference,
  settingBoolean,
  settingString,
} from "./lib/settings-helpers";
import {
  EmptyState,
} from "./components/leaf";
import { CoreStatusToast, useCoreStatus } from "./components/core-banner";
import { SourceRow } from "./components/source-row";
import { SourcePreview } from "./components/source-preview";
import {
  AccessibilityPermissionCallout,
  OnboardingFolderPicker,
  OnboardingYoutubePicker,
} from "./components/onboarding-pickers";
import {
  addSourceDisabled,
  uniqueYoutubeChannels,
  validateHttpUrl,
  waitForValidationFrame,
  youtubeChannelFromUrl,
} from "./lib/validation";
import { ConfirmDialog } from "./dialogs/confirm-dialog";
import { JobsSheet } from "./dialogs/jobs-sheet";
import { HomeScreen } from "./screens/home";
import { LibraryScreen } from "./screens/library";
import { MomentsScreen } from "./screens/moments";
import { ResultDetail } from "./screens/result-detail";
import { ItemDetail } from "./screens/item-detail";
import { SettingsScreen } from "./screens/settings";
import { ResultsScreen } from "./screens/results";
import { LocalModelConsent } from "./components/local-model-consent";
import { useLocalModelConsent } from "./lib/use-local-model-consent";
import { AddSourceDialog } from "./dialogs/add-source-dialog";
import { SourcesScreen } from "./screens/sources";
import { Onboarding } from "./screens/onboarding";
import { BrandLogo, BrandMark } from "./components/brand";
import { AccountDialogController, AccountRailButton } from "./components/account-sidebar";
import type {
  ApiStatus,
  AppData,
  ConfirmOptions,
  ConfirmRequest,
  DaemonInstallResult,
  DaemonStatus,
  Item,
  ItemSourceKind,
  ItemStatus,
  OnboardingYoutubeChannel,
  RequestConfirm,
  Result,
  RouteState,
  Source,
  SourceStatus,
  ValidationState,
  ValidationStatus,
  View,
} from "./lib/types";
import {
  isActiveJob,
  itemOriginalUrl,
  itemProgressLabel,
  itemSourceKind,
  itemSourceLabel,
  itemStatus,
  latestActiveJobForItem,
  mapItemRecord,
  normalizeJobProgress,
} from "./lib/items";
import { coarseStepKey } from "./lib/jobs";
import {
  mapSourceRecord,
  sourceName,
  sourceStatus,
  sourceType,
} from "./lib/sources";
import {
  mapSearchResults,
} from "./lib/results";
import { readRouteState, routeHash } from "./lib/route";
import { recordLastOpened } from "./lib/last-opened";
import {
  loadPersistedUiState,
  persistLastRoute,
  persistOnboardingCompleted,
  persistFirstRunActive,
} from "./lib/uiStore";
import type { PersistedRoute } from "./lib/uiStore";
import {
  checkForDesktopUpdate,
  downloadDesktopUpdate,
  getDesktopAppVersion,
  getDesktopUpdaterDiagnostics,
  getDesktopUpdaterState,
  hasDesktopHost,
  installDesktopUpdate,
  invokeHostCommand,
  openDialog,
  runDesktopUpdaterCheck,
  subscribeDesktopMenuCommand,
  subscribeDesktopUpdater,
  syncDesktopApplicationMenu,
  validateDesktopApplicationMenuShortcut,
} from "./lib/desktopHost";
import type { DesktopReleaseNotes, DesktopUpdate, DesktopUpdaterState } from "./lib/desktopHost";

type VisibleDesktopUpdaterState = Exclude<DesktopUpdaterState, { phase: "idle" }>;

// Top-level navigation. Sub-pages (`result-detail`, `item-detail`) are reached
// by clicking a search result or library item, not from the sidebar.
// `onboarding` auto-opens on a fresh/cleared install (no completed-onboarding
// flag) and can be re-run later via Settings → "Re-run onboarding"; it is not a
// permanent destination.
// All valid View ids — broader than the sidebar so persisted routes for
// sub-pages (result-detail, item-detail) and onboarding still rehydrate.
const viewIds: View[] = [
  "home",
  "results",
  "result-detail",
  "library",
  "moments",
  "item-detail",
  "sources",
  "settings",
  "onboarding",
];

// Mapping from sub-pages to their sidebar parent so the sidebar still
// highlights the right top-level item when a sub-page is active.
const sidebarParentFor: Partial<Record<View, View>> = {
  "results": "home",
  "result-detail": "home",
  "item-detail": "library",
};
const NEW_SOURCE_DEFAULT_HOTKEY = /mac/i.test(typeof navigator !== "undefined" ? navigator.platform : "")
  ? "Cmd+N"
  : "Ctrl+N";
const OPEN_SETTINGS_DEFAULT_HOTKEY = /mac/i.test(typeof navigator !== "undefined" ? navigator.platform : "")
  ? "Cmd+,"
  : "Ctrl+,";
const CLOSE_WINDOW_DEFAULT_HOTKEY = /mac/i.test(typeof navigator !== "undefined" ? navigator.platform : "")
  ? "Cmd+W"
  : "Ctrl+W";
const recentSearchesStorageKey = "cerul.recentSearches.v1";
const lastAutomaticUpdateCheckStorageKey = "cerul.updater.lastAutomaticCheckAt.v1";
const automaticUpdateCheckIntervalMs = 6 * 60 * 60 * 1000;
const automaticUpdateStartupDelayRangeMs = [30_000, 90_000] as const;
const automaticUpdateResumeDelayRangeMs = [10_000, 60_000] as const;
const automaticUpdateWakeProbeIntervalMs = 60_000;
const automaticUpdateWakeGapMs = 5 * 60 * 1000;
const automaticUpdateOfflineRetryMs = 15 * 60 * 1000;
const manualUpdateCheckCooldownMs = 30_000;

function hasOpenModalSurface() {
  // Every transient surface must be reachable from this selector, otherwise
  // page-level Escape handlers fire underneath it (e.g. detail "back").
  return Boolean(
    document.querySelector(".scrim, .account-pop, .menu, .model-combobox__pop, [role='dialog']"),
  );
}

function isEditableTarget(target: EventTarget | Element | null) {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable ||
      target.tagName === "INPUT" ||
      target.tagName === "TEXTAREA" ||
      target.tagName === "SELECT")
  );
}

function shouldIgnoreNewSourceShortcut(target: EventTarget | Element | null = document.activeElement) {
  return hasOpenModalSurface() || isEditableTarget(target);
}

function normalizeKeyToken(key: string): string {
  if (key === " ") return "Space";
  if (key === "+") return "Plus";
  if (key.length === 1) return key.toUpperCase();
  return key.replace(/^Arrow/, "");
}

function acceleratorMatchesEvent(accelerator: string, event: globalThis.KeyboardEvent): boolean {
  const parts = accelerator.split("+").map((part) => part.trim());
  const key = parts[parts.length - 1];
  const mods = new Set(parts.slice(0, -1));
  if ((mods.has("Cmd") || mods.has("Command")) !== event.metaKey) return false;
  if (mods.has("Ctrl") !== event.ctrlKey) return false;
  if (mods.has("Alt") !== event.altKey) return false;
  if (mods.has("Shift") !== event.shiftKey) return false;
  return normalizeKeyToken(event.key) === key;
}

async function readDaemonStatus() {
  if (!hasDesktopHost()) {
    return null;
  }
  try {
    return await invokeHostCommand<DaemonStatus>("daemon_status");
  } catch (error) {
    console.warn("failed to read Cerul daemon status", error);
    return null;
  }
}

async function installDaemon() {
  return invokeHostCommand<DaemonInstallResult>("install_daemon");
}

async function uninstallDaemon() {
  return invokeHostCommand<DaemonInstallResult>("uninstall_daemon");
}

function openAccessibilitySettings() {
  void invokeHostCommand("open_accessibility_settings").catch((error) => {
    console.warn("failed to open Accessibility settings", error);
  });
}

async function revealDataDirectory() {
  await invokeHostCommand("reveal_data_directory");
}

async function revealLogsDirectory() {
  await invokeHostCommand("reveal_logs_directory");
}

async function revealSourcePath(path: string) {
  await invokeHostCommand("reveal_source_path", { path });
}

type StorageLocations = {
  data_dir: string;
  cache_dir: string;
  models_dir: string;
  index_dir: string;
};

async function readStorageLocations() {
  return invokeHostCommand<StorageLocations>("storage_locations");
}

async function clearCacheDirectory() {
  return invokeHostCommand<{ cache_dir: string; bytes_removed: number }>("clear_cache");
}

async function resetLocalDataAndRestart() {
  return invokeHostCommand<{ scheduled: boolean; kind: string; targets: Array<{ label: string; path: string }> }>(
    "reset_local_data",
  );
}

async function factoryResetLocalDataAndRestart() {
  return invokeHostCommand<{ scheduled: boolean; kind: string; targets: Array<{ label: string; path: string }> }>(
    "factory_reset_local_data",
  );
}

async function setGlobalHotkey(label: string) {
  await invokeHostCommand("set_global_hotkey", { label });
}

async function syncApplicationMenu() {
  if (hasDesktopHost()) {
    await syncDesktopApplicationMenu();
  }
}

async function validateApplicationMenuShortcut(accelerator: string) {
  if (hasDesktopHost()) {
    await validateDesktopApplicationMenuShortcut(accelerator);
  }
}

async function syncNativeTheme() {
  await invokeHostCommand("sync_native_theme");
}

function readRecentSearches() {
  try {
    const raw = window.localStorage.getItem(recentSearchesStorageKey);
    if (!raw) {
      return [];
    }
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed)
      ? parsed
        .filter((value): value is string => typeof value === "string" && value.trim().length > 0)
        .slice(0, 5)
      : [];
  } catch {
    return [];
  }
}

function writeRecentSearches(searches: string[]) {
  try {
    window.localStorage.setItem(recentSearchesStorageKey, JSON.stringify(searches.slice(0, 5)));
  } catch {
    // Recent searches are a convenience only; storage failures should not block search.
  }
}

function randomDelay([min, max]: readonly [number, number]) {
  return min + Math.floor(Math.random() * (max - min + 1));
}

function readLastAutomaticUpdateCheckAt() {
  try {
    const raw = window.localStorage.getItem(lastAutomaticUpdateCheckStorageKey);
    if (!raw) {
      return null;
    }
    const parsed = Number(raw);
    return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
  } catch {
    return null;
  }
}

function writeLastAutomaticUpdateCheckAt(timestamp: number) {
  try {
    window.localStorage.setItem(lastAutomaticUpdateCheckStorageKey, String(timestamp));
  } catch {
    // Update checks still work without persistence; they just fall back to this session.
  }
}

function automaticUpdateCheckIsDue(now = Date.now()) {
  const lastCheckAt = readLastAutomaticUpdateCheckAt();
  if (!lastCheckAt) {
    return true;
  }
  if (lastCheckAt > now + automaticUpdateCheckIntervalMs) {
    return true;
  }
  return now - lastCheckAt >= automaticUpdateCheckIntervalMs;
}

function nextAutomaticUpdateCheckDelay(now = Date.now()) {
  const lastCheckAt = readLastAutomaticUpdateCheckAt();
  if (!lastCheckAt || lastCheckAt > now + automaticUpdateCheckIntervalMs) {
    return 0;
  }
  return Math.max(0, lastCheckAt + automaticUpdateCheckIntervalMs - now);
}

function wait(ms: number) {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

function searchIndexIsSettling(data: AppData) {
  return (
    data.sources.some((source) => source.status === "syncing") ||
    data.jobs.some(isActiveJob) ||
    data.items.some(
      (item) =>
        item.embeddingIndexStatus === "pending" ||
        item.visualIndexStatus === "pending",
    )
  );
}

const settingsSections = [
  "General",
  "Shortcuts",
  "Models",
  "Library",
  "Usage",
  "Advanced",
  "About",
] as const;
type SettingsSection = (typeof settingsSections)[number];
const settingsDefaultSection: SettingsSection = "General";

type ShortcutCommandDefinition = {
  id: string;
  settingKey: string;
  defaultValue: string;
  label: string;
  description: string;
  nativeMenu?: boolean;
  globalShortcut?: boolean;
};

function normalizeSettingsSection(section?: string | null): SettingsSection {
  if (!section) {
    return settingsDefaultSection;
  }
  if (section === "Cerul Cloud" || section === "Processing" || section === "Smart processing") {
    return "Models";
  }
  if (section === "Indexing" || section === "Storage" || section === "Library & Storage") {
    return "Library";
  }
  if (section === "Account" || section === "Account & Usage") {
    return "Usage";
  }
  if (section === "Settings" || section === "Preferences") {
    return settingsDefaultSection;
  }
  if (section === "Search" || section === "Summon search" || section === "唤起搜索") {
    return "Shortcuts";
  }
  if (settingsSections.includes(section as SettingsSection)) {
    return section as SettingsSection;
  }
  return settingsDefaultSection;
}

function hashQueryParam(name: string): string | null {
  const [, queryString = ""] = window.location.hash.replace(/^#/, "").split("?");
  return new URLSearchParams(queryString).get(name);
}

function fakeDesktopReleaseNotes(): DesktopReleaseNotes {
  return {
    publishedAt: new Date().toISOString(),
    sections: [
      {
        title: "Improved",
        items: [
          "Show the update log from the titlebar update button before opening the release page.",
          "Keep the updater action in place for download, restart, and release-page fallback states.",
          "Reuse the release notes generated by the existing release workflow.",
        ],
      },
      {
        title: "Fixed",
        items: ["Avoid showing an empty update card when detailed notes are unavailable."],
      },
    ],
  };
}

// Single source of truth for the non-online core-status wording, so the home
// status line and the rail footer never contradict each other (one used to
// say "正在启动" while the other said "核心离线" for the same state). The
// CoreBanner keeps its own prominent starting→unresponsive escalation.
function coreStatusText(status: ApiStatus, t: TFunction): string {
  return status === "connecting" ? t("shell.coreConnecting") : t("shell.coreOffline");
}

// Tracks, per running job, the wall-clock second its current coarse step began.
// The backend only timestamps the whole job, so we observe step transitions here
// to drive a "this step: M:SS" readout. A job's timer resets when its step
// changes; finished jobs are dropped.
function useStepStarts(jobs: api.JobRecord[]): Record<string, number> {
  const ref = useRef<Map<string, { step: string; at: number }>>(new Map());
  const [starts, setStarts] = useState<Record<string, number>>({});
  useEffect(() => {
    const now = Date.now() / 1000;
    const map = ref.current;
    const live = new Set<string>();
    let changed = false;
    for (const job of jobs) {
      const step = coarseStepKey(job);
      if (job.status !== "running" || !step) {
        continue;
      }
      live.add(job.id);
      const prev = map.get(job.id);
      if (!prev || prev.step !== step) {
        map.set(job.id, { step, at: now });
        changed = true;
      }
    }
    for (const id of Array.from(map.keys())) {
      if (!live.has(id)) {
        map.delete(id);
        changed = true;
      }
    }
    if (changed) {
      const next: Record<string, number> = {};
      map.forEach((value, id) => {
        next[id] = value.at;
      });
      setStarts(next);
    }
  }, [jobs]);
  return starts;
}

export function App() {
  return (
    <LangProvider>
      <AppWorkspace />
    </LangProvider>
  );
}

function AppWorkspace() {
  const t = useT();
  const exchangeOAuthCode = useAuthStore((state) => state.exchangeOAuthCode);
  const initialRoute = readRouteState();
  const [view, setViewState] = useState<View>(initialRoute.view);
  // First-run home guidance: true only for users who just finished the wizard,
  // until they run a search or dismiss the banner. Gates the ③+② first-run home.
  const [firstRunActive, setFirstRunActive] = useState(false);
  const [selectedItemId, setSelectedItemId] = useState<string | null>(initialRoute.itemId);
  const [selectedPlaybackChunkId, setSelectedPlaybackChunkId] = useState<string | null>(
    initialRoute.playbackChunkId,
  );
  const [selectedTimestamp, setSelectedTimestamp] = useState<string | null>(
    initialRoute.timestamp,
  );
  const [query, setQuery] = useState("");
  const [recentSearches, setRecentSearches] = useState<string[]>(() => readRecentSearches());
  const [showAddSource, setShowAddSource] = useState(false);
  const [showJobsSheet, setShowJobsSheet] = useState(false);
  const [confirmRequest, setConfirmRequest] = useState<ConfirmRequest | null>(null);
  const [updaterState, setUpdaterState] = useState<DesktopUpdaterState>({ phase: "idle" });
  const [updateNotesOpen, setUpdateNotesOpen] = useState(
    () => hashQueryParam("fakeUpdateNotesOpen") === "1",
  );
  const [onboardingStep, setOnboardingStep] = useState(0);
  const [settingsSection, setSettingsSection] = useState<string>(() =>
    normalizeSettingsSection(initialRoute.settingsSection),
  );
  const [modelDownloadState, setModelDownloadState] = useState<{
    status: "idle" | "saving_sources" | "downloading" | "error";
    error: string | null;
  }>({ status: "idle", error: null });
  const [onboardingFolders, setOnboardingFolders] = useState<string[]>([]);
  const [onboardingYoutubeChannels, setOnboardingYoutubeChannels] = useState<
    OnboardingYoutubeChannel[]
  >([]);
  const [apiStatus, setApiStatus] = useState<ApiStatus>("connecting");
  const [apiError, setApiError] = useState<string | null>(null);
  const [activityPollUntil, setActivityPollUntil] = useState(0);
  const coreLevel = useCoreStatus(apiStatus, apiError);
  const [data, setData] = useState<AppData>({
    sources: [],
    items: [],
    jobs: [],
    settings: {},
    whisperModels: [],
    daemonStatus: null,
    version: null,
  });
  const [liveResults, setLiveResults] = useState<Result[]>([]);
  const [searchDiagnostics, setSearchDiagnostics] = useState<api.SearchDiagnostics | null>(null);
  const [isSearching, setIsSearching] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);
  const lastSearchRef = useRef<{ query: string; retryWhenIdle: boolean } | null>(null);
  // Monotonic token: every runSearch bumps it, stale responses are dropped.
  const searchSeqRef = useRef(0);

  // When the core is offline we keep showing the last data we fetched (or empty
  // states) — never fake content the user might mistake for their own library.
  const visibleSources = data.sources;
  const visibleItems = data.items;
  const visibleResults = liveResults;
  const visibleJobs = apiStatus === "online" ? data.jobs : [];
  // Follow the OS by default — first launch on a light-mode Mac used to open dark.
  const themePreference = settingString(data.settings, "theme", "System");
  // Global indexing pause (the worker skips queued jobs while this is on).
  const indexingPaused = settingBoolean(data.settings, "indexing_paused", false);
  // The Tasks drawer hides orphaned jobs whose item was removed from the
  // library; cancelling a task now keeps the item and marks the job cancelled.
  const drawerJobs = visibleJobs.filter(
    (job) => !job.item_id || data.items.some((item) => item.id === job.item_id),
  );
  const currentItem = visibleItems.find((item) => item.id === selectedItemId) ?? null;
  const selectedResult = visibleResults.find(
    (result) =>
      (selectedPlaybackChunkId && result.playbackChunkId === selectedPlaybackChunkId) ||
      (result.itemId === selectedItemId && result.timestamp === selectedTimestamp),
  );
  const activeJobCount = visibleJobs.filter(isActiveJob).length;
  const syncingSources = visibleSources.filter((source) => source.status === "syncing");
  const syncingSourceCount = syncingSources.length;
  const backgroundActivityCount = activeJobCount + syncingSourceCount;
  const stepStarts = useStepStarts(visibleJobs);
  const kickActivityPolling = useCallback((durationMs = 120_000) => {
    const until = Date.now() + durationMs;
    setActivityPollUntil((current) => Math.max(current, until));
  }, []);

  // First-run on-device-model consent + download. Fetches capability and shows
  // the dialog once, gated on `local_models_prompted` so it never re-prompts —
  // and never fires before the core's capability route exists, because the
  // capability fetch simply rejects until then.
  const lmTrigger =
    apiStatus === "online" &&
    view !== "onboarding" &&
    !settingBoolean(data.settings, "local_models_prompted", false);
  const lm = useLocalModelConsent({ trigger: lmTrigger, apiOnline: apiStatus === "online" });
  const handleLmAgree = useCallback(() => {
    lm.agree();
    void api
      .updateSettings({ inference_mode: "local", local_models_prompted: true })
      .then(() => refreshCoreData())
      .catch(() => undefined);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lm.agree]);
  const handleLmDecline = useCallback(() => {
    lm.decline();
    void api
      .updateSettings({ inference_mode: "remote", local_models_prompted: true })
      .then(() => refreshCoreData())
      .catch(() => undefined);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lm.decline]);
  // Auto-dismiss the "models ready" toast after a few seconds.
  useEffect(() => {
    if (!lm.ready) return;
    const id = window.setTimeout(() => lm.dismissReady(), 6000);
    return () => window.clearTimeout(id);
  }, [lm.ready, lm.dismissReady]);

  // First-run cold start: a packaged app's core takes a few seconds to come up
  // while macOS verifies the bundle. If the user reached the onboarding
  // "core unreachable" error during that window, clear it once the core is
  // online so the step un-sticks on its own (the Start button re-enables too).
  useEffect(() => {
    if (
      apiStatus === "online" &&
      modelDownloadState.status === "error" &&
      modelDownloadState.error === t("common.coreUnreachable")
    ) {
      setModelDownloadState({ status: "idle", error: null });
    }
  }, [apiStatus, modelDownloadState, t]);

  useEffect(() => {
    function handleOAuthRoute(route: RouteState) {
      if (!route.oauthProvider && !route.oauthCode && !route.oauthState && !route.oauthError) {
        return false;
      }
      const settingsRoute = {
        view: "settings" as const,
        itemId: null,
        playbackChunkId: null,
        timestamp: null,
        settingsSection: "Usage",
      };
      setViewState(settingsRoute.view);
      setSelectedItemId(null);
      setSelectedPlaybackChunkId(null);
      setSelectedTimestamp(null);
      setShowJobsSheet(false);
      setShowAddSource(false);
      setSettingsSection(settingsRoute.settingsSection);
      window.history.replaceState(null, "", `#${routeHash("settings", { settingsSection: "Usage" })}`);
      void persistLastRoute(settingsRoute);

      if (route.oauthError) {
        console.warn("OAuth login failed", route.oauthError);
        return true;
      }
      if (!route.oauthCode || !route.oauthState) {
        console.warn("OAuth login callback was missing code or state");
        return true;
      }
      void exchangeOAuthCode({ code: route.oauthCode, state: route.oauthState }).catch((error) => {
        console.warn("OAuth login exchange failed", error);
      });
      return true;
    }

    function syncHashRoute() {
      const route = readRouteState();
      if (handleOAuthRoute(route)) {
        return;
      }
      setViewState(route.view);
      setSelectedItemId(route.itemId);
      setSelectedPlaybackChunkId(route.playbackChunkId);
      setSelectedTimestamp(route.timestamp);
      setShowJobsSheet(false);
      setShowAddSource(false);
      const normalizedRoute =
        route.view === "settings"
          ? { ...route, settingsSection: normalizeSettingsSection(route.settingsSection) }
          : route;
      if (normalizedRoute.view === "settings") {
        setSettingsSection(normalizedRoute.settingsSection ?? "General");
      }
      void persistLastRoute(normalizedRoute);
    }

    if (window.location.hash) {
      syncHashRoute();
    }
    window.addEventListener("hashchange", syncHashRoute);
    return () => window.removeEventListener("hashchange", syncHashRoute);
  }, [exchangeOAuthCode]);

  useEffect(() => {
    let cancelled = false;

    loadPersistedUiState()
      .then((state) => {
        if (cancelled) {
          return;
        }

        setFirstRunActive(Boolean(state.firstRunActive));

        // First run (a fresh or cleared install): no completed-onboarding flag,
        // no persisted route, and no explicit deep link → open the onboarding
        // intro rather than an empty home. Requiring no persisted route avoids
        // forcing existing users (who predate this flag but have navigated
        // before) back through onboarding on upgrade. The flag is set when the
        // wizard finishes (startIndexingFromOnboarding) or via Settings →
        // "Re-run onboarding".
        if (!state.hasCompletedOnboarding && !state.lastRoute && !window.location.hash) {
          setViewState("onboarding");
          return;
        }

        if (!window.location.hash && state.lastRoute) {
          restorePersistedRoute(state.lastRoute);
        }
      })
      .catch(() => undefined);

    return () => {
      cancelled = true;
    };
  }, []);

  // A search in the overlay window clears the shared first-run flag in the UI
  // store. Re-read it when this window regains focus so the guidance also
  // disappears live here, not only on the next launch. Active only while the
  // guidance is showing.
  useEffect(() => {
    if (!firstRunActive) {
      return;
    }
    function resync() {
      loadPersistedUiState()
        .then((state) => {
          if (!state.firstRunActive) {
            setFirstRunActive(false);
          }
        })
        .catch(() => undefined);
    }
    window.addEventListener("focus", resync);
    return () => window.removeEventListener("focus", resync);
  }, [firstRunActive]);

  useEffect(() => {
    void refreshCoreData();
  }, []);

  // Desktop auto-update: subscribe to shell-pushed updater state, sync the
  // current value, and keep background checks sparse. In the browser/fixture
  // harness, ?fakeUpdate=<version> renders the pill without a desktop host so
  // the flow stays reviewable.
  useEffect(() => {
    const fakeVersion = hashQueryParam("fakeUpdate");
    if (fakeVersion) {
      setUpdaterState({
        phase: "available",
        version: fakeVersion,
        releaseUrl: "https://github.com/cerul-ai/cerul-app/releases",
        canAutoInstall: false,
        releaseNotes:
          hashQueryParam("fakeUpdateNotes") !== null ? fakeDesktopReleaseNotes() : undefined,
      });
      return;
    }
    if (!hasDesktopHost()) {
      return;
    }
    const unsubscribe = subscribeDesktopUpdater(setUpdaterState);
    let cancelled = false;
    let checkInFlight = false;
    let timeoutId: number | null = null;
    let wakeProbeId: number | null = null;
    let lastWakeProbeAt = Date.now();

    function clearScheduledCheck() {
      if (timeoutId !== null) {
        window.clearTimeout(timeoutId);
        timeoutId = null;
      }
    }

    function scheduleNextDueCheck() {
      if (cancelled) {
        return;
      }
      const delay = nextAutomaticUpdateCheckDelay();
      clearScheduledCheck();
      timeoutId = window.setTimeout(() => void runAutomaticUpdateCheck(), delay);
    }

    function scheduleRetrySoon(force = false) {
      if (cancelled) {
        return;
      }
      clearScheduledCheck();
      timeoutId = window.setTimeout(
        () => void runAutomaticUpdateCheck({ force }),
        automaticUpdateOfflineRetryMs,
      );
    }

    async function runAutomaticUpdateCheck({ force = false }: { force?: boolean } = {}) {
      clearScheduledCheck();
      if (cancelled || checkInFlight) {
        return;
      }
      if (window.navigator.onLine === false) {
        scheduleRetrySoon(force);
        return;
      }
      if (!force && !automaticUpdateCheckIsDue()) {
        scheduleNextDueCheck();
        return;
      }
      checkInFlight = true;
      let succeeded = false;
      try {
        const next = await runDesktopUpdaterCheck();
        if (!cancelled) {
          setUpdaterState(next);
        }
        succeeded = true;
      } catch (error) {
        console.error("desktop updater automatic check failed", error);
      } finally {
        checkInFlight = false;
        if (succeeded) {
          // Only a check that actually reached the update server advances the
          // throttle. A transient failure now retries soon instead of persisting
          // a "checked" timestamp that used to strand users on an old build for
          // the full interval whenever the update host briefly errored.
          writeLastAutomaticUpdateCheckAt(Date.now());
          scheduleNextDueCheck();
        } else {
          scheduleRetrySoon(force);
        }
      }
    }

    function scheduleDueCheckAfter(delay: number) {
      if (cancelled || !automaticUpdateCheckIsDue()) {
        return;
      }
      clearScheduledCheck();
      timeoutId = window.setTimeout(() => void runAutomaticUpdateCheck(), delay);
    }

    function scheduleResumeCheck() {
      scheduleDueCheckAfter(randomDelay(automaticUpdateResumeDelayRangeMs));
    }

    function scheduleStartupCheck() {
      if (cancelled) {
        return;
      }
      // Every cold launch checks once, after a short randomized delay so we don't
      // race app startup or stampede the update host. The 6h throttle then governs
      // only the background/resume cadence of a long-running session — reopening
      // the app no longer silently skips the check.
      clearScheduledCheck();
      timeoutId = window.setTimeout(
        () => void runAutomaticUpdateCheck({ force: true }),
        randomDelay(automaticUpdateStartupDelayRangeMs),
      );
    }

    function handleVisibilityChange() {
      if (document.visibilityState === "visible") {
        scheduleResumeCheck();
      }
    }

    void getDesktopUpdaterState().then(setUpdaterState);
    scheduleStartupCheck();
    wakeProbeId = window.setInterval(() => {
      const now = Date.now();
      if (now - lastWakeProbeAt > automaticUpdateWakeGapMs) {
        scheduleResumeCheck();
      }
      lastWakeProbeAt = now;
    }, automaticUpdateWakeProbeIntervalMs);
    window.addEventListener("online", scheduleResumeCheck);
    window.addEventListener("focus", scheduleResumeCheck);
    document.addEventListener("visibilitychange", handleVisibilityChange);
    return () => {
      cancelled = true;
      unsubscribe();
      clearScheduledCheck();
      if (wakeProbeId !== null) {
        window.clearInterval(wakeProbeId);
      }
      window.removeEventListener("online", scheduleResumeCheck);
      window.removeEventListener("focus", scheduleResumeCheck);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
  }, []);

  const newSourceHotkey = settingString(data.settings, "hotkey_new_source", NEW_SOURCE_DEFAULT_HOTKEY);
  useEffect(() => {
    return subscribeDesktopMenuCommand((command) => {
      if (command.type !== "new_source" || hasOpenModalSurface()) {
        return;
      }
      if (command.triggeredByAccelerator && isEditableTarget(document.activeElement)) {
        return;
      }
      setShowAddSource(true);
    });
  }, []);

  useEffect(() => {
    function handleGlobalKeyDown(event: globalThis.KeyboardEvent) {
      if (acceleratorMatchesEvent(newSourceHotkey, event)) {
        // Don't stack a new dialog on top of an open modal or steal the
        // shortcut while the user is typing in a field.
        if (shouldIgnoreNewSourceShortcut(event.target)) {
          return;
        }
        event.preventDefault();
        setShowAddSource(true);
      }
    }

    window.addEventListener("keydown", handleGlobalKeyDown);
    return () => window.removeEventListener("keydown", handleGlobalKeyDown);
  }, [newSourceHotkey]);

  useEffect(() => {
    const root = document.documentElement;
    const media =
      typeof window.matchMedia === "function"
        ? window.matchMedia("(prefers-color-scheme: light)")
        : null;

    function applyTheme() {
      const resolvedTheme = resolveThemePreference(themePreference, media?.matches ?? false);
      root.dataset.theme = resolvedTheme;
      root.dataset.themePreference = themePreference.toLowerCase();
    }

    applyTheme();
    media?.addEventListener("change", applyTheme);

    return () => {
      media?.removeEventListener("change", applyTheme);
    };
  }, [themePreference]);

  useEffect(() => {
    const pollWindowOpen = activityPollUntil > Date.now();
    if (apiStatus !== "online" || (backgroundActivityCount === 0 && !pollWindowOpen)) {
      return;
    }

    const intervalId = window.setInterval(() => {
      void refreshCoreData();
    }, syncingSourceCount > 0 && activeJobCount === 0 ? 1500 : 2500);
    const timeoutId = pollWindowOpen
      ? window.setTimeout(() => {
          setActivityPollUntil((current) => (current <= Date.now() ? 0 : current));
        }, Math.max(250, activityPollUntil - Date.now() + 100))
      : null;
    return () => {
      window.clearInterval(intervalId);
      if (timeoutId !== null) {
        window.clearTimeout(timeoutId);
      }
    };
  }, [apiStatus, activeJobCount, syncingSourceCount, backgroundActivityCount, activityPollUntil]);

  // Items/sources are mapped through t() at fetch time; re-map once when the
  // user switches language so dates/status text don't stay in the old locale.
  const { lang } = useLang();
  const lastMappedLangRef = useRef(lang);
  useEffect(() => {
    if (lastMappedLangRef.current === lang) {
      return;
    }
    lastMappedLangRef.current = lang;
    if (apiStatus === "online") {
      void refreshCoreData();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lang]);

  // Auto-reconnect: while the core is unreachable, keep probing with a
  // capped exponential backoff instead of waiting for a manual Retry click.
  useEffect(() => {
    if (apiStatus === "online") {
      return;
    }
    let cancelled = false;
    let attempt = 0;
    let timeoutId = 0;

    const probe = () => {
      const delay = Math.min(2000 * 2 ** attempt, 15000);
      attempt += 1;
      timeoutId = window.setTimeout(() => {
        void refreshCoreData().then((result) => {
          if (!cancelled && result === null) probe();
        });
      }, delay);
    };
    probe();

    return () => {
      cancelled = true;
      window.clearTimeout(timeoutId);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [apiStatus]);

  async function refreshCoreData(): Promise<AppData | null> {
    setApiError(null);

    try {
      const [health, sourceRecords, itemRecords, jobRecords, settings, whisperModels, daemonStatus] =
        await Promise.all([
          api.health(),
          api.listSources(),
          api.listItems(),
          api.listJobs(),
          api.listSettings(),
          api.listWhisperModels(),
          readDaemonStatus(),
        ]);
      const mappedItems = itemRecords.map((record) => mapItemRecord(record, jobRecords, t));
      const hasSyncingSources = sourceRecords.some((source) => source.status === "syncing");
      const nextData: AppData = {
        sources: sourceRecords.map((source) => mapSourceRecord(source, mappedItems, t)),
        items: mappedItems,
        jobs: jobRecords,
        settings,
        whisperModels,
        daemonStatus,
        version: health.version,
      };
      setData(nextData);
      setApiStatus("online");
      const pendingRetry = lastSearchRef.current;
      if (pendingRetry?.retryWhenIdle && !hasSyncingSources && !jobRecords.some(isActiveJob)) {
        lastSearchRef.current = { query: pendingRetry.query, retryWhenIdle: false };
        const seqAtSchedule = searchSeqRef.current;
        api
          .search(pendingRetry.query, 20)
          .then((response) => {
            // A newer user-initiated search supersedes this idle retry.
            if (seqAtSchedule !== searchSeqRef.current) return;
            setLiveResults(mapSearchResults(response.results, mappedItems, t));
            setSearchDiagnostics(response.diagnostics);
            lastSearchRef.current = {
              query: pendingRetry.query,
              retryWhenIdle: false,
            };
          })
          .catch(() => undefined);
      }
      return nextData;
    } catch (error) {
      setApiStatus((current) => (current === "online" ? "error" : "offline"));
      setApiError(errorMessage(error));
      return null;
    }
  }

  async function restartCoreConnection() {
    setApiStatus("connecting");
    setApiError(null);
    await refreshCoreData();
  }

  function navigate(
    nextView: View,
    params: {
      itemId?: string | null;
      playbackChunkId?: string | null;
      timestamp?: string | null;
      settingsSection?: string | null;
    } = {},
  ) {
    setShowJobsSheet(false);
    setShowAddSource(false);
    setSelectedItemId(params.itemId ?? null);
    setSelectedPlaybackChunkId(params.playbackChunkId ?? null);
    setSelectedTimestamp(params.timestamp ?? null);
    const routeParams =
      nextView === "settings"
        ? {
            ...params,
            settingsSection: normalizeSettingsSection(params.settingsSection),
          }
        : params;
    if (nextView === "settings" && routeParams.settingsSection) {
      setSettingsSection(routeParams.settingsSection);
    }
    setViewState(nextView);
    const hash = routeHash(nextView, routeParams);
    window.location.hash = hash;
    if ((nextView === "item-detail" || nextView === "result-detail") && routeParams.itemId) {
      recordLastOpened(routeParams.itemId, routeParams.timestamp ?? null);
    }
    void persistLastRoute({
      view: nextView,
      itemId: routeParams.itemId ?? null,
      playbackChunkId: routeParams.playbackChunkId ?? null,
      timestamp: routeParams.timestamp ?? null,
      settingsSection: routeParams.settingsSection ?? null,
    });
  }

  function restorePersistedRoute(route: PersistedRoute) {
    if (!viewIds.includes(route.view as View)) {
      return;
    }

    const restoredView = route.view as View;
    setSelectedItemId(route.itemId ?? null);
    setSelectedPlaybackChunkId(route.playbackChunkId ?? null);
    setSelectedTimestamp(route.timestamp ?? null);
    const restoredRoute =
      restoredView === "settings"
        ? { ...route, settingsSection: normalizeSettingsSection(route.settingsSection) }
        : route;
    if (restoredView === "settings") {
      setSettingsSection(restoredRoute.settingsSection ?? "General");
    }
    setViewState(restoredView);
    window.location.hash = routeHash(restoredView, restoredRoute);
  }

  function requestConfirm(options: ConfirmOptions) {
    return new Promise<boolean>((resolve) => {
      setConfirmRequest({ ...options, resolve });
    });
  }

  function resolveConfirm(confirmed: boolean) {
    const request = confirmRequest;
    setConfirmRequest(null);
    request?.resolve(confirmed);
  }

  async function handleUpdateActivate() {
    if (updaterState.phase === "available") {
      // GitHub-release fallback is informational; only latest-mac.yml backed
      // states can use the automatic downloader.
      if (!hasDesktopHost() || !updaterState.canAutoInstall) {
        window.open(updaterState.releaseUrl, "_blank", "noopener,noreferrer");
        return;
      }
      const next = await downloadDesktopUpdate();
      setUpdaterState(next);
    } else if (updaterState.phase === "downloaded") {
      await installDesktopUpdate();
    } else if (updaterState.phase === "error") {
      window.open(updaterState.releaseUrl, "_blank", "noopener,noreferrer");
    }
  }

  function updateDownloadLabel(state: Extract<DesktopUpdaterState, { phase: "downloading" }>) {
    const speed = formatSpeed(state.bytesPerSecond);
    return speed ? `${state.percent}% · ${speed}` : `${state.percent}%`;
  }

  function updateDownloadTitle(state: Extract<DesktopUpdaterState, { phase: "downloading" }>) {
    const speed = formatSpeed(state.bytesPerSecond);
    const eta = state.etaSeconds != null ? formatDuration(state.etaSeconds, t) : null;
    return [
      t("shell.updateDownloadingTip"),
      `${state.percent}%`,
      speed,
      eta ? t("home.continueRemaining", { remaining: eta }) : null,
    ]
      .filter(Boolean)
      .join(" · ");
  }

  function updateStatusTip(state: VisibleDesktopUpdaterState) {
    switch (state.phase) {
      case "downloading":
        return updateDownloadTitle(state);
      case "preparing":
        return t("shell.updatePreparingTip");
      case "installing":
        return t("shell.updateInstallingTip");
      case "downloaded":
        return t("shell.updateReadyTip", { version: state.version });
      case "error":
        return t("shell.updateErrorTip", { message: state.message });
      case "available":
        return state.canAutoInstall
          ? t("shell.updateAvailableTip", { version: state.version })
          : t("shell.updateReleaseTip", { version: state.version });
    }
  }

  function updateNotesTitle(state: VisibleDesktopUpdaterState) {
    return "version" in state && state.version
      ? t("shell.updateNotes.title", { version: state.version })
      : t("shell.updateNotes.titleGeneric");
  }

  function updateNotesDate(state: VisibleDesktopUpdaterState) {
    const publishedAt = "releaseNotes" in state ? state.releaseNotes?.publishedAt : null;
    if (!publishedAt) {
      return null;
    }
    const date = new Date(publishedAt);
    if (!Number.isFinite(date.getTime())) {
      return null;
    }
    return new Intl.DateTimeFormat(lang === "zh" ? "zh-CN" : "en-US", {
      year: "numeric",
      month: "long",
      day: "numeric",
    }).format(date);
  }

  function updateNotesSections(state: VisibleDesktopUpdaterState) {
    const releaseSections = "releaseNotes" in state ? state.releaseNotes?.sections : null;
    if (releaseSections?.some((section) => section.items.length > 0)) {
      let remaining = 7;
      return releaseSections
        .map((section) => {
          const items = section.items.slice(0, remaining);
          remaining -= items.length;
          return {
            title: updateNotesSectionTitle(section.title),
            items,
          };
        })
        .filter((section) => section.items.length > 0);
    }
    return [
      {
        title: t("shell.updateNotes.section.status"),
        items: [
          state.phase === "available" ? t("shell.updateNotes.noNotes") : updateStatusTip(state),
        ],
      },
    ];
  }

  function updateNotesSectionTitle(title: string | undefined) {
    const key = title?.trim().toLowerCase();
    if (key === "new" || key === "new features") {
      return t("shell.updateNotes.section.new");
    }
    if (key === "improved" || key === "improvements") {
      return t("shell.updateNotes.section.improved");
    }
    if (key === "fixed" || key === "fixes") {
      return t("shell.updateNotes.section.fixed");
    }
    return title?.trim() || t("shell.updateNotes.section.highlights");
  }

  // First-run guidance ends the moment the user actually searches or dismisses
  // the banner — it never lingers or reappears.
  function resolveFirstRun() {
    if (!firstRunActive) {
      return;
    }
    setFirstRunActive(false);
    void persistFirstRunActive(false);
  }

  function submitSearch(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const submittedQuery =
      new FormData(event.currentTarget).get("query")?.toString() ??
      event.currentTarget.querySelector<HTMLInputElement>("input")?.value ??
      query;
    setQuery(submittedQuery);
    // Only a real (non-empty) search counts as completing the first-run step;
    // runSearch trims and bails on blank, so guard the resolve the same way.
    if (submittedQuery.trim()) {
      resolveFirstRun();
    }
    navigate("results");
    void runSearch(submittedQuery);
  }

  function runQuery(value: string) {
    setQuery(value);
    if (value.trim()) {
      resolveFirstRun();
    }
    navigate("results");
    void runSearch(value);
  }

  async function runSearch(value: string) {
    const trimmed = value.trim();
    if (!trimmed) {
      setSearchDiagnostics(null);
      return;
    }

    rememberRecentSearch(trimmed);
    const seq = ++searchSeqRef.current;
    const isCurrent = () => seq === searchSeqRef.current;
    setIsSearching(true);
    setSearchError(null);
    setSearchDiagnostics(null);
    try {
      const latestData = await refreshCoreData();
      if (!latestData && apiStatus !== "online") {
        throw new Error(t("common.coreUnreachable"));
      }
      const searchData = latestData ?? data;
      const itemsForResults = searchData.items;
      let retryWhenIndexSettles = searchIndexIsSettling(searchData);
      let response = await api.search(trimmed, 20);
      if (!isCurrent()) return;
      setLiveResults(mapSearchResults(response.results, itemsForResults, t));
      setSearchDiagnostics(response.diagnostics);
      if (response.results.length === 0 || retryWhenIndexSettles) {
        await wait(650);
        if (!isCurrent()) return;
        const refreshed = await refreshCoreData();
        retryWhenIndexSettles = refreshed ? searchIndexIsSettling(refreshed) : retryWhenIndexSettles;
        response = await api.search(trimmed, 20);
        if (!isCurrent()) return;
        setLiveResults(mapSearchResults(response.results, refreshed?.items ?? itemsForResults, t));
        setSearchDiagnostics(response.diagnostics);
      }
      lastSearchRef.current = {
        query: trimmed,
        retryWhenIdle: retryWhenIndexSettles,
      };
    } catch (error) {
      // A failed search is a search-level problem; flipping the whole app
      // into an offline/error state used to swap the UI to demo data.
      if (isCurrent()) setSearchError(errorMessage(error));
    } finally {
      if (isCurrent()) setIsSearching(false);
    }
  }

  function rememberRecentSearch(value: string) {
    setRecentSearches((current) => {
      const normalized = value.trim();
      if (!normalized) {
        return current;
      }
      const next = [normalized, ...current.filter((item) => item !== normalized)].slice(0, 5);
      writeRecentSearches(next);
      return next;
    });
  }

  async function startIndexingFromOnboarding() {
    if (apiStatus !== "online") {
      setModelDownloadState({ status: "error", error: t("common.coreUnreachable") });
      return;
    }

    const folders = uniqueStrings(onboardingFolders);
    const youtubeChannels = uniqueYoutubeChannels(onboardingYoutubeChannels);
    const sourceCount = folders.length + youtubeChannels.length;
    setModelDownloadState({
      status: sourceCount > 0 ? "saving_sources" : "downloading",
      error: null,
    });
    try {
      if (sourceCount > 0) {
        kickActivityPolling();
      }
      for (const folder of folders) {
        await api.addSource("folder_video", { path: folder });
      }
      for (const channel of youtubeChannels) {
        await api.addSource("youtube", { url: channel.url, max_videos: 50 });
      }
      if (sourceCount > 0) {
        kickActivityPolling();
      }
      setModelDownloadState({ status: "downloading", error: null });
      await api.updateSettings({
        inference_mode: "auto",
        asr_model: "whisper-1",
        active_embedding_profile: "gemini-embedding-2-3072",
      });
      await installDaemon();
      await refreshCoreData();
      setModelDownloadState({ status: "idle", error: null });
      void persistOnboardingCompleted(true);
      setFirstRunActive(true);
      void persistFirstRunActive(true);
      navigate("home");
    } catch (error) {
      setModelDownloadState({ status: "error", error: errorMessage(error) });
    }
  }

  const sidebarActiveView = sidebarParentFor[view] ?? view;
  const railItems: { id: View; labelKey: string; icon: LucideIcon }[] = [
    { id: "home", labelKey: "nav.home", icon: Search },
    { id: "library", labelKey: "nav.library", icon: Library },
    { id: "moments", labelKey: "nav.moments", icon: Star },
    { id: "sources", labelKey: "nav.sources", icon: Database },
  ];
  const mobileNavItems = [
    ...railItems,
    { id: "settings" as View, labelKey: "nav.settings", icon: Settings },
  ];
  const mobileTitleKey =
    mobileNavItems.find((item) => item.id === sidebarActiveView)?.labelKey ?? "nav.home";
  const settingsTakeoverActive = view === "settings";
  const onboardingActive = view === "onboarding";

  return (
    <div className="app" data-onboarding={onboardingActive ? "true" : undefined}>
      <AccountDialogController />
      <div className="titlebar">
        <div className="titlebar-lead">
          {updaterState.phase !== "idle" ? (
            <div
              className={`update-hover is-${updaterState.phase}${
                updateNotesOpen ? " is-open" : ""
              }`}
              onMouseEnter={() => setUpdateNotesOpen(true)}
              onMouseLeave={() => setUpdateNotesOpen(false)}
              onFocus={() => setUpdateNotesOpen(true)}
              onBlur={() => setUpdateNotesOpen(false)}
            >
              <button
                className={`rail-update is-${updaterState.phase}`}
                type="button"
                disabled={
                  updaterState.phase === "downloading" ||
                  updaterState.phase === "preparing" ||
                  updaterState.phase === "installing"
                }
                aria-label={updateStatusTip(updaterState)}
                aria-describedby="rail-update-popover"
                onClick={() => void handleUpdateActivate()}
              >
                {updaterState.phase === "downloading" ? (
                  <>
                    <Loader2 size={13} className="spin" />
                    <span className="rail-update-label">{updateDownloadLabel(updaterState)}</span>
                  </>
                ) : updaterState.phase === "preparing" ? (
                  <>
                    <Loader2 size={13} className="spin" />
                    <span className="rail-update-label">{t("shell.updatePreparing")}</span>
                  </>
                ) : updaterState.phase === "installing" ? (
                  <>
                    <Loader2 size={13} className="spin" />
                    <span className="rail-update-label">{t("shell.updateInstalling")}</span>
                  </>
                ) : updaterState.phase === "downloaded" ? (
                  <>
                    <RefreshCcw size={13} />
                    <span className="rail-update-label">{t("shell.updateRestart")}</span>
                  </>
                ) : updaterState.phase === "error" ? (
                  <>
                    <AlertTriangle size={13} />
                    <span className="rail-update-label">{t("shell.updateError")}</span>
                  </>
                ) : (
                  <>
                    {updaterState.canAutoInstall ? <Download size={13} /> : <ExternalLink size={13} />}
                    <span className="rail-update-label">{t("shell.update")}</span>
                  </>
                )}
              </button>
              <div className="update-popover" id="rail-update-popover" role="tooltip">
                <div className="update-popover-title">{updateNotesTitle(updaterState)}</div>
                {updateNotesDate(updaterState) ? (
                  <div className="update-popover-date">{updateNotesDate(updaterState)}</div>
                ) : null}
                <div className="update-popover-rule" aria-hidden="true" />
                {updateNotesSections(updaterState).map((section) => (
                  <section className="update-popover-section" key={section.title}>
                    <h3>{section.title}</h3>
                    <ul>
                      {section.items.map((item) => (
                        <li key={item}>{item}</li>
                      ))}
                    </ul>
                  </section>
                ))}
              </div>
            </div>
          ) : null}
        </div>
        <div className="titlebar-drag" aria-hidden="true" />
      </div>
      {!settingsTakeoverActive ? (
        <>
          <aside className="rail">
            <div className="rail-top">
              <button
                className="rail-brand"
                type="button"
                disabled={onboardingActive}
                onClick={() => navigate("home")}
                aria-label={t("shell.openHome")}
              >
                <BrandMark />
                <span className="rail-wordmark rail-label">Cerul</span>
              </button>
            </div>

            <nav className="rail-nav" aria-label={t("nav.home")}>
              {railItems.map((item) => {
                const Icon = item.icon;
                return (
                  <button
                    className={item.id === sidebarActiveView ? "rail-item active" : "rail-item"}
                    key={item.id}
                    type="button"
                    disabled={onboardingActive}
                    onClick={() => navigate(item.id)}
                    title={t(item.labelKey)}
                  >
                    <span className="rail-ind" aria-hidden="true" />
                    <Icon size={17} />
                    <span className="rail-label">{t(item.labelKey)}</span>
                  </button>
                );
              })}
            </nav>

            <div className="rail-bottom">
              <div className="rail-sep" aria-hidden="true" />
              <button
                className="rail-item"
                type="button"
                disabled={onboardingActive}
                onClick={() => setShowJobsSheet(true)}
                title={t("nav.jobs")}
              >
                <span className="rail-ind" aria-hidden="true" />
                <span style={{ position: "relative", display: "inline-flex" }}>
                  <ListChecks size={17} />
                  {backgroundActivityCount > 0 ? (
                    <span className="badge-count" aria-hidden="true">
                      {backgroundActivityCount > 9 ? "9+" : backgroundActivityCount}
                    </span>
                  ) : null}
                </span>
                <span className="rail-label">{t("nav.jobs")}</span>
              </button>
              <button
                className={sidebarActiveView === "settings" ? "rail-item active" : "rail-item"}
                type="button"
                disabled={onboardingActive}
                onClick={() => navigate("settings")}
                title={t("nav.settings")}
              >
                <span className="rail-ind" aria-hidden="true" />
                <Settings size={17} />
                <span className="rail-label">{t("nav.settings")}</span>
              </button>
              <AccountRailButton />
              {lm.minimized && lm.download && lm.download.phase !== "ready" ? (
                <button
                  type="button"
                  className="rail-dl-pill"
                  onClick={lm.reopen}
                  title={t("localModel.rail.downloading", { pct: lm.download.overall_progress })}
                >
                  <span className="ring" aria-hidden="true" />
                  <span className="rail-label clamp1">
                    {t("localModel.rail.downloading", { pct: lm.download.overall_progress })}
                  </span>
                </button>
              ) : null}
              <div className="rail-status mono">
                <span
                  className="rail-status-dot"
                  data-level={coreLevel === "grace" ? "ok" : coreLevel}
                  aria-hidden="true"
                />
                <span className="rail-label">
                  {coreLevel === "ok" || coreLevel === "grace"
                    ? t("shell.coreLocal")
                    : coreLevel === "starting"
                      ? t("shell.coreStarting")
                      : t("shell.coreUnresponsive")}
                </span>
              </div>
            </div>
          </aside>

          <div className="mobilebar">
            <button
              className="rail-brand"
              type="button"
              onClick={() => navigate("home")}
              aria-label={t("shell.openHome")}
            >
              <BrandMark />
            </button>
            <span className="tb-title clamp1">{t(mobileTitleKey)}</span>
            <button
              className="btn-icon sm"
              type="button"
              onClick={() => setShowJobsSheet(true)}
              aria-label={t("nav.jobs")}
            >
              <span style={{ position: "relative", display: "inline-flex" }}>
                <ListChecks size={17} />
                {backgroundActivityCount > 0 ? (
                  <span className="badge-count" aria-hidden="true">
                    {backgroundActivityCount > 9 ? "9+" : backgroundActivityCount}
                  </span>
                ) : null}
              </span>
            </button>
          </div>
        </>
      ) : null}

      <main className="content">
        {view === "onboarding" ? (
          <Onboarding
            step={onboardingStep}
            setStep={setOnboardingStep}
            apiStatus={apiStatus}
            folders={onboardingFolders}
            setFolders={setOnboardingFolders}
            youtubeChannels={onboardingYoutubeChannels}
            setYoutubeChannels={setOnboardingYoutubeChannels}
            modelDownloadState={modelDownloadState}
            onDone={startIndexingFromOnboarding}
          />
        ) : null}
        {view === "home" ? (
          <HomeScreen
            query={query}
            setQuery={setQuery}
            onSubmit={submitSearch}
            onAddSource={() => setShowAddSource(true)}
            onOpenItem={(item, timestamp) =>
              navigate("item-detail", { itemId: item.id, timestamp })
            }
            onOpenLibrary={() => navigate("library")}
            items={visibleItems}
            sources={visibleSources}
            jobs={visibleJobs}
            indexingPaused={indexingPaused}
            apiStatus={apiStatus}
            globalHotkey={settingString(data.settings, "global_hotkey", "Alt+Space")}
            firstRunActive={firstRunActive}
            onResolveFirstRun={resolveFirstRun}
            onRunQuery={runQuery}
          />
        ) : null}
        {view === "results" ? (
          <ResultsScreen
            query={query}
            setQuery={setQuery}
            onSubmit={submitSearch}
            onBack={() => navigate("home")}
            onOpen={(result) =>
              navigate("result-detail", {
                itemId: result.itemId,
                playbackChunkId: result.playbackChunkId,
                timestamp: result.timestamp,
              })
            }
            results={visibleResults}
            diagnostics={searchDiagnostics}
            isSearching={isSearching}
            error={searchError}
            apiStatus={apiStatus}
            hasIndexedItems={visibleItems.some((item) => item.status === "indexed")}
            hasActiveJobs={visibleJobs.some(isActiveJob)}
          />
        ) : null}
        {view === "result-detail" && !currentItem ? (
          <div className="screen">
            <EmptyState
              title={t("detail.notFound.title")}
              body={t("detail.notFound.body")}
              actionLabel={t("detail.notFound.back")}
              onAction={() => navigate("library")}
            />
          </div>
        ) : null}
        {view === "result-detail" && currentItem ? (
          <ResultDetail
            item={currentItem}
            startChunkId={selectedPlaybackChunkId}
            matchedSnippet={selectedResult?.snippet}
            moreMatches={selectedResult?.moreMatches}
            startTimestamp={selectedTimestamp ?? "00:00"}
            actionsEnabled={apiStatus === "online"}
            onLibrary={() => navigate("results")}
            onDeleteItem={async (itemToDelete) => {
              await api.deleteItem(itemToDelete.id);
              await refreshCoreData();
              navigate("library");
            }}
            onReindexItem={async (itemToReindex) => {
              kickActivityPolling();
              await api.reindexItem(itemToReindex.id);
              kickActivityPolling();
              await refreshCoreData();
            }}
            onItemUpdated={async () => {
              await refreshCoreData();
            }}
            requestConfirm={requestConfirm}
          />
        ) : null}
        {view === "library" ? (
          <LibraryScreen
            items={visibleItems}
            jobs={visibleJobs}
            syncingSources={syncingSources}
            stepStarts={stepStarts}
            indexingPaused={indexingPaused}
            actionsEnabled={apiStatus === "online"}
            onAddSource={() => setShowAddSource(true)}
            onOpenJobs={() => setShowJobsSheet(true)}
            onDeleteItems={async (itemIds, onProgress, options) => {
              const deletingIds = new Set(itemIds);
              setData((current) => ({
                ...current,
                items: current.items.filter((item) => !deletingIds.has(item.id)),
                jobs: current.jobs.filter((job) => !job.item_id || !deletingIds.has(job.item_id)),
              }));
              setLiveResults((current) =>
                current.filter((result) => !deletingIds.has(result.itemId)),
              );
              const total = itemIds.length;
              let completed = 0;
              const failures: unknown[] = [];
              onProgress?.(completed, total);
              for (const itemId of itemIds) {
                try {
                  await api.deleteItem(itemId, options);
                } catch (error) {
                  failures.push(error);
                } finally {
                  completed += 1;
                  onProgress?.(completed, total);
                }
              }
              try {
                if (failures.length > 0) {
                  throw new Error(
                    t("library.batch.deletePartialFailure", {
                      failed: failures.length,
                      total,
                      reason: errorMessage(failures[0]),
                    }),
                  );
                }
              } finally {
                await refreshCoreData();
              }
            }}
            onReindexItems={async (itemIds) => {
              if (itemIds.length > 0) {
                kickActivityPolling();
              }
              for (const itemId of itemIds) {
                await api.reindexItem(itemId);
              }
              if (itemIds.length > 0) {
                kickActivityPolling();
              }
              await refreshCoreData();
            }}
            onOpenItem={(item) => navigate("item-detail", { itemId: item.id })}
            requestConfirm={requestConfirm}
          />
        ) : null}
        {view === "moments" ? (
          <MomentsScreen
            actionsEnabled={apiStatus === "online"}
            onOpenItem={(moment) =>
              navigate("item-detail", { itemId: moment.item_id, timestamp: moment.timestamp })
            }
          />
        ) : null}
        {view === "item-detail" && !currentItem ? (
          <div className="screen">
            <EmptyState
              title={t("detail.notFound.title")}
              body={t("detail.notFound.body")}
              actionLabel={t("detail.notFound.back")}
              onAction={() => navigate("library")}
            />
          </div>
        ) : null}
        {view === "item-detail" && currentItem ? (
          <ItemDetail
            item={currentItem}
            apiStatus={apiStatus}
            actionsEnabled={apiStatus === "online"}
            startTimestamp={selectedTimestamp ?? "0:00"}
            startChunkId={selectedPlaybackChunkId}
            onBack={() => navigate("library")}
            onDeleteItem={async (itemToDelete) => {
              await api.deleteItem(itemToDelete.id);
              await refreshCoreData();
              navigate("library");
            }}
            onReindexItem={async (itemToReindex) => {
              kickActivityPolling();
              await api.reindexItem(itemToReindex.id);
              kickActivityPolling();
              await refreshCoreData();
            }}
            onItemUpdated={async () => {
              await refreshCoreData();
            }}
            requestConfirm={requestConfirm}
          />
        ) : null}
        {view === "sources" ? (
          <SourcesScreen
            sources={visibleSources}
            actionsEnabled={apiStatus === "online"}
            onAddSource={() => setShowAddSource(true)}
            onPauseSource={async (source) => {
              await api.pauseSource(source.id);
              await refreshCoreData();
            }}
            onResumeSource={async (source) => {
              await api.resumeSource(source.id);
              await refreshCoreData();
            }}
            onRemoveSource={async (source) => {
              await api.removeSource(source.id);
              await refreshCoreData();
            }}
            onRetryFailedSource={async (source) => {
              kickActivityPolling();
              await api.retryFailedSourceItems(source.id);
              kickActivityPolling();
              await refreshCoreData();
            }}
            onRetrySourceDiscovery={async (source) => {
              kickActivityPolling();
              await api.retrySourceDiscovery(source.id);
              kickActivityPolling();
              await refreshCoreData();
            }}
            onViewItems={() => navigate("library")}
            onOpenSettingsFix={(section) => navigate("settings", { settingsSection: section })}
            requestConfirm={requestConfirm}
          />
        ) : null}
        {view === "settings" ? (
          <SettingsScreen
            onBack={() => navigate("home")}
            section={settingsSection}
            setSection={setSettingsSection}
            apiStatus={apiStatus}
            coreLevel={coreLevel}
            settings={data.settings}
            daemonStatus={data.daemonStatus}
            onSettingsChange={async (settings) => {
              // Rejects if the write fails; saveSettings turns that into a false
              // result so callers don't report success after a swallowed error.
              await api.updateSettings(settings);
              await refreshCoreData();
              return true;
            }}
            requestConfirm={requestConfirm}
          />
        ) : null}
      </main>

      <nav className="bottomnav" aria-label={t("nav.home")}>
        {mobileNavItems.map((item) => {
          const Icon = item.icon;
          return (
            <button
              key={item.id}
              type="button"
              className={item.id === sidebarActiveView ? "active" : ""}
              onClick={() => navigate(item.id)}
            >
              <Icon size={18} />
              <span>{t(item.labelKey)}</span>
            </button>
          );
        })}
      </nav>

      <CoreStatusToast
        show={view !== "settings" && coreLevel === "unresponsive"}
        error={apiError}
        onAction={restartCoreConnection}
      />

      {showAddSource ? (
        <AddSourceDialog
          onClose={() => setShowAddSource(false)}
          requestConfirm={requestConfirm}
          onAddSource={async (type, config) => {
            kickActivityPolling();
            await api.addSource(type, config);
            kickActivityPolling();
            await refreshCoreData();
          }}
        />
      ) : null}
      {showJobsSheet ? (
        <JobsSheet
          jobs={drawerJobs}
          syncingSources={syncingSources}
          items={visibleItems}
          stepStarts={stepStarts}
          paused={indexingPaused}
          controlsEnabled={apiStatus === "online"}
          onTogglePause={async () => {
            try {
              await api.updateSettings({ indexing_paused: !indexingPaused });
              await refreshCoreData();
            } catch (error) {
              console.warn("failed to toggle indexing pause", error);
            }
          }}
          onCancelJob={async (job) => {
            const confirmed = await requestConfirm({
              title: t("jobs.confirm.cancel.title"),
              body: t("jobs.confirm.cancel.body"),
              confirmLabel: t("jobs.confirm.cancel.confirm"),
            });
            if (!confirmed) {
              return;
            }
            try {
              await api.cancelJob(job.id);
              await refreshCoreData();
            } catch (error) {
              console.warn("failed to cancel job", error);
            }
          }}
          onClose={() => setShowJobsSheet(false)}
          onOpenSettingsFix={(section) => {
            setShowJobsSheet(false);
            navigate("settings", { settingsSection: section });
          }}
          onOpenSources={() => {
            setShowJobsSheet(false);
            navigate("sources");
          }}
        />
      ) : null}
      <ConfirmDialog
        request={confirmRequest}
        onCancel={() => resolveConfirm(false)}
        onConfirm={() => resolveConfirm(true)}
      />
      {lm.show && !lm.minimized ? (
        <LocalModelConsent
          capability={lm.capability}
          download={lm.download}
          paused={lm.paused}
          onAgree={handleLmAgree}
          onDecline={handleLmDecline}
          onPause={lm.pauseDownload}
          onResume={lm.resumeDownload}
          onCancelDownload={lm.cancelDownload}
          onBackground={lm.background}
        />
      ) : null}
      {lm.ready ? (
        <button type="button" className="toast lm-toast" onClick={lm.dismissReady}>
          <Check size={15} />
          <span>{t("localModel.ready.toast")}</span>
        </button>
      ) : null}
    </div>
  );
}
