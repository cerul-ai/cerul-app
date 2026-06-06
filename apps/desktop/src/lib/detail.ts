// Result/Item detail page helpers. Extracted from App.tsx (B13 Phase E).

import type { Item } from "./types";

export function timestampDeepLink(itemId: string, timestamp: string) {
  return `cerul-app://item/${encodeURIComponent(itemId)}?t=${encodeURIComponent(timestamp)}`;
}

export function canOpenOriginalSource(item: Item) {
  return Boolean(item.originalUrl || item.rawPath);
}

export function originalSourceLabel(item: Item) {
  return item.originalUrl ? "Open in original source" : "Show in Finder";
}
