#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rg -qF "function handleResultsKeyDown" apps/desktop/src
rg -qF "const [expandedResultIds, setExpandedResultIds]" apps/desktop/src
rg -qF "const [sortMode, setSortMode]" apps/desktop/src
rg -qF "const displayedResults =" apps/desktop/src/screens/results.tsx
rg -qF "right.indexedAtEpoch" apps/desktop/src
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
rg -qF 'className="page wide results-page-r1"' apps/desktop/src/screens/results.tsx
rg -qF 'className="results-r1-head"' apps/desktop/src/screens/results.tsx
rg -qF 'className="results-card-list results-citation-stream"' apps/desktop/src/screens/results.tsx
rg -qF 'const citation = buildMomentCitation' apps/desktop/src/components/cards.tsx
rg -qF 'await writeClipboardText(citation)' apps/desktop/src/components/cards.tsx
rg -qF 'timestampDeepLink(' apps/desktop/src/components/cards.tsx
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
rg -qF ".results-citation-stream" apps/desktop/src/styles/selected-ui.css
rg -qF 'className="results-filter-rail"' apps/desktop/src/screens/results.tsx
rg -qF 'className="results-answer-rail"' apps/desktop/src/screens/results.tsx
rg -qF 'api.askLibrary' apps/desktop/src/screens/results.tsx
rg -qF 'api.askAgentLibrary' apps/desktop/src/screens/results.tsx
rg -qF 'onOpenCitation' apps/desktop/src/screens/results.tsx

echo "results_ui_smoke layout=R1_three_column_evidence_stream answer=comprehensive_grounded_qa bridge_search=single_owner sort=relevance_recent ranking=smart_literal citation_actions=jump_copy keyboard_nav=enabled thumbnails=keyframe_or_nearest_frame_url modality_icons=video_audio_image"
