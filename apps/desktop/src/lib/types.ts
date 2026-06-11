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
  | "result-detail"
  | "library"
  | "item-detail"
  | "sources"
  | "settings";

export type SourceStatus = "active" | "paused" | "error";
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
  id: string;
  itemId: string;
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
  scoreLabel: string;
  scoreTitle: string;
  chunkType: string;
  moreMatches: ResultMatch[];
};

export type ResultMatch = {
  id: string;
  timestamp: string;
  snippet: string;
  chunkType?: string;
  confidence: ResultConfidence;
  confidenceLabel: string;
  scoreLabel: string;
  scoreTitle: string;
};

export type ResultConfidence = "high" | "medium" | "low";
export type ResultModalityFilter = "all" | "video" | "audio" | "image";
export type ResultConfidenceFilter = "all" | "strong" | "review";
export type ResultTimeFilter = "all" | "first10" | "tenToThirty" | "thirtyPlus";

export type TranscriptLine = {
  id: string;
  time: string;
  text: string;
  active?: boolean;
};

export type Item = {
  id: string;
  title: string;
  sourceId: string;
  contentType: string;
  source: string;
  sourceKind: ItemSourceKind;
  duration: string;
  indexedAt: string;
  indexedAtEpoch: number | null;
  status: ItemStatus;
  error: string | null;
  rawPath: string | null;
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
  usage: import("./api").UsageTotals;
};

export type DetailIssue = {
  kind: "missing-file" | "source-unavailable" | "failed";
  title: string;
  message: string;
  primaryAction: "locate" | "open-original" | "reindex" | null;
  removeLabel: string;
};

export type Source = {
  id: string;
  type: "folder" | "file" | "youtube" | "web_video" | "podcast";
  name: string;
  status: SourceStatus;
  items: number;
  lastPolled: string;
  error: string | null;
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
  settings: api.SettingsMap;
  whisperModels: api.WhisperModelRecord[];
  daemonStatus: DaemonStatus | null;
  version: string | null;
};

export type RouteState = {
  view: View;
  itemId: string | null;
  chunkId: string | null;
  timestamp: string | null;
  settingsSection: string | null;
};
