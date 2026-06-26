// Result/Item detail page helpers. Extracted from App.tsx (B13 Phase E).

import type { Item } from "./types";

export function timestampDeepLink(
  itemId: string,
  timestamp: string,
  playbackChunkId?: string | null,
  view?: "item-detail" | "result-detail",
) {
  const params = new URLSearchParams({ t: timestamp });
  if (playbackChunkId) {
    params.set("playbackChunkId", playbackChunkId);
  }
  if (view) {
    params.set("view", view);
  }
  return `cerul-app://item/${encodeURIComponent(itemId)}?${params.toString()}`;
}

export function canOpenOriginalSource(item: Item) {
  return Boolean(item.originalUrl || item.rawPath);
}
