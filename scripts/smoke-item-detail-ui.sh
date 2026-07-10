#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF "export type ChunkRecord" apps/desktop/src/lib/api.ts
rg -qF "export async function listItemChunks" apps/desktop/src/lib/api.ts
rg -qF "type TranscriptLine" apps/desktop/src
rg -qF "sourceKind: ItemSourceKind" apps/desktop/src
rg -qF "rawPath: string | null" apps/desktop/src
rg -qF "originalUrl: string | null" apps/desktop/src
rg -qF "error: string | null" apps/desktop/src
rg -qF "mapChunkRecords(records, t)" apps/desktop/src
rg -qF ".listItemChunks(item.id)" apps/desktop/src
rg -qF 'actionsEnabled={apiStatus === "online"}' apps/desktop/src
rg -qF "const detailIssue = itemDetailIssue(item, t)" apps/desktop/src
rg -qF "DetailIssuePanel" apps/desktop/src
rg -qF "if (hasOpenModalSurface())" apps/desktop/src
rg -qF "const citationTimestampLink = timestampDeepLink(" apps/desktop/src/screens/item-detail.tsx
rg -qF "link={item.originalUrl ?? citationTimestampLink}" apps/desktop/src/screens/item-detail.tsx
rg -qF 'document.addEventListener("selectionchange", captureCiteSelection)' apps/desktop/src/screens/item-detail.tsx
rg -qF 'if (!lineId) return;' apps/desktop/src/screens/item-detail.tsx
rg -qF 'if (!text) {' apps/desktop/src/screens/item-detail.tsx
rg -qF "item.issue.missingFile.title" apps/desktop/src
rg -qF "item.issue.youtube.title" apps/desktop/src
rg -qF 'className="page-sub"' apps/desktop/src
rg -qF "itemModalityLabel(item, t)" apps/desktop/src
rg -qF 't("detail.indexedAt"' apps/desktop/src
rg -qF '"itemDetail.metric.ingested": "Ingested"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"itemDetail.metric.chunks": "Chunks"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"itemDetail.metric.model": "Model"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF "detail.stillProcessing" apps/desktop/src
rg -qF "function TranscriptSkeleton" apps/desktop/src
rg -qF "lines={transcriptLines}" apps/desktop/src
rg -qF ".transcript-skeleton" apps/desktop/src/styles/extensions.css
rg -qF ".detail-issue" apps/desktop/src/styles/extensions.css
rg -qF ".video-frame" apps/desktop/src/styles/extensions.css
rg -qF ".cplayer" apps/desktop/src/styles/extensions.css

echo "item_detail_ui_smoke chunks_api=enabled header_metadata=live transcript_loading=enabled source_issue_states=enabled processing_notice=enabled player=cerul_player"
