// Item-related helpers and mappers. Extracted from App.tsx (B13 Phase E).

import * as api from "./api";
import {
  cleanMediaTitle,
  compactPathParent,
  formatDuration,
  formatUnixTime,
  metadataString,
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
  if (url && (url.includes("youtube.com") || url.includes("youtu.be"))) {
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
    (url && (url.includes("bilibili.com") || url.includes("b23.tv")))
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

export function itemDetailIssue(item: Item, t: TFunction): DetailIssue | null {
  const error = item.error?.trim() ?? "";
  if (item.status !== "failed" && !error) {
    return null;
  }

  if (item.sourceKind === "folder" || item.rawPath) {
    return {
      kind: "missing-file",
      title: t("item.issue.missingFile.title"),
      message:
        error ||
        t("item.issue.missingFile.message", {
          path: item.rawPath ?? t("item.issue.missingFile.pathFallback"),
        }),
      primaryAction: "locate",
      removeLabel: t("item.issue.removeLabel"),
    };
  }

  if (item.sourceKind === "youtube") {
    return {
      kind: "source-unavailable",
      title: t("item.issue.youtube.title"),
      message: error || t("item.issue.youtube.message"),
      primaryAction: "open-original",
      removeLabel: t("item.issue.removeLabel"),
    };
  }

  if (item.sourceKind === "web_video") {
    return {
      kind: "source-unavailable",
      title: t("item.issue.webVideo.title"),
      message: error || t("item.issue.webVideo.message"),
      primaryAction: "open-original",
      removeLabel: t("item.issue.removeLabel"),
    };
  }

  return {
    kind: "failed",
    title: t("item.issue.failed.title"),
    message: error || t("item.issue.failed.message"),
    primaryAction: "reindex",
    removeLabel: t("item.issue.removeLabel"),
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
    duration: formatDuration(record.duration_sec),
    indexedAt: formatUnixTime(record.indexed_at),
    indexedAtEpoch: record.indexed_at,
    status,
    error: record.error,
    rawPath,
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
