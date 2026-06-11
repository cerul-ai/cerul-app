// Source-related helpers and mappers. Extracted from App.tsx (B13 Phase E).

import type * as api from "./api";
import { cleanMediaTitle, compactPathDisplay, formatUnixTime, sanitizeErrorText } from "./formatters";
import type { TFunction } from "./i18n";
import type { Item, Source, SourceStatus } from "./types";

export function sourceType(type: string): Source["type"] {
  if (type === "file_video") {
    return "file";
  }
  if (type === "youtube") {
    return "youtube";
  }
  if (type === "rss_podcast") {
    return "podcast";
  }
  return "folder";
}

export function sourceName(record: api.SourceRecord) {
  const configValue =
    record.config.path ??
    record.config.url ??
    record.config.feed_url ??
    record.config.channel_url ??
    record.id;
  if (typeof configValue !== "string") {
    return record.id;
  }
  if (record.type === "file_video") {
    return cleanMediaTitle(configValue);
  }
  if (record.type === "folder_video" || record.type === "folder_audio" || record.type === "folder_image") {
    return compactPathDisplay(configValue, 2) ?? configValue;
  }
  return configValue;
}

export function sourceStatus(status: string): SourceStatus {
  if (status === "paused") {
    return "paused";
  }
  if (status === "error" || status === "failed") {
    return "error";
  }
  return "active";
}

export function sourceError(record: api.SourceRecord, status: SourceStatus) {
  if (status !== "error") {
    return null;
  }
  const errorValue = record.config.error ?? record.config.last_error;
  if (typeof errorValue === "string" && errorValue.trim()) {
    return sanitizeErrorText(errorValue);
  }
  // No backend detail: return null so the UI renders its localized fallback
  // (sourceRow.errorFallback) instead of a hardcoded English sentence.
  return null;
}

export function mapSourceRecord(record: api.SourceRecord, allItems: Item[], t: TFunction): Source {
  const type = sourceType(record.type);
  const itemsForSource = allItems.filter((item) => item.sourceId === record.id).length;
  const status = sourceStatus(record.status);
  return {
    id: record.id,
    type,
    name: sourceName(record),
    status,
    items: itemsForSource,
    lastPolled: formatUnixTime(record.last_poll_at, t),
    error: sourceError(record, status),
  };
}
