// Shared types for the Cerul desktop app. Extracted from App.tsx as
// part of the B13 audit follow-up (Phase E — types + mappers).
//
// Types live here even when used by only one consumer so that future
// screen / dialog extractions (Phase C and D) can import without
// dragging the rest of App.tsx along.

import type * as api from "./api";

export type View =
  | "onboarding"
  | "home"
  | "results"
  | "library"
  | "moments"
  | "item-detail"
  | "sources"
  | "jobs"
  | "shares"
  | "settings";

export type SourceStatus = "active" | "syncing" | "paused" | "error";
export type ItemStatus = "indexed" | "indexing" | "failed";
export type VisualIndexStatus = "indexed" | "failed" | "pending" | null;
export type EmbeddingIndexStatus = "indexed" | "failed" | "pending" | null;
export type ItemSourceKind = "folder" | "youtube" | "web_video" | "podcast" | "unknown";
export type ApiStatus = "connecting" | "online" | "offline" | "error";
export type ValidationStatus = "idle" | "validating" | "valid" | "error";

export type ValidationState = {
  status: ValidationStatus;
  message: string | null;
};

export type SaveStatus = "idle" | "saving" | "saved" | "error";
export type SettingsActionStatus = "idle" | "running" | "done" | "error";
export type CoreBannerAction = "retry" | "restart";

export type ConfirmOptions = {
  title: string;
  body: string;
  confirmLabel: string;
};

export type ConfirmRequest = ConfirmOptions & {
  resolve: (confirmed: boolean) => void;
};

export type RequestConfirm = (options: ConfirmOptions) => Promise<boolean>;

export type OnboardingYoutubeChannel = {
  url: string;
  name: string;
  subscribers: string;
};

export type Result = {
  itemId: string;
  playbackChunkId: string;
  startSec: number | null;
  endSec: number | null;
  title: string;
  source: string;
  timestamp: string;
  indexedAtEpoch: number | null;
  duration: string;
  snippet: string;
  color: string;
  thumbnailUrl: string | null;
  confidence: ResultConfidence;
  confidenceLabel: string;
  score: number;
  // Derived from backend result order. Higher sorts earlier.
  rankScore: number;
  scoreLabel: string;
  scoreTitle: string;
  chunkType: string;
  moreMatches: ResultMatch[];
};

export type ResultMatch = {
  playbackChunkId: string;
  startSec: number | null;
  endSec: number | null;
  timestamp: string;
  snippet: string;
  chunkType?: string;
  confidence: ResultConfidence;
  confidenceLabel: string;
  scoreLabel: string;
  scoreTitle: string;
};

export type ResultConfidence = "high" | "medium" | "low";
export type ResultModalityFilter = "all" | "video" | "audio" | "image" | "document";
export type ResultConfidenceFilter = "all" | "strong" | "review";
export type ResultTimeFilter = "all" | "first10" | "tenToThirty" | "thirtyPlus";

export type TranscriptLine = {
  id: string;
  time: string;
  displayTime?: string;
  text: string;
  active?: boolean;
  /** Exact chunk bounds (seconds) when known — drives the clip-export trim
   * window and filename. Optional because fixtures/legacy paths only carry
   * the formatted `time`. */
  startSec?: number | null;
  endSec?: number | null;
};

export type Item = {
  id: string;
  title: string;
  sourceId: string;
  contentType: string;
  source: string;
  sourceKind: ItemSourceKind;
  duration: string;
  durationSec?: number | null;
  indexedAt: string;
  indexedAtEpoch: number | null;
  addedAtEpoch?: number | null;
  status: ItemStatus;
  error: string | null;
  rawPath: string | null;
  rawPathExists: boolean | null;
  originalUrl: string | null;
  color: string;
  thumbnailUrl: string | null;
  progress: number | null;
  progressLabel: string | null;
  etaLabel: string | null;
  visualIndexStatus: VisualIndexStatus;
  visualIndexMessage: string | null;
  embeddingIndexStatus: EmbeddingIndexStatus;
  embeddingIndexMessage: string | null;
  imageEmbeddingCount?: number;
  // False only when the pipeline confirmed the video has no audio track
  // (e.g. a screen recording). Absent/true for everything else.
  hasAudio?: boolean;
  playbackPosition: api.PlaybackPositionRecord | null;
  usage: import("./api").UsageTotals;
};

export type DetailIssue = {
  kind: "missing-file" | "source-unavailable" | "failed";
  title: string;
  message: string;
  primaryAction: "locate" | "open-original" | "reindex" | null;
  removeLabel: string;
  // Raw backend/ffmpeg error, surfaced in a collapsible "technical details"
  // section instead of being dumped into `message`. Null when there's no
  // underlying log (e.g. a clean source-unavailable case).
  rawError: string | null;
};

export type Source = {
  id: string;
  type: "folder" | "file" | "youtube" | "web_video" | "podcast";
  name: string;
  status: SourceStatus;
  items: number;
  failedItems: number;
  lastPolled: string;
  lastPolledEpoch?: number | null;
  error: string | null;
  fixSettingsSection: string | null;
};

export type DaemonStatus = {
  platform: string;
  installed: boolean;
  path: string | null;
};

export type DaemonInstallResult = DaemonStatus & {
  message: string;
};

export type AppData = {
  sources: Source[];
  items: Item[];
  jobs: api.JobRecord[];
  jobSummary: api.JobStatusSummary | null;
  settings: api.SettingsMap;
  whisperModels: api.WhisperModelRecord[];
  daemonStatus: DaemonStatus | null;
  version: string | null;
};

export type RouteState = {
  view: View;
  itemId: string | null;
  playbackChunkId: string | null;
  timestamp: string | null;
  settingsSection: string | null;
  origin: "results" | "library" | null;
  oauthProvider: string | null;
  oauthCode: string | null;
  oauthState: string | null;
  oauthError: string | null;
};
