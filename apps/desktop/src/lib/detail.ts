// Result/Item detail page helpers. Extracted from App.tsx (B13 Phase E).

import type { Item } from "./types";

export function timestampDeepLink(
  itemId: string,
  timestamp: string,
  playbackChunkId?: string | null,
  view?: "item-detail",
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

export function sourceFileDialogFilter(contentType: string) {
  if (contentType === "audio") {
    return { name: "Audio", extensions: ["mp3", "m4a", "wav", "flac", "aac", "ogg", "opus"] };
  }
  if (contentType === "image") {
    return { name: "Image", extensions: ["png", "jpg", "jpeg", "webp", "gif", "tif", "tiff"] };
  }
  if (contentType === "document") {
    return { name: "Document", extensions: ["pdf", "docx", "pptx", "md", "markdown", "txt"] };
  }
  return { name: "Video", extensions: ["mp4", "mkv", "webm", "mov", "m4v"] };
}
