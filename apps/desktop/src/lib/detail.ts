// Result/Item detail page helpers. Extracted from App.tsx (B13 Phase E).

import type { Item } from "./types";

export function timestampDeepLink(itemId: string, timestamp: string, playbackChunkId?: string | null) {
  const params = new URLSearchParams({ t: timestamp });
  if (playbackChunkId) {
    params.set("playbackChunkId", playbackChunkId);
  }
  return `cerul-app://item/${encodeURIComponent(itemId)}?${params.toString()}`;
}

export function canOpenOriginalSource(item: Item) {
  return Boolean(item.originalUrl || item.rawPath);
}
