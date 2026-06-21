#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF "function handleResultsKeyDown" apps/desktop/src
rg -qF "const [expandedResultIds, setExpandedResultIds]" apps/desktop/src
rg -qF "const [modalityFilter, setModalityFilter]" apps/desktop/src
rg -qF "const [sortMode, setSortMode]" apps/desktop/src
rg -qF "const filteredResults = results.filter" apps/desktop/src
rg -qF "const displayedResults =" apps/desktop/src
rg -qF "right.indexedAtEpoch" apps/desktop/src
rg -qF "modalityCounts.image" apps/desktop/src
rg -qF "function clearResultFilters" apps/desktop/src
rg -qF '"results.modeTabs.aria": "Result modality counts"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"results.modeTabs.shown": "shown"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF '"results.sort.recent": "Recent"' apps/desktop/src/lib/i18n-catalog.ts
rg -qF "function resultModality" apps/desktop/src
rg -qF "function ResultModalityIcon" apps/desktop/src
rg -qF "FileAudio" apps/desktop/src
rg -qF "Image as ImageIcon" apps/desktop/src
rg -qF "function resultThumbnailUrl" apps/desktop/src/lib/results.ts
rg -qF "record.nearest_frame_chunk_id" apps/desktop/src/lib/results.ts
rg -qF "return item?.thumbnailUrl ?? null" apps/desktop/src/lib/results.ts
rg -qF "result.thumbnailUrl" apps/desktop/src
rg -qF "chunkFrameUrl" apps/desktop/src/lib/api.ts
rg -qF "<ResultModalityIcon result={result} size={14} />" apps/desktop/src
rg -qF 'if ((event.metaKey || event.ctrlKey) && event.key === "ArrowDown")' apps/desktop/src
rg -qF 'event.key === "ArrowDown" || event.key === "ArrowUp"' apps/desktop/src
rg -qF 'event.key === "Enter" && event.target === event.currentTarget' apps/desktop/src
rg -qF "focusResult(nextIndex)" apps/desktop/src
rg -qF "data-result-index={index}" apps/desktop/src
rg -qF "function ResultsSkeletonList" apps/desktop/src
rg -qF "result-skeleton" apps/desktop/src
rg -qF "aria-selected={selected}" apps/desktop/src
rg -qF 'aria-expanded={result.moreMatches.length > 0 ? expanded : undefined}' apps/desktop/src
rg -qF "result.moreMatches.length > 0 && !expanded" apps/desktop/src
rg -qF "result.moreMatchesHint" apps/desktop/src
rg -qF "result.moreMatches.map" apps/desktop/src
rg -qF ".result-card.result-row.active" apps/desktop/src/styles/extensions.css
rg -qF ".result-card.result-row:focus-visible" apps/desktop/src/styles/extensions.css
rg -qF ".result-skeleton" apps/desktop/src/styles/extensions.css
rg -qF ".thumb.has-image img" apps/desktop/src/styles/extensions.css
rg -qF ".result-more-matches" apps/desktop/src/styles/extensions.css
rg -qF ".results-filter-row" apps/desktop/src/styles/app.css

echo "results_ui_smoke keyboard_nav=arrow_up_down enter_opens_active_result cmd_down_expands_more_matches filters=modality sort=recent_by_item_indexed_at thumbnails=keyframe_or_nearest_frame_url modality_icons=video_audio_image active_result_style=enabled loading_skeleton=enabled local_empty_state=enabled"
