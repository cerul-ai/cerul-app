# Local Data Reset and Model Cache Cleanup Plan

## Background

User report:

- The user cleared local cache/data.
- After restart, Cerul started downloading models again.
- Opening the jobs/task list still showed previously failed indexing jobs.

Current code explains both behaviors:

- `Clear cache` only removes `data/cache`.
- `Reset local data` removes the entire app data directory, including:
  - `cerul.db`
  - `indexes/qdrant`
  - `cache`
  - `models`
  - local provider keys and app settings
- The jobs drawer reads from SQLite `jobs` via `/internal/jobs?scope=drawer`.
- Drawer scope intentionally shows queued/running jobs, completed jobs from the last 24 hours, and failed jobs from the last 7 days.

So if the user clicked `Clear cache`, old failed jobs are expected to remain because the DB is untouched. If the user clicked `Reset local data`, models are expected to be deleted because `models/` is inside `data_dir`.

This is technically consistent with current code, but it does not match the user expectation: clearing local library/cache data should not force a large model re-download.

## Problem Statement

The current storage actions mix three different user intents:

1. Free temporary disk space.
2. Clear local library/indexing data and start fresh.
3. Delete downloaded model weights.

These need separate controls. Models are expensive to download and should not be deleted as a side effect of clearing local videos, indexes, or failed jobs.

There is also a possible source of confusion after reset: if a different Cerul Core process is already healthy, Electron can attach to it without owning it. In that case reset/restart may not affect the Core/data directory the UI is currently reading from.

## Desired Behavior

### Clear Cache

Purpose: free temporary/download cache space.

Should delete:

- `data/cache`
- Source download cache under the default cache location.

Should not delete:

- `cerul.db`
- `jobs`
- `items`
- `indexes/qdrant`
- `models`
- provider keys
- app settings

UI copy must make clear that this does not reset the library or jobs.

### Clear Local Library Data

Purpose: reset the local library and indexing state while keeping model weights.

Should delete or reset:

- SQLite library data tables:
  - `sources`
  - `items`
  - `jobs`
  - `chunks`
  - `moments`
  - retrieval/search units
  - usage events tied to local processing
- Search index directory: `indexes/qdrant`
- Downloaded/imported media cache and processing intermediates: `cache`
- Endpoint metadata if it points to stale Core state.

Should preserve:

- `models`
- bundled runtimes if they are expensive to rebuild and not logically library data
- user-level app preferences where possible
- provider keys unless the action explicitly says it resets credentials

After restart:

- The jobs drawer should be empty.
- The library should be empty.
- Local model capability should still report previously downloaded models as installed.
- Indexing new media should not require re-downloading already installed models.

### Delete Local Models

Purpose: intentionally free model-weight disk space.

Should use the existing local model deletion path:

- `POST /models/local/delete`

Should delete:

- user-downloadable model cache roots under `models/`, such as Hugging Face hub, Cerul mirror, and ModelScope copies for selected groups.

Should not delete:

- OCR bundled weights
- library DB
- jobs
- sources/items
- search index
- downloaded media

This already mostly exists in `crates/cerul-api/src/local_models.rs`; the settings storage reset path should stop duplicating this behavior by deleting the whole `models/` directory.

## Proposed Implementation

### 1. Replace the current single "Reset local data" behavior

Current Electron behavior:

- `reset_local_data` schedules `rm -rf` for the whole `data_dir`.
- Since `models` is inside `data_dir`, model weights are deleted.

Change it to a narrower reset. The implementation should clear library tables
through Core first, then restart and remove filesystem artifacts while Core is
down.

Instead of deleting:

```text
data_dir
```

clear these SQLite tables:

```text
sources
items
jobs
chunks
moments
retrieval_units
item_understandings
ignored_items
inference_usage_events tied to items/jobs
```

then delete:

```text
data_dir/indexes
data_dir/cache
data_dir/logs/pipeline-jobs.jsonl (optional)
data_dir/endpoint.json
<media_dir>/sources when downloads were redirected outside data_dir
```

Preserve:

```text
data_dir/models
data_dir/runtimes
data_dir/cerul.db
provider rows
settings rows
embedding profiles
```

Open question:

- Provider keys currently live under app data as `provider-keys.json`. For a "clear local library data" action, preserve them. For a "factory reset" action, delete them.

### 2. Add a separate "Factory Reset" only if needed

If the product still needs a destructive reset, add a second danger-zone action:

- Label: `Reset everything`
- Copy: explicitly says it deletes library, settings, provider keys, model weights, cache, and local runtime state.
- Implementation can keep the current whole-directory deletion.

This avoids surprising normal users who just want to remove local videos and index state.

### 3. Make model deletion a model-page action

The latest code already has:

- `api.deleteLocalModels(modelIds?)`
- `/models/local/delete`

Keep model deletion in Settings -> Models, not in Storage -> Clear local data.

Optional improvement:

- Add a storage-row shortcut: `Delete local models...`
- This should navigate to the Models settings section or open a focused confirm dialog using the existing delete endpoint.

### 4. Clear stale jobs deterministically

Use a DB-preserving backend endpoint so the action can reset library state
without deleting provider/settings rows or forcing model re-downloads. The
endpoint should run a single SQLite transaction:

```sql
DELETE FROM jobs;
DELETE FROM chunks;
DELETE FROM moments;
DELETE FROM retrieval_units;
DELETE FROM items;
DELETE FROM sources;
```

Then reset related indexing/search metadata by deleting filesystem indexes while
Core is down during the restart sequence. This avoids partial state while still
preserving app-level settings, provider configuration, and model files.

### 5. Avoid attaching to stale external Core during reset

Current startup behavior can attach to an already-healthy Core and set `ownsApiProcess = false`.

For reset flows:

- Read the active internal API health/diagnostics before scheduling deletion.
- Verify the Core data dir matches Electron `appPaths().data_dir`.
- If it does not match, block reset and show an error with both paths.
- If Electron does not own the Core, still call an internal graceful shutdown endpoint if available, or refuse reset with a clear message.

Minimum acceptable fix:

- Add a diagnostic check before reset:
  - `storage_locations.data_dir`
  - Core `/internal/diagnostics` or `/internal/storage/usage` data dir
- If they mismatch, do not delete anything.

This prevents "I reset but old jobs remain" when the UI is connected to another Core/data directory.

## UI Copy Changes

Suggested Chinese labels:

- `清除缓存`
  - Description: `删除临时下载和处理中间文件。不会清空资料库、任务列表、索引或本地模型。`

- `清空本机资料库`
  - Description: `删除本机资料库、任务列表、搜索索引和已下载媒体，但保留已下载的本地模型和账号/Provider 设置。Cerul 会重启。`
  - Confirm: `清空资料库并重启`

- `删除本地模型`
  - Description: `删除已下载的本地模型权重。下次使用本地处理时需要重新下载。`

- Optional `恢复出厂状态`
  - Description: `删除资料库、索引、缓存、本地模型、登录状态、Provider 密钥和所有本机设置。`

## Tests

### Electron unit/smoke

Add tests around reset target calculation:

- `clearCache` only removes `cache_dir`.
- `resetLocalLibraryDataTargets` includes index/cache/endpoint/job log targets.
- `resetLocalLibraryDataTargets` does not include `models_dir`.
- `factoryResetTargets` includes `data_dir`.

### API/storage tests

If adding a backend cleanup endpoint:

- Seed sources/items/jobs/chunks/retrieval units.
- Run cleanup.
- Assert those tables are empty.
- Assert settings/provider keys are preserved if that is the selected behavior.
- Assert model cache files are untouched.

### Installed-app smoke

Scenario:

1. Download local models.
2. Index a video and force one failed job.
3. Run `Clear Local Library Data`.
4. Restart app.
5. Assert:
   - jobs drawer is empty
   - library is empty
   - `models/local/capability` still reports downloaded groups installed
   - indexing a new video does not start a model download unless a model was genuinely missing

### Stale Core guard

Scenario:

1. Start a Core with `CERUL_DATA_DIR=/tmp/cerul-a`.
2. Launch Electron with default `~/Library/Application Support/Cerul`.
3. Attempt reset.
4. Assert reset is refused with a path mismatch message.

## Recommended Patch Order

1. Add target helpers in Electron:
   - `localLibraryResetTargets()`
   - `factoryResetTargets()`
2. Change current `reset_local_data` to preserve `models/`.
3. Update storage settings copy and confirm text.
4. Add stale-Core/data-dir mismatch guard.
5. Add tests/smokes.
6. Optionally add a separate factory reset action if still needed.

## Decision

The default destructive action in Settings -> Storage should become "Clear Local Library Data" and preserve model weights.

Model deletion should be explicit and separate. This matches user expectation, avoids expensive re-downloads, and makes the jobs-list behavior understandable: clearing cache alone does not clear jobs; clearing local library data does.
