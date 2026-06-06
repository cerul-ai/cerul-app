// Library screen sort helpers. Extracted from App.tsx (B13 Phase E).

import type { Item } from "./types";

export function durationMinutes(duration: string) {
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
  return 0;
}
