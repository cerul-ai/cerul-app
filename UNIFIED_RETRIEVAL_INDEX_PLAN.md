# Unified Retrieval Index Plan

## Decision

Cerul App should replace the current hybrid search path with a single unified retrieval-unit index.

This is a hard cutover, not an incremental migration. After the release ships, search must use the new index shape only. Existing local search artifacts from the previous hybrid design are invalid for ranking and should be deleted or ignored until rebuilt into the new format.

Distinguish two things the word "cutover" is doing here:

- **Code / architecture is a hard cutover.** The old hybrid retrieval path (three-way RRF, separate image branch, source masks, calibrated match-score) is deleted, not feature-flagged. There is exactly one retrieval mode in the product.
- **Live index data is migrated build-then-swap.** The new collection is built and self-checked before the old one is removed, so an existing user is never left with a silent empty search box mid-upgrade. The old collection is retained only as deployment safety data; the product does not keep serving old hybrid results. This does not violate "no incremental migration of the design."

## Current Problems

The current desktop search path is too fragmented:

- Transcript/OCR text chunks, raw keyframe image embeddings, and SQLite FTS are searched independently.
- Results are merged at query time with RRF, which is rank-based and does not sufficiently respect absolute vector similarity.
- Raw image embedding results can pollute normal typed searches because the image branch always contributes candidates even when visual similarity is weak.
- Speech, OCR, visual context, title, and video-understanding summaries do not reinforce each other before embedding; they only meet after retrieval, which is too late.
- Search has too many moving parts: query embedding, FTS, text vector search, image vector search, RRF, hydration, dedupe, calibrated score display.
- User-facing scores are hard to explain because the displayed score is not the same signal as raw vector similarity.
- Video-understanding chunks are currently searchable mostly through text fallback, not as first-class semantic retrieval units.

The old API felt better because it moved complexity to indexing: it built a richer `content_text` retrieval unit before embedding, then searched a single semantic space.

## Target Architecture

### Core Principle

Build multimodal meaning at index time. Search should be simple:

```text
typed query -> query embedding -> one vector index collection -> hydrate retrieval units
```

No text-vector/image-vector/FTS three-way merge on the hot path.

### Retrieval Unit

Introduce a local `retrieval_units` representation. Each unit is the smallest searchable semantic moment.

Recommended fields:

- `id`
- `item_id`
- `unit_index`
- `unit_kind`: `moment`, `summary`, `visual`, `image`
- `start_sec`
- `end_sec`
- `content_text`
- `transcript_text`
- `ocr_text`
- `visual_text`
- `summary_text`
- `representative_chunk_id`
- `representative_frame_path`
- `embedding_profile_id`
- `index_version`
- `metadata`
- `created_at`
- `updated_at`

The vector-index payload should stay small:

- `unit_id`
- `item_id`
- `unit_kind`
- `start_sec`
- `end_sec`
- `index_version`

Hydration should come from SQLite, not from vector-index payload blobs.

### Unit Construction

For audio/video items, build units from time windows. Prefer meaning-aligned boundaries over arbitrary cuts:

- If video-understanding chapters/events exist, align unit boundaries to them. An arbitrary cut mid-sentence or mid-topic dilutes the embedding and is a real quality loss.
- Otherwise fall back to deterministic windows: ~30 seconds with ~5 seconds overlap.
- Avoid embedding every transcript line.
- Prefer one high-signal unit over many tiny weak units.

Each unit should combine available evidence:

```text
Title: {item_title}
Source: {source_title_or_url}
Time: {start_sec}-{end_sec}
Transcript: {transcript text in window}
On-screen text: {OCR near this window}
Visual context: {video-understanding event/chapter text near this window}
Topics/Summary: {item-level understanding summary when useful}
```

Rules:

- Empty fields are omitted.
- Text is normalized and length-limited before embedding, using a **per-field budget and priority order** (transcript first, then OCR, then visual/understanding, then summary). Do not naively concatenate and tail-truncate, or the OCR/summary at the end gets silently dropped and the vector stops representing them.
- Nearby OCR should be attached by timestamp or nearest keyframe.
- Video-understanding events/chapters should attach by overlapping timestamp.
- Item-level summary should be used sparingly so every unit does not become identical. Prepending the same summary to every unit of a video collapses those units together in vector space and weakens per-item dedupe and ranking.
- Representative frame is for display and playback context; it is not a separate search branch.

For image-only items:

- If OCR/EXIF/title gives useful text, build one `image` unit from that text.
- If there is no useful text, embed the image itself into the same unified collection, using the same embedding profile when it supports image embeddings.
- Do not create a separate image-search collection for normal typed search.

### Embedding

Use one active embedding profile per index version.

Cloud path:

- Use Gemini Embedding 2 for query and retrieval-unit embeddings.
- Prefer text `content_text` document embeddings for units that have textual evidence.
- **Open decision before committing to a single collection:** confirm whether Gemini Embedding 2 can embed images into the same space as text. If it cannot, image-only items with no OCR/text cannot enter the unified collection in cloud mode — they must either be grounded by OCR/caption text or be explicitly marked unsearchable in cloud mode. Do not assume one collection works on the cloud path until this is verified.

Local path:

- Use the existing Qwen3-VL embedding profile.
- Textual retrieval units are embedded as text.
- Visual-only units can be embedded from the representative image only when there is no meaningful text.

The key design point is not dimension count. It is that the unit vector represents the combined moment, not an isolated transcript chunk or an isolated keyframe.

### Search API

`POST /search` should:

1. Trim and validate `q`.
2. Generate one query embedding for the active profile.
3. Vector-search one collection: `retrieval_units_{profile}_{index_version}`. This is primary recall.
4. Run one cheap lexical recall pass over `content_text` (FTS + CJK literal) and **union** its hits into the candidate set.
5. Hydrate units from SQLite.
6. Rank (see Ranking) and return.

Removed vs today: no RRF, no separate image-vector branch, no three-way rank fusion. Kept: a single lexical recall pass — but only as a candidate-union step feeding **one** vector-based ranking, not a co-equal RRF input that contributes its own rank.

Why lexical recall stays on the main path (this is a correctness requirement, not optional polish): dense embeddings — especially local quantized profiles — are unreliable for exact CJK terms, names, product codes, and verbatim quotes. A "literal boost applied only to already-retrieved units" cannot rescue a unit the vector search never returned. The current code depends on a CJK literal `LIKE` recall path (`sqlite_literal_search`) precisely because FTS5's default tokenizer does not segment CJK; dropping it from the main path is a recall regression disguised as a simplification. Lexical recall must therefore **add candidates**, not merely re-rank vector candidates.

Pure-FTS-only mode still remains as the degraded fallback when query embedding is unavailable, reported as fallback diagnostics.

### Ranking

Ranking should be simple and inspectable:

- Primary rank: vector similarity from the unified retrieval-unit collection.
- Exact-match handling: strong pin only applies to high-confidence literal intent — quoted spans, longer verbatim phrases, proper nouns/entities, product/model names, IDs, and code-like strings. Short or high-frequency terms such as "AI", "cloud", or "video" should receive a bounded lexical boost, not an unconditional pin. This keeps "I remember the exact words / exact name" queries reliable without letting common words dominate ranking.
- Lexical-only candidates (units surfaced by the lexical recall pass but absent from the vector top-k) are scored by fetching their own unit-vector similarity, so every candidate shares one comparable score; high-confidence exact candidates are pinned/boosted by the rule above.
- Per-item cap: default max 2-3 units per item in the final result set.
- Near-duplicate window cap: suppress units from the same item whose time ranges overlap heavily.
- Playback precision: a unit spans ~20-45s, but the returned `start_sec` should resolve to the best **sub-unit line** — locate the query terms inside the window and jump there, not to the window start. Otherwise a "jump to the moment" product lands the user up to ~30s early.

The UI score should be derived from the same vector score used for ranking, plus an explicit "exact match" flag when a result is pinned. No separate fused-score/match-score story.

### Diagnostics

Replace current diagnostics with unified-index diagnostics:

- `retrieval_mode`: `unified_vector` / `fts_fallback` / `empty`
- `fallback_reason`
- `embedding_profile_id`
- `index_version`
- `retrieval_unit_count`
- `vector_index_collection`
- `vector_index_point_count`
- `indexed_item_count`
- `items_needing_rebuild`
- `items_blocked_by_missing_source`
- `query_embedding_ms`
- `vector_index_search_ms`
- `hydrate_ms`

Diagnostics should prove the app is not accidentally searching old text/image collections.

## Relevance Eval & Quality Gate

The entire justification for this change is "more accurate." "Feels better" cannot be the release gate, especially since the old API's accuracy came from cloud Gemini 3072 while the local path is a quantized Qwen3-VL profile embedding a longer, more multilingual fused unit. Build a small labeled relevance set and require the new index to not regress before the cutover ships.

### Labeled query set

- A JSONL file of `{query, expected (title/text substring or item_id), expected_time_range?, query_type, grade?}` rows, run against the real populated library (not a synthetic tempdir).
- Label by human-readable key (video title + a transcript snippet or rough timestamp) and resolve to `item_id` at eval time, so rows are cheap to hand-author.
- Stratify by `query_type` so a regression in one mode cannot hide behind a gain in another:
  - semantic / paraphrase (the main intended win)
  - exact entity / proper noun (the lexical-recall risk)
  - verbatim phrase / quote
  - on-screen text / OCR
  - visual ("red-background launch", "whiteboard diagram")
  - bilingual zh / en / mixed
- Start at ~30-50 queries (>=5 per type) and grow toward ~100-150. Coverage of types matters more than raw count.

### Metrics and gate

- Recall@k (k = `retrieval_limit`) is primary: it directly catches the exact-entity recall regression.
- precision@k / nDCG@k for ranking quality; MRR for known-item queries.
- Moment accuracy for results whose item is correct: is `start_sec` within +/- N seconds of the true moment (tests playback precision).
- Report per-type and **gate per-type**: the new index must be `>=` the current desktop baseline on every type, and should aim to match old `cerul-api` on semantic queries.

### Harness

- A script that reads the query set, hits the running Core `/search`, computes the metrics, and prints a table plus a diff against a saved baseline.
- Run it against current desktop (baseline), old `cerul-api` if reachable (ceiling), and the new unified index.
- Reuse the same set to tune window length, truncation budget, and the exact-match boost. Keep a held-out slice so the design is not tuned to the eval.

## Hard Cutover Plan

### Index Version

Introduce a new search index version, separate from model snapshot versions.

Example:

```text
SEARCH_INDEX_VERSION = 2
```

Any item without retrieval units for the active `SEARCH_INDEX_VERSION` is not searchable in the new system.

### Existing Users

On first launch after this release, migrate the index with **build-then-swap**. Never delete the old index before the new one is built and self-checked:

1. Keep the old collection in place (read-only) as a rollback/safety artifact and show a clear "upgrading search index" state — never a silent empty result set, and never a hidden fallback to the old hybrid ranking path.
2. Preserve canonical user data untouched:
   - sources
   - items
   - raw media paths
   - transcript chunks/lines
   - OCR chunks
   - keyframes
   - video-understanding records
3. Build new retrieval units from canonical artifacts and write them into a **new** unified vector-index collection alongside the old one.
4. Re-embed units into the new collection. Note the real cost: for the cloud profile this is API + network spend proportional to (items x units); for local it is GPU time. This re-embed is the dominant migration cost — it is not a free metadata rebuild.
5. Self-check the new collection (SQLite unit count == vector point count; sample queries return) before flipping.
6. Atomically flip the active index pointer to the new collection.
7. Only then garbage-collect the old text/image collections.

**Failed/incomplete items are the common case here, not the exception.** In the current library a large fraction of items are `failed` and may have no transcript/OCR chunks to rebuild from; they cannot become units without a full reprocess (ASR/OCR), which is the slow local path. Mark them `blocked` with a reason and surface a count — do not silently treat them as searchable-but-empty. Realistically the upstream indexing-failure root causes (see `CERUL_INDEXING_SEARCH_MODEL_BACKLOG.md`) should land before or with this cutover, or the new index ships on top of a mostly-failed library and looks worse than the old one regardless of retrieval quality.

### New Users

New installs only create the unified retrieval-unit index. Old text/image vector collection creation should not run.

### Repair/Rebuild

Settings > Storage > repair search index should rebuild the unified retrieval-unit index only:

- Rebuild SQLite retrieval units from canonical artifacts.
- Rebuild the single vector-index collection.
- Do not resurrect old hybrid collections.
- Do not call this "vector database repair" in user-facing copy.

## Implementation Tasks

### 1. Storage Schema

Add a migration for:

- `retrieval_units`
- retrieval-unit FTS table if needed for fallback/diagnostics
- item-level `search_index_version`
- item-level `search_index_status`: `pending`, `indexed`, `failed`, `blocked`
- failure reason fields

The existing `chunks` table remains useful for transcript display, playback, keyframes, OCR, and rebuild inputs. It should no longer be the primary search result table.

### 2. Unit Builder

Create a deterministic builder in storage or pipeline code:

- Input: item record, transcript chunks/lines, OCR chunks, keyframes, video-understanding record.
- Output: retrieval units with stable IDs.
- Stable ID shape should include item id, index version, unit kind, and unit index.

The builder should be unit-tested with:

- transcript-only video
- transcript + OCR video
- visual-only video
- image-only item
- item with video-understanding chapters/events
- long video with many transcript lines
- empty or failed source

### 3. Embedding Writer

Replace the current text/image vector writing path with unified unit embedding:

- Batch embed retrieval-unit `content_text`.
- For textless image units, batch embed representative image paths.
- Write all vectors to one vector-index collection.
- Store `unit_id` payload.
- Mark item `search_index_status='indexed'` only after SQLite units and vector points are both committed.

Failure policy:

- If embedding fails, keep canonical artifacts but mark search index failed.
- Do not claim the item is semantically searchable.
- FTS fallback may still work only when explicitly reported as fallback.

### 4. Search Service

Rewrite `cerul-search` around retrieval units:

- Remove RRF from the main path.
- Remove the independent image-vector collection search and source masks from the main path.
- Vector-search one collection, then union a cheap lexical recall pass (FTS + CJK literal) into the candidate set — recall augmenter only, not a co-equal ranking source.
- Rank by vector score; strongly pin only high-confidence exact matches (quoted/long phrases, entities, product/model names, IDs, code-like strings); apply bounded lexical boost for short/common terms; score lexical-only candidates by their own unit vector.
- Hydrate `retrieval_units`.
- Resolve `start_sec` to the best sub-unit line for playback, not the window start.
- Attach item title/source/duration/thumbnail.
- Return representative frame and timestamp.

Keep result shape compatible with the desktop UI where possible, but source it from retrieval units instead of chunks.

### 5. UI Mapping

Update result mapping to treat results as retrieval units:

- Display unit snippet from `content_text`, preferring transcript text when present.
- Use representative frame for thumbnail.
- Use `start_sec` for playback jump.
- Use vector score as the result score.
- Remove wording that implies a fused rank score.

Overlay search should keep the 100ms debounce and benefit from the shorter unified search path.

### 6. Startup Cleanup

On API startup:

- Ensure the active unified collection exists.
- Detect old search index versions.
- Schedule rebuild jobs for items that need the new index.
- Do not serve old hybrid results.
- Garbage-collect old text/image vector collections only **after** the new unified collection passes its self-check and the active index pointer has flipped. Never delete-before-build.

Cleanup must be scoped to Cerul's namespace and active data directory.

### 7. Tests

Required tests:

- Unit builder output stability.
- Retrieval-unit embedding count matches vector-index point count.
- Search does not query old text/image collections.
- Search returns hydrated unit metadata in rank order.
- Startup marks old indexes stale and queues rebuild.
- Repair rebuilds unified collection only.
- Missing source does not silently preserve old searchability.
- Chinese literal fallback still works when query embedding is unavailable.

### 8. Smoke Validation

Add or update smoke scripts:

- Add a small video.
- Wait for unified index.
- Search by spoken phrase.
- Search by OCR phrase.
- Search by semantic topic.
- Search through overlay.
- Restart app and confirm unified index persists.
- Run repair and confirm collection is rebuilt.

### 9. Relevance Eval Harness

Build the labeled query set and eval script described in **Relevance Eval & Quality Gate**:

- `eval/search/queries.jsonl` — stratified, human-readable labels resolved to `item_id` at run time.
- A runner that hits the running Core `/search`, computes Recall@k / precision@k / MRR / moment accuracy, and diffs against a saved baseline.
- Capture a baseline from the current desktop build before the cutover, so the gate has something to compare against.
- Wire the per-type gate into the release checklist: no per-type regression vs baseline.

Success criteria:

- `/search/diagnostics` reports `retrieval_mode='unified_vector'` on normal searches.
- Vector-index collection name is unified, not text/image split.
- No RRF path is used.
- Results remain usable after restart.
- Overlay search feels faster than the old hybrid path.
- Relevance eval passes the quality gate: no per-type Recall@k or precision@k regression vs the current desktop baseline, and exact-entity / verbatim queries do not regress.
- During upgrade, an existing library never shows a silent empty search box. Until the swap completes, the product shows an explicit upgrading state; it does not silently serve old hybrid results.

## Files Likely To Change

- `crates/cerul-storage/src/chunks.rs`
- `crates/cerul-storage/src/vectors.rs`
- `crates/cerul-storage/migrations/*`
- `crates/cerul-pipeline/src/run.rs`
- `crates/cerul-search/src/lib.rs`
- `crates/cerul-api/src/lib.rs`
- `crates/cerul-api/src/jobs.rs`
- `crates/cerul-api/src/video_understanding.rs`
- `apps/desktop/src/lib/api.ts`
- `apps/desktop/src/lib/results.ts`
- `apps/desktop/src/App.tsx`
- `apps/desktop/src/OverlayApp.tsx`
- search smoke scripts under `scripts/`
- relevance eval set + runner under `eval/search/`

## Non-Goals

- No gradual migration that keeps the old hybrid search *code path* alive. (Build-then-swap of the *index data* is required and is a different thing — deployment safety, not a second retrieval mode.)
- No pure-vector-only main path that drops lexical recall for exact CJK/entity queries. Lexical recall stays as a candidate-union augmenter.
- No RRF tuning as the primary fix.
- No reranker-first strategy.
- No separate raw-image search branch in normal typed search.
- No claim that old items are searchable before their unified index is built.
- No user-facing implementation-specific vector index terminology.

## Open Risks

- Existing users with missing raw files may not be fully rebuildable. They should be marked blocked, not silently served stale search results.
- Local model failures will be more visible because semantic search depends on unified unit embeddings.
- Video-understanding is optional today. The initial unified index must work without it, using transcript/OCR/title first.
- Image-only items with no OCR/text may need representative image embeddings in the unified collection; this should stay a single collection, not a separate retrieval branch.
- **Cloud image embedding unverified.** The single-collection design assumes the active profile can embed images into the text space. Gemini Embedding 2 may not; confirm before committing (see Embedding).
- **Local embedding quality on long fused units is unvalidated.** The fused `content_text` is longer and more multilingual than a single transcript chunk; the old API's accuracy used cloud Gemini 3072. Gate the cutover on the relevance eval, not on "feels better."
- **Most of the current library is `failed`** and may not be rebuildable into units without full reprocess; the new index can launch mostly empty if upstream failures are not fixed first.
- **Exact-term recall regression** if lexical recall is ever reduced to a re-rank-only boost. It must add candidates, or names/quotes the vector misses become unsearchable.

## Final State

After this release, Cerul search should have one clean mental model:

```text
Cerul turns each media item into searchable moments.
Each moment has one embedding.
Search queries that moment index directly.
```

The old hybrid implementation should be treated as obsolete infrastructure and removed from the normal product path.
