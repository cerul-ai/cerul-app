# Cerul indexing state drift investigation - 2026-06-19

Branch: `codex/investigate-indexing-state-drift`

## Verified runtime state

- The visible app was served by `/Users/jessytsui/cerul-ai/cerul-app` on branch `codex/fix-pr72-shipit-rescue-order` at `b960022`. This worktree was branched from `c264cad` (`v0.0.21`).
- Local API health was OK: `GET /health` returned `{"status":"ok","version":"0.0.1"}`.
- The shared local DB was `~/Library/Application Support/Cerul/cerul.db`.
- `settings.indexing_paused` was `true`.
- DB summary:
  - `items`: 3 rows, all `status='discovered'`, all `indexed_at IS NULL`.
  - `jobs`: 9 rows: 5 `completed`, 1 `failed`, 3 `queued`.
  - `ignored_items`: 0 rows.
- Diagnostics showed:
  - `local_models.phase = "ready"`.
  - `runtime.local_runtime_ready = false`.
  - `runtime.local_runtime_error = "Install MLX runtime packages: pyclipper."`
  - `search.indexed_item_count = 0`.

## Item-level evidence

- `你心里有个名字`
  - Current item state: `discovered`, `indexed_at = NULL`.
  - Existing index artifacts: 7 keyframes, 7 transcript chunks, 58 transcript lines.
  - Metadata says `embedding_index_status='indexed'`, `visual_index_status='indexed'`, `transcript_index_status='indexed'`.
  - Job history: one completed job at 2026-06-18 19:01, one later failed job at 2026-06-18 20:19-20:57, and one current queued job.
  - Failure: `transcribe failed in MLX sidecar: RemoteProtocolError: peer closed connection without sending complete message body`.
- `moshi`
  - Current item state: `discovered`, `indexed_at = NULL`.
  - Existing index artifacts: 33 keyframes, 33 transcript chunks, 33 OCR chunks, 168 transcript lines.
  - Metadata says `embedding_index_status='indexed'`, `visual_index_status='indexed'`, `ocr_index_status='indexed'`.
  - Job history: several completed jobs and one current queued job.
- `YTDown_YouTube_Media_3DlXq9nsQOE_001_1080p`
  - Current item state: `discovered`, `indexed_at = NULL`.
  - Existing index artifacts: 0 chunks and 0 keyframes.
  - Current job state: queued, never started.

## Why the UI looks contradictory

1. Search/home counts are driven by item status, not by chunk/vector presence.
   The frontend maps an item to indexed only when `status === "indexed"` or `indexed_at !== null`.
   Because all three rows are currently `discovered` with `indexed_at = NULL`, the home screen reports 0 indexed media and locks search, even though two items still have chunks and vectors.

2. The task drawer calls `/jobs` without a status filter.
   The backend returns historical completed/failed jobs plus active queued jobs. That is why old work appears alongside the current three queued jobs.

3. Queued jobs are treated as active jobs in the frontend.
   `isActiveJob()` returns true for both `queued` and `running`, so a paused queue can look like active indexing. The backend worker exits early while `indexing_paused = true`, so these queued jobs are not actually being consumed.

4. Rebuild/reindex paths intentionally reset item readiness.
   `queue_items_for_embedding_mode_rebuild()` selects indexed/fetching/processing items, changes indexed rows back to `discovered`, clears `indexed_at`, and inserts queued jobs. `reindex_item()` does the same for a single item. This creates a window where old artifacts still exist but the item is product-visible as not indexed.

5. The missing screenshot for the third item is real backend state.
   Video thumbnails are written early, immediately after frame sampling through `replace_item_keyframes()`. The third item has no keyframe chunks, so its job has not reached frame sampling.

6. Local model readiness and local runtime readiness are different.
   Model files are ready, but the runtime check currently fails on missing `pyclipper`. That does not make the API unhealthy, but it prevents reliable local processing and explains why queued local indexing cannot make progress cleanly after resume.

## Likely root cause chain

The DB is in a mixed state after a rebuild/reindex attempt while indexing was paused or later paused again:

1. Two videos were indexed successfully and left chunks/vectors in storage.
2. A rebuild/reindex path reset their item rows from `indexed` to `discovered`, cleared `indexed_at`, and queued new jobs.
3. `indexing_paused = true` prevents the worker from consuming those queued jobs.
4. One rebuild attempt for `你心里有个名字` failed in the MLX sidecar during transcription, but another queued job still exists for the same item.
5. The third video has only a discovered item row and a queued job; it has never produced keyframes.
6. `ignored_items` is empty, so removed local items are not currently tombstoned in this DB. Active file sources can rediscover the same raw paths.

## Fix direction

- Split UI language/counts between `queued`, `running`, and `paused`; do not call queued jobs "in progress" when `indexing_paused = true`.
- Make the home "recent indexed" list filter to truly indexed items, or rename it when showing queued/discovered items.
- Consider deriving item readiness from completed index metadata/chunks during rebuild windows, or add a separate `rebuild_pending` status instead of clearing `indexed_at` immediately.
- Make `/jobs` default to recent active/failed jobs for the drawer, or group historical completed jobs separately.
- Surface local runtime dependency failures (`pyclipper`) as the blocking reason for local indexing, distinct from model-download readiness.
- Verify why `ignored_items` is empty after local removals; removed local file items should leave a tombstone keyed by source/external_id/raw_path when the source remains active.

## Implemented in this branch

- Rebuild/reindex no longer clears `indexed_at` or downgrades already indexed items before the replacement index succeeds.
- Worker restart/requeue and failed rebuild jobs preserve the previous indexed item state when an old index is still available.
- Startup now repairs drifted rows whose item metadata already says transcript/embedding/visual/OCR indexing succeeded.
- Startup repair prefers the item's last completed indexing job timestamp for `indexed_at`, instead of making repaired items look newly indexed at app launch.
- Frontend item mapping treats indexed artifact metadata, including OCR-only indexed metadata, as searchable, so drifted rows recover visually even before a DB repair run.
- Home "recent indexed" filters to indexed items only.
- Home, library strip, and jobs drawer distinguish paused queued work from active running work.
- The desktop task drawer now requests `/jobs?scope=drawer`; that scope keeps active queued/running jobs plus recent failed jobs, without flooding the drawer with completed history.
- Removing a local item now writes a raw-path tombstone even if the stored item lacks `external_id`, so active file sources are less likely to rediscover deleted local media.

## Remaining operational note

- This branch fixes the state drift and UI/task-list confusion. It does not install missing local runtime dependencies; if `runtime.local_runtime_error` still reports `pyclipper`, local-only indexing still needs the runtime repaired or indexing switched to a working remote/auto path.
