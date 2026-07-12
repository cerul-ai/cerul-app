#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

! test -f apps/desktop/src/screens/result-detail.tsx
! rg -qF 'view === "result-detail"' apps/desktop/src/App.tsx
rg -qF "startChunkId" apps/desktop/src
rg -qF "const [copyStatus, setCopyStatus]" apps/desktop/src
rg -qF "const [currentTimestamp, setCurrentTimestamp]" apps/desktop/src
rg -qF "const videoRef = useRef<HTMLVideoElement | null>(null)" apps/desktop/src
rg -qF "const [itemAction, setItemAction]" apps/desktop/src
rg -qF "ClipExportButton" apps/desktop/src
rg -qF "function resolveClipTarget()" apps/desktop/src
rg -qF "const detailIssue = itemDetailIssue(item, t)" apps/desktop/src
rg -qF "function hasOpenModalSurface" apps/desktop/src
rg -qF "if (hasOpenModalSurface())" apps/desktop/src
rg -qF "api.videoSegmentUrl(playableChunkId)" apps/desktop/src
rg -qF ".listItemChunks(item.id)" apps/desktop/src
rg -qF "function selectPlaybackChunkId" apps/desktop/src
rg -qF "export type DetailIssue" apps/desktop/src
rg -qF "export function DetailIssuePanel" apps/desktop/src
rg -qF "export function itemDetailIssue(item: Item, t: TFunction)" apps/desktop/src
rg -qF "item.issue.missingFile.title" apps/desktop/src
rg -qF "item.issue.youtube.title" apps/desktop/src
rg -qF "detail.issue.locate" apps/desktop/src
rg -qF "item.issue.removeLabel" apps/desktop/src
rg -qF "detail.stillProcessing" apps/desktop/src
rg -qF "function seekTo(timestamp: string" apps/desktop/src
rg -qF "async function locateSourceFile" apps/desktop/src
rg -qF "async function reindexCurrentItem" apps/desktop/src
rg -qF "async function deleteCurrentItem" apps/desktop/src
rg -qF "openDialog({" apps/desktop/src
rg -qF "onDeleteItem={async (itemToDelete)" apps/desktop/src
rg -qF "onReindexItem={async (itemToReindex)" apps/desktop/src
rg -qF "api.deleteItem(itemToDelete.id)" apps/desktop/src
rg -qF "api.reindexItem(itemToReindex.id)" apps/desktop/src
rg -qF "onClick={() => onSeek?.(line.time, line)}" apps/desktop/src
rg -qF "onSeek={seekTo}" apps/desktop/src
rg -qF "matchTime={startTimestamp}" apps/desktop/src
rg -qF "line.time === matchTime" apps/desktop/src
rg -qF "playbackChunkId: result.playbackChunkId" apps/desktop/src
rg -qF "cerul-app://item/" apps/desktop/src
rg -qF "function canOpenOriginalSource(item: Item)" apps/desktop/src
rg -qF "async function openOriginalSourceForItem(item: Item, t: TFunction)" apps/desktop/src
rg -qF 't("detail.source.openOriginal")' apps/desktop/src
rg -qF 't("detail.source.reveal")' apps/desktop/src
rg -qF "revealSourcePath(item.rawPath)" apps/desktop/src
rg -qF "detail.source.reveal" apps/desktop/src
rg -qF "navigator.clipboard.writeText" apps/desktop/src
rg -qF "document.execCommand(\"copy\")" apps/desktop/src
rg -qF "detail.copy.success" apps/desktop/src
rg -qF "resolveTarget={resolveClipTarget}" apps/desktop/src
rg -qF 't("detail.action.exportingClip")' apps/desktop/src
rg -qF 't("detail.action.clipExported")' apps/desktop/src
rg -qF "common.reindexQueued" apps/desktop/src
rg -qF "common.confirm.delete.body" apps/desktop/src
rg -qF "case \"reveal_source_path\"" apps/electron-shell/src/main.ts
rg -qF "function revealSource" apps/electron-shell/src/main.ts
rg -qF "shell.showItemInFolder(source)" apps/electron-shell/src/main.ts
rg -qF "invokeHostCommand(\"reveal_source_path\"" apps/desktop/src/App.tsx
rg -qF ".cplayer" apps/desktop/src/styles/extensions.css
rg -qF ".player-loading" apps/desktop/src/styles/extensions.css
rg -qF ".video-frame-unavailable" apps/desktop/src/styles/extensions.css
rg -qF ".detail-issue" apps/desktop/src/styles/extensions.css
rg -qF ".detail-issue-actions" apps/desktop/src/styles/extensions.css
rg -qF ".seg-btn.matched" apps/desktop/src/styles/app.css
rg -qF ".transcript .seg-btn" apps/desktop/src/styles/extensions.css
rg -qF 'const [leftWidth, setLeftWidth] = useState(19)' apps/desktop/src/components/SplitStage.tsx
rg -qF 'if (showChapters) setNavigationTab("chapters")' apps/desktop/src/components/SplitStage.tsx
rg -qF '.page.wide.detail-workbench-page { width:100%; max-width:none; height:100%; margin:0' apps/desktop/src/styles/selected-ui.css
rg -qF '.detail-workbench-page .transcript .seg-btn { width:100%' apps/desktop/src/styles/selected-ui.css
rg -qF '.detail-workbench-page .detail-right-stack { min-height:0; height:100%; display:flex; flex-direction:column; overflow:hidden; }' apps/desktop/src/styles/selected-ui.css
rg -qF 'onShare={item.contentType === "video" && cloudAuthStatus === "signedIn" ? createPublicShare : undefined}' apps/desktop/src/screens/item-detail.tsx
rg -qF 'Public sharing is an enhancement to citation copy' apps/desktop/src/components/citation-card.tsx
rg -qF 'recordManagedShare(' apps/desktop/src/screens/item-detail.tsx
rg -qF 'cloudUserId,' apps/desktop/src/screens/item-detail.tsx
rg -qF 'share.identity?.itemId === item.id' apps/desktop/src/screens/item-detail.tsx
rg -qF 'share.identity.chunkId === shareChunkId' apps/desktop/src/screens/item-detail.tsx
rg -qF 'share.headline === citationDraft.quote' apps/desktop/src/screens/item-detail.tsx
rg -qF 'const audioPlaybackUrl =' apps/desktop/src/screens/item-detail.tsx
rg -qF 'const imagePreviewUrl =' apps/desktop/src/screens/item-detail.tsx
rg -qF 'ref={handleAudioElement}' apps/desktop/src/screens/item-detail.tsx
rg -qF 'src={imagePreviewUrl}' apps/desktop/src/screens/item-detail.tsx
rg -qF 't("understanding.action.reanalyze")' apps/desktop/src/screens/item-detail.tsx
rg -qF 'matchedSnippet={resultContext?.snippet}' apps/desktop/src/screens/item-detail.tsx
rg -qF 'onBack={() => navigate(detailOrigin)}' apps/desktop/src/App.tsx
! rg -qF 'addToBasket' apps/desktop/src/components/citation-card.tsx

echo "detail_ui_smoke layout=fluid_three_pane route=single_item_detail origin_aware_back=enabled media=video_audio_image_document default_navigation=chapters transcript=vertical_only citation_actions=single_copy_public_link share_identity=item_chunk_timestamp_quote analysis=compact_reanalyze copy_timestamp_link=cerul_deep_link playback_chunk_deeplink=enabled clipboard_fallback=enabled open_original=link_or_finder source_missing=enabled youtube_unavailable=enabled transcript_partial=enabled status_feedback=enabled seek_controls=transcript_and_matches matched_marker=enabled item_delete_reindex=enabled"
