// Library screen sort helpers. Extracted from App.tsx (B13 Phase E).

import type { Item } from "./types";

export function durationMinutes(duration: string) {
  // Primary: YouTube-style colon format (H:MM:SS or M:SS) — current formatDuration output.
  const parts = duration.split(":").map((part) => Number(part.trim()));
  if (parts.length >= 2 && parts.every((value) => Number.isFinite(value))) {
    const [hours, minutes, seconds] =
      parts.length === 3 ? parts : [0, parts[0], parts[1]];
    // Keep fractional minutes so sub-minute clips (e.g. "0:29") don't collapse to
    // 0 — otherwise they'd contribute nothing to aggregate runtime and tie with
    // unknown-duration items when sorting. Callers round only for display.
    return (hours * 3600 + minutes * 60 + seconds) / 60;
  }
  // Legacy fallback: "Xh Ym" strings.
  const hours = Number(/(\d+)\s*h/.exec(duration)?.[1] ?? 0);
  const minutes = Number(/(\d+)\s*m/.exec(duration)?.[1] ?? 0);
  return hours * 60 + minutes;
}

export function sortLibraryItems(
  a: Item,
  b: Item,
  sortKey: "recent" | "longest" | "shortest" | "title",
) {
  if (sortKey === "longest") {
    return durationMinutes(b.duration) - durationMinutes(a.duration);
  }
  if (sortKey === "shortest") {
    return durationMinutes(a.duration) - durationMinutes(b.duration);
  }
  if (sortKey === "title") {
    return a.title.localeCompare(b.title);
  }
  // "recent" means date added, including items that have not finished indexing.
  // Fall back to the legacy indexed timestamp for records created before the
  // discovered_at migration.
  return (b.addedAtEpoch ?? b.indexedAtEpoch ?? 0) -
    (a.addedAtEpoch ?? a.indexedAtEpoch ?? 0);
}
