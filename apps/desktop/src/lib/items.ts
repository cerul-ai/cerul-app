// Item-related helpers and mappers. Extracted from App.tsx (B13 Phase E).

import * as api from "./api";
import {
  cleanMediaTitle,
  compactPathParent,
  formatDuration,
  formatUnixTime,
  metadataString,
  sanitizeErrorText,
} from "./formatters";
import { jobStepProgressPercent } from "./jobs";
import type { TFunction } from "./i18n";
import type {
  DetailIssue,
  EmbeddingIndexStatus,
  Item,
  ItemSourceKind,
  ItemStatus,
  VisualIndexStatus,
} from "./types";

export function itemStatus(status: string, indexedAt: number | null): ItemStatus {
  if (status === "failed" || status === "error") {
    return "failed";
  }
  if (status === "indexed" || indexedAt !== null) {
    return "indexed";
  }
  return "indexing";
}

export function itemColor(contentType: string) {
  if (contentType === "audio") {
    return "amber";
  }
  if (contentType === "image") {
    return "rose";
  }
  return "mint";
}

// Short localised label for an item's source kind (YouTube / 播客 / 网页视频 /
// 本机). Used in the continue-watching meta line and the library list view.
export function itemKindLabel(item: Item, t: TFunction): string {
  switch (item.sourceKind) {
    case "youtube":
      return t("item.kind.youtube");
    case "podcast":
      return t("item.kind.podcast");
    case "web_video":
      return t("item.kind.web");
    default:
      return t("item.kind.local");
  }
}

export function itemSourceLabel(record: api.ItemRecord, t: TFunction) {
  // Prefer descriptive metadata (channel / uploader / playlist / source)
  // and fall back to the raw_path basename. Never expose the raw source
  // UUID (`source-18b0ea0aaaf9b510`) to the user.
  const fromMetadata =
    metadataString(record.metadata, "channel") ??
    metadataString(record.metadata, "uploader") ??
    metadataString(record.metadata, "playlist") ??
    metadataString(record.metadata, "source");
  if (fromMetadata) return fromMetadata;

  const rawPath =
    record.raw_path ?? metadataString(record.metadata, "raw_path") ?? null;
  if (rawPath) {
    return compactPathParent(rawPath) ?? t("item.source.localFile");
  }

  return t("item.source.local");
}

export function itemOriginalUrl(record: api.ItemRecord) {
  const metadataUrl =
    metadataString(record.metadata, "webpage_url") ??
    metadataString(record.metadata, "original_url") ??
    metadataString(record.metadata, "source_url") ??
    metadataString(record.metadata, "url");
  if (metadataUrl) {
    return metadataUrl;
  }
  if (record.external_id && record.source_id.toLowerCase().includes("youtube")) {
    return `https://www.youtube.com/watch?v=${record.external_id}`;
  }
  return null;
}

export function itemSourceKind(
  record: api.ItemRecord,
  rawPath: string | null,
): ItemSourceKind {
  const url = itemOriginalUrl(record);
  const platform = metadataString(record.metadata, "platform");
  if (url && urlHostMatches(url, ["youtube.com", "youtu.be"])) {
    return "youtube";
  }
  if (
    platform === "youtube" ||
    record.source_id.toLowerCase().includes("youtube")
  ) {
    return "youtube";
  }
  if (
    platform === "bilibili" ||
    (url && urlHostMatches(url, ["bilibili.com", "b23.tv"]))
  ) {
    return "web_video";
  }
  if (metadataString(record.metadata, "feed_url") || metadataString(record.metadata, "episode_url")) {
    return "podcast";
  }
  if (rawPath) {
    return "folder";
  }
  return "unknown";
}

function urlHostMatches(value: string, hosts: string[]) {
  try {
    const hostname = new URL(value).hostname.replace(/^www\./, "").toLowerCase();
    return hosts.some((host) => hostname === host || hostname.endsWith(`.${host}`));
  } catch {
    return false;
  }
}

// Pipeline failures arrive as raw ffmpeg/backend stderr. We never want to dump
// that at the user, so map the few recurring shapes to a plain-language reason.
// Returns null when nothing specific matches, so the caller falls back to the
// source-kind branches (missing file / unavailable source / generic).
export type FailureReason = "unreadable_media" | "ffmpeg_unavailable";

function isNoAudioOnlyError(rawError: string): boolean {
  const e = rawError.toLowerCase();
  return (
    e.includes("does not contain any stream") ||
    e.includes("output file is empty") ||
    e.includes("stream map") ||
    /\bno audio\b/.test(e)
  );
}

export function classifyFailureReason(rawError: string): FailureReason | null {
  const e = rawError.toLowerCase();
  if (!e) {
    return null;
  }
  // Container can't be parsed: unfinalised/corrupt file (e.g. an in-progress
  // Screen Studio recording with no moov atom -> "Duration: N/A").
  if (
    e.includes("could not find codec parameters") ||
    e.includes("moov atom not found") ||
    e.includes("invalid data found") ||
    e.includes("unspecified pixel format") ||
    e.includes("duration: n/a")
  ) {
    return "unreadable_media";
  }
  // The bundled ffmpeg itself is missing / not launchable.
  if (e.includes("ffmpeg") && (e.includes("enoent") || e.includes("not found") || e.includes("no such file"))) {
    return "ffmpeg_unavailable";
  }
  return null;
}

export function isSourceFileMissingError(rawError: string): boolean {
  const e = rawError.toLowerCase();
  return (
    e.includes("source file does not exist") ||
    e.includes("source file missing") ||
    e.includes("source path does not exist") ||
    e.includes("input file does not exist") ||
    e.startsWith("file not found:") ||
    (e.includes("no such file or directory") &&
      (e.includes("source") || e.includes("raw_path")))
  );
}

export function itemDetailIssue(item: Item, t: TFunction): DetailIssue | null {
  const error = item.error?.trim() ?? "";
  if (item.status !== "failed" && !error) {
    return null;
  }
  const rawError = error || null;

  // No-audio videos are valid for Cerul: the backend indexes visual frames and
  // skips speech transcription. Older failed rows with this exact error should
  // not keep showing a red "cannot transcribe" panel.
  if (isNoAudioOnlyError(error)) {
    return null;
  }

  // A classified pipeline failure is more accurate than the source-kind guess:
  // a local file that ffmpeg can't transcribe is NOT "missing", so this takes
  // precedence over the missing-file branch below.
  const reason = classifyFailureReason(error);
  if (reason) {
    return {
      kind: "failed",
      title: t(`item.issue.${reason}.title`),
      message: t(`item.issue.${reason}.message`),
      // Re-indexing won't fix a broken container, so for local files point at
      // the file instead; otherwise allow a retry.
      primaryAction: reason === "unreadable_media"
        ? item.rawPath
          ? "locate"
          : null
        : "reindex",
      removeLabel: t("item.issue.removeLabel"),
      rawError,
    };
  }

  if (
    item.rawPath &&
    item.rawPathExists === false &&
    isSourceFileMissingError(error)
  ) {
    return {
      kind: "missing-file",
      title: t("item.issue.missingFile.title"),
      message: t("item.issue.missingFile.message", {
        path: item.rawPath ?? t("item.issue.missingFile.pathFallback"),
      }),
      primaryAction: "locate",
      removeLabel: t("item.issue.removeLabel"),
      rawError,
    };
  }

  if (item.sourceKind === "youtube") {
    return {
      kind: "source-unavailable",
      title: t("item.issue.youtube.title"),
      message: t("item.issue.youtube.message"),
      primaryAction: "open-original",
      removeLabel: t("item.issue.removeLabel"),
      rawError,
    };
  }

  if (item.sourceKind === "web_video") {
    return {
      kind: "source-unavailable",
      title: t("item.issue.webVideo.title"),
      message: t("item.issue.webVideo.message"),
      primaryAction: "open-original",
      removeLabel: t("item.issue.removeLabel"),
      rawError,
    };
  }

  return {
    kind: "failed",
    title: t("item.issue.failed.title"),
    message: t("item.issue.failed.message"),
    primaryAction: "reindex",
    removeLabel: t("item.issue.removeLabel"),
    rawError,
  };
}

export function itemVisualIndexStatus(record: api.ItemRecord): VisualIndexStatus {
  const status = metadataString(record.metadata, "visual_index_status");
  if (status === "indexed" || status === "failed" || status === "pending") {
    return status;
  }
  return null;
}

export function itemVisualIndexMessage(
  record: api.ItemRecord,
  status: VisualIndexStatus,
  t: TFunction,
) {
  if (status !== "failed") {
    return null;
  }
  const error = metadataString(record.metadata, "visual_index_error");
  return error
    ? t("item.visual.failedWithError", { error })
    : t("item.visual.failed");
}

export function itemEmbeddingIndexStatus(record: api.ItemRecord): EmbeddingIndexStatus {
  const status = metadataString(record.metadata, "embedding_index_status");
  if (status === "indexed" || status === "failed" || status === "pending") {
    return status;
  }
  return null;
}

export function itemEmbeddingIndexMessage(
  record: api.ItemRecord,
  status: EmbeddingIndexStatus,
  t: TFunction,
) {
  if (status !== "failed") {
    return null;
  }
  const error = metadataString(record.metadata, "embedding_index_error");
  return error
    ? t("item.embedding.failedWithError", { error })
    : t("item.embedding.failed");
}

// The pipeline records `has_audio: false` for video-only files (e.g. screen
// recordings). Treat anything else — older items, audio, images, an absent
// flag — as having audio so existing labels never regress.
export function itemHasAudio(record: api.ItemRecord): boolean {
  return record.metadata?.has_audio !== false;
}

export function itemPlaybackPosition(record: api.ItemRecord): api.PlaybackPositionRecord | null {
  const raw = record.metadata.playback_position;
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) {
    return null;
  }
  const position = raw as Record<string, unknown>;
  const positionSec = typeof position.position_sec === "number" ? position.position_sec : null;
  if (positionSec === null || !Number.isFinite(positionSec) || positionSec < 1) {
    return null;
  }
  if (isNearEndPosition(positionSec, record.duration_sec)) {
    return null;
  }
  const timestamp =
    typeof position.timestamp === "string" && position.timestamp.trim()
      ? position.timestamp
      : formatPlaybackTimestamp(positionSec);
  const chunkId =
    typeof position.chunk_id === "string" && position.chunk_id.trim()
      ? position.chunk_id
      : null;
  const updatedAt = typeof position.updated_at === "number" ? position.updated_at : null;

  return {
    item_id: record.id,
    position_sec: positionSec,
    timestamp,
    chunk_id: chunkId,
    updated_at: updatedAt,
  };
}

export function isNearEndPosition(positionSec: number, durationSec: number | null | undefined) {
  if (!Number.isFinite(positionSec) || !durationSec || !Number.isFinite(durationSec) || durationSec <= 0) {
    return false;
  }
  const remainingSec = durationSec - positionSec;
  return remainingSec <= 8 || positionSec / durationSec >= 0.98;
}

function formatPlaybackTimestamp(positionSec: number) {
  const totalSeconds = Math.max(0, Math.floor(positionSec));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`
    : `${minutes}:${String(seconds).padStart(2, "0")}`;
}

export function mapItemRecord(
  record: api.ItemRecord,
  jobRecords: api.JobRecord[],
  t: TFunction,
): Item {
  const status = itemStatus(record.status, record.indexed_at);
  const job = latestActiveJobForItem(record.id, jobRecords);
  const itemProgress = status === "indexing" && job ? jobStepProgressPercent(job) / 100 : null;
  const rawPath = record.raw_path ?? metadataString(record.metadata, "raw_path");
  const visualIndexStatus = itemVisualIndexStatus(record);
  const embeddingIndexStatus = itemEmbeddingIndexStatus(record);

  return {
    id: record.id,
    title: cleanMediaTitle(record.title ?? rawPath ?? record.external_id ?? record.id),
    sourceId: record.source_id,
    contentType: record.content_type,
    source: itemSourceLabel(record, t),
    sourceKind: itemSourceKind(record, rawPath),
    duration: formatDuration(record.duration_sec, t),
    durationSec: record.duration_sec,
    indexedAt: formatUnixTime(record.indexed_at, t),
    indexedAtEpoch: record.indexed_at,
    status,
    error: record.error ? sanitizeErrorText(record.error) : null,
    rawPath,
    rawPathExists: record.raw_path_exists ?? null,
    originalUrl: itemOriginalUrl(record),
    color: itemColor(record.content_type),
    thumbnailUrl: record.thumbnail_chunk_id ? api.chunkFrameUrl(record.thumbnail_chunk_id) : null,
    progress: itemProgress,
    progressLabel: job && itemProgress !== null ? itemProgressLabel(job, itemProgress, t) : null,
    etaLabel: job && itemProgress !== null ? itemEtaLabel(job, t) : null,
    visualIndexStatus,
    visualIndexMessage: itemVisualIndexMessage(record, visualIndexStatus, t),
    embeddingIndexStatus,
    embeddingIndexMessage: itemEmbeddingIndexMessage(record, embeddingIndexStatus, t),
    hasAudio: itemHasAudio(record),
    playbackPosition: itemPlaybackPosition(record),
    usage: record.usage,
  };
}

export function latestActiveJobForItem(itemId: string, jobRecords: api.JobRecord[]) {
  return jobRecords
    .filter((job) => job.item_id === itemId && isActiveJob(job))
    .sort((a, b) => (b.started_at ?? 0) - (a.started_at ?? 0))[0];
}

export function isActiveJob(job: api.JobRecord) {
  return job.status === "queued" || job.status === "running";
}

export function normalizeJobProgress(progress: number) {
  return Math.min(Math.max(progress, 0), 1);
}

export function itemProgressLabel(job: api.JobRecord, progress: number, t: TFunction) {
  if (job.status === "queued") {
    return t("jobs.status.queued");
  }
  return itemJobStageLabel(job.stage, t) ?? job.stage_message ?? `${Math.round(progress * 100)}%`;
}

// Localized stage label, reusing the shared jobs.stage.* catalog keys; returns
// null for an unknown stage so the caller falls back to the backend message.
function itemJobStageLabel(stage: string | null, t: TFunction): string | null {
  if (!stage) {
    return null;
  }
  const key = `jobs.stage.${stage}`;
  const label = t(key);
  return label === key ? null : label;
}

// Rough time-remaining estimate. Uses the backend's time-weighted progress (not
// the step-even bar value) so the estimate tracks elapsed time; the stages
// aren't perfectly linear, so it's labelled "~".
export function itemEtaLabel(job: api.JobRecord, t: TFunction): string | null {
  if (job.status !== "running" || job.started_at === null) {
    return null;
  }
  const progress = normalizeJobProgress(job.progress);
  if (progress < 0.12 || progress >= 0.99) {
    return null;
  }
  const elapsedSec = Date.now() / 1000 - job.started_at;
  if (elapsedSec <= 1) {
    return null;
  }
  const remainingSec = (elapsedSec * (1 - progress)) / progress;
  return t("item.eta.left", { duration: formatEtaDuration(remainingSec) });
}

export function formatEtaDuration(seconds: number): string {
  if (seconds < 90) {
    return `${Math.max(1, Math.round(seconds))}s`;
  }
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) {
    return `${minutes}m`;
  }
  return `${Math.floor(minutes / 60)}h${minutes % 60}m`;
}
