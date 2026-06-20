#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF "const [libraryQuery, setLibraryQuery]" apps/desktop/src
rg -qF "const [sourceFilter, setSourceFilter]" apps/desktop/src
rg -qF "const [statusFilter, setStatusFilter]" apps/desktop/src
rg -qF "const [sortKey, setSortKey]" apps/desktop/src
rg -qF "const [viewMode, setViewMode]" apps/desktop/src
rg -qF "const [selectedItemIds, setSelectedItemIds]" apps/desktop/src
rg -qF "async function runBatchAction" apps/desktop/src
rg -qF "const activeJobCount = visibleJobs.filter(isActiveJob).length" apps/desktop/src
rg -qF ": [];" apps/desktop/src/App.tsx
rg -qF "<IndexingStrip" apps/desktop/src/App.tsx
rg -qF "jobs={jobs}" apps/desktop/src/App.tsx
rg -qF "items={items}" apps/desktop/src/App.tsx
rg -qF "stepStarts={stepStarts}" apps/desktop/src/App.tsx
rg -qF "paused={indexingPaused}" apps/desktop/src/App.tsx
rg -qF "onOpen={onOpenJobs}" apps/desktop/src/App.tsx
rg -qF "active.reduce(" apps/desktop/src/components/indexing-strip.tsx
rg -qF "/ active.length" apps/desktop/src/components/indexing-strip.tsx
rg -qF 'rep.job_type !== "index_audio"' apps/desktop/src/components/indexing-strip.tsx
rg -qF "window.setInterval" apps/desktop/src
rg -qF "function isActiveJob" apps/desktop/src
rg -qF "jobRecords" apps/desktop/src
rg -qF "mapItemRecord(record, jobRecords, t)" apps/desktop/src
rg -qF "latestActiveJobForItem" apps/desktop/src
rg -qF "progress: itemProgress" apps/desktop/src
rg -qF "thumbnailUrl: record.thumbnail_chunk_id ? api.chunkFrameUrl(record.thumbnail_chunk_id) : null" apps/desktop/src
rg -qF "item-progress-overlay" apps/desktop/src
rg -qF "function ItemModalityIcon" apps/desktop/src
rg -qF "item.thumbnailUrl" apps/desktop/src
rg -qF "Math.round(item.progress * 100)" apps/desktop/src
rg -qF "onDeleteItems={async (itemIds" apps/desktop/src
rg -qF "onReindexItems={async (itemIds)" apps/desktop/src
rg -qF "api.deleteItem(itemId)" apps/desktop/src
rg -qF "api.reindexItem(itemId)" apps/desktop/src
rg -qF "const filteredItems = items" apps/desktop/src
rg -qF "sortLibraryItems(a, b, sortKey)" apps/desktop/src
rg -qF "function durationMinutes(duration: string)" apps/desktop/src
rg -qF 'className={viewMode === "grid" ? "lib-grid" : "tbl lib-table"}' apps/desktop/src
rg -qF 'viewMode === "list" ? "item-card list" : "item-card"' apps/desktop/src
rg -qF '"library.empty.filtered.title": "No matching items"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"library.batch.aria": "Selected library item actions"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"library.itemCard.selectAria": "Select item"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF "export type JobRecord" apps/desktop/src/lib/api.ts
rg -qF "thumbnail_chunk_id: string | null" apps/desktop/src/lib/api.ts
rg -qF "export function chunkFrameUrl" apps/desktop/src/lib/api.ts
rg -qF "export async function listJobs" apps/desktop/src/lib/api.ts
rg -qF "export async function deleteItem" apps/desktop/src/lib/api.ts
rg -qF "export async function reindexItem" apps/desktop/src/lib/api.ts
rg -qF '"/items/:id"' crates/cerul-api/src/lib.rs
rg -qF "get(get_item).patch(update_item).delete(remove_item)" crates/cerul-api/src/lib.rs
rg -qF '.route("/items/:id/reindex", post(reindex_item))' crates/cerul-api/src/lib.rs
rg -qF "async fn remove_item" crates/cerul-api/src/lib.rs
rg -qF "async fn reindex_item" crates/cerul-api/src/lib.rs
rg -qF "item_delete_and_reindex_update_storage" crates/cerul-api/src/lib.rs
rg -qF "list_items_includes_first_frame_thumbnail_chunk" crates/cerul-api/src/lib.rs
rg -qF ".library-filter-row .select" apps/desktop/src/styles/extensions.css
rg -qF ".segmented" apps/desktop/src/styles/ui.css
rg -qF ".lib-grid" apps/desktop/src/styles/app.css
rg -qF ".tbl" apps/desktop/src/styles/app.css
rg -qF ".item-card.list" apps/desktop/src/styles/extensions.css
rg -qF ".item-card-shell.selected" apps/desktop/src/styles/extensions.css
rg -qF ".item-thumb.has-image img" apps/desktop/src/styles/extensions.css
rg -qF ".item-progress-overlay" apps/desktop/src/styles/extensions.css

echo "library_ui_smoke search_filter_sort=enabled grid_list_toggle=enabled thumbnails=first_frame_chunk grid_list_thumbnail=enabled batch_delete_reindex=enabled indexing_progress_overlay=enabled active_job_polling=enabled"
