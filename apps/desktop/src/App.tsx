// Cerul Desktop — main application shell.
//
// NOTE on size: this file still hosts every screen, dialog, and helper.
// Splitting it into smaller modules is a tracked follow-up. Phase A
// of that split is done (formatters and settings helpers moved into
// ./lib/formatters.ts and ./lib/settings-helpers.ts). The remaining
// phases are tracked in this comment so the next contributor can pick
// up cleanly:
//
//   Phase B — extract leaf components into ./components/
//     InlineNotice, EmptyState, Metric, ModelDownloadBanner,
//     CoreBanner, ResultCard, ItemCard, ItemModalityIcon,
//     DetailIssuePanel.
//   Phase C — extract screens into ./screens/
//     HomeScreen, ResultsScreen, ResultDetail, LibraryScreen,
//     ItemDetail, SourcesScreen, SettingsScreen, Onboarding.
//   Phase D — extract dialogs into ./dialogs/
//     ConfirmDialog, JobsSheet, AddSourceDialog (+ tabs).
//   Phase E — extract item / source / job / result mappers into
//     ./lib/mappers.ts and route helpers into ./lib/route.ts.
//
// Each phase should land as its own PR so the diff stays reviewable.

import {
  AlertTriangle,
  Check,
  ChevronRight,
  CircleDot,
  Clock,
  Copy,
  Cpu,
  Database,
  Download,
  ExternalLink,
  FileAudio,
  FileVideo,
  Folder,
  HardDrive,
  Image as ImageIcon,
  Info,
  Library,
  ListChecks,
  ListFilter,
  Loader2,
  Mic,
  MoreHorizontal,
  Eye,
  Pause,
  Play,
  Plus,
  Podcast,
  RefreshCcw,
  ReceiptText,
  Search,
  Settings,
  SlidersHorizontal,
  Sparkles,
  Trash2,
  Video,
  Wrench,
  Youtube,
  X,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { FormEvent, KeyboardEvent, ReactNode } from "react";
import * as api from "./lib/api";
import { LangProvider, useLang, useT, type TFunction } from "./lib/i18n";
import {
  errorMessage,
  extractChunkIdFromThumbnail,
  formatBytes,
  formatDuration,
  formatTimestamp,
  formatUnixTime,
  formatUsd,
  metadataString,
  parseTimestampSeconds,
  pluralize,
  uniqueStrings,
} from "./lib/formatters";
import {
  resolveThemePreference,
  settingBoolean,
  settingNumber,
  settingString,
} from "./lib/settings-helpers";
import {
  EmptyState,
  InlineNotice,
} from "./components/leaf";
import {
  ProgressBar,
  StatusBadge,
  TranscriptList,
  TranscriptSkeleton,
  highlightSnippet,
} from "./components/transcript";
import { DetailIssuePanel } from "./components/detail-issue-panel";
import { CerulPlayer, type PlayerMarker } from "./components/player";
import {
  ItemCard,
  ItemModalityIcon,
  ResultCard,
  ResultModalityIcon,
} from "./components/cards";
import { CoreBanner } from "./components/core-banner";
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
import { IndexingStrip } from "./components/indexing-strip";
import { AddSourceDialog } from "./dialogs/add-source-dialog";
import { SourcesScreen } from "./screens/sources";
import { Onboarding } from "./screens/onboarding";
import { BrandLogo, BrandMark } from "./components/brand";
import { AccountRailButton } from "./components/account-sidebar";
import type {
  ApiStatus,
  AppData,
  ConfirmOptions,
  ConfirmRequest,
  CoreBannerAction,
  DaemonInstallResult,
  DaemonStatus,
  DetailIssue,
  Item,
  ItemSourceKind,
  ItemStatus,
  OnboardingYoutubeChannel,
  RequestConfirm,
  Result,
  ResultMatch,
  ResultModalityFilter,
  RouteState,
  SaveStatus,
  SettingsActionStatus,
  Source,
  SourceStatus,
  TranscriptLine,
  ValidationState,
  ValidationStatus,
  View,
} from "./lib/types";
import {
  isActiveJob,
  itemColor,
  itemDetailIssue,
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
  sourceError,
  sourceName,
  sourceStatus,
  sourceType,
} from "./lib/sources";
import {
  mapChunkRecords,
  mapSearchResults,
  resultModality,
  selectPlaybackChunkId,
} from "./lib/results";
import { readRouteState, routeHash } from "./lib/route";
import {
  canOpenOriginalSource,
  timestampDeepLink,
} from "./lib/detail";
import { durationMinutes, sortLibraryItems } from "./lib/library";
import { loadPersistedUiState, persistLastRoute, persistSidebarCollapsed } from "./lib/uiStore";
import type { PersistedRoute } from "./lib/uiStore";
import {
  checkForDesktopUpdate,
  hasDesktopHost,
  invokeHostCommand,
  openDialog,
} from "./lib/desktopHost";
import type { DesktopUpdate } from "./lib/desktopHost";

// Top-level navigation. Sub-pages (`result-detail`, `item-detail`) are reached
// by clicking a search result or library item, not from the sidebar.
// `onboarding` is a one-time flow accessed via Settings → "Re-run onboarding"
// after first launch, not a permanent destination.
// All valid View ids — broader than the sidebar so persisted routes for
// sub-pages (result-detail, item-detail) and onboarding still rehydrate.
const viewIds: View[] = [
  "home",
  "results",
  "result-detail",
  "library",
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
const globalHotkeyOptions = ["Alt+Space", "Ctrl+Space", "Ctrl+Shift+Space", "Cmd+Shift+Space"];
const recentSearchesStorageKey = "cerul.recentSearches.v1";

function hasOpenModalSurface() {
  return Boolean(document.querySelector(".scrim, .modal-backdrop, [role='dialog'][aria-modal='true']"));
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

async function setGlobalHotkey(label: string) {
  await invokeHostCommand("set_global_hotkey", { label });
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

function wait(ms: number) {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

function searchIndexIsSettling(data: AppData) {
  return (
    data.jobs.some(isActiveJob) ||
    data.items.some(
      (item) =>
        item.embeddingIndexStatus === "pending" ||
        item.visualIndexStatus === "pending",
    )
  );
}

const results: Result[] = [
  {
    id: "chunk-1",
    itemId: "item-1",
    title: "Karpathy - Software Is Changing Again",
    source: "YouTube / Andrej Karpathy",
    timestamp: "12:34",
    indexedAtEpoch: 1780444800,
    duration: "1:18:22",
    snippet:
      "The interesting part of test-time compute is that the model can spend more budget after the prompt arrives.",
    color: "mint",
    thumbnailUrl: null,
    confidence: "high",
    confidenceLabel: "Best",
    score: 0.048,
    scoreLabel: "Match 100%",
    scoreTitle: "Model similarity score",
    chunkType: "transcript",
    moreMatches: [
      {
        id: "chunk-1-2",
        timestamp: "18:02",
        snippet:
          "If the answer is difficult, test-time compute lets the system search longer before committing.",
        confidence: "medium",
        confidenceLabel: "Strong",
        scoreLabel: "Match 74%",
        scoreTitle: "Model similarity score",
      },
      {
        id: "chunk-1-3",
        timestamp: "41:27",
        snippet:
          "You can think of the runtime budget as part of the interface, not just the model.",
        confidence: "medium",
        confidenceLabel: "Strong",
        scoreLabel: "Match 68%",
        scoreTitle: "Model similarity score",
      },
      {
        id: "chunk-1-4",
        timestamp: "52:09",
        snippet:
          "The UI should cite the exact moment because the useful unit is a clip, not a whole file.",
        confidence: "low",
        confidenceLabel: "Review",
        scoreLabel: "Match 41%",
        scoreTitle: "Model similarity score",
      },
    ],
  },
  {
    id: "chunk-2",
    itemId: "item-2",
    title: "API-first Media Systems",
    source: "Folders / Talks 2026",
    timestamp: "34:10",
    indexedAtEpoch: 1780358400,
    duration: "49:08",
    snippet:
      "A media memory layer needs exact phrase search and semantic retrieval because users remember both words and scenes.",
    color: "amber",
    thumbnailUrl: null,
    confidence: "medium",
    confidenceLabel: "Strong",
    score: 0.031,
    scoreLabel: "Match 65%",
    scoreTitle: "Model similarity score",
    chunkType: "transcript",
    moreMatches: [
      {
        id: "chunk-2-2",
        timestamp: "37:44",
        snippet:
          "When search crosses transcript and frames, the result card needs to explain why it matched.",
        confidence: "low",
        confidenceLabel: "Review",
        scoreLabel: "Match 48%",
        scoreTitle: "Model similarity score",
      },
    ],
  },
  {
    id: "chunk-3",
    itemId: "item-3",
    title: "Podcast - Agents in Production",
    source: "Podcast RSS / Engineering Notes",
    timestamp: "08:52",
    indexedAtEpoch: 1778544000,
    duration: "56:41",
    snippet:
      "The agent should cite the moment in the source, not just return an answer without a timestamp.",
    color: "rose",
    thumbnailUrl: null,
    confidence: "low",
    confidenceLabel: "Review",
    score: 0.018,
    scoreLabel: "Match 38%",
    scoreTitle: "Model similarity score",
    chunkType: "transcript",
    moreMatches: [],
  },
];

const emptyUsageTotals: api.UsageTotals = {
  event_count: 0,
  request_count: 0,
  input_tokens: 0,
  output_tokens: 0,
  audio_seconds: 0,
  image_count: 0,
  video_seconds: 0,
  estimated_usd: 0,
  billed_credits: 0,
  unpriced_events: 0,
};

const items: Item[] = [
  {
    id: "item-1",
    title: "Karpathy - Software Is Changing Again",
    sourceId: "source-2",
    contentType: "video",
    source: "Andrej Karpathy",
    sourceKind: "youtube",
    duration: "1 h 18 m",
    indexedAt: "Today",
    indexedAtEpoch: 1780444800,
    status: "indexed",
    error: null,
    rawPath: null,
    originalUrl: "https://www.youtube.com/watch?v=demo-karpathy",
    color: "mint",
    thumbnailUrl: null,
    progress: null,
    progressLabel: null,
    etaLabel: null,
    visualIndexStatus: null,
    visualIndexMessage: null,
    embeddingIndexStatus: null,
    embeddingIndexMessage: null,
    usage: emptyUsageTotals,
  },
  {
    id: "item-2",
    title: "API-first Media Systems",
    sourceId: "source-1",
    contentType: "video",
    source: "Talks 2026",
    sourceKind: "folder",
    duration: "49 m",
    indexedAt: "Yesterday",
    indexedAtEpoch: 1780358400,
    status: "indexing",
    error: null,
    rawPath: "~/Movies/conferences/local-first-ai-systems.mp4",
    originalUrl: null,
    color: "amber",
    thumbnailUrl: null,
    progress: 0.42,
    progressLabel: "42%",
    etaLabel: null,
    visualIndexStatus: null,
    visualIndexMessage: null,
    embeddingIndexStatus: "pending",
    embeddingIndexMessage: null,
    usage: emptyUsageTotals,
  },
  {
    id: "item-3",
    title: "Agents in Production",
    sourceId: "source-3",
    contentType: "audio",
    source: "Engineering Notes",
    sourceKind: "podcast",
    duration: "56 m",
    indexedAt: "May 12",
    indexedAtEpoch: 1778544000,
    status: "indexed",
    error: null,
    rawPath: null,
    originalUrl: "https://example.com/engineering-notes/agents-in-production",
    color: "rose",
    thumbnailUrl: null,
    progress: null,
    progressLabel: null,
    etaLabel: null,
    visualIndexStatus: null,
    visualIndexMessage: null,
    embeddingIndexStatus: null,
    embeddingIndexMessage: null,
    usage: emptyUsageTotals,
  },
  {
    id: "item-4",
    title: "Multimodal Search Review",
    sourceId: "source-1",
    contentType: "video",
    source: "Design Reviews",
    sourceKind: "folder",
    duration: "23 m",
    indexedAt: "May 11",
    indexedAtEpoch: 1778457600,
    status: "failed",
    error: "The original file moved or was deleted from ~/Movies/design-reviews/multimodal-search-review.mp4.",
    rawPath: "~/Movies/design-reviews/multimodal-search-review.mp4",
    originalUrl: null,
    color: "steel",
    thumbnailUrl: null,
    progress: null,
    progressLabel: null,
    etaLabel: null,
    visualIndexStatus: null,
    visualIndexMessage: null,
    embeddingIndexStatus: null,
    embeddingIndexMessage: null,
    usage: emptyUsageTotals,
  },
  {
    id: "item-5",
    title: "Deleted YouTube Lecture",
    sourceId: "source-2",
    contentType: "video",
    source: "Andrej Karpathy",
    sourceKind: "youtube",
    duration: "41 m",
    indexedAt: "May 10",
    indexedAtEpoch: 1778371200,
    status: "failed",
    error: "yt-dlp fetch failed: video unavailable or private.",
    rawPath: null,
    originalUrl: "https://www.youtube.com/watch?v=deleted-demo",
    color: "mint",
    thumbnailUrl: null,
    progress: null,
    progressLabel: null,
    etaLabel: null,
    visualIndexStatus: null,
    visualIndexMessage: null,
    embeddingIndexStatus: null,
    embeddingIndexMessage: null,
    usage: emptyUsageTotals,
  },
];

const sources: Source[] = [
  {
    id: "source-1",
    type: "folder",
    name: "~/Movies/conferences",
    status: "active",
    items: 82,
    lastPolled: "2 min ago",
    error: null,
  },
  {
    id: "source-2",
    type: "youtube",
    name: "Andrej Karpathy",
    status: "active",
    items: 24,
    lastPolled: "1 h ago",
    error: null,
  },
  {
    id: "source-3",
    type: "podcast",
    name: "Engineering Notes",
    status: "paused",
    items: 17,
    lastPolled: "Yesterday",
    error: null,
  },
  {
    id: "source-4",
    type: "folder",
    name: "~/Movies/archive-drive",
    status: "error",
    items: 9,
    lastPolled: "May 12",
    error: "Cerul cannot reach this folder. Locate the drive or remove the source.",
  },
];

const demoJobs: api.JobRecord[] = [
  {
    id: "job-1",
    item_id: "item-2",
    job_type: "index_video",
    status: "running",
    started_at: Math.floor(Date.now() / 1000) - 75,
    finished_at: null,
    error: null,
    progress: 0.42,
    stage: "transcribing",
    stage_message: "Transcribing audio",
    usage: emptyUsageTotals,
  },
  {
    id: "job-2",
    item_id: "item-4",
    job_type: "index_video",
    status: "failed",
    started_at: null,
    finished_at: null,
    error: "The source file is missing.",
    progress: 1,
    stage: "failed",
    stage_message: "Index failed",
    usage: emptyUsageTotals,
  },
];

const transcript: TranscriptLine[] = [
  {
    id: "sample-1",
    time: "12:10",
    text: "Before we talk about search quality, we need to separate lexical recall from semantic recall.",
  },
  {
    id: "sample-2",
    time: "12:34",
    text: "The interesting part of test-time compute is that the model can spend more budget after the prompt arrives.",
    active: true,
  },
  {
    id: "sample-3",
    time: "13:02",
    text: "This changes how we evaluate memory products because the retrieval layer becomes part of the reasoning loop.",
  },
  {
    id: "sample-4",
    time: "13:41",
    text: "If the citation lands on the wrong moment, the user loses trust even when the answer sounds plausible.",
  },
];

const settingsSections = ["Models", "General", "Indexing", "Storage", "Advanced", "About"] as const;
type SettingsSection = (typeof settingsSections)[number];

function normalizeSettingsSection(section?: string | null): SettingsSection {
  if (section === "Cerul Cloud") {
    return "Models";
  }
  if (settingsSections.includes(section as SettingsSection)) {
    return section as SettingsSection;
  }
  return "Models";
}

function viewChromeLabel(view: View, settingsSection: string) {
  if (view === "result-detail" || view === "item-detail") {
    return "Item";
  }
  if (view === "settings") {
    return `Settings · ${settingsSection}`;
  }
  if (view === "onboarding") {
    return "Welcome";
  }
  return view[0].toUpperCase() + view.slice(1);
}

function visualFixtureModeEnabled() {
  const [, queryString = ""] = window.location.hash.replace(/^#/, "").split("?");
  const params = new URLSearchParams(queryString);
  return params.get("fixture") === "design";
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
  const initialRoute = readRouteState();
  const [view, setViewState] = useState<View>(initialRoute.view);
  const [selectedItemId, setSelectedItemId] = useState<string | null>(initialRoute.itemId);
  const [selectedChunkId, setSelectedChunkId] = useState<string | null>(initialRoute.chunkId);
  const [selectedTimestamp, setSelectedTimestamp] = useState<string | null>(
    initialRoute.timestamp,
  );
  const [query, setQuery] = useState(() =>
    visualFixtureModeEnabled() ? "test-time compute" : "",
  );
  const [recentSearches, setRecentSearches] = useState<string[]>(() => readRecentSearches());
  const [showAddSource, setShowAddSource] = useState(false);
  const [showJobsSheet, setShowJobsSheet] = useState(false);
  const [confirmRequest, setConfirmRequest] = useState<ConfirmRequest | null>(null);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
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
  const [isSearching, setIsSearching] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);
  const lastSearchRef = useRef<{ query: string; retryWhenIdle: boolean } | null>(null);

  const visualFixtureMode = visualFixtureModeEnabled();
  const screenApiStatus: ApiStatus = visualFixtureMode ? "online" : apiStatus;
  const visibleSources = visualFixtureMode
    ? sources
    : apiStatus === "online"
      ? data.sources
      : sources;
  const visibleItems = visualFixtureMode
    ? items
    : apiStatus === "online"
      ? data.items
      : items;
  const visibleResults = visualFixtureMode
    ? results
    : apiStatus === "online"
      ? liveResults
      : results;
  const visibleJobs = visualFixtureMode
    ? demoJobs
    : apiStatus === "online"
      ? data.jobs
      : [];
  const themePreference = settingString(data.settings, "theme", "Dark");
  const currentItem =
    visibleItems.find((item) => item.id === selectedItemId) ?? visibleItems[0] ?? items[0];
  const activeJobCount = visibleJobs.filter(isActiveJob).length;
  const stepStarts = useStepStarts(visibleJobs);

  useEffect(() => {
    function syncHashRoute() {
      const route = readRouteState();
      setViewState(route.view);
      setSelectedItemId(route.itemId);
      setSelectedChunkId(route.chunkId);
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

    window.addEventListener("hashchange", syncHashRoute);
    return () => window.removeEventListener("hashchange", syncHashRoute);
  }, []);

  useEffect(() => {
    let cancelled = false;

    loadPersistedUiState()
      .then((state) => {
        if (cancelled) {
          return;
        }

        if (typeof state.sidebarCollapsed === "boolean") {
          setSidebarCollapsed(state.sidebarCollapsed);
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

  useEffect(() => {
    void refreshCoreData();
  }, []);

  useEffect(() => {
    function handleGlobalKeyDown(event: globalThis.KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "n") {
        event.preventDefault();
        setShowAddSource(true);
      }
    }

    window.addEventListener("keydown", handleGlobalKeyDown);
    return () => window.removeEventListener("keydown", handleGlobalKeyDown);
  }, []);

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
    if (apiStatus !== "online" || activeJobCount === 0) {
      return;
    }

    const intervalId = window.setInterval(() => {
      void refreshCoreData();
    }, 2500);
    return () => window.clearInterval(intervalId);
  }, [apiStatus, activeJobCount]);

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
      const nextData: AppData = {
        sources: sourceRecords.map((source) => mapSourceRecord(source, mappedItems)),
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
      if (pendingRetry?.retryWhenIdle && !jobRecords.some(isActiveJob)) {
        lastSearchRef.current = { query: pendingRetry.query, retryWhenIdle: false };
        api
          .search(pendingRetry.query, 20)
          .then((records) => {
            setLiveResults(mapSearchResults(records, mappedItems, t));
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
      chunkId?: string | null;
      timestamp?: string | null;
      settingsSection?: string | null;
    } = {},
  ) {
    setShowJobsSheet(false);
    setShowAddSource(false);
    setSelectedItemId(params.itemId ?? null);
    setSelectedChunkId(params.chunkId ?? null);
    setSelectedTimestamp(params.timestamp ?? null);
    const routeParams =
      nextView === "settings"
        ? {
            ...params,
            settingsSection:
              params.settingsSection === undefined
                ? params.settingsSection
                : normalizeSettingsSection(params.settingsSection),
          }
        : params;
    if (nextView === "settings" && routeParams.settingsSection) {
      setSettingsSection(routeParams.settingsSection);
    }
    setViewState(nextView);
    const hash = routeHash(nextView, routeParams);
    window.location.hash = hash;
    void persistLastRoute({
      view: nextView,
      itemId: routeParams.itemId ?? null,
      chunkId: routeParams.chunkId ?? null,
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
    setSelectedChunkId(route.chunkId ?? null);
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

  function toggleSidebarCollapsed() {
    setSidebarCollapsed((current) => {
      const next = !current;
      void persistSidebarCollapsed(next);
      return next;
    });
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

  function submitSearch(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const submittedQuery =
      new FormData(event.currentTarget).get("query")?.toString() ??
      event.currentTarget.querySelector<HTMLInputElement>("input")?.value ??
      query;
    setQuery(submittedQuery);
    navigate("results");
    void runSearch(submittedQuery);
  }

  async function runSearch(value: string) {
    const trimmed = value.trim();
    if (visualFixtureMode) {
      setQuery(trimmed || value);
      setLiveResults(results);
      setSearchError(null);
      navigate("results", {});
      return;
    }
    if (!trimmed) {
      return;
    }

    rememberRecentSearch(trimmed);
    setIsSearching(true);
    setSearchError(null);
    try {
      const latestData = await refreshCoreData();
      if (!latestData && apiStatus !== "online") {
        throw new Error(t("common.coreUnreachable"));
      }
      const searchData = latestData ?? data;
      const itemsForResults = searchData.items;
      let retryWhenIndexSettles = searchIndexIsSettling(searchData);
      let found = await api.search(trimmed, 20);
      setLiveResults(mapSearchResults(found, itemsForResults, t));
      if (found.length === 0 || retryWhenIndexSettles) {
        await wait(650);
        const refreshed = await refreshCoreData();
        retryWhenIndexSettles = refreshed ? searchIndexIsSettling(refreshed) : retryWhenIndexSettles;
        found = await api.search(trimmed, 20);
        setLiveResults(mapSearchResults(found, refreshed?.items ?? itemsForResults, t));
      }
      lastSearchRef.current = {
        query: trimmed,
        retryWhenIdle: retryWhenIndexSettles,
      };
    } catch (error) {
      setSearchError(errorMessage(error));
      setApiStatus("error");
    } finally {
      setIsSearching(false);
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
      for (const folder of folders) {
        await api.addSource("folder_video", { path: folder });
      }
      for (const channel of youtubeChannels) {
        await api.addSource("youtube", { url: channel.url, max_videos: 50 });
      }
      setModelDownloadState({ status: "downloading", error: null });
      await api.updateSettings({
        inference_mode: "remote",
        asr_model: "whisper-1",
        active_embedding_profile: "gemini-embedding-2-3072",
      });
      await installDaemon();
      await refreshCoreData();
      setModelDownloadState({ status: "idle", error: null });
      navigate("home");
    } catch (error) {
      setModelDownloadState({ status: "error", error: errorMessage(error) });
    }
  }

  const sidebarActiveView = sidebarParentFor[view] ?? view;
  const railItems: { id: View; labelKey: string; icon: LucideIcon }[] = [
    { id: "home", labelKey: "nav.home", icon: Search },
    { id: "library", labelKey: "nav.library", icon: Library },
    { id: "sources", labelKey: "nav.sources", icon: Database },
  ];
  const mobileNavItems = [
    ...railItems,
    { id: "settings" as View, labelKey: "nav.settings", icon: Settings },
  ];
  const mobileTitleKey =
    mobileNavItems.find((item) => item.id === sidebarActiveView)?.labelKey ?? "nav.home";

  return (
    <div className="app" data-onboarding={view === "onboarding" ? "true" : undefined}>
      <aside className="rail" data-collapsed={sidebarCollapsed ? "true" : undefined}>
        <div className="rail-top">
          <button
            className="rail-brand"
            type="button"
            onClick={() => navigate("home")}
            aria-label={t("shell.openHome")}
          >
            <BrandMark />
          </button>
          <button
            className="btn-icon sm rail-collapse"
            type="button"
            aria-label={sidebarCollapsed ? t("shell.expandRail") : t("shell.collapseRail")}
            onClick={toggleSidebarCollapsed}
          >
            <ChevronRight size={16} />
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
            onClick={() => setShowJobsSheet(true)}
            title={t("nav.jobs")}
          >
            <span className="rail-ind" aria-hidden="true" />
            <span style={{ position: "relative", display: "inline-flex" }}>
              <ListChecks size={17} />
              {activeJobCount > 0 ? (
                <span className="badge-count" aria-hidden="true">
                  {activeJobCount > 9 ? "9+" : activeJobCount}
                </span>
              ) : null}
            </span>
            <span className="rail-label">{t("nav.jobs")}</span>
          </button>
          <button
            className={sidebarActiveView === "settings" ? "rail-item active" : "rail-item"}
            type="button"
            onClick={() => navigate("settings")}
            title={t("nav.settings")}
          >
            <span className="rail-ind" aria-hidden="true" />
            <Settings size={17} />
            <span className="rail-label">{t("nav.settings")}</span>
          </button>
          <AccountRailButton />
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
            {activeJobCount > 0 ? (
              <span className="badge-count" aria-hidden="true">
                {activeJobCount > 9 ? "9+" : activeJobCount}
              </span>
            ) : null}
          </span>
        </button>
      </div>

      <main className="content">
        {screenApiStatus !== "online" ? (
          <CoreBanner
            status={apiStatus}
            error={apiError}
            onAction={restartCoreConnection}
          />
        ) : null}
        {view === "onboarding" ? (
          <Onboarding
            step={onboardingStep}
            setStep={setOnboardingStep}
            apiStatus={screenApiStatus}
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
            onOpenItem={(item) => navigate("item-detail", { itemId: item.id })}
            onOpenLibrary={() => navigate("library")}
            items={visibleItems}
            sources={visibleSources}
            jobs={visibleJobs}
            apiStatus={screenApiStatus}
            onOpenModelSettings={() => navigate("settings", { settingsSection: "Models" })}
            globalHotkey={settingString(data.settings, "global_hotkey", "Alt+Space")}
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
                chunkId: result.id,
                timestamp: result.timestamp,
              })
            }
            results={visibleResults}
            isSearching={isSearching}
            error={searchError}
            apiStatus={screenApiStatus}
            hasIndexedItems={visibleItems.some((item) => item.status === "indexed")}
            hasActiveJobs={visibleJobs.some(isActiveJob)}
          />
        ) : null}
        {view === "result-detail" ? (
          <ResultDetail
            item={currentItem}
            startChunkId={selectedChunkId}
            startTimestamp={selectedTimestamp ?? "00:00"}
            actionsEnabled={screenApiStatus === "online"}
            onLibrary={() => navigate("results")}
            onDeleteItem={async (itemToDelete) => {
              await api.deleteItem(itemToDelete.id);
              await refreshCoreData();
              navigate("library");
            }}
            onReindexItem={async (itemToReindex) => {
              await api.reindexItem(itemToReindex.id);
              await refreshCoreData();
            }}
            requestConfirm={requestConfirm}
          />
        ) : null}
        {view === "library" ? (
          <LibraryScreen
            items={visibleItems}
            jobs={visibleJobs}
            stepStarts={stepStarts}
            actionsEnabled={screenApiStatus === "online"}
            onAddSource={() => setShowAddSource(true)}
            onOpenJobs={() => setShowJobsSheet(true)}
            onDeleteItems={async (itemIds) => {
              for (const itemId of itemIds) {
                await api.deleteItem(itemId);
              }
              await refreshCoreData();
            }}
            onReindexItems={async (itemIds) => {
              for (const itemId of itemIds) {
                await api.reindexItem(itemId);
              }
              await refreshCoreData();
            }}
            onOpenItem={(item) => navigate("item-detail", { itemId: item.id })}
            requestConfirm={requestConfirm}
          />
        ) : null}
        {view === "item-detail" ? (
          <ItemDetail
            item={currentItem}
            apiStatus={screenApiStatus}
            actionsEnabled={screenApiStatus === "online"}
            startTimestamp={selectedTimestamp ?? "0:00"}
            onBack={() => navigate("library")}
            modelLabel={asrModelLabel(settingString(data.settings, "asr_model", "whisper-1"))}
            onDeleteItem={async (itemToDelete) => {
              await api.deleteItem(itemToDelete.id);
              await refreshCoreData();
              navigate("library");
            }}
            onReindexItem={async (itemToReindex) => {
              await api.reindexItem(itemToReindex.id);
              await refreshCoreData();
            }}
            requestConfirm={requestConfirm}
          />
        ) : null}
        {view === "sources" ? (
          <SourcesScreen
            sources={visibleSources}
            actionsEnabled={screenApiStatus === "online"}
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
            onViewItems={() => navigate("library")}
            requestConfirm={requestConfirm}
          />
        ) : null}
        {view === "settings" ? (
          <SettingsScreen
            section={settingsSection}
            setSection={setSettingsSection}
            apiStatus={screenApiStatus}
            settings={data.settings}
            daemonStatus={data.daemonStatus}
            version={data.version}
            onSettingsChange={async (settings) => {
              await api.updateSettings(settings);
              await refreshCoreData();
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

      {showAddSource ? (
        <AddSourceDialog
          onClose={() => setShowAddSource(false)}
          onAddSource={async (type, config) => {
            await api.addSource(type, config);
            await refreshCoreData();
          }}
        />
      ) : null}
      {showJobsSheet ? (
        <JobsSheet
          jobs={visibleJobs}
          items={visibleItems}
          stepStarts={stepStarts}
          onClose={() => setShowJobsSheet(false)}
        />
      ) : null}
      <ConfirmDialog
        request={confirmRequest}
        onCancel={() => resolveConfirm(false)}
        onConfirm={() => resolveConfirm(true)}
      />
    </div>
  );
}

function submitSearchInputOnEnter(event: KeyboardEvent<HTMLInputElement>) {
  if (event.key !== "Enter" || event.nativeEvent.isComposing) {
    return;
  }
  event.preventDefault();
  event.currentTarget.form?.requestSubmit();
}

function HomeScreen({
  query,
  setQuery,
  onSubmit,
  onAddSource,
  onOpenItem,
  onOpenLibrary,
  items,
  sources,
  jobs,
  apiStatus,
  onOpenModelSettings,
  globalHotkey,
}: {
  query: string;
  setQuery: (value: string) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  onAddSource: () => void;
  onOpenItem: (item: Item) => void;
  onOpenLibrary: () => void;
  items: Item[];
  sources: Source[];
  jobs: api.JobRecord[];
  apiStatus: ApiStatus;
  onOpenModelSettings: () => void;
  globalHotkey: string;
}) {
  const t = useT();
  const indexedCount = items.filter((item) => item.status === "indexed").length;
  const activeSources = sources.filter((source) => source.status === "active").length;
  const activeJobs = jobs.filter(isActiveJob);
  const hasSources = sources.length > 0;
  const searchDisabled = hasSources && indexedCount === 0;
  const runtimeMinutes = items.reduce((total, item) => total + durationMinutes(item.duration), 0);
  const runtimeHours = Math.floor(runtimeMinutes / 60);
  const runtimeRemainder = runtimeMinutes % 60;
  const recentIndexed = items.filter((item) => item.status === "indexed").slice(0, 4);

  const statusLabel =
    activeJobs.length > 0
      ? t("home.status.indexingJobs", { count: activeJobs.length })
      : apiStatus === "online"
        ? searchDisabled
          ? t("home.status.indexingFirst")
          : t("home.status.indexedCount", { count: indexedCount })
        : t("home.status.coreStarting");

  function handleSearchSubmit(event: FormEvent<HTMLFormElement>) {
    if (searchDisabled) {
      event.preventDefault();
      return;
    }

    onSubmit(event);
  }

  if (!hasSources && apiStatus === "online") {
    return (
      <div className="page">
        <div className="state" style={{ marginTop: 96 }}>
          <div className="state-icon">
            <BrandMark />
          </div>
          <div className="state-title">{t("home.empty.title")}</div>
          <div className="state-sub">{t("home.empty.body")}</div>
          <div className="row gap-2" style={{ marginTop: 4 }}>
            <button className="btn btn-primary" type="button" onClick={onAddSource}>
              <Plus size={16} />
              <span>{t("home.empty.addFirst")}</span>
            </button>
            <button className="btn btn-secondary" type="button" onClick={onOpenModelSettings}>
              <Wrench size={16} />
              <span>{t("home.empty.configureModels")}</span>
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="page home-page" style={{ maxWidth: 760 }}>
      <div className="home-search-stage">
        <h1>{t("home.heading")}</h1>
        <p className="muted">
          {t("home.summary", {
            count: indexedCount,
            runtime:
              runtimeHours > 0
                ? t("home.runtime.hm", { hours: runtimeHours, minutes: runtimeRemainder })
                : t("home.runtime.m", { minutes: runtimeMinutes || 0 }),
            sources: activeSources,
          })}
        </p>

        <form
          className={searchDisabled ? "search-wrap disabled" : "search-wrap"}
          onSubmit={handleSearchSubmit}
          style={{ width: "100%", maxWidth: 600, marginTop: 30 }}
        >
          <Search size={18} />
          <input
            className="search-input"
            name="query"
            disabled={searchDisabled}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={submitSearchInputOnEnter}
            placeholder={
              searchDisabled ? t("home.searchLockedPlaceholder") : t("home.searchPlaceholder")
            }
            aria-label={t("home.searchAria")}
            aria-describedby={searchDisabled ? "home-search-helper" : undefined}
          />
        </form>
        {searchDisabled ? (
          <p className="field-hint" id="home-search-helper" style={{ marginTop: 10 }}>
            {t("home.lockedHint")}
          </p>
        ) : null}

        <div className="row gap-3 home-status-line">
          <span className="chip neutral">
            <span className="dot" />
            {statusLabel}
          </span>
          <span className="faint">{t("home.hotkeyHint", { hotkey: globalHotkey })}</span>
          <button className="btn btn-ghost sm" type="button" onClick={onOpenModelSettings}>
            <Wrench size={14} />
            <span>{t("home.metric.configure")}</span>
          </button>
        </div>
      </div>

      <div className="home-recent-block">
        <div className="row" style={{ justifyContent: "space-between", marginBottom: 14 }}>
          <p className="section-label" style={{ margin: 0 }}>{t("home.recentIndexed")}</p>
          <div className="row gap-2">
            <button className="btn btn-ghost sm" type="button" onClick={onOpenLibrary}>
              <span>{t("home.browseLibrary")}</span>
              <ChevronRight size={14} />
            </button>
            <button className="btn btn-ghost sm" type="button" onClick={onAddSource}>
              <Plus size={14} />
              <span>{t("home.addSource")}</span>
            </button>
          </div>
        </div>
        {recentIndexed.length > 0 ? (
          <div className="home-recent-grid">
            {recentIndexed.map((item) => (
              <RecentIndexedCard key={item.id} item={item} onOpen={() => onOpenItem(item)} />
            ))}
          </div>
        ) : (
          <EmptyState
            title={t("library.empty.none.title")}
            body={t("library.empty.none.body")}
            actionLabel={t("library.empty.addSource")}
            onAction={onAddSource}
          />
        )}
      </div>
    </div>
  );
}

function RecentIndexedCard({ item, onOpen }: { item: Item; onOpen: () => void }) {
  const t = useT();
  return (
    <button className="card hover lib-card recent-indexed-card" type="button" onClick={onOpen}>
      <span className={`thumb ${item.thumbnailUrl ? "has-image" : item.color}`}>
        {item.thumbnailUrl ? (
          <img src={item.thumbnailUrl} alt="" loading="lazy" />
        ) : (
          <ItemModalityIcon item={item} size={20} />
        )}
        {item.contentType !== "image" && item.duration ? (
          <small className="thumb-duration mono">{item.duration}</small>
        ) : null}
      </span>
      <span className="body">
        <strong className="clamp2">{item.title}</strong>
        <span className="recent-card-meta muted">
          {item.contentType !== "video" ? <ItemModalityIcon item={item} size={13} /> : null}
          <span>
            {item.indexedAt === "Never"
              ? t("library.itemCard.notIndexed")
              : t("library.itemCard.indexedAt", { when: item.indexedAt })}
          </span>
        </span>
        {item.visualIndexStatus === "failed" ? (
          <span className="item-warning chip warn">
            <span className="dot" />
            {t("library.itemCard.transcriptOnly")}
          </span>
        ) : null}
        {item.embeddingIndexStatus === "failed" ? (
          <span className="item-warning chip warn">
            <span className="dot" />
            {t("library.itemCard.partialIndex")}
          </span>
        ) : null}
      </span>
    </button>
  );
}

function ResultsScreen({
  query,
  setQuery,
  onSubmit,
  onBack,
  onOpen,
  results,
  isSearching,
  error,
  apiStatus,
  hasIndexedItems,
  hasActiveJobs,
}: {
  query: string;
  setQuery: (value: string) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  onBack: () => void;
  onOpen: (result: Result) => void;
  results: Result[];
  isSearching: boolean;
  error: string | null;
  apiStatus: ApiStatus;
  hasIndexedItems: boolean;
  hasActiveJobs: boolean;
}) {
  const t = useT();
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [expandedResultIds, setExpandedResultIds] = useState<Set<string>>(() => new Set());
  const [modalityFilter, setModalityFilter] = useState<ResultModalityFilter>("all");
  const [sortMode, setSortMode] = useState<"relevance" | "recent">("relevance");
  const filtersActive =
    modalityFilter !== "all" || sortMode !== "relevance";
  const filteredResults = results.filter((result) => {
    const matchesModality = modalityFilter === "all" || resultModality(result) === modalityFilter;
    return matchesModality;
  });
  const displayedResults =
    sortMode === "recent"
      ? [...filteredResults].sort(
          (left, right) =>
            (right.indexedAtEpoch ?? 0) - (left.indexedAtEpoch ?? 0) ||
            right.score - left.score,
        )
      : filteredResults;
  const modalityCounts = {
    all: results.length,
    audio: results.filter((result) => resultModality(result) === "audio").length,
    image: results.filter((result) => resultModality(result) === "image").length,
    video: results.filter((result) => resultModality(result) === "video").length,
  };
  const hasQuery = query.trim().length > 0;
  const hasSearched = hasQuery || results.length > 0;

  useEffect(() => {
    setSelectedIndex(0);
    setExpandedResultIds(new Set());
  }, [query, results.length, modalityFilter, sortMode]);

  function focusResult(index: number) {
    window.requestAnimationFrame(() => {
      document.querySelector<HTMLElement>(`[data-result-index="${index}"]`)?.focus();
    });
  }

  function clearResultFilters() {
    setModalityFilter("all");
    setSortMode("relevance");
  }

  function handleResultsKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (!displayedResults.length) {
      return;
    }

    if ((event.metaKey || event.ctrlKey) && event.key === "ArrowDown") {
      event.preventDefault();
      const selectedResult = displayedResults[Math.min(selectedIndex, displayedResults.length - 1)];
      if (selectedResult.moreMatches.length > 0) {
        setExpandedResultIds((current) => {
          const next = new Set(current);
          if (next.has(selectedResult.id)) {
            next.delete(selectedResult.id);
          } else {
            next.add(selectedResult.id);
          }
          return next;
        });
      }
      return;
    }

    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const direction = event.key === "ArrowDown" ? 1 : -1;
      const nextIndex = (selectedIndex + direction + displayedResults.length) % displayedResults.length;
      setSelectedIndex(nextIndex);
      focusResult(nextIndex);
    }

    if (event.key === "Enter" && event.target === event.currentTarget) {
      event.preventDefault();
      onOpen(displayedResults[Math.min(selectedIndex, displayedResults.length - 1)]);
    }
  }

  return (
    <>
      <div className="topbar">
        <div className="tb-inner">
          <button className="btn-icon" type="button" onClick={onBack} aria-label={t("results.backHome")}>
            <ChevronRight size={16} style={{ transform: "rotate(180deg)" }} />
          </button>
          <form className="search-wrap" onSubmit={onSubmit} style={{ flex: 1, maxWidth: 480 }}>
            <Search size={16} style={{ left: 12, width: 16, height: 16 }} />
            <input
              className="input"
              name="query"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={submitSearchInputOnEnter}
              placeholder={t("results.searchPlaceholder")}
              aria-label={t("results.searchAria")}
              style={{ height: 38, paddingLeft: 38 }}
            />
          </form>
          <span className="muted mono" style={{ fontSize: 12, marginLeft: "auto" }}>
            {t("results.status.hits", { count: displayedResults.length })}
          </span>
        </div>
      </div>

      <div className="page">
        <div className="row results-filter-row">
          <div className="segmented" aria-label={t("results.modeTabs.aria")}>
            <button
              type="button"
              className={modalityFilter === "all" ? "active" : ""}
              onClick={() => setModalityFilter("all")}
            >
              {t("results.modeTabs.all")} <span className="chip neutral">{modalityCounts.all}</span>
            </button>
            <button
              type="button"
              className={modalityFilter === "video" ? "active" : ""}
              onClick={() => setModalityFilter("video")}
            >
              {t("results.modeTabs.video")} <span className="chip neutral">{modalityCounts.video}</span>
            </button>
            <button
              type="button"
              className={modalityFilter === "image" ? "active" : ""}
              onClick={() => setModalityFilter("image")}
            >
              {t("results.modeTabs.shown")} <span className="chip neutral">{modalityCounts.image}</span>
            </button>
            <button
              type="button"
              className={modalityFilter === "audio" ? "active" : ""}
              onClick={() => setModalityFilter("audio")}
            >
              {t("results.modeTabs.audio")} <span className="chip neutral">{modalityCounts.audio}</span>
            </button>
          </div>
          <div className="row gap-2">
            <span className="muted" style={{ fontSize: 12.5 }}>{t("results.sort.label")}</span>
            <div className="segmented">
              <button
                type="button"
                className={sortMode === "relevance" ? "active" : ""}
                onClick={() => setSortMode("relevance")}
              >
                {t("results.sort.relevance")}
              </button>
              <button
                type="button"
                className={sortMode === "recent" ? "active" : ""}
                onClick={() => setSortMode("recent")}
              >
                {t("results.sort.recent")}
              </button>
            </div>
          </div>
        </div>

        {error ? (
          <div className="state danger" role="alert" style={{ marginTop: 12 }}>
            <div className="state-icon">
              <AlertTriangle size={18} />
            </div>
            <div className="state-sub">{error}</div>
          </div>
        ) : null}
        {apiStatus !== "online" ? (
          <p className="field-hint" style={{ marginTop: 10 }}>
            {t("results.notice.demo")}
          </p>
        ) : null}
        {hasSearched && !isSearching ? (
          <div className="row" style={{ alignItems: "center", gap: 10, marginTop: 12 }}>
            <span className="muted">
              {t("results.summary.count", {
                count: displayedResults.length,
                total: results.length,
              })}
            </span>
            {filtersActive ? (
              <button type="button" className="btn btn-ghost sm" onClick={clearResultFilters}>
                {t("common.clearFilters")}
              </button>
            ) : null}
          </div>
        ) : null}

        <div
          className={displayedResults.length > 0 || isSearching ? "card results-card-list" : "results-card-list"}
          tabIndex={displayedResults.length ? 0 : undefined}
          onKeyDown={handleResultsKeyDown}
          aria-label={t("results.list.aria")}
        >
          {isSearching ? <ResultsSkeletonList /> : null}
          {!isSearching && displayedResults.length > 0
            ? displayedResults.map((result, index) => (
              <ResultCard
                key={result.id}
                result={result}
                index={index}
                selected={index === selectedIndex}
                expanded={expandedResultIds.has(result.id)}
                onFocus={() => setSelectedIndex(index)}
                onOpen={onOpen}
                query={query}
              />
            ))
            : null}
          {!isSearching && displayedResults.length === 0 ? (
            !hasSearched ? (
              <EmptyState
                title={t("results.empty.initial.title")}
                body={t("results.empty.initial.body")}
              />
            ) : (
              <EmptyState
                title={
                  results.length > 0 && filtersActive
                    ? t("results.empty.filtered.title")
                    : !hasIndexedItems && hasActiveJobs
                      ? t("results.empty.indexing.title")
                      : t("results.empty.none.title")
                }
                body={
                  results.length > 0 && filtersActive
                    ? t("results.empty.filtered.body")
                    : !hasIndexedItems && hasActiveJobs
                      ? t("results.empty.indexing.body")
                      : t("results.empty.none.body")
                }
              />
            )
          ) : null}
        </div>
      </div>
    </>
  );
}

function ResultsSkeletonList() {
  return (
    <>
      {[0, 1, 2].map((index) => (
        <div className="result-row result-skeleton" key={index} aria-hidden="true">
          <span className="sk" style={{ width: 132, height: 74, borderRadius: "var(--r-md)" }} />
          <span className="col gap-2" style={{ paddingTop: 4 }}>
            <span className="sk" style={{ height: 13, width: "70%" }} />
            <span className="sk" style={{ height: 11, width: "92%" }} />
            <span className="sk" style={{ height: 11, width: "55%" }} />
          </span>
          <span className="sk" style={{ height: 11, width: 44 }} />
        </div>
      ))}
    </>
  );
}

function ResultDetail({
  item,
  startChunkId,
  startTimestamp,
  actionsEnabled,
  onLibrary,
  onDeleteItem,
  onReindexItem,
  requestConfirm,
}: {
  item: Item;
  startChunkId: string | null;
  startTimestamp: string;
  actionsEnabled: boolean;
  onLibrary: () => void;
  onDeleteItem: (item: Item) => Promise<void>;
  onReindexItem: (item: Item) => Promise<void>;
  requestConfirm: RequestConfirm;
}) {
  const t = useT();
  const [copyStatus, setCopyStatus] = useState<"idle" | "copied" | "error">("idle");
  const [currentTimestamp, setCurrentTimestamp] = useState(startTimestamp);
  const [isPlaying, setIsPlaying] = useState(true);
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const shouldAutoPlayRef = useRef(true);
  const [mediaState, setMediaState] = useState<{
    status: "idle" | "loading" | "ready" | "error";
    chunkId: string | null;
    lines: TranscriptLine[];
    message: string | null;
  }>({ status: "idle", chunkId: null, lines: transcript, message: null });
  const [itemAction, setItemAction] = useState<{
    status: "idle" | "locating" | "reindexing" | "deleting" | "queued" | "error";
    message: string | null;
  }>({ status: "idle", message: null });
  const [clipExportStatus, setClipExportStatus] = useState<"idle" | "exporting" | "done">("idle");
  const detailIssue = itemDetailIssue(item, t);
  const transcriptLines =
    actionsEnabled && mediaState.status !== "idle" ? mediaState.lines : transcript;
  const playbackUrl =
    item.contentType === "video" && mediaState.chunkId
      ? api.videoSegmentUrl(mediaState.chunkId)
      : null;
  const timestampLink = timestampDeepLink(item.id, currentTimestamp);
  const transcriptPartial = item.status === "indexing";
  const itemBusy =
    itemAction.status === "locating" ||
    itemAction.status === "reindexing" ||
    itemAction.status === "deleting";
  const canExportClip = item.contentType === "video" && Boolean(mediaState.chunkId);
  const otherMatches = transcriptLines
    .filter((line) => line.time !== startTimestamp)
    .slice(0, 2)
    .map((line) => line.time);
  const playerMarkers: PlayerMarker[] = transcriptLines
    .map((line) => ({
      seconds: parseTimestampSeconds(line.time),
      label: line.time,
      text: line.text,
      match: line.time === startTimestamp,
    }))
    .filter((marker) => Number.isFinite(marker.seconds) && marker.seconds >= 0);

  useEffect(() => {
    setCurrentTimestamp(startTimestamp);
    setIsPlaying(true);
    setItemAction({ status: "idle", message: null });
    setClipExportStatus("idle");
  }, [item.id, startTimestamp]);

  useEffect(() => {
    if (visualFixtureModeEnabled()) {
      setMediaState({
        status: "ready",
        chunkId: startChunkId,
        lines: transcript,
        message: null,
      });
      return;
    }
    if (!actionsEnabled) {
      setMediaState({ status: "idle", chunkId: null, lines: transcript, message: null });
      return;
    }

    let cancelled = false;
    setMediaState({ status: "loading", chunkId: startChunkId, lines: [], message: null });
    api
      .listItemChunks(item.id)
      .then((records) => {
        if (cancelled) {
          return;
        }
        setMediaState({
          status: "ready",
          chunkId: selectPlaybackChunkId(records, startTimestamp, startChunkId),
          lines: mapChunkRecords(records),
          message: null,
        });
      })
      .catch((error) => {
        if (cancelled) {
          return;
        }
        setMediaState({
          status: "error",
          chunkId: startChunkId,
          lines: [],
          message: errorMessage(error),
        });
      });

    return () => {
      cancelled = true;
    };
  }, [actionsEnabled, item.id, startChunkId, startTimestamp]);

  useEffect(() => {
    shouldAutoPlayRef.current = isPlaying;
  }, [isPlaying]);

  useEffect(() => {
    const video = videoRef.current;
    if (!video || !playbackUrl) {
      return;
    }

    let cancelled = false;
    const targetSeconds = parseTimestampSeconds(currentTimestamp);
    const syncVideo = () => {
      if (cancelled) {
        return;
      }
      if (Number.isFinite(targetSeconds)) {
        const maxTime = Number.isFinite(video.duration)
          ? Math.max(video.duration - 0.1, 0)
          : targetSeconds;
        video.currentTime = Math.min(targetSeconds, maxTime);
      }
      if (shouldAutoPlayRef.current) {
        void video.play().catch(() => {
          if (!cancelled) {
            setIsPlaying(false);
          }
        });
      }
    };

    if (video.readyState >= 1) {
      syncVideo();
    } else {
      video.addEventListener("loadedmetadata", syncVideo, { once: true });
    }

    return () => {
      cancelled = true;
      video.removeEventListener("loadedmetadata", syncVideo);
    };
  }, [currentTimestamp, playbackUrl]);

  useEffect(() => {
    if (copyStatus === "idle") {
      return;
    }

    const timeout = window.setTimeout(() => setCopyStatus("idle"), 1600);
    return () => window.clearTimeout(timeout);
  }, [copyStatus]);

  useEffect(() => {
    function onKeyDown(event: globalThis.KeyboardEvent) {
      if (hasOpenModalSurface()) {
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        onLibrary();
        return;
      }
      const target = event.target as HTMLElement | null;
      if (
        target &&
        (target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.tagName === "BUTTON" ||
          target.tagName === "A" ||
          target.tagName === "SELECT" ||
          target.tagName === "VIDEO" ||
          target.isContentEditable ||
          target.getAttribute("role") === "button")
      ) {
        return;
      }
      const video = videoRef.current;
      if (event.key === " " || event.code === "Space") {
        event.preventDefault();
        if (video) {
          if (video.paused) {
            void video.play().catch(() => undefined);
          } else {
            video.pause();
          }
        } else {
          setIsPlaying((playing) => !playing);
        }
        return;
      }
      if (!video) {
        return;
      }
      if (event.key === "ArrowRight") {
        event.preventDefault();
        video.currentTime = Math.min(video.duration || Number.POSITIVE_INFINITY, video.currentTime + 5);
      } else if (event.key === "ArrowLeft") {
        event.preventDefault();
        video.currentTime = Math.max(0, video.currentTime - 5);
      } else if (event.key === "ArrowUp") {
        event.preventDefault();
        video.volume = Math.min(1, video.volume + 0.1);
      } else if (event.key === "ArrowDown") {
        event.preventDefault();
        video.volume = Math.max(0, video.volume - 0.1);
      } else if (event.key.toLowerCase() === "m") {
        video.muted = !video.muted;
      } else if (event.key.toLowerCase() === "f") {
        if (document.fullscreenElement) {
          void document.exitFullscreen().catch(() => undefined);
        } else if (video.requestFullscreen) {
          void video.requestFullscreen().catch(() => undefined);
        }
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onLibrary]);

  async function copyTimestampLink() {
    try {
      await writeClipboardText(timestampLink);
      setCopyStatus("copied");
    } catch {
      setCopyStatus("error");
    }
  }

  function seekTo(timestamp: string) {
    setCurrentTimestamp(timestamp);
    setClipExportStatus("idle");
    setIsPlaying(true);
    const targetSeconds = parseTimestampSeconds(timestamp);
    const nearestLine = transcriptLines
      .filter((line) => Number.isFinite(parseTimestampSeconds(line.time)))
      .sort(
        (left, right) =>
          Math.abs(parseTimestampSeconds(left.time) - targetSeconds) -
          Math.abs(parseTimestampSeconds(right.time) - targetSeconds),
      )[0];
    if (nearestLine) {
      setMediaState((current) => ({ ...current, chunkId: nearestLine.id }));
    }
  }

  async function exportCurrentClip() {
    if (!canExportClip || !mediaState.chunkId) {
      return;
    }
    setClipExportStatus("exporting");
    setItemAction({ status: "idle", message: null });
    try {
      const response = await fetch(api.videoClipUrl(mediaState.chunkId));
      if (!response.ok) {
        throw new Error(t("detail.action.exportFailed", { status: response.status }));
      }
      const blob = await response.blob();
      const objectUrl = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = objectUrl;
      anchor.download = `cerul-clip-${currentTimestamp.replace(/:/g, "-")}.mp4`;
      document.body.appendChild(anchor);
      anchor.click();
      anchor.remove();
      window.setTimeout(() => URL.revokeObjectURL(objectUrl), 4000);
      setClipExportStatus("done");
      setItemAction({ status: "idle", message: t("detail.action.clipExported") });
    } catch (error) {
      setClipExportStatus("idle");
      setItemAction({ status: "error", message: errorMessage(error) });
    }
  }

  async function locateSourceFile() {
    setItemAction({ status: "locating", message: null });
    const selected = await openDialog({
      multiple: false,
      directory: false,
      filters: [{ name: "Video", extensions: ["mp4", "mkv", "webm", "mov", "m4v"] }],
    }).catch(() => null);
    if (typeof selected === "string" && selected.trim()) {
      setItemAction({
        status: "queued",
        message: t("detail.locatedSource", { path: selected }),
      });
      return;
    }
    setItemAction({ status: "idle", message: null });
  }

  async function openOriginalSource() {
    if (!canOpenOriginalSource(item)) {
      return;
    }
    if (!item.originalUrl) {
      setItemAction({ status: "locating", message: null });
    }
    try {
      const message = await openOriginalSourceForItem(item, t);
      if (!item.originalUrl) {
        setItemAction({ status: "queued", message });
      }
    } catch (error) {
      setItemAction({ status: "error", message: errorMessage(error) });
    }
  }

  async function reindexCurrentItem() {
    if (!actionsEnabled) {
      setItemAction({ status: "error", message: t("common.coreUnreachable") });
      return;
    }

    const confirmed = await requestConfirm({
      title: t("common.confirm.reindex.title"),
      body: t("common.confirm.reindex.body"),
      confirmLabel: t("common.reindex"),
    });
    if (!confirmed) {
      return;
    }

    setItemAction({ status: "reindexing", message: null });
    try {
      await onReindexItem(item);
      setItemAction({ status: "queued", message: t("common.reindexQueued") });
    } catch (error) {
      setItemAction({ status: "error", message: errorMessage(error) });
    }
  }

  async function deleteCurrentItem() {
    if (!actionsEnabled) {
      setItemAction({ status: "error", message: t("common.coreUnreachable") });
      return;
    }
    const confirmed = await requestConfirm({
      title: t("common.confirm.delete.title"),
      body: t("common.confirm.delete.body", { title: item.title }),
      confirmLabel: t("common.delete"),
    });
    if (!confirmed) {
      return;
    }

    setItemAction({ status: "deleting", message: null });
    try {
      await onDeleteItem(item);
    } catch (error) {
      setItemAction({ status: "error", message: errorMessage(error) });
    }
  }

  return (
    <div className="detail-view">
      <div className="topbar">
        <div className="tb-inner" style={{ maxWidth: 1180 }}>
          <button className="btn-icon" type="button" onClick={onLibrary} aria-label={t("detail.backToResults")}>
            <ChevronRight size={16} style={{ transform: "rotate(180deg)" }} />
          </button>
          <span className="tb-title clamp1">{item.title}</span>
          <div className="row gap-2" style={{ marginLeft: "auto" }}>
            <button className="btn btn-ghost sm" type="button" onClick={copyTimestampLink}>
              {copyStatus === "copied" ? <Check size={15} /> : <Copy size={15} />}
              <span>{copyStatus === "copied" ? t("detail.copy.copied") : t("detail.copy.label")}</span>
            </button>
            <button
              className="btn btn-secondary sm"
              type="button"
              disabled={!canOpenOriginalSource(item) || itemBusy}
              onClick={() => void openOriginalSource()}
            >
              {item.originalUrl ? <ExternalLink size={15} /> : <Folder size={15} />}
              <span>{item.originalUrl ? t("detail.source.openOriginal") : t("detail.source.reveal")}</span>
            </button>
          </div>
        </div>
      </div>

      <div className="page" style={{ maxWidth: 1180 }}>
        <div className="detail-split">
          <div className="detail-media">
            <div className="row gap-2" style={{ marginBottom: 12, flexWrap: "wrap" }}>
              <span className="chip neutral">{item.source}</span>
              <span className="chip success">
                <span className="dot" />
                {item.indexedAt === "Never" ? t("detail.notIndexed") : t("detail.indexedAt", { when: item.indexedAt })}
              </span>
              <span className="mono faint" style={{ fontSize: 12 }}>{item.duration}</span>
            </div>
            {detailIssue ? (
              <div className="video-frame thumb video-frame-unavailable">
                <div className="stripes" aria-hidden="true" />
                <DetailIssuePanel
                  issue={detailIssue}
                  actionStatus={itemAction.status}
                  actionsEnabled={actionsEnabled}
                  hasOriginalUrl={Boolean(item.originalUrl)}
                  onLocate={() => void locateSourceFile()}
                  onOpenOriginal={() => void openOriginalSource()}
                  onReindex={() => void reindexCurrentItem()}
                  onRemove={() => void deleteCurrentItem()}
                />
              </div>
            ) : playbackUrl ? (
              <CerulPlayer
                videoRef={videoRef}
                src={playbackUrl}
                markers={playerMarkers}
                ariaLabel={t("itemDetail.player.aria", { title: item.title })}
                onPlay={() => setIsPlaying(true)}
                onPause={() => setIsPlaying(false)}
                onSeekMarker={(marker) => seekTo(marker.label)}
              />
            ) : mediaState.status === "loading" ? (
              <div className={`video-frame thumb ${item.color}`}>
                <div className="stripes" aria-hidden="true" />
                <div className="player-loading" role="status">
                  <Loader2 size={24} />
                  <span>{t("detail.player.preparing")}</span>
                </div>
              </div>
            ) : (
              <div className={`video-frame thumb ${item.color}`}>
                <div className="stripes" aria-hidden="true" />
                <div className="player-placeholder">
                  <button
                    className="play-button"
                    type="button"
                    aria-label={isPlaying ? t("detail.player.pauseAria") : t("detail.player.playAria")}
                    onClick={() => setIsPlaying((playing) => !playing)}
                  >
                    {isPlaying ? <Pause size={22} fill="currentColor" /> : <Play size={22} fill="currentColor" />}
                  </button>
                </div>
              </div>
            )}

            <div className="row gap-2" style={{ marginTop: 14, flexWrap: "wrap" }}>
              {item.contentType === "video" ? (
                <button
                  className="btn btn-secondary sm"
                  type="button"
                  disabled={!canExportClip || itemBusy || clipExportStatus === "exporting"}
                  onClick={exportCurrentClip}
                >
                  {clipExportStatus === "exporting" ? <Loader2 size={15} /> : <Download size={15} />}
                  <span>
                    {clipExportStatus === "exporting"
                      ? t("detail.action.exportingClip")
                      : clipExportStatus === "done"
                        ? t("detail.action.clipExported")
                        : t("detail.action.exportClip")}
                  </span>
                </button>
              ) : null}
              <button className="btn btn-secondary sm" type="button" disabled={itemBusy} onClick={() => void reindexCurrentItem()}>
                {itemAction.status === "reindexing" ? <Loader2 size={15} /> : <RefreshCcw size={15} />}
                <span>{itemAction.status === "reindexing" ? t("common.reindexing") : t("common.reindex")}</span>
              </button>
              <button className="btn btn-danger sm" type="button" disabled={itemBusy} onClick={() => void deleteCurrentItem()}>
                {itemAction.status === "deleting" ? <Loader2 size={15} /> : <Trash2 size={15} />}
                <span>{itemAction.status === "deleting" ? t("common.deleting") : t("common.delete")}</span>
              </button>
            </div>

            <VideoUnderstandingPanel
              item={item}
              enabled={actionsEnabled}
              onSeek={seekTo}
              requestConfirm={requestConfirm}
            />
          </div>

          <div className="detail-transcript">
            <div className="row" style={{ justifyContent: "space-between", alignItems: "center", marginBottom: 8 }}>
              <div>
                <p className="section-label" style={{ marginBottom: 2 }}>{t("detail.transcript.eyebrow")}</p>
                <span className="faint mono" style={{ fontSize: 12 }}>{t("detail.transcript.chunkCount", { count: transcriptLines.length })}</span>
              </div>
              {otherMatches.length > 0 ? (
                <div className="row gap-1" aria-label={t("detail.otherMatches")}>
                  {otherMatches.map((timestamp) => (
                    <button
                      key={timestamp}
                      type="button"
                      className={timestamp === currentTimestamp ? "chip accent" : "chip neutral"}
                      onClick={() => seekTo(timestamp)}
                    >
                      {timestamp}
                    </button>
                  ))}
                </div>
              ) : null}
            </div>

            {copyStatus === "error" ? <InlineNotice tone="error" message={t("detail.copy.error")} /> : null}
            {copyStatus === "copied" ? <InlineNotice tone="muted" message={t("detail.copy.success")} /> : null}
            {itemAction.message ? (
              <InlineNotice
                tone={itemAction.status === "error" ? "error" : "muted"}
                message={itemAction.message}
              />
            ) : null}
            {transcriptPartial ? <InlineNotice tone="muted" message={t("detail.stillProcessing")} /> : null}
            {item.visualIndexMessage ? <InlineNotice tone="muted" message={item.visualIndexMessage} /> : null}
            {item.embeddingIndexMessage ? <InlineNotice tone="muted" message={item.embeddingIndexMessage} /> : null}
            {mediaState.status === "loading" ? <TranscriptSkeleton /> : null}
            {mediaState.status === "error" && mediaState.message ? (
              <InlineNotice tone="error" message={mediaState.message} />
            ) : null}
            <TranscriptList
              lines={transcriptLines}
              activeTime={currentTimestamp}
              matchTime={startTimestamp}
              onSeek={seekTo}
            />
          </div>
        </div>
      </div>
    </div>
  );
}

function VideoUnderstandingPanel({
  item,
  enabled,
  onSeek,
  requestConfirm,
}: {
  item: Item;
  enabled: boolean;
  onSeek?: (timestamp: string) => void;
  requestConfirm: RequestConfirm;
}) {
  const t = useT();
  const [state, setState] = useState<{
    status: "idle" | "loading" | "analyzing" | "loaded" | "error";
    record: api.VideoUnderstandingRecord | null;
    message: string | null;
  }>({ status: "idle", record: null, message: null });
  const record = state.record;
  const isPending = state.status === "loading" || state.status === "analyzing";
  // Elapsed timer for the analyze run. The request is a single blocking call
  // (upload → Gemini processing → generate) with no server-side progress, so an
  // indeterminate bar + elapsed clock is the honest signal that work is ongoing.
  const [analyzeElapsedMs, setAnalyzeElapsedMs] = useState(0);

  useEffect(() => {
    if (!enabled || item.contentType !== "video") {
      setState({ status: "idle", record: null, message: null });
      return;
    }

    let cancelled = false;
    setState({ status: "loading", record: null, message: null });
    api
      .getItemUnderstanding(item.id)
      .then((next) => {
        if (!cancelled) {
          setState({ status: "loaded", record: next, message: null });
        }
      })
      .catch(() => {
        // A missing/unavailable understanding record is not a hard error for
        // this secondary panel — fall back to the "not analyzed" empty state
        // instead of flashing a red notice. Explicit Analyze failures below
        // still surface their message.
        if (!cancelled) {
          setState({ status: "idle", record: null, message: null });
        }
      });

    return () => {
      cancelled = true;
    };
  }, [enabled, item.contentType, item.id]);

  useEffect(() => {
    if (state.status !== "analyzing") {
      setAnalyzeElapsedMs(0);
      return;
    }
    const startedAt = performance.now();
    const interval = window.setInterval(() => {
      setAnalyzeElapsedMs(performance.now() - startedAt);
    }, 500);
    return () => window.clearInterval(interval);
  }, [state.status]);

  if (item.contentType !== "video") {
    return null;
  }

  async function analyze() {
    if (!enabled || isPending) {
      return;
    }
    const confirmed = await requestConfirm({
      title: t("understanding.confirm.title"),
      body: t("understanding.confirm.body"),
      confirmLabel: t("understanding.confirm.label"),
    });
    if (!confirmed) {
      return;
    }
    setState((current) => ({
      status: "analyzing",
      record: current.record,
      message: null,
    }));
    try {
      const next = await api.analyzeItemUnderstanding(item.id);
      setState({ status: "loaded", record: next, message: null });
    } catch (error) {
      setState((current) => ({
        status: "error",
        record: current.record,
        message: errorMessage(error),
      }));
    }
  }

  const analysisStatus = record?.status ?? "not_started";
  const statusLabel =
    state.status === "loading"
      ? t("understanding.status.loading")
      : state.status === "analyzing"
        ? t("understanding.status.analyzing")
        : analysisStatus === "completed"
          ? t("understanding.status.analyzed")
          : analysisStatus === "failed"
            ? t("understanding.status.failed")
            : t("understanding.status.notAnalyzed");
  const statusChipClass =
    analysisStatus === "completed"
      ? "chip success"
      : analysisStatus === "failed"
        ? "chip danger"
        : state.status === "analyzing" || state.status === "loading"
          ? "chip accent"
          : "chip neutral";
  const summary = record?.summary?.trim() ?? "";
  const chapters = record?.chapters ?? [];
  const events = record?.events ?? [];
  const topics = record?.topics ?? [];
  const canAnalyze = enabled && !isPending;
  const privacyNote = t("understanding.privacyNote");

  return (
    <section className={`understanding-panel ${analysisStatus}`}>
      <div className="understanding-header">
        <div>
          <p className="section-label" style={{ marginBottom: 2 }}>{t("understanding.eyebrow")}</p>
          <strong>{t("understanding.title")}</strong>
        </div>
        <span className={statusChipClass}>
          <span className="dot" />
          {statusLabel}
        </span>
      </div>

      {state.message ? <InlineNotice tone="error" message={state.message} /> : null}
      {record?.error && analysisStatus === "failed" ? (
        <InlineNotice tone="error" message={record.error} />
      ) : null}

      {state.status === "analyzing" ? (
        <div className="understanding-progress" role="status" aria-live="polite">
          <div className="understanding-progress-track" aria-hidden="true">
            <span className="understanding-progress-fill" />
          </div>
          <div className="understanding-progress-meta">
            <span>{t("understanding.status.analyzing")}</span>
            <span className="mono faint">{formatTimestamp(Math.round(analyzeElapsedMs / 1000))}</span>
          </div>
          <p className="field-hint">{t("understanding.progress.hint")}</p>
        </div>
      ) : null}

      {summary ? (
        <p className="understanding-summary">{summary}</p>
      ) : state.status === "loading" ? (
        <div className="understanding-skeleton" aria-hidden="true">
          <span className="sk" />
          <span className="sk" />
        </div>
      ) : (
        <p className="field-hint">{t("understanding.empty")}</p>
      )}

      {topics.length > 0 ? (
        <div className="understanding-topics" aria-label={t("understanding.topics.aria")}>
          {topics.slice(0, 8).map((topic) => (
            <span key={topic} className="chip neutral">{topic}</span>
          ))}
        </div>
      ) : null}

      <p className="field-hint">{privacyNote}</p>

      {chapters.length > 0 ? (
        <div className="understanding-list">
          <strong>{t("understanding.chapters")}</strong>
          {chapters.slice(0, 4).map((chapter, index) => (
            <button
              className="understanding-row"
              key={`${chapter.title}-${index}`}
              type="button"
              disabled={!onSeek}
              onClick={() => onSeek?.(formatTimestamp(chapter.start_sec))}
            >
              <span className="kbd">{formatTimestamp(chapter.start_sec)}</span>
              <p>
                <b>{chapter.title}</b>
                {chapter.summary ? ` ${chapter.summary}` : ""}
              </p>
            </button>
          ))}
        </div>
      ) : null}

      {events.length > 0 ? (
        <div className="understanding-list">
          <strong>{t("understanding.keyMoments")}</strong>
          {events.slice(0, 5).map((event, index) => (
            <button
              className="understanding-row"
              key={`${event.caption}-${index}`}
              type="button"
              disabled={!onSeek}
              onClick={() => onSeek?.(formatTimestamp(event.start_sec))}
            >
              <span className="kbd">{formatTimestamp(event.start_sec)}</span>
              <p>
                {event.caption}
                {typeof event.confidence === "number"
                  ? ` · ${Math.round(event.confidence * 100)}%`
                  : ""}
              </p>
            </button>
          ))}
        </div>
      ) : null}

      <button
        type="button"
        className="btn btn-primary sm understanding-action"
        disabled={!canAnalyze}
        onClick={() => void analyze()}
      >
        {isPending ? <Loader2 size={15} /> : <Sparkles size={15} />}
        <span>
          {isPending
            ? t("understanding.status.analyzing")
            : analysisStatus === "completed"
              ? t("understanding.action.reanalyze")
              : t("understanding.action.analyze")}
        </span>
      </button>
    </section>
  );
}

async function openOriginalSourceForItem(item: Item, t: TFunction) {
  if (item.originalUrl) {
    window.open(item.originalUrl, "_blank", "noopener,noreferrer");
    return t("detail.source.opened");
  }
  if (item.rawPath) {
    await revealSourcePath(item.rawPath);
    return t("detail.source.revealed");
  }
  throw new Error(t("detail.source.unavailable"));
}

async function writeClipboardText(text: string) {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }

  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();
  const copied = document.execCommand("copy");
  document.body.removeChild(textarea);

  if (!copied) {
    throw new Error("clipboard copy failed");
  }
}

function LibraryScreen({
  items,
  jobs,
  stepStarts,
  actionsEnabled,
  onAddSource,
  onDeleteItems,
  onReindexItems,
  onOpenItem,
  onOpenJobs,
  requestConfirm,
}: {
  items: Item[];
  jobs: api.JobRecord[];
  stepStarts: Record<string, number>;
  actionsEnabled: boolean;
  onAddSource: () => void;
  onDeleteItems: (itemIds: string[]) => Promise<void>;
  onReindexItems: (itemIds: string[]) => Promise<void>;
  onOpenItem: (item: Item) => void;
  onOpenJobs: () => void;
  requestConfirm: RequestConfirm;
}) {
  const t = useT();
  const [libraryQuery, setLibraryQuery] = useState("");
  const [sourceFilter, setSourceFilter] = useState("all");
  const [statusFilter, setStatusFilter] = useState("all");
  const [sortKey, setSortKey] = useState<"recent" | "longest" | "shortest" | "title">("recent");
  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");
  const [selectedItemIds, setSelectedItemIds] = useState<Set<string>>(new Set());
  const [batchState, setBatchState] = useState<{
    status: "idle" | "reindexing" | "deleting" | "error";
    message: string | null;
  }>({ status: "idle", message: null });
  const sourceOptions = Array.from(new Set(items.map((item) => item.source))).sort((a, b) =>
    a.localeCompare(b),
  );
  const normalizedQuery = libraryQuery.trim().toLowerCase();
  const filtersActive =
    normalizedQuery !== "" ||
    sourceFilter !== "all" ||
    statusFilter !== "all" ||
    sortKey !== "recent";
  const filteredItems = items
    .filter((item) => {
      const matchesQuery =
        normalizedQuery === "" ||
        item.title.toLowerCase().includes(normalizedQuery) ||
        item.source.toLowerCase().includes(normalizedQuery);
      const matchesSource = sourceFilter === "all" || item.source === sourceFilter;
      const matchesStatus = statusFilter === "all" || item.status === statusFilter;
      return matchesQuery && matchesSource && matchesStatus;
    })
    .sort((a, b) => sortLibraryItems(a, b, sortKey));
  const selectedCount = selectedItemIds.size;
  const batchPending = batchState.status === "reindexing" || batchState.status === "deleting";

  useEffect(() => {
    const itemIds = new Set(items.map((item) => item.id));
    setSelectedItemIds((current) => {
      const next = new Set(Array.from(current).filter((itemId) => itemIds.has(itemId)));
      return next.size === current.size ? current : next;
    });
  }, [items]);

  function clearLibraryFilters() {
    setLibraryQuery("");
    setSourceFilter("all");
    setStatusFilter("all");
    setSortKey("recent");
  }

  function toggleItemSelection(itemId: string, selected: boolean) {
    setBatchState({ status: "idle", message: null });
    setSelectedItemIds((current) => {
      const next = new Set(current);
      if (selected) {
        next.add(itemId);
      } else {
        next.delete(itemId);
      }
      return next;
    });
  }

  async function runBatchAction(action: "reindex" | "delete") {
    if (!actionsEnabled) {
      setBatchState({
        status: "error",
        message: t("common.coreUnreachable"),
      });
      return;
    }

    const itemIds = Array.from(selectedItemIds);
    if (itemIds.length === 0) {
      return;
    }
    if (action === "delete") {
      const confirmed = await requestConfirm({
        title: t("library.batch.confirm.title"),
        body: t("library.batch.confirm.body", { count: itemIds.length }),
        confirmLabel: t("library.batch.confirm.label"),
      });
      if (!confirmed) {
        return;
      }
    }

    setBatchState({ status: action === "delete" ? "deleting" : "reindexing", message: null });
    try {
      if (action === "delete") {
        await onDeleteItems(itemIds);
      } else {
        await onReindexItems(itemIds);
      }
      setSelectedItemIds(new Set());
      setBatchState({ status: "idle", message: null });
    } catch (error) {
      setBatchState({ status: "error", message: errorMessage(error) });
    }
  }

  return (
    <div className="page wide">
      <div className="page-head row" style={{ alignItems: "flex-end", justifyContent: "space-between" }}>
        <div>
          <p className="page-eyebrow">{t("library.eyebrow")}</p>
          <h1 className="page-h1">{t("library.heading")}</h1>
        </div>
        <div className="segmented" aria-label={t("library.view.aria")}>
          <button
            className={viewMode === "grid" ? "active" : ""}
            type="button"
            aria-label={t("library.view.grid")}
            aria-pressed={viewMode === "grid"}
            onClick={() => setViewMode("grid")}
          >
            <Library size={15} />
          </button>
          <button
            className={viewMode === "list" ? "active" : ""}
            type="button"
            aria-label={t("library.view.list")}
            aria-pressed={viewMode === "list"}
            onClick={() => setViewMode("list")}
          >
            <ListFilter size={15} />
          </button>
        </div>
      </div>
      <IndexingStrip jobs={jobs} items={items} stepStarts={stepStarts} onOpen={onOpenJobs} />
      <div className="row gap-2 library-filter-row" style={{ flexWrap: "wrap", alignItems: "center" }}>
        <div className="search-wrap" style={{ flex: "1 1 240px" }}>
          <Search size={17} />
          <input
            className="search-input"
            value={libraryQuery}
            placeholder={t("library.searchPlaceholder")}
            onChange={(event) => setLibraryQuery(event.currentTarget.value)}
          />
        </div>
        <select
          className="select"
          aria-label={t("library.filter.sourceAria")}
          value={sourceFilter}
          onChange={(event) => setSourceFilter(event.currentTarget.value)}
        >
          <option value="all">{t("library.filter.allSources")}</option>
          {sourceOptions.map((source) => (
            <option key={source} value={source}>
              {source}
            </option>
          ))}
        </select>
        <select
          className="select"
          aria-label={t("library.filter.statusAria")}
          value={statusFilter}
          onChange={(event) => setStatusFilter(event.currentTarget.value)}
        >
          <option value="all">{t("library.filter.allStatuses")}</option>
          <option value="indexed">{t("library.status.indexed")}</option>
          <option value="indexing">{t("library.status.indexing")}</option>
          <option value="failed">{t("library.status.failed")}</option>
        </select>
        <select
          className="select"
          aria-label={t("library.sort.aria")}
          value={sortKey}
          onChange={(event) =>
            setSortKey(event.currentTarget.value as "recent" | "longest" | "shortest" | "title")
          }
        >
          <option value="recent">{t("library.sort.recent")}</option>
          <option value="longest">{t("library.sort.longest")}</option>
          <option value="shortest">{t("library.sort.shortest")}</option>
          <option value="title">{t("library.sort.title")}</option>
        </select>
      </div>
      <div className="row" style={{ alignItems: "center", gap: 10, marginTop: 12 }}>
        <span className="muted">
          {t("library.summary.count", { count: filteredItems.length, total: items.length })}
        </span>
        {filtersActive ? (
          <button type="button" className="btn btn-ghost sm" onClick={clearLibraryFilters}>
            {t("common.clearFilters")}
          </button>
        ) : null}
      </div>
      {batchState.status === "error" && batchState.message ? (
        <InlineNotice tone="error" message={batchState.message} />
      ) : null}
      {selectedCount > 0 ? (
        <div
          className="card pad row gap-2"
          aria-label={t("library.batch.aria")}
          style={{ alignItems: "center", position: "sticky", top: 12, zIndex: 5, marginTop: 12 }}
        >
          <span className="chip accent">
            <span className="dot" />
            {t("library.batch.selected", { count: selectedCount })}
          </span>
          <span className="grow" />
          <button
            type="button"
            className="btn btn-secondary sm"
            disabled={batchPending || !actionsEnabled}
            onClick={() => void runBatchAction("reindex")}
          >
            {batchState.status === "reindexing" ? <Loader2 size={15} /> : <RefreshCcw size={15} />}
            <span>{batchState.status === "reindexing" ? t("common.reindexing") : t("common.reindex")}</span>
          </button>
          <button
            type="button"
            className="btn btn-danger sm"
            disabled={batchPending || !actionsEnabled}
            onClick={() => void runBatchAction("delete")}
          >
            {batchState.status === "deleting" ? <Loader2 size={15} /> : <Trash2 size={15} />}
            <span>{batchState.status === "deleting" ? t("common.deleting") : t("common.delete")}</span>
          </button>
          <button
            type="button"
            className="btn btn-ghost sm"
            disabled={batchPending}
            onClick={() => setSelectedItemIds(new Set())}
          >
            {t("library.batch.clear")}
          </button>
        </div>
      ) : null}
      <div className={viewMode === "grid" ? "lib-grid" : "tbl"} style={{ marginTop: 16 }}>
        {items.length > 0 && filteredItems.length > 0
          ? filteredItems.map((item) => (
            <ItemCard
              key={item.id}
              item={item}
              viewMode={viewMode}
              selectable
              selected={selectedItemIds.has(item.id)}
              onSelect={(selected) => toggleItemSelection(item.id, selected)}
              onOpen={() => onOpenItem(item)}
            />
          ))
          : null}
        {items.length === 0 ? (
          <EmptyState
            title={t("library.empty.none.title")}
            body={t("library.empty.none.body")}
            actionLabel={t("library.empty.addSource")}
            onAction={onAddSource}
          />
        ) : null}
        {items.length > 0 && filteredItems.length === 0 ? (
          <EmptyState
            title={t("library.empty.filtered.title")}
            body={t("library.empty.filtered.body")}
            actionLabel={t("common.clearFilters")}
            onAction={clearLibraryFilters}
          />
        ) : null}
      </div>
    </div>
  );
}

function ItemDetail({
  item,
  apiStatus,
  actionsEnabled,
  startTimestamp,
  modelLabel,
  onBack,
  onDeleteItem,
  onReindexItem,
  requestConfirm,
}: {
  item: Item;
  apiStatus: ApiStatus;
  actionsEnabled: boolean;
  startTimestamp: string;
  modelLabel: string;
  onBack: () => void;
  onDeleteItem: (item: Item) => Promise<void>;
  onReindexItem: (item: Item) => Promise<void>;
  requestConfirm: RequestConfirm;
}) {
  const t = useT();
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const [currentTimestamp, setCurrentTimestamp] = useState(startTimestamp);
  const [chunkState, setChunkState] = useState<{
    status: "idle" | "loading" | "loaded" | "error";
    lines: TranscriptLine[];
    message: string | null;
  }>({ status: "idle", lines: transcript, message: null });
  const [itemAction, setItemAction] = useState<{
    status: "idle" | "locating" | "reindexing" | "deleting" | "queued" | "error";
    message: string | null;
  }>({ status: "idle", message: null });
  const detailIssue = itemDetailIssue(item, t);
  const transcriptLines =
    apiStatus === "online" && chunkState.status !== "idle" ? chunkState.lines : transcript;
  const playerMarkers: PlayerMarker[] = transcriptLines
    .map((line) => ({ seconds: parseTimestampSeconds(line.time), label: line.time, text: line.text }))
    .filter((marker) => Number.isFinite(marker.seconds) && marker.seconds >= 0);
  const chunkValue =
    chunkState.status === "loaded"
      ? String(chunkState.lines.length)
      : item.status === "indexing"
        ? t("itemDetail.chunks.processing")
        : String(transcript.length);
  // Show a real inline video player whenever we have any chunk to point
  // at: prefer the existing thumbnail chunk (so we can use the same chunk
  // id used for the keyframe), otherwise use the first transcript line.
  const playableChunkId =
    item.contentType === "video"
      ? extractChunkIdFromThumbnail(item.thumbnailUrl) ??
        (chunkState.status === "loaded" && chunkState.lines[0]?.id) ??
        null
      : null;
  const itemPlaybackUrl = playableChunkId ? api.videoSegmentUrl(playableChunkId) : null;

  useEffect(() => {
    setItemAction({ status: "idle", message: null });
  }, [item.id]);

  useEffect(() => {
    setCurrentTimestamp(startTimestamp);
    if (startTimestamp === "0:00") {
      return;
    }
    seekTo(startTimestamp);
  }, [item.id, itemPlaybackUrl, startTimestamp]);

  useEffect(() => {
    function onKeyDown(event: globalThis.KeyboardEvent) {
      if (hasOpenModalSurface()) {
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        onBack();
        return;
      }
      const target = event.target as HTMLElement | null;
      if (
        target &&
        (target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.tagName === "BUTTON" ||
          target.tagName === "A" ||
          target.tagName === "SELECT" ||
          target.tagName === "VIDEO" ||
          target.isContentEditable ||
          target.getAttribute("role") === "button")
      ) {
        return;
      }
      const video = videoRef.current;
      if (!video) {
        return;
      }
      if (event.key === " " || event.code === "Space") {
        event.preventDefault();
        if (video.paused) {
          void video.play().catch(() => undefined);
        } else {
          video.pause();
        }
      } else if (event.key === "ArrowRight") {
        event.preventDefault();
        video.currentTime = Math.min(video.duration || Number.POSITIVE_INFINITY, video.currentTime + 5);
      } else if (event.key === "ArrowLeft") {
        event.preventDefault();
        video.currentTime = Math.max(0, video.currentTime - 5);
      } else if (event.key === "ArrowUp") {
        event.preventDefault();
        video.volume = Math.min(1, video.volume + 0.1);
      } else if (event.key === "ArrowDown") {
        event.preventDefault();
        video.volume = Math.max(0, video.volume - 0.1);
      } else if (event.key.toLowerCase() === "m") {
        video.muted = !video.muted;
      } else if (event.key.toLowerCase() === "f") {
        if (document.fullscreenElement) {
          void document.exitFullscreen().catch(() => undefined);
        } else if (video.requestFullscreen) {
          void video.requestFullscreen().catch(() => undefined);
        }
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onBack]);

  useEffect(() => {
    if (apiStatus !== "online") {
      setChunkState({ status: "idle", lines: transcript, message: null });
      return;
    }

    let cancelled = false;
    setChunkState({ status: "loading", lines: [], message: null });
    api
      .listItemChunks(item.id)
      .then((records) => {
        if (cancelled) {
          return;
        }
        setChunkState({ status: "loaded", lines: mapChunkRecords(records), message: null });
      })
      .catch((error) => {
        if (cancelled) {
          return;
        }
        setChunkState({ status: "error", lines: [], message: errorMessage(error) });
      });

    return () => {
      cancelled = true;
    };
  }, [apiStatus, item.id]);

  async function locateSourceFile() {
    setItemAction({ status: "locating", message: null });
    const selected = await openDialog({
      multiple: false,
      directory: false,
      filters: [{ name: "Video", extensions: ["mp4", "mkv", "webm", "mov", "m4v"] }],
    }).catch(() => null);
    if (typeof selected === "string" && selected.trim()) {
      setItemAction({
        status: "queued",
        message: t("detail.locatedSource", { path: selected }),
      });
      return;
    }
    setItemAction({ status: "idle", message: null });
  }

  async function openOriginalSource() {
    if (!canOpenOriginalSource(item)) {
      return;
    }
    if (!item.originalUrl) {
      setItemAction({ status: "locating", message: null });
    }
    try {
      const message = await openOriginalSourceForItem(item, t);
      if (!item.originalUrl) {
        setItemAction({ status: "queued", message });
      }
    } catch (error) {
      setItemAction({ status: "error", message: errorMessage(error) });
    }
  }

  async function reindexCurrentItem() {
    if (!actionsEnabled) {
      setItemAction({ status: "error", message: t("common.coreUnreachable") });
      return;
    }

    const confirmed = await requestConfirm({
      title: t("common.confirm.reindex.title"),
      body: t("common.confirm.reindex.body"),
      confirmLabel: t("common.reindex"),
    });
    if (!confirmed) {
      return;
    }

    setItemAction({ status: "reindexing", message: null });
    try {
      await onReindexItem(item);
      setItemAction({ status: "queued", message: t("common.reindexQueued") });
    } catch (error) {
      setItemAction({ status: "error", message: errorMessage(error) });
    }
  }

  async function deleteCurrentItem() {
    if (!actionsEnabled) {
      setItemAction({ status: "error", message: t("common.coreUnreachable") });
      return;
    }
    const confirmed = await requestConfirm({
      title: t("common.confirm.delete.title"),
      body: t("common.confirm.delete.body", { title: item.title }),
      confirmLabel: t("common.delete"),
    });
    if (!confirmed) {
      return;
    }

    setItemAction({ status: "deleting", message: null });
    try {
      await onDeleteItem(item);
    } catch (error) {
      setItemAction({ status: "error", message: errorMessage(error) });
    }
  }

  // Seek the inline player to a timestamp. The /video-segment endpoint serves the
  // full source video with Range support, so the loaded src is the whole file —
  // we just move currentTime. Drives the transcript rows and the Gemini chapters
  // / key moments.
  function seekTo(timestamp: string) {
    const targetSeconds = parseTimestampSeconds(timestamp);
    if (!Number.isFinite(targetSeconds)) {
      return;
    }
    setCurrentTimestamp(timestamp);
    const video = videoRef.current;
    if (!video) {
      return;
    }
    const applySeek = () => {
      const maxTime = Number.isFinite(video.duration)
        ? Math.max(video.duration - 0.1, 0)
        : targetSeconds;
      video.currentTime = Math.min(targetSeconds, maxTime);
      void video.play().catch(() => undefined);
    };
    if (video.readyState >= 1) {
      applySeek();
    } else {
      video.addEventListener("loadedmetadata", applySeek, { once: true });
    }
  }

  return (
    <div className="page wide">
      <div className="page-head">
        <button className="btn btn-ghost sm" type="button" onClick={onBack}>
          <ChevronRight size={15} style={{ transform: "rotate(180deg)" }} />
          <span>{t("library.heading")}</span>
        </button>
        <h1 className="page-h1" style={{ marginTop: 12 }}>{item.title}</h1>
        <p className="page-sub">
          {item.source} ·{" "}
          {item.indexedAt === "Never"
            ? t("detail.notIndexed")
            : t("detail.indexedAt", { when: item.indexedAt })}
        </p>
      </div>

      <div className="detail-split">
        <div className="detail-media">
          {detailIssue ? (
            <div className={`video-frame ${item.color} video-frame-unavailable`}>
              <DetailIssuePanel
                issue={detailIssue}
                actionStatus={itemAction.status}
                actionsEnabled={actionsEnabled}
                hasOriginalUrl={Boolean(item.originalUrl)}
                onLocate={() => void locateSourceFile()}
                onOpenOriginal={() => void openOriginalSource()}
                onReindex={() => void reindexCurrentItem()}
                onRemove={() => void deleteCurrentItem()}
              />
            </div>
          ) : itemPlaybackUrl ? (
            <CerulPlayer
              videoRef={videoRef}
              src={itemPlaybackUrl}
              markers={playerMarkers}
              ariaLabel={t("itemDetail.player.aria", { title: item.title })}
              onSeekMarker={(marker) => seekTo(marker.label)}
            />
          ) : (
            <div className={`video-frame ${item.color}`}>
              <button
                className="play-button"
                type="button"
                aria-label={
                  item.status === "indexing"
                    ? t("itemDetail.player.waitingAria")
                    : t("itemDetail.player.noChunkAria")
                }
                disabled
              >
                <Play size={24} fill="currentColor" />
              </button>
            </div>
          )}
          <div className="proptable" style={{ marginTop: 16 }}>
            <div className="proprow">
              <span className="k">{t("itemDetail.metric.source")}</span>
              <span className="v">{item.source}</span>
            </div>
            <div className="proprow">
              <span className="k">{t("itemDetail.metric.ingested")}</span>
              <span className="v">{item.indexedAt === "Never" ? t("detail.notIndexed") : item.indexedAt}</span>
            </div>
            <div className="proprow">
              <span className="k">{t("itemDetail.metric.duration")}</span>
              <span className="v mono">{item.duration}</span>
            </div>
            <div className="proprow">
              <span className="k">{t("itemDetail.metric.chunks")}</span>
              <span className="v mono">{chunkValue}</span>
            </div>
            <div className="proprow">
              <span className="k">{t("itemDetail.metric.usage")}</span>
              <span className="v mono">{formatUsd(item.usage.estimated_usd)}</span>
            </div>
            <div className="proprow">
              <span className="k">{t("itemDetail.metric.model")}</span>
              <span className="v">{modelLabel}</span>
            </div>
          </div>
        </div>
        <div className="detail-transcript">
          <VideoUnderstandingPanel
            item={item}
            enabled={actionsEnabled}
            onSeek={seekTo}
            requestConfirm={requestConfirm}
          />
          {itemAction.message ? (
            <p
              className={itemAction.status === "error" ? "field-error" : "field-hint"}
              role="status"
            >
              {itemAction.message}
            </p>
          ) : null}
          {chunkState.status === "loading" ? <TranscriptSkeleton /> : null}
          {chunkState.status === "error" && chunkState.message ? (
            <InlineNotice tone="error" message={chunkState.message} />
          ) : null}
          {chunkState.status === "loaded" &&
          transcriptLines.length === 0 &&
          item.status === "indexing" ? (
            <InlineNotice tone="muted" message={t("detail.stillProcessing")} />
          ) : null}
          {item.visualIndexMessage ? (
            <InlineNotice tone="muted" message={item.visualIndexMessage} />
          ) : null}
          {item.embeddingIndexMessage ? (
            <InlineNotice tone="muted" message={item.embeddingIndexMessage} />
          ) : null}
          {chunkState.status !== "loading" && transcriptLines.length > 0 ? (
            <TranscriptList lines={transcriptLines} activeTime={currentTimestamp} onSeek={seekTo} />
          ) : null}
        </div>
      </div>
    </div>
  );
}

function SettingsScreen({
  section,
  setSection,
  apiStatus,
  settings,
  daemonStatus,
  version,
  onSettingsChange,
  requestConfirm,
}: {
  section: string;
  setSection: (section: string) => void;
  apiStatus: ApiStatus;
  settings: api.SettingsMap;
  daemonStatus: DaemonStatus | null;
  version: string | null;
  onSettingsChange: (settings: api.SettingsMap) => Promise<void>;
  requestConfirm: RequestConfirm;
}) {
  const t = useT();
  const sectionIcons: Record<string, LucideIcon> = {
    General: SlidersHorizontal,
    Models: Cpu,
    Indexing: ListChecks,
    Storage: HardDrive,
    Advanced: Wrench,
    About: Info,
  };
  const sectionLabels: Record<string, string> = {
    General: t("settings.section.general"),
    Models: t("settings.section.models"),
    Indexing: t("settings.section.indexing"),
    Storage: t("settings.section.storage"),
    Advanced: t("settings.section.advanced"),
    About: t("settings.section.about"),
  };
  const controlsDisabled = apiStatus !== "online";
  const activeSection = normalizeSettingsSection(section);
  const [saveState, setSaveState] = useState<{
    status: SaveStatus;
    message: string;
  }>({ status: "idle", message: t("settings.save.idle") });

  useEffect(() => {
    if (saveState.status !== "saved") {
      return;
    }

    const timeout = window.setTimeout(() => {
      setSaveState({ status: "idle", message: t("settings.save.idle") });
    }, 1600);
    return () => window.clearTimeout(timeout);
  }, [saveState.status]);

  async function saveSettings(settingsPatch: api.SettingsMap) {
    if (controlsDisabled) {
      setSaveState({
        status: "error",
        message: t("settings.save.unreachable"),
      });
      return;
    }

    setSaveState({ status: "saving", message: t("settings.save.saving") });
    try {
      await onSettingsChange(settingsPatch);
      setSaveState({ status: "saved", message: t("settings.save.saved") });
    } catch (error) {
      setSaveState({ status: "error", message: errorMessage(error) });
    }
  }

  async function saveGlobalHotkey(label: string) {
    if (controlsDisabled) {
      setSaveState({
        status: "error",
        message: t("settings.save.unreachable"),
      });
      return;
    }

    setSaveState({ status: "saving", message: t("settings.save.saving") });
    try {
      await setGlobalHotkey(label);
      await onSettingsChange({ global_hotkey: label });
      setSaveState({ status: "saved", message: t("settings.save.hotkeyUpdated") });
    } catch (error) {
      setSaveState({ status: "error", message: errorMessage(error) });
    }
  }

  async function saveStartAtLogin(enabled: boolean) {
    if (controlsDisabled) {
      setSaveState({
        status: "error",
        message: t("settings.save.unreachable"),
      });
      return;
    }

    setSaveState({ status: "saving", message: t("settings.save.saving") });
    try {
      const result = enabled ? await installDaemon() : await uninstallDaemon();
      await onSettingsChange({ start_at_login: result.installed });
      setSaveState({
        status: enabled === result.installed ? "saved" : "error",
        message:
          enabled === result.installed
            ? result.message
            : t("settings.save.startupUnavailable"),
      });
    } catch (error) {
      setSaveState({ status: "error", message: errorMessage(error) });
    }
  }

  const saveChipClass =
    saveState.status === "error"
      ? "chip danger"
      : saveState.status === "saved"
        ? "chip success"
        : saveState.status === "saving"
          ? "chip neutral"
          : "chip neutral";

  return (
    <div className="page">
      <div className="page-head row" style={{ alignItems: "flex-end", justifyContent: "space-between" }}>
        <div>
          <p className="page-eyebrow">{t("settings.eyebrow")}</p>
          <h1 className="page-h1">{sectionLabels[activeSection] ?? activeSection}</h1>
        </div>
        <span className={saveChipClass} role="status" aria-live="polite">
          {saveState.status === "saving" ? <Loader2 size={13} /> : <Check size={13} />}
          {saveState.message}
        </span>
      </div>

      <div className="settings-wrap">
        <nav className="settings-nav" aria-label={t("settings.nav.aria")}>
          {settingsSections.map((item) => {
            const Icon = sectionIcons[item];
            return (
              <button
                key={item}
                type="button"
                className={item === activeSection ? "active" : ""}
                onClick={() => setSection(item)}
              >
                {Icon ? <Icon size={16} /> : null}
                <span>{sectionLabels[item] ?? item}</span>
              </button>
            );
          })}
        </nav>
        <div className="settings-panel">
          {apiStatus !== "online" ? (
            <p className="field-hint" style={{ marginBottom: 18 }}>{t("settings.offlineNotice")}</p>
          ) : null}
          {activeSection === "General" ? (
            <GeneralSettings
              settings={settings}
              daemonStatus={daemonStatus}
              disabled={controlsDisabled}
              onSettingsChange={saveSettings}
              onStartAtLoginChange={saveStartAtLogin}
              onHotkeyChange={saveGlobalHotkey}
            />
          ) : null}
          {activeSection === "Indexing" ? (
            <IndexingSettings
              settings={settings}
              disabled={controlsDisabled}
              onSettingsChange={saveSettings}
            />
          ) : null}
          {activeSection === "Models" ? (
            <ModelsSettings
              settings={settings}
              disabled={controlsDisabled}
              onSettingsChange={saveSettings}
              requestConfirm={requestConfirm}
            />
          ) : null}
          {activeSection === "Storage" ? <StorageSettings disabled={controlsDisabled} /> : null}
          {activeSection === "Advanced" ? (
            <AdvancedSettings
              settings={settings}
              disabled={controlsDisabled}
              onSettingsChange={saveSettings}
            />
          ) : null}
          {activeSection === "About" ? <AboutSettings version={version} /> : null}
        </div>
      </div>
    </div>
  );
}

function GeneralSettings({
  settings,
  daemonStatus,
  disabled,
  onSettingsChange,
  onStartAtLoginChange,
  onHotkeyChange,
}: {
  settings: api.SettingsMap;
  daemonStatus: DaemonStatus | null;
  disabled: boolean;
  onSettingsChange: (settings: api.SettingsMap) => Promise<void>;
  onStartAtLoginChange: (enabled: boolean) => Promise<void>;
  onHotkeyChange: (label: string) => Promise<void>;
}) {
  const t = useT();
  const { lang, setLang } = useLang();
  const theme = settingString(settings, "theme", "Dark");
  const globalHotkey = settingString(settings, "global_hotkey", "Alt+Space");
  const startAtLoginEnabled =
    daemonStatus?.installed ?? settingBoolean(settings, "start_at_login", true);
  const startAtLoginStatus = daemonStatus
    ? daemonStatus.installed
      ? daemonStatus.path
        ? t("settings.general.daemon.installedAt", { path: daemonStatus.path })
        : t("settings.general.daemon.installed")
      : t("settings.general.daemon.notInstalled")
    : t("settings.general.daemon.checking");
  const languageOptions: { value: string; label: string; disabled?: boolean }[] = [
    { value: "zh", label: t("settings.general.language.zh") },
    { value: "en", label: t("settings.general.language.en") },
    { value: "zh-TW", label: "繁體中文 (即将支持)", disabled: true },
    { value: "ja", label: "日本語 (即将支持)", disabled: true },
  ];

  return (
    <>
      <SettingsGroup title={t("settings.general.appearance")}>
        <SettingRow
          label={t("settings.general.theme")}
          control={
            <Segmented
              values={["System", "Light", "Dark"]}
              value={theme}
              disabled={disabled}
              onChange={(value) => void onSettingsChange({ theme: value })}
            />
          }
        />
        <SettingRow
          label={t("settings.general.language")}
          control={
            <div className="segmented">
              {languageOptions.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  className={option.value === lang ? "active" : ""}
                  disabled={option.disabled}
                  onClick={() => {
                    if (option.value === "zh" || option.value === "en") {
                      setLang(option.value);
                    }
                  }}
                >
                  {option.label}
                </button>
              ))}
            </div>
          }
        />
      </SettingsGroup>
      <SettingsGroup title={t("settings.general.startup")}>
        <SettingRow
          label={t("settings.general.startAtLogin")}
          description={startAtLoginStatus}
          control={
            <Toggle
              checked={startAtLoginEnabled}
              disabled={disabled}
              onChange={(checked) => void onStartAtLoginChange(checked)}
            />
          }
        />
        <SettingRow
          label={t("settings.general.closeToTray.label")}
          control={
            <Toggle
              checked={settingBoolean(settings, "close_to_tray", true)}
              disabled={disabled}
              onChange={(checked) => void onSettingsChange({ close_to_tray: checked })}
            />
          }
        />
      </SettingsGroup>
      <SettingsGroup title={t("settings.general.shortcuts")}>
        <SettingRow
          label={t("settings.general.globalHotkey")}
          description={t("settings.general.globalHotkey.hint")}
          control={
            <select
              className="select"
              value={globalHotkey}
              disabled={disabled}
              onChange={(event) => void onHotkeyChange(event.currentTarget.value)}
            >
              {globalHotkeyOptions.map((option) => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))}
            </select>
          }
        />
        <SettingRow
          label={t("settings.general.accessibility.label")}
          description={t("settings.general.accessibility.description")}
          control={
            <button className="btn btn-secondary sm" type="button" onClick={openAccessibilitySettings}>
              {t("settings.general.accessibility.openButton")}
            </button>
          }
        />
      </SettingsGroup>
    </>
  );
}

function IndexingSettings({
  settings,
  disabled,
  onSettingsChange,
}: {
  settings: api.SettingsMap;
  disabled: boolean;
  onSettingsChange: (settings: api.SettingsMap) => Promise<void>;
}) {
  const t = useT();
  const concurrentJobs = Math.min(Math.max(settingNumber(settings, "concurrent_jobs", 2), 1), 4);

  return (
    <>
      <SettingsGroup title={t("settings.indexing.performance.title")}>
        <SettingRow
          label={t("settings.indexing.concurrentJobs.label")}
          description={t("settings.indexing.concurrentJobs.description")}
          control={
            <div className="col gap-2" style={{ alignItems: "flex-end" }}>
              <span className="chip neutral">
                {concurrentJobs} {t("settings.indexing.concurrentJobs.unit")}
              </span>
              <input
                type="range"
                min={1}
                max={4}
                value={concurrentJobs}
                disabled={disabled}
                onChange={(event) => void onSettingsChange({ concurrent_jobs: Number(event.currentTarget.value) })}
              />
            </div>
          }
        />
        <SettingRow
          label={t("settings.indexing.pauseOnBattery.label")}
          control={
            <Toggle
              checked={settingBoolean(settings, "pause_on_battery", false)}
              disabled={disabled}
              onChange={(checked) => void onSettingsChange({ pause_on_battery: checked })}
            />
          }
        />
        <SettingRow
          label={t("settings.indexing.pauseLowPower.label")}
          control={
            <Toggle
              checked={settingBoolean(settings, "pause_in_low_power_mode", true)}
              disabled={disabled}
              onChange={(checked) => void onSettingsChange({ pause_in_low_power_mode: checked })}
            />
          }
        />
      </SettingsGroup>
      <SettingsGroup title={t("settings.indexing.files.title")}>
        <SettingRow
          label={t("settings.indexing.keepRaw.label")}
          description={t("settings.indexing.keepRaw.description")}
          control={
            <Toggle
              checked={settingBoolean(settings, "keep_raw_video_files", false)}
              disabled={disabled}
              onChange={(checked) => void onSettingsChange({ keep_raw_video_files: checked })}
            />
          }
        />
        <SettingRow
          stacked
          label={t("settings.indexing.scanMac.label")}
          control={<ScanThisMacControl disabled={disabled} />}
        />
      </SettingsGroup>
    </>
  );
}

function ModelsSettings({
  settings,
  disabled,
  onSettingsChange,
  requestConfirm,
}: {
  settings: api.SettingsMap;
  disabled: boolean;
  onSettingsChange: (settings: api.SettingsMap) => Promise<void>;
  requestConfirm: RequestConfirm;
}) {
  const t = useT();
  const selectedAsr = settingString(settings, "asr_model", "whisper-1");
  const selectedAsrProvider = settingString(settings, "asr_provider_id", "");
  const selectedEmbeddingProvider = settingString(settings, "embedding_provider_id", "");
  const selectedVideoUnderstandingProvider = settingString(settings, "video_understanding_provider_id", "");
  const selectedVideoUnderstandingModel = settingString(
    settings,
    "video_understanding_model",
    "gemini-3.5-flash",
  );
  const inferenceMode = settingString(settings, "inference_mode", "remote");
  const [catalog, setCatalog] = useState<api.ModelCatalogResponse | null>(null);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [modelTab, setModelTab] = useState<"setup" | "catalog">("setup");
  const [providers, setProviders] = useState<api.ProviderRecord[]>([]);
  const [providersError, setProvidersError] = useState<string | null>(null);
  const [usageSummary, setUsageSummary] = useState<api.UsageSummary | null>(null);

  async function loadProviders() {
    try {
      const next = await api.listProviders();
      setProviders(next);
      setProvidersError(null);
    } catch (error) {
      setProvidersError(errorMessage(error));
    }
  }

  useEffect(() => {
    let cancelled = false;
    async function tick() {
      try {
        const nextCatalog = await api.getModelCatalog();
        if (!cancelled) {
          setCatalog(nextCatalog);
          setCatalogError(null);
        }
      } catch (error) {
        if (!cancelled) setCatalogError(errorMessage(error));
      }
    }
    void tick();
    const interval = window.setInterval(() => void tick(), 2000);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, []);

  useEffect(() => {
    void loadProviders();
  }, []);

  useEffect(() => {
    let cancelled = false;
    async function tick() {
      try {
        const summary = await api.usageSummary();
        if (!cancelled) {
          setUsageSummary(summary);
        }
      } catch {
        if (!cancelled) {
          setUsageSummary(null);
        }
      }
    }
    void tick();
    const interval = window.setInterval(() => void tick(), 5000);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, []);

  const asrModels = catalog?.models.filter((model) => model.capability === "asr") ?? [];
  const remoteAsrModels = asrModels.filter((model) => model.tier !== "local");
  const remoteAsrOptions = remoteAsrModels.length > 0 ? remoteAsrModels : fallbackAsrModels;
  const activeRemoteAsr = selectedAsr.trim() || (remoteAsrOptions[0]?.id ?? "");
  const embeddingModels =
    catalog?.models.filter((model) => model.capability === "multimodal_embedding") ?? [];
  const videoUnderstandingModels =
    catalog?.models.filter((model) => model.capability === "video_understanding") ?? [];
  const coreModels = catalog?.models.filter((model) => model.required_for_first_search) ?? [];
  const recommendedModels = catalog?.models.filter((model) => model.recommended && !model.required_for_first_search) ?? [];
  const optionalModels = catalog?.models.filter((model) => !model.recommended && !model.required_for_first_search) ?? [];
  const activeProfile = catalog?.active_embedding_profile;
  const runtimeReady = catalog?.runtime.api_runtime_ready ?? false;
  const runtimeIssue = catalog?.runtime.last_error ?? null;
  const localRuntimeReady = catalog?.runtime.local_runtime_ready ?? false;
  const localRuntimeIssue = catalog?.runtime.local_runtime_error ?? null;
  // The runtime banner reflects the selected mode. Remote provider readiness is
  // still used for remote model blockers in the catalog tab.
  const isLocalMode = inferenceMode === "local";
  const localAsrLabel =
    asrModels.find((model) => model.tier === "local")?.label ?? t("settings.models.localAsr.fallbackLabel");
  const bannerReady = isLocalMode ? localRuntimeReady : runtimeReady;
  const bannerBadge = isLocalMode
    ? localRuntimeReady
      ? t("settings.models.runtime.badge.localReady")
      : t("settings.models.runtime.badge.runtimeNeeded")
    : runtimeReady
      ? t("settings.models.runtime.badge.apiReady")
      : t("settings.models.runtime.badge.connectionNeeded");
  const bannerMessage = isLocalMode
    ? localRuntimeReady
      ? t("settings.models.runtime.msg.localReady")
      : localRuntimeIssue ?? t("settings.models.runtime.msg.localChecking")
    : runtimeIssue ??
      t("settings.models.runtime.msg.remoteReady");
  const asrProviderOptions = providers.filter(
    (provider) =>
      provider.type === "openai" ||
      provider.type === "openai-compatible" ||
      provider.type === "gemini",
  );
  const embeddingProviderOptions = providers.filter((provider) => provider.type === "gemini");
  const videoUnderstandingProviderOptions = providers.filter((provider) => provider.type === "gemini");
  const modelGroups = [
    {
      id: "core",
      title: t("settings.models.group.core.title"),
      body: t("settings.models.group.core.body"),
      models: coreModels,
      empty: t("settings.models.group.core.empty"),
    },
    {
      id: "recommended",
      title: t("settings.models.group.recommended.title"),
      body: t("settings.models.group.recommended.body"),
      models: recommendedModels,
      empty: t("settings.models.group.recommended.empty"),
    },
    {
      id: "optional",
      title: t("settings.models.group.optional.title"),
      body: t("settings.models.group.optional.body"),
      models: optionalModels,
      empty: t("settings.models.group.optional.empty"),
    },
  ];

  return (
    <div className="models-settings-panel">
      {catalogError ? <InlineNotice tone="error" message={catalogError} /> : null}

      <section className={bannerReady ? "model-runtime-card ready" : "model-runtime-card warning"}>
        <div>
          <p className="model-section-kicker">{t("settings.models.runtime.kicker")}</p>
          <h2>{catalog?.runtime.platform ?? t("settings.models.runtime.checkingPlatform")}</h2>
          <p>{bannerMessage}</p>
        </div>
        <span className={bannerReady ? "chip success" : "chip warn"}>
          <span className="dot" />
          {bannerBadge}
        </span>
      </section>

      <nav className="segmented model-tabs" role="tablist" aria-label={t("settings.models.tabs.aria")}>
        <button
          type="button"
          role="tab"
          aria-selected={modelTab === "setup"}
          className={modelTab === "setup" ? "active" : ""}
          onClick={() => setModelTab("setup")}
        >
          {t("settings.models.tab.setup")}
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={modelTab === "catalog"}
          className={modelTab === "catalog" ? "active" : ""}
          onClick={() => setModelTab("catalog")}
        >
          {t("settings.models.tab.catalog")}
        </button>
      </nav>

      {modelTab === "setup" && (
        <>
      <InferenceModeOverview
        inferenceMode={inferenceMode}
        usageSummary={usageSummary}
      />

      <SettingsGroup title={t("settings.models.inferenceMode.title")}>
        <SettingRow
          label={t("settings.models.inferenceMode.label")}
          description={t("settings.models.inferenceMode.description")}
          control={
            <Segmented
              values={[t("settings.models.inferenceMode.remote"), t("settings.models.inferenceMode.local")]}
              value={inferenceMode === "local" ? t("settings.models.inferenceMode.local") : t("settings.models.inferenceMode.remote")}
              disabled={disabled}
              onChange={(value) =>
                void onSettingsChange({
                  inference_mode: value === t("settings.models.inferenceMode.local") ? "local" : "remote",
                })
              }
            />
          }
        />
        <p className="settings-help">{t("settings.models.inferenceMode.localRequirements")}</p>
      </SettingsGroup>

      <ProviderConnections
        providers={providers}
        error={providersError}
        disabled={disabled}
        onRefresh={loadProviders}
        requestConfirm={requestConfirm}
      />

      <section className="model-control-grid" aria-label={t("settings.models.controlGrid.aria")}>
        <TranscriptionControl
          isLocalMode={isLocalMode}
          localAsrLabel={localAsrLabel}
          providers={asrProviderOptions}
          selectedProviderId={selectedAsrProvider}
          models={remoteAsrOptions}
          selectedModelId={activeRemoteAsr}
          disabled={disabled}
          onSettingsChange={onSettingsChange}
        />

        <EmbeddingControl
          models={embeddingModels}
          activeProfile={activeProfile}
          providers={embeddingProviderOptions}
          selectedProviderId={selectedEmbeddingProvider}
          localActive={isLocalMode}
          localRuntimeReady={localRuntimeReady}
          localRuntimeIssue={localRuntimeIssue}
          disabled={disabled}
          onSettingsChange={onSettingsChange}
        />

        <VideoUnderstandingControl
          models={videoUnderstandingModels}
          providers={videoUnderstandingProviderOptions}
          selectedProviderId={selectedVideoUnderstandingProvider}
          selectedModelId={selectedVideoUnderstandingModel}
          disabled={disabled}
          onSettingsChange={onSettingsChange}
        />
      </section>
        </>
      )}

      {modelTab === "catalog" && (
      <section className="model-catalog-shell">
        <div className="model-catalog-heading">
          <div>
            <p className="model-section-kicker">{t("settings.models.catalog.kicker")}</p>
            <h2>{t("settings.models.catalog.title")}</h2>
          </div>
          <span className={runtimeReady ? "chip success" : "chip warn"}>
            <span className="dot" />
            {runtimeReady ? t("settings.models.catalog.statusReady") : t("settings.models.catalog.statusSetup")}
          </span>
        </div>

        {modelGroups.map((group) => (
          <section className="model-section" key={group.id}>
            <div className="model-section-label">
              <strong>{group.title}</strong>
              <p>{group.body}</p>
            </div>
            <ModelCatalogList models={group.models} empty={group.empty} runtimeIssue={runtimeIssue} />
          </section>
        ))}
      </section>
      )}

    </div>
  );
}

type AsrModelOption = Pick<api.ModelCatalogRecord, "id" | "label" | "size_label">;

const customModelOptionValue = "__custom_model__";

const fallbackAsrModels: AsrModelOption[] = [
  { id: "whisper-1", label: "OpenAI Whisper", size_label: "usage-based" },
  { id: "gpt-4o-mini-transcribe", label: "OpenAI GPT-4o mini transcribe", size_label: "usage-based" },
  { id: "gpt-4o-transcribe", label: "OpenAI GPT-4o transcribe", size_label: "usage-based" },
];

function TranscriptionControl({
  isLocalMode,
  localAsrLabel,
  providers,
  selectedProviderId,
  models,
  selectedModelId,
  disabled,
  onSettingsChange,
}: {
  isLocalMode: boolean;
  localAsrLabel: string;
  providers: api.ProviderRecord[];
  selectedProviderId: string;
  models: AsrModelOption[];
  selectedModelId: string;
  disabled: boolean;
  onSettingsChange: (settings: api.SettingsMap) => Promise<void>;
}) {
  const t = useT();
  const [customModel, setCustomModel] = useState(selectedModelId || "whisper-1");
  const [discoveredModels, setDiscoveredModels] = useState<api.ProviderModelRecord[]>([]);
  const [discoveredProviderId, setDiscoveredProviderId] = useState<string | null>(null);
  const [action, setAction] = useState<{
    status: "idle" | "running" | "done" | "error";
    message: string | null;
  }>({ status: "idle", message: null });

  useEffect(() => {
    setCustomModel(selectedModelId || "whisper-1");
  }, [selectedModelId]);

  useEffect(() => {
    setDiscoveredModels([]);
    setDiscoveredProviderId(null);
    setAction({ status: "idle", message: null });
  }, [selectedProviderId]);

  const activeProviderId = providers.some((provider) => provider.id === selectedProviderId)
    ? selectedProviderId
    : "";
  const providerForDiscovery =
    providers.find((provider) => provider.id === activeProviderId) ??
    providers.find((provider) => provider.has_key) ??
    providers[0] ??
    null;
  const discoveredOptions = discoveredModels.map((model) => ({
    id: model.id,
    label: model.label || model.id,
    size_label: model.source || "provider",
  }));
  const mergedModels = mergeAsrModelOptions(models, discoveredOptions);
  const selectedKnown = mergedModels.some((model) => model.id === selectedModelId);
  const modelSelectValue = selectedKnown ? selectedModelId : customModelOptionValue;
  const providerLabel =
    providerForDiscovery?.label ?? t("settings.models.transcription.providerFallback");

  async function selectProvider(providerId: string) {
    setDiscoveredModels([]);
    setDiscoveredProviderId(null);
    await onSettingsChange({ asr_provider_id: providerId });
  }

  async function selectModel(modelId: string) {
    if (modelId === customModelOptionValue) {
      return;
    }
    setCustomModel(modelId);
    await onSettingsChange({ asr_model: modelId });
  }

  async function saveCustomModel() {
    const model = customModel.trim();
    if (!model) {
      setAction({ status: "error", message: t("settings.models.transcription.error.modelEmpty") });
      return;
    }
    setAction({
      status: "running",
      message: t("settings.models.transcription.status.savingCustom"),
    });
    try {
      await onSettingsChange({ asr_model: model });
      setAction({ status: "done", message: t("settings.models.transcription.status.savedCustom") });
    } catch (err) {
      setAction({ status: "error", message: errorMessage(err) });
    }
  }

  async function exploreProviderModels() {
    if (!providerForDiscovery) {
      setAction({ status: "error", message: t("settings.models.transcription.error.noProvider") });
      return;
    }
    if (!providerForDiscovery.has_key) {
      setAction({ status: "error", message: t("settings.models.transcription.error.noKey") });
      return;
    }
    setAction({
      status: "running",
      message: t("settings.models.transcription.status.exploring", {
        provider: providerForDiscovery.label,
      }),
    });
    try {
      const models = await api.discoverProviderModels(providerForDiscovery.id);
      setDiscoveredModels(models);
      setDiscoveredProviderId(providerForDiscovery.id);
      setAction({
        status: "done",
        message:
          models.length > 0
            ? t("settings.models.transcription.status.explored", {
                count: models.length,
                provider: providerForDiscovery.label,
              })
            : t("settings.models.transcription.status.exploredEmpty", {
                provider: providerForDiscovery.label,
              }),
      });
    } catch (err) {
      setAction({ status: "error", message: errorMessage(err) });
    }
  }

  return (
    <article className="model-control-card">
      <p className="model-section-kicker">{t("settings.models.transcription.kicker")}</p>
      {isLocalMode ? (
        <>
          <div className="model-select-row">
            <select className="select" value="local" disabled>
              <option value="local">{localAsrLabel}</option>
            </select>
          </div>
          <p className="field-hint">
            {t("settings.models.transcription.localHelp", { model: localAsrLabel })}
          </p>
        </>
      ) : (
        <>
          <div className="model-select-row">
            <select
              className="select"
              value={activeProviderId}
              disabled={disabled || providers.length === 0}
              onChange={(event) => void selectProvider(event.currentTarget.value)}
            >
              <option value="">{t("settings.models.transcription.autoProvider")}</option>
              {providers.map((provider) => (
                <option key={provider.id} value={provider.id}>
                  {provider.label}
                  {provider.has_key ? "" : t("settings.models.provider.noKeySuffix")}
                </option>
              ))}
            </select>
          </div>
          <div className="model-select-row">
            <select
              className="select"
              value={modelSelectValue}
              disabled={disabled}
              onChange={(event) => void selectModel(event.currentTarget.value)}
            >
              {mergedModels.map((model) => (
                <option key={model.id} value={model.id}>
                  {model.label} - {model.size_label}
                </option>
              ))}
              <option value={customModelOptionValue}>
                {selectedKnown
                  ? t("settings.models.transcription.customOption")
                  : t("settings.models.transcription.customSelected", { model: selectedModelId })}
              </option>
            </select>
          </div>
          <div className="model-custom-row">
            <input
              className="settings-input"
              value={customModel}
              disabled={disabled}
              placeholder={t("settings.models.transcription.customPlaceholder")}
              aria-label={t("settings.models.transcription.customAria")}
              onChange={(event) => setCustomModel(event.currentTarget.value)}
            />
            <button
              type="button"
              className="btn btn-secondary sm"
              disabled={disabled || action.status === "running" || !customModel.trim()}
              onClick={() => void saveCustomModel()}
            >
              <Check size={16} />
              <span>{t("settings.models.transcription.useCustom")}</span>
            </button>
          </div>
          <div className="model-discovery-actions">
            <button
              type="button"
              className="btn btn-secondary sm"
              disabled={
                disabled ||
                action.status === "running" ||
                !providerForDiscovery ||
                !providerForDiscovery.has_key
              }
              onClick={() => void exploreProviderModels()}
            >
              <RefreshCcw size={16} />
              <span>{t("settings.models.transcription.explore")}</span>
            </button>
            {discoveredProviderId ? (
              <span className="field-hint">
                {t("settings.models.transcription.lastExplored", { provider: providerLabel })}
              </span>
            ) : null}
          </div>
          <p className="field-hint">{t("settings.models.transcription.remoteHelp")}</p>
          {action.message ? (
            <InlineNotice
              tone={action.status === "error" ? "error" : "muted"}
              message={action.message}
            />
          ) : null}
        </>
      )}
    </article>
  );
}

function mergeAsrModelOptions(base: AsrModelOption[], discovered: AsrModelOption[]) {
  const seen = new Set<string>();
  const merged: AsrModelOption[] = [];
  for (const model of [...base, ...discovered]) {
    if (!model.id || seen.has(model.id)) {
      continue;
    }
    seen.add(model.id);
    merged.push(model);
  }
  return merged;
}

function InferenceModeOverview({
  inferenceMode,
  usageSummary,
}: {
  inferenceMode: string;
  usageSummary: api.UsageSummary | null;
}) {
  const t = useT();
  const activeBadge = t("settings.models.overview.badge.active");
  const availableBadge = t("settings.models.overview.badge.available");
  const modes = [
    {
      id: "remote",
      label: t("settings.models.inferenceMode.remote"),
      badge: inferenceMode === "local" ? availableBadge : activeBadge,
      cost: usageSummary?.remote.estimated_usd ?? 0,
      events: usageSummary?.remote.event_count ?? 0,
    },
    {
      id: "local",
      label: t("settings.models.inferenceMode.local"),
      badge: inferenceMode === "local" ? activeBadge : availableBadge,
      cost: usageSummary?.local.estimated_usd ?? 0,
      events: usageSummary?.local.event_count ?? 0,
    },
  ];

  return (
    <section className="inference-mode-grid" aria-label={t("settings.models.overview.aria")}>
      {modes.map((mode) => (
        <article className="inference-mode-card" key={mode.id}>
          <div>
            <strong>{mode.label}</strong>
            <span>{mode.badge}</span>
          </div>
          <dl>
            <div>
              <dt>{t("settings.models.overview.estimatedCost")}</dt>
              <dd>{formatUsd(mode.cost)}</dd>
            </div>
            <div>
              <dt>{t("settings.models.overview.usageEvents")}</dt>
              <dd>{mode.events}</dd>
            </div>
          </dl>
        </article>
      ))}
    </section>
  );
}

type RemoteProviderType = Exclude<api.ProviderType, "local">;

const providerTypeOptions: { value: RemoteProviderType; label: string; placeholder: string }[] = [
  {
    value: "openai",
    label: "OpenAI",
    placeholder: "https://api.openai.com/v1",
  },
  {
    value: "gemini",
    label: "Gemini",
    placeholder: "https://generativelanguage.googleapis.com/v1beta",
  },
  {
    value: "openai-compatible",
    label: "OpenAI-compatible",
    placeholder: "https://your-provider.example/v1",
  },
];

function ProviderConnections({
  providers,
  error,
  disabled,
  onRefresh,
  requestConfirm,
}: {
  providers: api.ProviderRecord[];
  error: string | null;
  disabled: boolean;
  onRefresh: () => Promise<void>;
  requestConfirm: RequestConfirm;
}) {
  const t = useT();
  const typeLabel = (type: api.ProviderType) =>
    type === "openai"
      ? t("settings.models.providers.type.openai")
      : type === "gemini"
        ? t("settings.models.providers.type.gemini")
        : type === "openai-compatible"
          ? t("settings.models.providers.type.openaiCompatible")
          : type;
  const [mode, setMode] = useState<"create" | "edit" | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [form, setForm] = useState({
    type: "gemini" as RemoteProviderType,
    label: "Gemini",
    base_url: "",
    api_key: "",
  });
  const [action, setAction] = useState<{
    status: "idle" | "running" | "done" | "error";
    message: string | null;
  }>({ status: "idle", message: null });

  // The bundled local runtime ("Local on this Mac") is surfaced by the runtime
  // and Local model cards above — it is not a remote API key, so it does not
  // belong in this list. Show only genuinely remote provider connections here.
  const remoteProviders = providers.filter((provider) => provider.type !== "local");

  function openCreate() {
    setMode("create");
    setEditingId(null);
    setForm({
      type: "gemini",
      label: "Gemini",
      base_url: "",
      api_key: "",
    });
    setAction({ status: "idle", message: null });
  }

  function openEdit(provider: api.ProviderRecord) {
    if (provider.type === "local") {
      return;
    }
    setMode("edit");
    setEditingId(provider.id);
    setForm({
      type: provider.type,
      label: provider.label,
      base_url: provider.base_url ?? "",
      api_key: "",
    });
    setAction({ status: "idle", message: null });
  }

  function closeForm() {
    setMode(null);
    setEditingId(null);
    setAction({ status: "idle", message: null });
  }

  function updateType(type: RemoteProviderType) {
    const option = providerTypeOptions.find((item) => item.value === type);
    const defaultLabels = providerTypeOptions.map((item) => item.label);
    setForm((current) => ({
      ...current,
      type,
      label:
        !current.label.trim() || defaultLabels.includes(current.label)
          ? option?.label ?? current.label
          : current.label,
    }));
  }

  async function saveConnection(testAfterSave: boolean) {
    if (!mode) {
      return;
    }
    if (!form.label.trim()) {
      setAction({ status: "error", message: t("settings.models.providers.error.labelEmpty") });
      return;
    }
    if (form.type === "openai-compatible" && !form.base_url.trim()) {
      setAction({
        status: "error",
        message: t("settings.models.providers.error.baseUrlRequired"),
      });
      return;
    }

    setAction({
      status: "running",
      message: testAfterSave
        ? t("settings.models.providers.status.savingTesting")
        : t("settings.models.providers.status.saving"),
    });
    try {
      const apiKey = form.api_key.trim();
      const baseUrl = form.base_url.trim();
      const saved =
        mode === "create"
          ? await api.createProvider({
              type: form.type,
              label: form.label,
              ...(baseUrl ? { base_url: baseUrl } : {}),
              ...(apiKey ? { api_key: apiKey } : {}),
            })
          : await api.updateProvider(editingId ?? "", {
              label: form.label,
              base_url: baseUrl,
              ...(apiKey ? { api_key: apiKey } : {}),
            });
      const tested = testAfterSave ? await api.testProvider(saved.id) : saved;
      await onRefresh();
      setMode("edit");
      setEditingId(tested.id);
      setForm({
        type: tested.type === "local" ? form.type : tested.type,
        label: tested.label,
        base_url: tested.base_url ?? "",
        api_key: "",
      });
      setAction({
        status: tested.status === "error" ? "error" : "done",
        message: testAfterSave
          ? tested.last_error ??
            t("settings.models.providers.status.tested", {
              status:
                tested.status === "error"
                  ? t("settings.models.providers.status.failed")
                  : t("settings.models.providers.status.succeeded"),
            })
          : t("settings.models.providers.status.saved"),
      });
    } catch (err) {
      setAction({ status: "error", message: errorMessage(err) });
    }
  }

  async function removeConnection(provider: api.ProviderRecord) {
    if (provider.type === "local") {
      return;
    }
    const confirmed = await requestConfirm({
      title: t("settings.models.providers.confirm.title"),
      body: t("settings.models.providers.confirm.body", { label: provider.label }),
      confirmLabel: t("settings.models.providers.delete"),
    });
    if (!confirmed) {
      return;
    }
    setAction({ status: "running", message: t("settings.models.providers.status.deleting") });
    try {
      await api.deleteProvider(provider.id);
      if (editingId === provider.id) {
        closeForm();
      }
      await onRefresh();
      setAction({ status: "done", message: t("settings.models.providers.status.deleted") });
    } catch (err) {
      setAction({ status: "error", message: errorMessage(err) });
    }
  }

  const activeType = providerTypeOptions.find((item) => item.value === form.type);

  return (
    <section className="model-connections-shell">
      <div className="model-catalog-heading">
        <div>
          <p className="model-section-kicker">{t("settings.models.providers.kicker")}</p>
          <h2>{t("settings.models.providers.title")}</h2>
        </div>
        <button
          type="button"
          className="btn btn-secondary sm"
          disabled={disabled}
          onClick={openCreate}
        >
          <Plus size={16} />
          <span>{t("settings.models.providers.add")}</span>
        </button>
      </div>

      {error ? <InlineNotice tone="error" message={error} /> : null}

      <div className="provider-list">
        {remoteProviders.length === 0 ? (
          <p className="provider-empty">{t("settings.models.providers.empty")}</p>
        ) : null}
        {remoteProviders.map((provider) => (
          <article className="provider-row" key={provider.id}>
            <div className="provider-main">
              <strong>{provider.label}</strong>
              <div className="provider-meta mono">
                {[
                  typeLabel(provider.type),
                  provider.base_url || null,
                  provider.has_key
                    ? t("settings.models.providers.keySaved")
                    : t("settings.models.providers.noKey"),
                ]
                  .filter(Boolean)
                  .join(" · ")}
              </div>
              {provider.last_error ? (
                <p className="settings-help danger">{provider.last_error}</p>
              ) : null}
            </div>
            <span className={`model-state ${provider.status}`}>
              {providerStatusLabel(provider.status, t)}
            </span>
            <div className="provider-actions">
              <button
                type="button"
                className="btn btn-ghost sm"
                disabled={disabled}
                onClick={() => openEdit(provider)}
              >
                {t("settings.models.providers.edit")}
              </button>
              <button
                type="button"
                className="btn btn-danger sm"
                disabled={disabled}
                onClick={() => void removeConnection(provider)}
              >
                {t("settings.models.providers.delete")}
              </button>
            </div>
          </article>
        ))}
      </div>

      {mode ? (
        <form
          className="provider-form"
          onSubmit={(event) => {
            event.preventDefault();
            void saveConnection(false);
          }}
        >
          <div className="provider-form-grid">
            <label>
              <span>{t("settings.models.providers.form.type")}</span>
              <select
                value={form.type}
                disabled={disabled || mode === "edit"}
                onChange={(event) => updateType(event.currentTarget.value as RemoteProviderType)}
              >
                {providerTypeOptions.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </label>
            <label>
              <span>{t("settings.models.providers.form.label")}</span>
              <input
                value={form.label}
                disabled={disabled}
                onChange={(event) => setForm((current) => ({ ...current, label: event.currentTarget.value }))}
              />
            </label>
            <label>
              <span>{t("settings.models.providers.form.baseUrl")}</span>
              <input
                value={form.base_url}
                disabled={disabled}
                placeholder={activeType?.placeholder}
                onChange={(event) => setForm((current) => ({ ...current, base_url: event.currentTarget.value }))}
              />
              <small>
                {form.type === "openai-compatible"
                  ? t("settings.models.providers.form.baseUrlHelp.required")
                  : t("settings.models.providers.form.baseUrlHelp.optional")}
              </small>
            </label>
            <label>
              <span>{t("settings.models.providers.form.apiKey")}</span>
              <input
                type="password"
                value={form.api_key}
                disabled={disabled}
                placeholder={mode === "edit" ? t("settings.models.providers.form.apiKeyPlaceholder") : ""}
                onChange={(event) => setForm((current) => ({ ...current, api_key: event.currentTarget.value }))}
              />
            </label>
          </div>
          {action.message ? (
            <InlineNotice
              tone={action.status === "error" ? "error" : "muted"}
              message={action.message}
            />
          ) : null}
          <div className="provider-form-actions">
            <button
              type="submit"
              className="btn btn-primary sm"
              disabled={disabled || action.status === "running"}
            >
              <Check size={16} />
              <span>{t("settings.models.providers.form.save")}</span>
            </button>
            <button
              type="button"
              className="btn btn-secondary sm"
              disabled={disabled || action.status === "running"}
              onClick={() => void saveConnection(true)}
            >
              <RefreshCcw size={16} />
              <span>{t("settings.models.providers.form.test")}</span>
            </button>
            <button type="button" className="btn btn-ghost sm" onClick={closeForm}>
              {t("settings.models.providers.form.cancel")}
            </button>
          </div>
        </form>
      ) : null}
    </section>
  );
}

function providerStatusLabel(status: api.ProviderRecord["status"], t: TFunction) {
  if (status === "ready") {
    return t("settings.models.providers.status.ready");
  }
  if (status === "error") {
    return t("settings.models.providers.status.error");
  }
  return t("settings.models.providers.status.unconfigured");
}

function modelStatusLabel(model: api.ModelCatalogRecord, t: TFunction) {
  if (model.selected) {
    return t("settings.models.catalog.status.selected");
  }
  if (model.installed) {
    return t("settings.models.catalog.status.installed");
  }
  return t("settings.models.catalog.status.notInstalled");
}

function modelStatusClass(model: api.ModelCatalogRecord) {
  if (model.selected) {
    return "selected";
  }
  if (model.installed) {
    return "installed";
  }
  return "missing";
}

function compactModelBlocker(model: api.ModelCatalogRecord, runtimeIssue: string | null) {
  if (!model.blocked_reason) {
    return null;
  }
  if (runtimeIssue && model.blocked_reason === runtimeIssue) {
    return null;
  }
  return model.blocked_reason;
}

function ModelCatalogList({
  models,
  empty,
  runtimeIssue,
}: {
  models: api.ModelCatalogRecord[];
  empty: string;
  runtimeIssue: string | null;
}) {
  const t = useT();
  if (models.length === 0) {
    return <span className="settings-help">{empty}</span>;
  }
  return (
    <div className="model-catalog-list">
      {models.map((model) => {
        const blocker = compactModelBlocker(model, runtimeIssue);
        return (
          <article key={model.id} className="model-catalog-row">
            <div className="model-card-main">
              <div>
                <strong>{model.label}</strong>
                <p className="settings-help">
                  {model.capability} · {model.format} · {model.size_label}
                </p>
              </div>
              <span className={`model-state ${modelStatusClass(model)}`}>
                {modelStatusLabel(model, t)}
              </span>
            </div>
            <div className="model-card-meta">
              <code>{model.id}</code>
              <span>{model.tier}</span>
              {blocker ? <em>{blocker}</em> : null}
            </div>
          </article>
        );
      })}
    </div>
  );
}

function EmbeddingControl({
  models,
  activeProfile,
  providers,
  selectedProviderId,
  localActive,
  localRuntimeReady,
  localRuntimeIssue,
  disabled,
  onSettingsChange,
}: {
  models: api.ModelCatalogRecord[];
  activeProfile: api.EmbeddingProfile | null | undefined;
  providers: api.ProviderRecord[];
  selectedProviderId: string;
  localActive: boolean;
  localRuntimeReady: boolean;
  localRuntimeIssue: string | null;
  disabled: boolean;
  onSettingsChange: (settings: api.SettingsMap) => Promise<void>;
}) {
  const t = useT();
  const [info, setInfo] = useState<api.EmbeddingStatus | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [triggerError, setTriggerError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    async function tick() {
      try {
        const next = await api.getEmbeddingStatus();
        if (!cancelled) {
          setInfo(next);
          setStatusError(null);
        }
      } catch (err) {
        if (!cancelled) setStatusError(errorMessage(err));
      }
    }
    void tick();
    const interval = window.setInterval(() => void tick(), 2000);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, []);

  async function prepareNow() {
    setTriggerError(null);
    try {
      const next = await api.prepareEmbeddingModels();
      setInfo(next);
    } catch (err) {
      setTriggerError(errorMessage(err));
    }
  }

  // Local profiles store the MLX model string (e.g. mlx-community/...), which
  // matches the catalog entry's `source`, not its `id`. Match either so the
  // active local embedding model stays selected instead of falling back.
  const activeModelId =
    models.find(
      (model) =>
        model.id === activeProfile?.model_id || model.source === activeProfile?.model_id,
    )?.id ??
    models[0]?.id ??
    "";
  const dimensions = activeProfile?.output_dimension ?? 3072;

  // In local mode the embedder is the bundled MLX model, so the Gemini-only
  // readiness poll below doesn't apply — describe the local model instead of
  // telling the user to connect a Gemini provider.
  let statusText: string;
  if (localActive) {
    statusText = localRuntimeReady
      ? t("settings.models.embedding.status.localReady", { dimensions })
      : t("settings.models.embedding.status.localUnavailable", {
          issue: localRuntimeIssue ?? t("settings.models.embedding.status.checkingRuntime"),
        });
  } else if (statusError) {
    statusText = t("settings.models.embedding.status.unavailable", { error: statusError });
  } else if (!info) {
    statusText = t("settings.models.embedding.status.checking");
  } else if (info.preparing) {
    statusText = t("settings.models.embedding.status.testing");
  } else if (info.ready) {
    statusText = t("settings.models.embedding.status.remoteReady", { dimensions });
  } else {
    statusText = t("settings.models.embedding.status.connectProvider");
  }

  const showPrepare = !localActive && Boolean(info && !info.ready && !info.preparing);

  return (
    <article className="model-control-card">
      <p className="model-section-kicker">{t("settings.models.embedding.kicker")}</p>
      {localActive ? null : (
        <div className="model-select-row">
          <select
            className="select"
            value={selectedProviderId}
            disabled={disabled || providers.length === 0}
            onChange={(event) =>
              void onSettingsChange({ embedding_provider_id: event.currentTarget.value })
            }
          >
            <option value="">{t("settings.models.embedding.autoProvider")}</option>
            {providers.map((provider) => (
              <option key={provider.id} value={provider.id}>
                {provider.label}
                {provider.has_key ? "" : t("settings.models.provider.noKeySuffix")}
              </option>
            ))}
          </select>
        </div>
      )}
      <div className="model-select-row">
        <select className="select" value={activeModelId} disabled={models.length === 0} onChange={() => {}}>
          {models.length > 0 ? (
            models.map((model) => (
              <option key={model.id} value={model.id} disabled={model.id !== activeModelId}>
                {model.label}
                {model.id === activeModelId ? "" : t("settings.models.embedding.notAvailableSuffix")}
              </option>
            ))
          ) : (
            <option value="">{t("settings.models.embedding.loadingOption")}</option>
          )}
        </select>
      </div>
      <p className="settings-help">{statusText}</p>
      {showPrepare ? (
        <button
          type="button"
          className="model-inline-action"
          onClick={() => void prepareNow()}
        >
          {t("settings.models.embedding.testButton")}
        </button>
      ) : null}
      {!localActive && info?.last_error ? (
        <p className="settings-help" style={{ color: "var(--danger)" }}>
          {t("settings.models.embedding.lastError", { error: info.last_error })}
        </p>
      ) : null}
      {triggerError ? (
        <p className="settings-help" style={{ color: "var(--danger)" }}>
          {triggerError}
        </p>
      ) : null}
    </article>
  );
}

function VideoUnderstandingControl({
  models,
  providers,
  selectedProviderId,
  selectedModelId,
  disabled,
  onSettingsChange,
}: {
  models: api.ModelCatalogRecord[];
  providers: api.ProviderRecord[];
  selectedProviderId: string;
  selectedModelId: string;
  disabled: boolean;
  onSettingsChange: (settings: api.SettingsMap) => Promise<void>;
}) {
  const t = useT();
  const activeModelId = models.some((model) => model.id === selectedModelId)
    ? selectedModelId
    : models[0]?.id ?? selectedModelId;

  return (
    <article className="model-control-card">
      <p className="model-section-kicker">{t("settings.models.video.kicker")}</p>
      <div className="model-select-row">
        <select
          className="select"
          value={selectedProviderId}
          disabled={disabled || providers.length === 0}
          onChange={(event) =>
            void onSettingsChange({ video_understanding_provider_id: event.currentTarget.value })
          }
        >
          <option value="">{t("settings.models.video.autoProvider")}</option>
          {providers.map((provider) => (
            <option key={provider.id} value={provider.id}>
              {provider.label}
              {provider.has_key ? "" : t("settings.models.provider.noKeySuffix")}
            </option>
          ))}
        </select>
      </div>
      <div className="model-select-row">
        <select
          className="select"
          value={activeModelId}
          disabled={disabled || models.length === 0}
          onChange={(event) =>
            void onSettingsChange({ video_understanding_model: event.currentTarget.value })
          }
        >
          {models.length > 0 ? (
            models.map((model) => (
              <option key={model.id} value={model.id}>
                {model.label} - {model.size_label}
              </option>
            ))
          ) : (
            <option value={activeModelId}>{t("settings.models.video.fallbackModel")}</option>
          )}
        </select>
      </div>
      <p className="settings-help">
        {t("settings.models.video.help")}
      </p>
    </article>
  );
}

function asrModelLabel(modelId: string) {
  if (modelId === "gpt-4o-transcribe") return "GPT-4o transcribe";
  if (modelId === "gpt-4o-mini-transcribe") return "GPT-4o mini transcribe";
  if (modelId.startsWith("gemini-")) return "Gemini Audio";
  return "Whisper API";
}

function ScanThisMacControl({ disabled }: { disabled: boolean }) {
  const t = useT();
  const FOLDERS: { labelKey: string; path: string }[] = [
    { labelKey: "settings.indexing.scan.folder.movies", path: "~/Movies" },
    { labelKey: "settings.indexing.scan.folder.downloads", path: "~/Downloads" },
    { labelKey: "settings.indexing.scan.folder.desktop", path: "~/Desktop" },
    { labelKey: "settings.indexing.scan.folder.documents", path: "~/Documents" },
    { labelKey: "settings.indexing.scan.folder.pictures", path: "~/Pictures" },
  ];
  const [selected, setSelected] = useState<Set<string>>(new Set(["~/Movies", "~/Downloads"]));
  const [state, setState] = useState<"idle" | "running" | "done" | "error">("idle");
  const [report, setReport] = useState<string | null>(null);

  function toggle(path: string) {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }

  async function scanNow() {
    if (selected.size === 0) return;
    setState("running");
    setReport(null);
    const errors: string[] = [];
    let added = 0;
    for (const path of selected) {
      try {
        await api.addSource("folder_video", { path });
        added += 1;
      } catch (err) {
        errors.push(`${path}: ${errorMessage(err)}`);
      }
    }
    setState(errors.length > 0 ? "error" : "done");
    setReport(
      errors.length > 0
        ? `${t("settings.indexing.scan.report.partial", { added, failed: errors.length })}\n${errors.join("\n")}`
        : t("settings.indexing.scan.report.done", { added }),
    );
  }

  return (
    <div className="settings-stack-control">
      <div className="folder-list">
        {FOLDERS.map((folder) => {
          const on = selected.has(folder.path);
          return (
            <button
              key={folder.path}
              type="button"
              className={on ? "folder-row on" : "folder-row"}
              disabled={disabled || state === "running"}
              aria-pressed={on}
              onClick={() => toggle(folder.path)}
            >
              <span className="folder-check" aria-hidden="true">
                {on ? <Check size={13} /> : null}
              </span>
              <Folder size={15} className="folder-glyph" />
              <span className="folder-name">{t(folder.labelKey)}</span>
              <span className="folder-path mono">{folder.path}</span>
            </button>
          );
        })}
      </div>
      <button
        type="button"
        className="btn btn-secondary sm"
        disabled={disabled || state === "running" || selected.size === 0}
        onClick={() => void scanNow()}
      >
        {state === "running"
          ? t("settings.indexing.scan.adding")
          : t("settings.indexing.scan.addButton", { count: selected.size })}
      </button>
      {report ? <p className="settings-help">{report}</p> : null}
    </div>
  );
}

function UsageValue({
  totals,
  fallback,
}: {
  totals: api.UsageTotals | null | undefined;
  fallback: string;
}) {
  const t = useT();
  if (!totals || totals.event_count === 0) {
    return <span className="settings-value muted">{fallback}</span>;
  }
  const details = [
    totals.audio_seconds > 0 ? formatDuration(totals.audio_seconds) : null,
    totals.image_count > 0
      ? t(totals.image_count === 1 ? "jobs.usage.images.one" : "jobs.usage.images.other", {
          count: totals.image_count,
        })
      : null,
    totals.input_tokens > 0
      ? t("jobs.usage.inputTokens", { count: totals.input_tokens.toLocaleString() })
      : null,
    totals.unpriced_events > 0 ? t("jobs.usage.unpriced", { count: totals.unpriced_events }) : null,
  ].filter(Boolean);

  return (
    <span className="settings-value">
      {formatUsd(totals.estimated_usd)}
      {details.length > 0 ? <small>{details.join(" · ")}</small> : null}
    </span>
  );
}

function StorageSettings({ disabled }: { disabled: boolean }) {
  const t = useT();
  const [locations, setLocations] = useState<StorageLocations | null>(null);
  const [action, setAction] = useState<{
    status: SettingsActionStatus;
    message: string | null;
  }>({ status: "idle", message: null });
  const busy = action.status === "running";

  useEffect(() => {
    let cancelled = false;
    void readStorageLocations()
      .then((value) => {
        if (!cancelled) {
          setLocations(value);
        }
      })
      .catch((error) => {
        console.warn("failed to read Cerul storage locations", error);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  async function runStorageAction(actionName: "reveal-data" | "clear-cache") {
    setAction({ status: "running", message: null });
    try {
      if (actionName === "reveal-data") {
        await revealDataDirectory();
        setAction({ status: "done", message: t("settings.storage.message.dataOpened") });
        return;
      }
      if (actionName === "clear-cache") {
        const result = await clearCacheDirectory();
        setAction({
          status: "done",
          message: t("settings.storage.message.cacheCleared", { size: formatBytes(result.bytes_removed) }),
        });
        return;
      }
    } catch (error) {
      setAction({ status: "error", message: errorMessage(error) });
    }
  }

  return (
    <>
      <SettingsGroup title={t("settings.storage.group.title")}>
        <SettingRow
          label={t("settings.storage.dataDir.label")}
          control={
            <div className="settings-inline-action">
              <code>{locations?.data_dir ?? "~/Library/Application Support/Cerul"}</code>
              <button
                className="btn btn-secondary sm"
                type="button"
                disabled={busy}
                onClick={() => void runStorageAction("reveal-data")}
              >
                <Folder size={16} />
                <span>{t("settings.storage.dataDir.reveal")}</span>
              </button>
            </div>
          }
        />
        <SettingRow label={t("settings.storage.cacheSize.label")} control={<ProgressBar value={58} />} />
      </SettingsGroup>
      <div className="settings-actions">
        <button
          className="btn btn-secondary sm"
          type="button"
          disabled={busy}
          onClick={() => void runStorageAction("clear-cache")}
        >
          {busy ? <Loader2 size={16} /> : <HardDrive size={16} />}
          <span>{t("settings.storage.clearCache")}</span>
        </button>
      </div>
      {action.message ? (
        <InlineNotice tone={action.status === "error" ? "error" : "muted"} message={action.message} />
      ) : null}
    </>
  );
}

function AdvancedSettings({
  settings,
  disabled,
  onSettingsChange,
}: {
  settings: api.SettingsMap;
  disabled: boolean;
  onSettingsChange: (settings: api.SettingsMap) => Promise<void>;
}) {
  const t = useT();
  const binding = settingString(settings, "api_binding", "127");
  const remoteApiKey = settingString(settings, "remote_api_key", "");
  const logLevel = settingString(settings, "log_level", "info");
  const [logAction, setLogAction] = useState<{
    status: SettingsActionStatus;
    message: string | null;
  }>({ status: "idle", message: null });
  const [telemetryExpanded, setTelemetryExpanded] = useState(false);

  async function openLogsFolder() {
    setLogAction({ status: "running", message: null });
    try {
      await revealLogsDirectory();
      setLogAction({ status: "done", message: t("settings.advanced.message.logsOpened") });
    } catch (error) {
      setLogAction({ status: "error", message: errorMessage(error) });
    }
  }

  return (
    <>
      <SettingsGroup title={t("settings.advanced.localApi.title")}>
        <SettingRow
          label={t("settings.advanced.binding.label")}
          description={t("settings.advanced.binding.description")}
          control={
            <select
              value={binding}
              disabled={disabled}
              onChange={(event) => void onSettingsChange({ api_binding: event.currentTarget.value })}
            >
              <option value="127">{t("settings.advanced.binding.localOnly")}</option>
              <option value="0">{t("settings.advanced.binding.allInterfaces")}</option>
            </select>
          }
        />
        {binding === "0" ? (
          <SettingRow
            label={t("settings.advanced.remoteKey.label")}
            control={
              <input
                className="settings-input"
                type="password"
                value={remoteApiKey}
                disabled={disabled}
                placeholder={t("settings.advanced.remoteKey.placeholder")}
                onChange={(event) =>
                  void onSettingsChange({ remote_api_key: event.currentTarget.value })
                }
              />
            }
          />
        ) : null}
      </SettingsGroup>
      <SettingsGroup title={t("settings.advanced.privacy.title")}>
        <SettingRow
          label={t("settings.advanced.telemetry.label")}
          description={t("settings.advanced.telemetry.description")}
          control={
            <div className="settings-stack-control">
              <Toggle
                checked={settingBoolean(settings, "telemetry", false)}
                disabled={disabled}
                onChange={(checked) => void onSettingsChange({ telemetry: checked })}
              />
              <button
                className="btn btn-ghost sm"
                type="button"
                aria-expanded={telemetryExpanded}
                onClick={() => setTelemetryExpanded((expanded) => !expanded)}
              >
                {t("settings.advanced.telemetry.detailsToggle")}
              </button>
              {telemetryExpanded ? (
                <p className="settings-help">{t("settings.advanced.telemetry.detailsBody")}</p>
              ) : null}
            </div>
          }
        />
      </SettingsGroup>
      <SettingsGroup title={t("settings.advanced.diagnostics.title")}>
        <SettingRow
          label={t("settings.advanced.logLevel.label")}
          control={
            <select
              value={logLevel}
              disabled={disabled}
              onChange={(event) => void onSettingsChange({ log_level: event.currentTarget.value })}
            >
              <option value="info">{t("settings.advanced.logLevel.info")}</option>
              <option value="debug">{t("settings.advanced.logLevel.debug")}</option>
            </select>
          }
        />
      </SettingsGroup>
      <div className="settings-actions">
        <button
          className="btn btn-secondary sm"
          type="button"
          disabled={logAction.status === "running"}
          onClick={() => void openLogsFolder()}
        >
          {logAction.status === "running" ? <Loader2 size={16} /> : <Folder size={16} />}
          <span>{t("settings.advanced.openLogs")}</span>
        </button>
        <button
          className="btn btn-secondary sm"
          type="button"
          onClick={() => {
            // Mark onboarding as not completed so the next launch re-runs it.
            void persistLastRoute({ view: "onboarding" });
            window.location.reload();
          }}
        >
          <RefreshCcw size={16} />
          <span>{t("settings.advanced.rerunOnboarding")}</span>
        </button>
      </div>
      {logAction.message ? (
        <InlineNotice
          tone={logAction.status === "error" ? "error" : "muted"}
          message={logAction.message}
        />
      ) : null}
    </>
  );
}

function AboutSettings({ version }: { version: string | null }) {
  const t = useT();
  type AvailableDesktopUpdate = Exclude<DesktopUpdate, null>;
  const [updateState, setUpdateState] = useState<{
    status: SettingsActionStatus;
    message: string | null;
    update: AvailableDesktopUpdate | null;
  }>({ status: "idle", message: null, update: null });

  async function checkForUpdates() {
    setUpdateState({ status: "running", message: null, update: null });
    try {
      const update = await checkForDesktopUpdate();
      setUpdateState({
        status: "done",
        message: update
          ? t("settings.about.update.ready", { version: update.version })
          : t("settings.about.update.upToDate"),
        update,
      });
    } catch (error) {
      setUpdateState({ status: "error", message: errorMessage(error), update: null });
    }
  }

  return (
    <>
      <SettingsGroup title={t("settings.about.group.title")}>
        <SettingRow
          label={t("settings.about.version.label")}
          control={<span className="settings-value">{version ?? "0.0.1-alpha.3"}</span>}
        />
        <SettingRow
          label={t("settings.about.license.label")}
          control={<span className="settings-value">{t("settings.about.license.value")}</span>}
        />
        <SettingRow
          label={t("settings.about.commit.label")}
          control={<span className="settings-value">{t("settings.about.commit.value")}</span>}
        />
        <SettingRow
          label={t("settings.about.buildDate.label")}
          control={<span className="settings-value">{t("settings.about.buildDate.value")}</span>}
        />
      </SettingsGroup>
      <div className="settings-actions">
        <button
          className="btn btn-secondary sm"
          type="button"
          onClick={() => window.open("https://github.com/cerul-ai/cerul-app", "_blank", "noopener,noreferrer")}
        >
          <ExternalLink size={16} />
          <span>{t("settings.about.github")}</span>
        </button>
        <button
          className="btn btn-secondary sm"
          type="button"
          onClick={() => window.open("https://cerul.ai/docs", "_blank", "noopener,noreferrer")}
        >
          <ExternalLink size={16} />
          <span>{t("settings.about.docs")}</span>
        </button>
        <button
          className="btn btn-secondary sm"
          type="button"
          onClick={() => window.open("mailto:support@cerul.ai", "_blank", "noopener,noreferrer")}
        >
          <ExternalLink size={16} />
          <span>{t("settings.about.support")}</span>
        </button>
        <button
          className="btn btn-secondary sm"
          type="button"
          disabled={updateState.status === "running"}
          onClick={() => void checkForUpdates()}
        >
          {updateState.status === "running" ? <Loader2 size={16} /> : <RefreshCcw size={16} />}
          <span>{t("settings.about.checkUpdates")}</span>
        </button>
        {updateState.update ? (
          <button
            className="btn btn-secondary sm"
            type="button"
            onClick={() => window.open(updateState.update!.url, "_blank", "noopener,noreferrer")}
          >
            <ExternalLink size={16} />
            <span>{t("settings.about.update.openRelease")}</span>
          </button>
        ) : null}
      </div>
      {updateState.message ? (
        <InlineNotice
          tone={updateState.status === "error" ? "error" : "muted"}
          message={updateState.message}
        />
      ) : null}
    </>
  );
}

function SettingRow({
  label,
  description,
  control,
  stacked = false,
}: {
  label: string;
  description?: string;
  control: ReactNode;
  stacked?: boolean;
}) {
  return (
    <div className={stacked ? "setting-row setting-row-stacked" : "setting-row"}>
      <div className="setting-row-label">
        <span>{label}</span>
        {description ? <small>{description}</small> : null}
      </div>
      <div className="setting-row-control">{control}</div>
    </div>
  );
}

function SettingsGroup({ title, children }: { title?: string; children: ReactNode }) {
  return (
    <section className="settings-group">
      {title ? <p className="settings-group-title">{title}</p> : null}
      <div className="settings-group-rows">{children}</div>
    </section>
  );
}

function Segmented({
  values,
  value,
  disabled = false,
  onChange,
}: {
  values: string[];
  value: string;
  disabled?: boolean;
  onChange: (value: string) => void;
}) {
  return (
    <div className="segmented">
      {values.map((option) => (
        <button
          key={option}
          type="button"
          className={option === value ? "active" : ""}
          disabled={disabled}
          onClick={() => onChange(option)}
        >
          {option}
        </button>
      ))}
    </div>
  );
}

function Toggle({
  checked = false,
  disabled = false,
  onChange,
}: {
  checked?: boolean;
  disabled?: boolean;
  onChange?: (checked: boolean) => void;
}) {
  return (
    <input
      className="toggle"
      type="checkbox"
      checked={checked}
      disabled={disabled}
      onChange={(event) => onChange?.(event.currentTarget.checked)}
    />
  );
}
