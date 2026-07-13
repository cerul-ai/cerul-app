use std::path::{Path as FsPath, PathBuf};

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use cerul_storage::AppPaths;
use rusqlite::{types::Value as SqlValue, Transaction};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    attach_item_usage, attach_raw_path_exists, chunk_from_row, chunk_path,
    clear_generated_display_title_with_tx, clear_item_unified_search_index_with_tx, clip_window,
    current_unix_seconds, enqueue_embedding_rebuild_job, format_playback_timestamp,
    image_content_type, item_from_row, item_raw_path_for_chunk, list_limit, not_found,
    parse_content_type, playback_position_from_metadata, safe_filename_part, split_filter_values,
    video_clip_cache_path, video_clip_filename, video_clip_source_for_chunk, video_file_response,
    video_understanding, ApiError, ApiResult, ApiState, ChunkRecord, ItemRecord,
    PlaybackPositionRecord,
};

pub(crate) fn router() -> Router<ApiState> {
    Router::new()
        .route("/items", get(list_items))
        .route(
            "/items/:id",
            get(get_item).patch(update_item).delete(remove_item),
        )
        .route(
            "/items/:id/playback",
            get(get_item_playback_position).patch(update_item_playback_position),
        )
        .route("/items/:id/reindex", post(reindex_item))
        .route("/items/:id/chunks", get(list_item_chunks))
        .route(
            "/items/:id/understanding",
            get(video_understanding::get_item_understanding)
                .post(video_understanding::analyze_item_understanding),
        )
        .route("/chunks/:id/frame", get(get_chunk_frame))
        .route("/chunks/:id/video-segment", get(get_chunk_video_segment))
        .route("/chunks/:id/video-clip", get(get_chunk_video_clip))
}

#[derive(Debug, Deserialize)]
pub(crate) struct VideoClipQuery {
    /// Symmetric padding (legacy / fallback). Used for both sides when
    /// before_sec/after_sec are absent.
    padding_sec: Option<f64>,
    /// Seconds to extend before the chunk start (overrides padding_sec).
    before_sec: Option<f64>,
    /// Seconds to extend after the chunk end (overrides padding_sec).
    after_sec: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct UpdatePlaybackPositionRequest {
    position_sec: f64,
    chunk_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListItemsQuery {
    limit: Option<usize>,
    /// Offset-style cursor. Kept as a string-free integer so invalid values get
    /// rejected by Axum before reaching SQLite.
    cursor: Option<usize>,
    status: Option<String>,
    source_id: Option<String>,
    light: Option<bool>,
    include_usage: Option<bool>,
}

async fn list_items(
    State(state): State<ApiState>,
    Query(query): Query<ListItemsQuery>,
) -> ApiResult<Json<Vec<ItemRecord>>> {
    let limit = list_limit(query.limit);
    let offset = query.cursor.unwrap_or(0);
    let light = query.light.unwrap_or(false);
    let include_usage = query.include_usage.unwrap_or(!light);
    let statuses = split_filter_values(query.status.as_deref());
    let metadata_expr = if light { "NULL" } else { "i.metadata" };
    let conn = cerul_storage::sqlite::open(&state.paths)?;
    let mut params: Vec<SqlValue> = Vec::new();
    let mut sql = format!(
        r#"
        SELECT i.id, i.source_id, i.content_type, i.external_id, i.title,
               COALESCE(i.duration_sec, (
                   SELECT MAX(c2.end_sec)
                   FROM chunks c2
                   WHERE c2.item_id = i.id
               )) AS duration_sec,
               i.raw_path, i.discovered_at, i.indexed_at, i.status, i.error, {metadata_expr} AS metadata,
               (
                   SELECT c.id
                   FROM chunks c
                   WHERE c.item_id = i.id
                     AND c.frame_path IS NOT NULL
                   ORDER BY COALESCE(c.start_sec, 0), c.id
                   LIMIT 1
               ) AS thumbnail_chunk_id
        FROM items i
        WHERE i.status != 'deleting'
        "#
    );
    if !statuses.is_empty() {
        sql.push_str(" AND i.status IN (");
        sql.push_str(
            &std::iter::repeat_n("?", statuses.len())
                .collect::<Vec<_>>()
                .join(", "),
        );
        sql.push(')');
        params.extend(statuses.into_iter().map(SqlValue::from));
    }
    if let Some(source_id) = query.source_id.filter(|value| !value.trim().is_empty()) {
        sql.push_str(" AND i.source_id = ?");
        params.push(SqlValue::from(source_id));
    }
    sql.push_str(
        r#"
        ORDER BY COALESCE(i.discovered_at, i.indexed_at, 0) DESC, i.id ASC
        LIMIT ? OFFSET ?
        "#,
    );
    params.push(SqlValue::from(limit as i64));
    params.push(SqlValue::from(offset as i64));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), item_from_row)?;
    let mut items = rows.collect::<Result<Vec<_>, _>>()?;
    if include_usage {
        attach_item_usage(&state.paths, &mut items);
    }

    Ok(Json(items))
}

#[derive(Debug, Deserialize)]
struct UpdateItemRequest {
    raw_path: Option<String>,
}

/// Currently supports relocating a media file that moved on disk: updates
/// raw_path (after verifying the file exists) and clears a stale
/// missing-file error so a subsequent re-index can run against it.
async fn update_item(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateItemRequest>,
) -> ApiResult<Json<ItemRecord>> {
    if let Some(raw_path) = req.raw_path.as_deref() {
        let trimmed = raw_path.trim();
        if trimmed.is_empty() {
            return Err(ApiError::bad_request("raw_path must not be empty"));
        }
        let path = FsPath::new(trimmed);
        if !path.is_file() {
            return Err(ApiError::bad_request(format!("file not found: {trimmed}")));
        }

        let (previous_raw_path, indexed_at, previous_error) =
            item_raw_path_patch_state(&state.paths, &id)?;
        let same_path = previous_raw_path
            .as_deref()
            .map(|previous| paths_refer_to_same_file(FsPath::new(previous), path))
            .unwrap_or(false);
        cerul_storage::set_item_raw_path(&state.paths, &id, path).map_err(|error| {
            if error.to_string().contains("item not found") {
                ApiError::not_found(format!("item not found: {id}"))
            } else {
                ApiError::internal(error)
            }
        })?;
        if previous_error
            .as_deref()
            .is_some_and(is_source_file_missing_error)
        {
            clear_stale_missing_file_error(&state.paths, &id, indexed_at.is_some())?;
        }
        tracing::info!(
            item_id = %id,
            raw_path = %trimmed,
            raw_path_exists = true,
            same_path,
            was_indexed = indexed_at.is_some(),
            "updated item raw path"
        );
    }
    get_item(State(state), Path(id)).await
}

fn item_raw_path_patch_state(
    paths: &AppPaths,
    item_id: &str,
) -> ApiResult<(Option<String>, Option<i64>, Option<String>)> {
    let conn = cerul_storage::sqlite::open(paths)?;
    conn.query_row(
        "SELECT raw_path, indexed_at, error FROM items WHERE id = ?1",
        [item_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .map_err(|error| match error {
        rusqlite::Error::QueryReturnedNoRows => {
            ApiError::not_found(format!("item not found: {item_id}"))
        }
        other => ApiError::internal(other.into()),
    })
}

fn clear_stale_missing_file_error(
    paths: &AppPaths,
    item_id: &str,
    restore_indexed_status: bool,
) -> ApiResult<()> {
    let conn = cerul_storage::sqlite::open(paths)?;
    let status = if restore_indexed_status {
        "indexed"
    } else {
        "failed"
    };
    conn.execute(
        "UPDATE items SET error = NULL, status = ?2 WHERE id = ?1",
        rusqlite::params![item_id, status],
    )?;
    Ok(())
}

fn is_source_file_missing_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("source file does not exist")
        || normalized.contains("source file missing")
        || normalized.contains("source path does not exist")
        || normalized.contains("input file does not exist")
        || normalized.starts_with("file not found:")
        || (normalized.contains("no such file or directory")
            && (normalized.contains("source") || normalized.contains("raw_path")))
}

fn paths_refer_to_same_file(left: &FsPath, right: &FsPath) -> bool {
    if left == right {
        return true;
    }
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

async fn get_item(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> ApiResult<Json<ItemRecord>> {
    let conn = cerul_storage::sqlite::open(&state.paths)?;
    let item = conn.query_row(
        r#"
        SELECT i.id, i.source_id, i.content_type, i.external_id, i.title,
               COALESCE(i.duration_sec, (
                   SELECT MAX(c2.end_sec)
                   FROM chunks c2
                   WHERE c2.item_id = i.id
               )) AS duration_sec,
               i.raw_path, i.discovered_at, i.indexed_at, i.status, i.error, i.metadata,
               (
                   SELECT c.id
                   FROM chunks c
                   WHERE c.item_id = i.id
                     AND c.frame_path IS NOT NULL
                   ORDER BY COALESCE(c.start_sec, 0), c.id
                   LIMIT 1
               ) AS thumbnail_chunk_id
        FROM items i
        WHERE i.id = ?1
        "#,
        [id.as_str()],
        item_from_row,
    )?;
    let mut item = item;
    attach_raw_path_exists(&mut item);
    attach_item_usage(&state.paths, std::slice::from_mut(&mut item));

    Ok(Json(item))
}

async fn get_item_playback_position(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> ApiResult<Json<PlaybackPositionRecord>> {
    let item = cerul_storage::get_item(&state.paths, &id)
        .map_err(|_| ApiError::not_found(format!("item not found: {id}")))?;
    Ok(Json(playback_position_from_metadata(
        &item.id,
        &item.metadata,
    )))
}

async fn update_item_playback_position(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(request): Json<UpdatePlaybackPositionRequest>,
) -> ApiResult<Json<PlaybackPositionRecord>> {
    if !request.position_sec.is_finite() || request.position_sec < 0.0 {
        return Err(ApiError::bad_request(
            "position_sec must be a finite non-negative number",
        ));
    }

    let updated_at = current_unix_seconds();
    let position_sec = request.position_sec;
    let chunk_id = request.chunk_id.filter(|value| !value.trim().is_empty());
    cerul_storage::update_item_metadata(&state.paths, &id, |metadata| {
        metadata.insert(
            "playback_position".to_string(),
            json!({
                "position_sec": position_sec,
                "timestamp": format_playback_timestamp(position_sec),
                "chunk_id": chunk_id,
                "updated_at": updated_at,
            }),
        );
    })
    .map_err(|error| {
        if error.to_string().contains("item not found") {
            ApiError::not_found(format!("item not found: {id}"))
        } else {
            ApiError::internal(error)
        }
    })?;

    Ok(Json(PlaybackPositionRecord {
        item_id: id,
        position_sec,
        timestamp: format_playback_timestamp(position_sec),
        chunk_id,
        updated_at: Some(updated_at),
    }))
}

#[derive(Debug, Deserialize)]
struct RemoveItemQuery {
    /// Skip the ignored-item tombstone so source discovery (or a manual re-add)
    /// can bring the item back later. Used by the library's "clear failed"
    /// cleanup, whose dialog promises the items can be re-imported — a normal
    /// delete still tombstones so a removed item isn't re-discovered.
    #[serde(default)]
    keep_discoverable: bool,
}

async fn remove_item(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Query(query): Query<RemoveItemQuery>,
) -> ApiResult<Json<Value>> {
    let item = cerul_storage::get_item(&state.paths, &id)
        .map_err(|_| ApiError::not_found(format!("item not found: {id}")))?;
    let has_running_jobs = item_has_running_jobs(&state.paths, &item.id)?;
    if !has_running_jobs {
        cleanup_item_artifacts(&state.paths, &item).await?;
    }

    let mut conn = cerul_storage::sqlite::open(&state.paths)?;
    let tx = conn.transaction()?;
    if !query.keep_discoverable {
        remember_removed_item(&tx, &item)?;
    }
    tx.execute(
        r#"
        UPDATE jobs
        SET status = 'cancelled',
            finished_at = strftime('%s','now'),
            error = NULL,
            progress = 1,
            stage = 'cancelled',
            stage_message = 'Cancelled'
        WHERE item_id = ?1
          AND status IN ('queued', 'running', 'failed')
        "#,
        [id.as_str()],
    )?;
    let removed = if has_running_jobs {
        tx.execute(
            r#"
            UPDATE items
            SET status = 'deleting',
                error = NULL
            WHERE id = ?1
            "#,
            [id.as_str()],
        )?
    } else {
        tx.execute("DELETE FROM items WHERE id = ?1", [id.as_str()])?
    };
    if removed != 1 {
        return Err(ApiError::not_found(format!("item not found: {id}")));
    }
    tx.commit()?;

    Ok(Json(json!({ "status": "removed", "id": id })))
}

pub(crate) fn item_has_running_jobs(paths: &AppPaths, item_id: &str) -> anyhow::Result<bool> {
    let conn = cerul_storage::sqlite::open(paths)?;
    let running: i64 = conn.query_row(
        r#"
        SELECT COUNT(*)
        FROM jobs
        WHERE item_id = ?1
          AND status = 'running'
        "#,
        [item_id],
        |row| row.get(0),
    )?;
    Ok(running > 0)
}

fn remember_removed_item(
    tx: &Transaction<'_>,
    item: &cerul_storage::StoredItem,
) -> anyhow::Result<()> {
    let raw_path = item.raw_path.as_deref().or_else(|| {
        item.metadata
            .get("raw_path")
            .and_then(serde_json::Value::as_str)
    });
    let Some(external_id) = item
        .external_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or(raw_path)
    else {
        return Ok(());
    };
    tx.execute(
        r#"
        INSERT INTO ignored_items (source_id, external_id, raw_path, reason, ignored_at)
        VALUES (?1, ?2, ?3, 'removed_from_library', strftime('%s','now'))
        ON CONFLICT(source_id, external_id) DO UPDATE SET
            ignored_at = excluded.ignored_at,
            raw_path = COALESCE(excluded.raw_path, ignored_items.raw_path),
            reason = excluded.reason
        "#,
        (item.source_id.as_str(), external_id, raw_path),
    )?;
    Ok(())
}

pub(crate) async fn cleanup_item_artifacts(
    paths: &AppPaths,
    item: &cerul_storage::StoredItem,
) -> anyhow::Result<()> {
    if let Err(error) = cerul_storage::vectors::delete_item_embeddings(paths, &item.id).await {
        tracing::warn!(
            item_id = %item.id,
            %error,
            "failed to delete item embeddings; continuing item cleanup"
        );
    }
    for cache_key in item_pipeline_cache_keys(item) {
        remove_file_if_exists(
            paths
                .cache
                .join("pipeline")
                .join("audio")
                .join(format!("{cache_key}.wav")),
        )
        .await?;
        remove_dir_if_exists(paths.cache.join("pipeline").join("frames").join(cache_key)).await?;
    }
    remove_clip_cache_for_item(paths, &item.id).await?;
    // Never remove raw_path here. "Remove from library" means delete Cerul's
    // index and processed derivatives only; source media needs a separate,
    // explicit cache-cleaning action.
    Ok(())
}

pub(crate) fn item_pipeline_cache_keys(item: &cerul_storage::StoredItem) -> Vec<String> {
    let legacy = cerul_pipeline::run::cache_key_for_discovery_id(item.discovery_id());
    let scoped = cerul_pipeline::run::cache_key_for_item(&item.id, item.discovery_id());
    if legacy == scoped {
        vec![legacy]
    } else {
        vec![legacy, scoped]
    }
}

async fn remove_file_if_exists(path: PathBuf) -> anyhow::Result<()> {
    match tokio::fs::remove_file(&path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

async fn remove_dir_if_exists(path: PathBuf) -> anyhow::Result<()> {
    match tokio::fs::remove_dir_all(&path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

async fn remove_clip_cache_for_item(paths: &AppPaths, item_id: &str) -> anyhow::Result<()> {
    let clips_dir = paths.cache.join("clips");
    let mut entries = match tokio::fs::read_dir(&clips_dir).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let item_prefix = format!("{}-", safe_filename_part(item_id));

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with(&item_prefix) {
            remove_file_if_exists(path).await?;
        }
    }
    Ok(())
}

async fn reindex_item(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let mut conn = cerul_storage::sqlite::open(&state.paths)?;
    let tx = conn.transaction()?;
    let content_type: String = tx.query_row(
        "SELECT content_type FROM items WHERE id = ?1",
        [id.as_str()],
        |row| row.get(0),
    )?;
    let content_type = parse_content_type(&content_type)?;
    tx.execute(
        r#"
        UPDATE items
        SET status = CASE
                WHEN indexed_at IS NOT NULL OR status = 'indexed' THEN 'indexed'
                ELSE 'discovered'
            END,
            indexed_at = CASE
                WHEN indexed_at IS NOT NULL OR status = 'indexed' THEN indexed_at
                ELSE NULL
            END,
            error = NULL
        WHERE id = ?1
        "#,
        [id.as_str()],
    )?;
    tx.execute(
        "DELETE FROM item_understandings WHERE item_id = ?1",
        [id.as_str()],
    )?;
    tx.execute(
        "DELETE FROM chunks WHERE item_id = ?1 AND chunk_type = 'understanding'",
        [id.as_str()],
    )?;
    clear_generated_display_title_with_tx(&tx, &id)?;
    clear_item_unified_search_index_with_tx(&tx, &id)?;
    let queued_job = enqueue_embedding_rebuild_job(&tx, &id, content_type, true)?;
    tx.commit()?;

    Ok(Json(json!({
        "status": if queued_job { "queued" } else { "already_queued" },
        "id": id,
        "queued_job": queued_job
    })))
}

async fn list_item_chunks(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<ChunkRecord>>> {
    let conn = cerul_storage::sqlite::open(&state.paths)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT id, item_id, chunk_type, start_sec, end_sec, text, frame_path, metadata
        FROM chunks
        WHERE item_id = ?1
        ORDER BY COALESCE(start_sec, 0), id ASC
        "#,
    )?;
    let rows = stmt.query_map([id.as_str()], chunk_from_row)?;

    Ok(Json(rows.collect::<Result<Vec<_>, _>>()?))
}

pub(crate) async fn get_chunk_frame(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let Some(path) = chunk_path(&state.paths, &id, "frame_path")? else {
        return Ok(not_found("frame not found"));
    };
    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(not_found("frame not found"));
        }
        Err(error) => return Err(error.into()),
    };
    let content_type = image_content_type(&path);
    let mut response = Body::from(bytes).into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=3600"),
    );
    Ok(response)
}

pub(crate) async fn get_chunk_video_segment(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let Some(path) = item_raw_path_for_chunk(&state.paths, &id)? else {
        return Ok(not_found("video segment not found"));
    };
    video_file_response(&path, headers.get(header::RANGE)).await
}

pub(crate) async fn get_chunk_video_clip(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Query(query): Query<VideoClipQuery>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let Some(source) = video_clip_source_for_chunk(&state.paths, &id)? else {
        return Ok(not_found("video clip not found"));
    };
    let fallback = query.padding_sec.unwrap_or(2.0);
    let (start_sec, duration_sec) = clip_window(
        source.start_sec,
        source.end_sec,
        query.before_sec.unwrap_or(fallback),
        query.after_sec.unwrap_or(fallback),
    );
    let clip_path = video_clip_cache_path(&state.paths, &id, start_sec, duration_sec);

    cerul_pipeline::ffmpeg::export_clip(
        std::path::Path::new(&source.raw_path),
        &clip_path,
        start_sec,
        duration_sec,
    )
    .await?;

    let clip_path_string = clip_path.to_string_lossy().to_string();
    let mut response = video_file_response(&clip_path_string, headers.get(header::RANGE)).await?;
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"{}\"",
            video_clip_filename(source.title.as_deref(), &id, start_sec)
        ))
        .map_err(|error| ApiError::internal(anyhow::anyhow!(error)))?,
    );
    Ok(response)
}
