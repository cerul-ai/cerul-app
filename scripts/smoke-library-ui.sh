#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF "const [libraryQuery, setLibraryQuery]" apps/desktop/src
rg -qF "const [sourceFilter, setSourceFilter]" apps/desktop/src
rg -qF "const [statusFilter, setStatusFilter]" apps/desktop/src
rg -qF "const [durationFilter, setDurationFilter]" apps/desktop/src
rg -qF "const [dateFilter, setDateFilter]" apps/desktop/src
rg -qF "const [contentFilter, setContentFilter]" apps/desktop/src
rg -qF "const [sortKey, setSortKey]" apps/desktop/src
rg -qF "const [selectedItemIds, setSelectedItemIds]" apps/desktop/src
rg -qF "async function runBatchAction" apps/desktop/src
rg -qF 'const activeJobCount = apiStatus === "online" && data.jobSummary' apps/desktop/src
rg -qF "data.jobSummary.queued_jobs + data.jobSummary.running_jobs" apps/desktop/src
rg -qF ": [];" apps/desktop/src/App.tsx
! rg -qF "<IndexingStrip" apps/desktop/src/screens/library.tsx
! rg -qF "item.usage.event_count" apps/desktop/src/components/cards.tsx
! rg -qF "itemCapabilityChips" apps/desktop/src/components/cards.tsx
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
rg -qF "api.deleteItem(itemId, options)" apps/desktop/src
rg -qF "keepDiscoverable?: boolean" apps/desktop/src
rg -qF "{ keepDiscoverable: true }" apps/desktop/src
rg -qF "api.reindexItem(itemId)" apps/desktop/src
rg -qF "const filteredItems = items" apps/desktop/src
rg -qF "sortLibraryItems(a, b, sortKey)" apps/desktop/src
rg -qF "function durationMinutes(duration: string)" apps/desktop/src
rg -qF 'if (!hasColonDuration && !hasLegacyDurationUnit) return null' apps/desktop/src/screens/library.tsx
rg -qF 'if (filter === "month") return ageDays <= 30' apps/desktop/src/screens/library.tsx
rg -qF 'viewMode === "list" ? "item-card list" : "item-card"' apps/desktop/src
rg -qF 'library-final-page' apps/desktop/src/screens/library.tsx
rg -qF 'className="library-final-layout"' apps/desktop/src/screens/library.tsx
rg -qF 'viewMode === "grid" ? "lib-grid library-l3-grid library-view-collection"' apps/desktop/src/screens/library.tsx
rg -qF 'viewMode={viewMode}' apps/desktop/src/screens/library.tsx
rg -qF 'useState<"grid" | "list">("grid")' apps/desktop/src/screens/library.tsx
rg -qF 'className="library-filter-rail"' apps/desktop/src/screens/library.tsx
rg -qF 'className="library-view-switch"' apps/desktop/src/screens/library.tsx
rg -qF 'startViewTransition' apps/desktop/src/screens/library.tsx
rg -qF 'transitionName={`library-item-${index}`}' apps/desktop/src/screens/library.tsx
rg -qF '"library.empty.filtered.title": "No matching items"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"library.batch.aria": "Selected library item actions"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"library.itemCard.selectAria": "Select item"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF "export type JobRecord" apps/desktop/src/lib/api.ts
rg -qF "thumbnail_chunk_id: string | null" apps/desktop/src/lib/api.ts
rg -qF "export function chunkFrameUrl" apps/desktop/src/lib/api.ts
rg -qF "export async function listJobs" apps/desktop/src/lib/api.ts
rg -qF "export async function deleteItem" apps/desktop/src/lib/api.ts
rg -qF "export async function reindexItem" apps/desktop/src/lib/api.ts
rg -qF '"/items/:id"' crates/cerul-api/src/routes/library.rs
rg -qF "get(get_item).patch(update_item).delete(remove_item)" crates/cerul-api/src/routes/library.rs
rg -qF '.route("/items/:id/reindex", post(reindex_item))' crates/cerul-api/src/routes/library.rs
rg -qF "async fn remove_item" crates/cerul-api/src/routes/library.rs
rg -qF "async fn reindex_item" crates/cerul-api/src/routes/library.rs
rg -qF "item_delete_and_reindex_update_storage" crates/cerul-api/src/lib.rs
rg -qF 'total: items.length' apps/desktop/src/screens/library.tsx
rg -qF "list_items_includes_first_frame_thumbnail_chunk" crates/cerul-api/src/lib.rs
rg -qF ".library-filter-row .select" apps/desktop/src/styles/extensions.css
rg -qF ".segmented" apps/desktop/src/styles/ui.css
rg -qF ".lib-grid" apps/desktop/src/styles/app.css
rg -qF ".tbl" apps/desktop/src/styles/app.css
rg -qF ".item-card.list" apps/desktop/src/styles/extensions.css
rg -qF ".item-card-shell.selected" apps/desktop/src/styles/extensions.css
rg -qF ".item-thumb.has-image img" apps/desktop/src/styles/extensions.css
rg -qF ".item-progress-overlay" apps/desktop/src/styles/extensions.css
rg -qF ".library-retrieval-controls" apps/desktop/src/styles/selected-ui.css
rg -qF ".library-filter-rail button.active" apps/desktop/src/styles/selected-ui.css
rg -qF "selection-pointer-sweep" apps/desktop/src/styles/selected-ui.css

echo "library_ui_smoke layout=F2_persistent_filter_rail status=L3_abnormal_only selection=A4_pointer_sweep filters=duration_date_source_content_status date_month=inclusive unknown_duration=excluded views=grid_default_and_list motion=360ms_spatial_reflow usage_metadata=hidden"
