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
  if (type === "web_video") {
    return "web_video";
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
  if (
    record.type === "folder_video" ||
    record.type === "folder_audio" ||
    record.type === "folder_image" ||
    record.type === "folder_document"
  ) {
    return compactPathDisplay(configValue, 2) ?? configValue;
  }
  return displayUrl(configValue);
}

// URLs shown as source names drop the protocol, www and share/tracking query
// (spm_id_from, vd_source, si, …). Identity params survive: the YouTube video
// id (?v=) and the bilibili part selector (?p=N). Non-URL values pass through.
function displayUrl(value: string) {
  try {
    const url = new URL(value);
    const host = url.hostname.replace(/^www\./, "");
    const path = url.pathname.replace(/\/$/, "");
    const kept = new URLSearchParams();
    const videoId = url.searchParams.get("v");
    if (videoId) {
      kept.set("v", videoId);
    }
    const part = url.searchParams.get("p");
    if (part && part !== "1") {
      kept.set("p", part);
    }
    const query = kept.toString();
    return `${host}${path}${query ? `?${query}` : ""}`;
  } catch {
    return value;
  }
}

export function sourceStatus(status: string): SourceStatus {
  if (status === "syncing") {
    return "syncing";
  }
  if (status === "paused") {
    return "paused";
  }
  if (status === "error" || status === "failed") {
    return "error";
  }
  return "active";
}

function decodedPathParts(pathname: string): string[] {
  return pathname
    .split("/")
    .filter(Boolean)
    .map((part) => {
      try {
        return decodeURIComponent(part);
      } catch {
        return part;
      }
    });
}

function isHostOrSubdomain(host: string, domain: string): boolean {
  return host === domain || host.endsWith(`.${domain}`);
}

/**
 * Turns a stored source URL into a short but distinct connection label.
 * Path identity matters here: two feeds on the same host, or two YouTube
 * Shorts/channel URLs, must not collapse to the same generic row name.
 */
export function sourceConnectorDisplayName(source: Source, fallback: string): string {
  if (source.type === "folder" || source.type === "file") {
    const clean = source.name.replace(/[\\/]+$/, "");
    return clean.split(/[\\/]/).pop() || fallback;
  }

  try {
    const url = new URL(source.name.includes("://") ? source.name : `https://${source.name}`);
    const host = url.hostname.replace(/^www\./, "");
    const parts = decodedPathParts(url.pathname);
    const isBilibili =
      isHostOrSubdomain(host, "bilibili.com") || isHostOrSubdomain(host, "b23.tv");
    if (isBilibili) {
      const authorId = host === "space.bilibili.com" ? parts[0] : null;
      const videoId = parts.find((part) => /^BV/i.test(part));
      const shortId = isHostOrSubdomain(host, "b23.tv") ? parts[0] : null;
      const label = authorId || videoId || shortId;
      return label ? `Bilibili · ${label}` : source.name || fallback;
    }

    if (isHostOrSubdomain(host, "youtube.com") || host === "youtu.be") {
      const keyedPathIndex = parts.findIndex((part) =>
        ["shorts", "live", "embed", "c", "user", "channel"].includes(part.toLowerCase()),
      );
      const keyedPathId = keyedPathIndex >= 0 ? parts[keyedPathIndex + 1] : null;
      const channelId = parts.find((part) => part.startsWith("@") || /^UC[\w-]+$/i.test(part));
      const videoId =
        url.searchParams.get("v") || (host === "youtu.be" ? parts[0] : null);
      const label = channelId || videoId || keyedPathId;
      return label ? `YouTube · ${label}` : source.name || fallback;
    }

    if (source.type === "podcast" && parts.length > 0) {
      return `${host} · ${parts.join("/")}`;
    }

    if (parts.length > 0) {
      return `${host} · ${parts.join("/")}`;
    }
    return source.name || host || fallback;
  } catch {
    return source.name || fallback;
  }
}

export function sourceError(record: api.SourceRecord, status: SourceStatus, t: TFunction) {
  if (status !== "error") {
    return null;
  }
  const errorCode = record.config.last_error_code;
  const errorValue = record.config.error ?? record.config.last_error;
  if (
    typeof errorCode === "string" &&
    errorCode.trim() &&
    errorCode !== "unknown_processing_error"
  ) {
    return t(`jobs.error.${errorCode}`, { capability: t("source.preview.webVideoTitle") });
  }
  if (typeof errorValue === "string" && errorValue.trim()) {
    return sanitizeErrorText(errorValue);
  }
  // No backend detail: return null so the UI renders its localized fallback
  // (sourceRow.errorFallback) instead of a hardcoded English sentence.
  return null;
}

export function sourceFixSettingsSection(record: api.SourceRecord, status: SourceStatus) {
  if (status !== "error") {
    return null;
  }
  const section = record.config.last_error_settings_section;
  if (typeof section !== "string") {
    return null;
  }
  const normalized = section.trim();
  if (!normalized) {
    return null;
  }
  return normalized === "Sources" ? "Indexing" : normalized;
}

export function mapSourceRecord(record: api.SourceRecord, allItems: Item[], t: TFunction): Source {
  const type = sourceType(record.type);
  const itemsForSource = allItems.filter((item) => item.sourceId === record.id);
  const status = sourceStatus(record.status);
  return {
    id: record.id,
    type,
    name: sourceName(record),
    status,
    items: itemsForSource.length,
    failedItems: itemsForSource.filter((item) => item.status === "failed").length,
    lastPolled: formatUnixTime(record.last_poll_at, t),
    lastPolledEpoch: record.last_poll_at,
    error: sourceError(record, status, t),
    fixSettingsSection: sourceFixSettingsSection(record, status),
  };
}
