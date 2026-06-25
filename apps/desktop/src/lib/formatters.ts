// Pure formatting helpers. Extracted from App.tsx as the first
// step of the B13 audit follow-up (split App.tsx into modules).
//
// Helpers that surface user-visible words take an optional TFunction (t-last
// convention); without it they fall back to English for non-UI callers.

import type { TFunction } from "./i18n";

export function formatDuration(seconds: number | null, t?: TFunction) {
  if (!seconds || seconds <= 0) {
    return t ? t("time.unknown") : "Unknown";
  }
  const total = Math.round(seconds);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const secs = total % 60;
  const pad = (value: number) => String(value).padStart(2, "0");
  // YouTube-style H:MM:SS / M:SS to match the redesign baseline.
  if (hours > 0) {
    return `${hours}:${pad(minutes)}:${pad(secs)}`;
  }
  return `${minutes}:${pad(secs)}`;
}

export function basenameFromPath(path: string | null | undefined) {
  if (!path) return null;
  const cleaned = path.replace(/\/+$/, "");
  const segments = cleaned.split(/[\\/]/).filter(Boolean);
  return segments[segments.length - 1] ?? null;
}

export function compactPathParent(path: string | null | undefined, segmentCount = 2) {
  if (!path) return null;
  const cleaned = path.replace(/\/+$/, "");
  const segments = cleaned.split(/[\\/]/).filter(Boolean);
  const parentSegments = segments.slice(0, -1);
  if (parentSegments.length === 0) return null;
  return parentSegments.slice(-segmentCount).join("/");
}

export function compactPathDisplay(path: string | null | undefined, segmentCount = 2) {
  if (!path) return null;
  const cleaned = path.replace(/\/+$/, "");
  const segments = cleaned.split(/[\\/]/).filter(Boolean);
  if (segments.length === 0) return null;
  return segments.slice(-segmentCount).join("/");
}

export function cleanMediaTitle(value: string | null | undefined) {
  const basename = basenameFromPath(value) ?? value?.trim() ?? "";
  if (!basename) return "";
  const withoutExtension = basename.replace(/\.[a-z0-9]{2,5}$/i, "");
  const cleaned = withoutExtension
    .replace(/^YTDown[_ -]+(?:YouTube|Bilibili|TikTok)[_ -]+/i, "")
    .replace(/[_ -]+Media[_ -]+[A-Za-z0-9_-]+(?:[_ -]+\d+)?(?:[_ -]+\d{3,4}p)?$/i, "")
    .replace(/[_ -]+\d{3,4}p$/i, "")
    .replace(/[_ -]+(?:001|002|003)$/i, "")
    .replace(/[_-]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  return cleaned || withoutExtension || basename;
}

export function formatTimestamp(seconds: number | null) {
  if (seconds === null || seconds < 0) {
    return "00:00";
  }
  const total = Math.round(seconds);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const remaining = `${total % 60}`.padStart(2, "0");
  // Match formatDuration above: a 2h video shows 1:32:30, not 92:30.
  if (hours > 0) {
    return `${hours}:${`${minutes}`.padStart(2, "0")}:${remaining}`;
  }
  return `${minutes}:${remaining}`;
}

export function parseTimestampSeconds(timestamp: string) {
  const parts = timestamp.split(":").map((part) => Number.parseInt(part, 10));
  if (parts.some((part) => Number.isNaN(part))) {
    return 0;
  }
  return parts.reduce((total, part) => total * 60 + part, 0);
}

export function formatUnixTime(value: number | null, t?: TFunction) {
  if (!value) {
    return t ? t("time.never") : "Never";
  }
  const date = new Date(value * 1000);
  if (Number.isNaN(date.getTime())) {
    return t ? t("time.unknown") : "Unknown";
  }
  const now = new Date();
  if (
    date.getFullYear() === now.getFullYear() &&
    date.getMonth() === now.getMonth() &&
    date.getDate() === now.getDate()
  ) {
    return t ? t("time.today") : "Today";
  }
  // The locale tag lives in the catalog so dates read natively in either
  // language (en: "May 12" / zh: "5月12日") instead of mixing scripts.
  return date.toLocaleDateString(t ? t("time.localeTag") : "en-US", {
    month: "short",
    day: "numeric",
  });
}

export function formatBytes(bytes: number) {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[unitIndex]}`;
}

/** Download speed from bytes/sec → "21.4 MB/s" or "640 KB/s". Null when there
 * is no live rate (idle / between samples). */
export function formatSpeed(bps: number | null | undefined): string | null {
  if (!bps || !Number.isFinite(bps) || bps <= 0) {
    return null;
  }
  const mb = bps / 1024 / 1024;
  if (mb >= 1) {
    return `${mb.toFixed(1)} MB/s`;
  }
  return `${Math.max(1, Math.round(bps / 1024))} KB/s`;
}

export function formatUsd(value: number | null | undefined) {
  if (!value || value <= 0) {
    return "$0.00";
  }
  if (value < 0.01) {
    return `$${value.toFixed(4)}`;
  }
  return `$${value.toFixed(2)}`;
}

export function pluralize(count: number, singular: string, plural?: string) {
  const word = count === 1 ? singular : plural ?? `${singular}s`;
  return `${count} ${word}`;
}

export function uniqueStrings(values: string[]) {
  return Array.from(new Set(values.map((value) => value.trim()).filter(Boolean)));
}

export function sanitizeErrorText(value: string) {
  return value
    .replace(/\s*\(\.env\)/gi, "")
    .replace(/\s+from\s+\.env\b/gi, "")
    .replace(/\s+via\s+\.env\b/gi, "")
    .trim();
}

export function errorMessage(error: unknown) {
  return sanitizeErrorText(error instanceof Error ? error.message : String(error));
}

export function metadataString(metadata: Record<string, unknown>, key: string) {
  const value = metadata?.[key];
  return typeof value === "string" && value.trim() ? value : null;
}

export function extractChunkIdFromThumbnail(url: string | null): string | null {
  if (!url) return null;
  const match = url.match(/\/chunks\/([^/]+)\/frame/);
  return match ? decodeURIComponent(match[1]) : null;
}

const IS_MAC =
  typeof navigator !== "undefined" && /Mac|iP(hone|ad|od)/.test(navigator.platform ?? "");

/** Render a stored hotkey id ("Alt+Space") in the platform's idiom:
 * mac users think in ⌥ Space, not Alt+Space. The stored value is unchanged —
 * this is display-only. */
export function formatHotkeyLabel(label: string): string {
  if (!IS_MAC) {
    // "Plus" is the canonical token for the "+" key (it can't live raw in a
    // "+"-separated accelerator); render it back as "+" for display.
    return label
      .split("+")
      .map((part) => (part.trim() === "Plus" ? "+" : part.trim()))
      .join("+");
  }
  return label
    .split("+")
    .map((part) => {
      switch (part.trim()) {
        case "Alt":
          return "\u2325"; // ⌥
        case "Cmd":
          return "\u2318"; // ⌘
        case "Ctrl":
          return "\u2303"; // ⌃
        case "Shift":
          return "\u21e7"; // ⇧
        case "Plus":
          return "+";
        default:
          return part.trim();
      }
    })
    .join(" ");
}

/** Build a copy-paste citation for a moment as Markdown: a quote (when we have
 * the line), then the source + timestamp, then a link that works outside Cerul
 * (the original URL when known, otherwise the in-app deep link). This is what
 * turns "copy timestamp" from an internal-only deep link into something you can
 * paste into a doc or message. */
export function buildMomentCitation(opts: {
  title: string;
  timestamp: string;
  quote?: string | null;
  link: string;
}): string {
  const lines: string[] = [];
  const quote = opts.quote?.trim();
  if (quote) {
    lines.push(`> ${quote}`, "");
  }
  lines.push(`— ${opts.title} @ ${opts.timestamp}`);
  lines.push(opts.link);
  return lines.join("\n");
}
