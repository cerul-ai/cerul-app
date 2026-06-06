use std::{
    collections::BTreeMap,
    net::SocketAddr,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    body::Body,
    extract::{ConnectInfo, Path, Query, Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
    Json, Router,
};
use cerul_models::{ContentType, DiscoveredItem, HealthResponse};
use cerul_storage::AppPaths;
use rusqlite::{OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

mod api_models;
pub mod jobs;
pub mod models;
pub mod providers;
pub mod video_understanding;

#[derive(Debug, Clone)]
pub struct ApiState {
    paths: AppPaths,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SourceRecord {
    pub id: String,
    #[serde(rename = "type")]
    pub source_type: String,
    pub config: Value,
    pub status: String,
    pub last_poll_at: Option<i64>,
    pub created_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct AddSourceRequest {
    #[serde(rename = "type")]
    pub source_type: String,
    pub config: Value,
}

#[derive(Debug, Deserialize)]
pub struct RssPreviewRequest {
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RssPreviewResponse {
    pub feed_url: String,
    pub title: String,
    pub image_url: Option<String>,
    pub episode_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AddSourceSummary {
    pub source: SourceRecord,
    pub items: Vec<AddedSourceItem>,
    pub queued_jobs: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AddedSourceItem {
    pub id: String,
    pub external_id: Option<String>,
    pub title: Option<String>,
    pub status: String,
    pub queued_job: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ItemRecord {
    pub id: String,
    pub source_id: String,
    pub content_type: String,
    pub external_id: Option<String>,
    pub title: Option<String>,
    pub duration_sec: Option<f64>,
    pub raw_path: Option<String>,
    pub indexed_at: Option<i64>,
    pub status: String,
    pub error: Option<String>,
    pub metadata: Value,
    pub thumbnail_chunk_id: Option<String>,
    pub usage: cerul_storage::UsageTotals,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChunkRecord {
    pub id: String,
    pub item_id: String,
    pub chunk_type: String,
    pub start_sec: Option<f64>,
    pub end_sec: Option<f64>,
    pub text: Option<String>,
    pub frame_path: Option<String>,
    pub metadata: Value,
}

#[derive(Debug, Deserialize)]
struct VideoClipQuery {
    padding_sec: Option<f64>,
}

#[derive(Debug)]
struct VideoClipSource {
    raw_path: String,
    title: Option<String>,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: String,
    pub item_id: Option<String>,
    pub job_type: String,
    pub status: String,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub error: Option<String>,
    pub progress: f64,
    pub stage: Option<String>,
    pub stage_message: Option<String>,
    pub usage: cerul_storage::UsageTotals,
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    error: anyhow::Error,
}

impl ApiError {
    pub(crate) fn internal(error: anyhow::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error,
        }
    }

    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            error: anyhow::anyhow!(message.into()),
        }
    }

    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            error: anyhow::anyhow!(message.into()),
        }
    }
}

type ApiResult<T> = Result<T, ApiError>;

pub fn crate_ready() -> bool {
    true
}

pub fn router() -> Router {
    let paths = AppPaths::resolve().expect("failed to resolve Cerul app paths");
    router_with_paths(paths)
}

pub fn router_with_paths(paths: AppPaths) -> Router {
    if let Err(error) = providers::bootstrap_env_providers(&paths) {
        tracing::warn!(%error, "failed to bootstrap env providers");
    }
    if let Err(error) = sync_indexing_schema_side_effects(&paths) {
        tracing::warn!(%error, "failed to sync indexing schema side effects");
    }
    let state = ApiState { paths };

    Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics))
        .route("/openapi.json", get(openapi_json))
        .route("/search", post(search))
        .route("/sources", get(list_sources).post(add_source))
        .route("/sources/preview/rss", post(preview_rss_source))
        .route("/sources/:id", delete(remove_source))
        .route("/sources/:id/pause", post(pause_source))
        .route("/sources/:id/resume", post(resume_source))
        .route("/items", get(list_items))
        .route("/items/:id", get(get_item).delete(remove_item))
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
        .route("/jobs", get(list_jobs))
        .route("/usage/events", get(list_usage_events))
        .route("/usage/summary", get(usage_summary))
        .route("/models/catalog", get(models::model_catalog))
        .route("/models/whisper", get(models::list_whisper_models))
        .route(
            "/models/whisper/:id/download",
            post(models::download_whisper_model),
        )
        .route(
            "/models/whisper/auto-download-status",
            get(models::get_auto_download_status),
        )
        .route("/models/embed/status", get(models::get_embedding_status))
        .route(
            "/models/embed/prepare",
            post(models::prepare_embedding_models),
        )
        .route(
            "/providers",
            get(providers::list_providers).post(providers::create_provider),
        )
        .route(
            "/providers/:id",
            patch(providers::update_provider).delete(providers::delete_provider),
        )
        .route("/providers/:id/test", post(providers::test_provider))
        .route("/settings", get(list_settings).patch(update_settings))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_remote_auth,
        ))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub async fn serve() -> anyhow::Result<()> {
    let paths = AppPaths::resolve()?;
    let addr = configured_addr(&paths)?;
    serve_with_paths(paths, addr).await
}

pub async fn serve_with_paths(paths: AppPaths, addr: SocketAddr) -> anyhow::Result<()> {
    if let Err(error) = providers::bootstrap_env_providers(&paths) {
        tracing::warn!(%error, "failed to bootstrap env providers");
    }
    if let Err(error) = jobs::requeue_interrupted_jobs(&paths) {
        tracing::warn!(%error, "failed to requeue interrupted Cerul jobs");
    }
    let _worker = jobs::spawn_default_job_worker(paths.clone());
    let _qdrant_shutdown = QdrantShutdownGuard;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        router_with_paths(paths).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

struct QdrantShutdownGuard;

impl Drop for QdrantShutdownGuard {
    fn drop(&mut self) {
        api_models::shutdown_local_query_sidecar();
        cerul_storage::vectors::shutdown_qdrant_sidecar();
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::warn!(%error, "failed to listen for ctrl-c shutdown signal");
        }
    };

    #[cfg(unix)]
    {
        let terminate = async {
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(mut signal) => {
                    signal.recv().await;
                }
                Err(error) => {
                    tracing::warn!(%error, "failed to listen for terminate shutdown signal");
                    std::future::pending::<()>().await;
                }
            }
        };
        tokio::select! {
            _ = ctrl_c => {},
            _ = terminate => {},
        }
    }

    #[cfg(not(unix))]
    {
        ctrl_c.await;
    }
}

pub fn default_addr() -> SocketAddr {
    "127.0.0.1:7777"
        .parse()
        .expect("default Cerul API address is valid")
}

pub fn configured_addr(paths: &AppPaths) -> anyhow::Result<SocketAddr> {
    let host = match setting_string(paths, "api_binding")?.as_deref() {
        Some("0") | Some("0.0.0.0") => "0.0.0.0",
        _ => "127.0.0.1",
    };

    Ok(format!("{host}:7777").parse()?)
}

async fn require_remote_auth(State(state): State<ApiState>, req: Request, next: Next) -> Response {
    let remote_addr = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0);
    if remote_addr
        .map(|addr| addr.ip().is_loopback())
        .unwrap_or(true)
    {
        return next.run(req).await;
    }

    let Ok(Some(required_key)) = setting_string(&state.paths, "remote_api_key") else {
        return unauthorized_remote_api();
    };
    if required_key.trim().is_empty() {
        return unauthorized_remote_api();
    }

    if bearer_token(req.headers()).is_some_and(|token| token == required_key.as_str()) {
        return next.run(req).await;
    }

    unauthorized_remote_api()
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .filter(|token| !token.trim().is_empty())
}

fn unauthorized_remote_api() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"))],
        Json(json!({
            "error": "remote API key required"
        })),
    )
        .into_response()
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

async fn metrics() -> Response {
    (
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        "cerul_up 1\n",
    )
        .into_response()
}

async fn openapi_json() -> Json<Value> {
    let mut paths = serde_json::Map::new();
    for (path, methods) in API_PATHS {
        let mut method_map = serde_json::Map::new();
        for method in *methods {
            method_map.insert(
                method.to_ascii_lowercase(),
                json!({
                    "responses": {
                        "200": { "description": "OK" }
                    }
                }),
            );
        }
        paths.insert((*path).to_string(), Value::Object(method_map));
    }

    Json(json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Cerul API",
            "version": env!("CARGO_PKG_VERSION")
        },
        "paths": paths
    }))
}

async fn search(
    State(state): State<ApiState>,
    Json(req): Json<cerul_search::SearchRequest>,
) -> ApiResult<Json<Vec<cerul_search::SearchResult>>> {
    let query = req.q.clone();
    let paths = state.paths.clone();
    let query_embedding =
        tokio::task::spawn_blocking(move || api_models::embed_query(&paths, &query)).await;

    match query_embedding {
        Ok(Ok(embedding)) => Ok(Json(
            cerul_search::search_with_vector_for_profile(
                &state.paths,
                req,
                embedding.vector,
                &embedding.profile,
            )
            .await?,
        )),
        Ok(Err(error)) => {
            tracing::warn!(%error, "API semantic query embedding unavailable; falling back to FTS");
            Ok(Json(
                cerul_search::search_fts_only(&state.paths, req).await?,
            ))
        }
        Err(error) => {
            tracing::warn!(%error, "API query embedding task failed; falling back to FTS");
            Ok(Json(
                cerul_search::search_fts_only(&state.paths, req).await?,
            ))
        }
    }
}

async fn list_sources(State(state): State<ApiState>) -> ApiResult<Json<Vec<SourceRecord>>> {
    let conn = cerul_storage::sqlite::open(&state.paths)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT id, type, config, status, last_poll_at, created_at
        FROM sources
        ORDER BY created_at DESC, id ASC
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        let config: String = row.get(2)?;
        Ok(SourceRecord {
            id: row.get(0)?,
            source_type: row.get(1)?,
            config: parse_json(&config),
            status: row.get(3)?,
            last_poll_at: row.get(4)?,
            created_at: row.get(5)?,
        })
    })?;

    Ok(Json(rows.collect::<Result<Vec<_>, _>>()?))
}

async fn add_source(
    State(state): State<ApiState>,
    Json(req): Json<AddSourceRequest>,
) -> ApiResult<Json<SourceRecord>> {
    let summary = add_source_to_paths(&state.paths, req).await?;
    Ok(Json(summary.source))
}

async fn preview_rss_source(
    Json(req): Json<RssPreviewRequest>,
) -> ApiResult<Json<RssPreviewResponse>> {
    let preview = cerul_sources::rss_podcast::preview_feed(&req.url).await?;

    Ok(Json(RssPreviewResponse {
        feed_url: preview.feed_url,
        title: preview.title,
        image_url: preview.image_url,
        episode_count: preview.episode_count,
    }))
}

pub async fn add_source_to_paths(
    paths: &AppPaths,
    req: AddSourceRequest,
) -> anyhow::Result<AddSourceSummary> {
    let id = new_id("source");
    let plugin = cerul_sources::build(&req.source_type, req.config.clone())?;
    let content_type = primary_content_type(&*plugin)?;
    let discovered_items = plugin.discover().await?;
    let config = req.config.to_string();
    let mut conn = cerul_storage::sqlite::open(paths)?;
    let tx = conn.transaction()?;
    let mut items = Vec::with_capacity(discovered_items.len());
    let mut queued_jobs = 0;

    tx.execute(
        "INSERT INTO sources (id, type, config, status) VALUES (?1, ?2, ?3, 'active')",
        (&id, &req.source_type, &config),
    )?;

    for item in &discovered_items {
        let item_id = upsert_discovered_item(&tx, &id, content_type, item)?;
        let queued_job = enqueue_index_job(&tx, &item_id, content_type)?;
        if queued_job {
            queued_jobs += 1;
        }
        items.push(AddedSourceItem {
            id: item_id,
            external_id: Some(item.external_id.clone()),
            title: item.title.clone(),
            status: "discovered".to_string(),
            queued_job,
        });
    }

    tx.execute(
        "UPDATE sources SET last_poll_at = strftime('%s','now') WHERE id = ?1",
        [id.as_str()],
    )?;
    tx.commit()?;

    Ok(AddSourceSummary {
        source: source_by_id(paths, &id)?,
        items,
        queued_jobs,
    })
}

async fn remove_source(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let item_ids = cerul_storage::item_ids_for_source(&state.paths, &id)?;
    for item_id in item_ids {
        let item = cerul_storage::get_item(&state.paths, &item_id)?;
        cleanup_item_artifacts(&state.paths, &item).await?;
    }
    let conn = cerul_storage::sqlite::open(&state.paths)?;
    conn.execute("DELETE FROM sources WHERE id = ?1", [id.as_str()])?;
    Ok(Json(json!({ "status": "removed", "id": id })))
}

async fn pause_source(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    set_source_status(&state.paths, &id, "paused")?;
    Ok(Json(json!({ "status": "paused", "id": id })))
}

async fn resume_source(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    set_source_status(&state.paths, &id, "active")?;
    Ok(Json(json!({ "status": "active", "id": id })))
}

async fn list_items(State(state): State<ApiState>) -> ApiResult<Json<Vec<ItemRecord>>> {
    let conn = cerul_storage::sqlite::open(&state.paths)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT i.id, i.source_id, i.content_type, i.external_id, i.title,
               COALESCE(i.duration_sec, (
                   SELECT MAX(c2.end_sec)
                   FROM chunks c2
                   WHERE c2.item_id = i.id
               )) AS duration_sec,
               i.raw_path, i.indexed_at, i.status, i.error, i.metadata,
               (
                   SELECT c.id
                   FROM chunks c
                   WHERE c.item_id = i.id
                     AND c.frame_path IS NOT NULL
                   ORDER BY COALESCE(c.start_sec, 0), c.id
                   LIMIT 1
               ) AS thumbnail_chunk_id
        FROM items i
        ORDER BY i.indexed_at DESC, i.id ASC
        "#,
    )?;
    let rows = stmt.query_map([], item_from_row)?;
    let mut items = rows.collect::<Result<Vec<_>, _>>()?;
    attach_item_usage(&state.paths, &mut items);

    Ok(Json(items))
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
               i.raw_path, i.indexed_at, i.status, i.error, i.metadata,
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
    attach_item_usage(&state.paths, std::slice::from_mut(&mut item));

    Ok(Json(item))
}

async fn remove_item(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let item = cerul_storage::get_item(&state.paths, &id)
        .map_err(|_| ApiError::not_found(format!("item not found: {id}")))?;
    cleanup_item_artifacts(&state.paths, &item).await?;
    let conn = cerul_storage::sqlite::open(&state.paths)?;
    let removed = conn.execute("DELETE FROM items WHERE id = ?1", [id.as_str()])?;
    if removed != 1 {
        return Err(ApiError::not_found(format!("item not found: {id}")));
    }

    Ok(Json(json!({ "status": "removed", "id": id })))
}

async fn cleanup_item_artifacts(
    paths: &AppPaths,
    item: &cerul_storage::StoredItem,
) -> anyhow::Result<()> {
    cerul_storage::vectors::delete_item_embeddings(paths, &item.id).await?;
    let cache_key = cerul_pipeline::run::cache_key_for_discovery_id(item.discovery_id());
    remove_file_if_exists(
        paths
            .cache
            .join("pipeline")
            .join("audio")
            .join(format!("{cache_key}.wav")),
    )
    .await?;
    remove_dir_if_exists(paths.cache.join("pipeline").join("frames").join(cache_key)).await?;
    remove_clip_cache_for_item(paths, &item.id).await?;
    Ok(())
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
        SET status = 'discovered',
            indexed_at = NULL,
            error = NULL
        WHERE id = ?1
        "#,
        [id.as_str()],
    )?;
    tx.execute(
        "DELETE FROM item_understandings WHERE item_id = ?1",
        [id.as_str()],
    )?;
    let queued_job = enqueue_index_job(&tx, &id, content_type)?;
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

async fn get_chunk_frame(
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

async fn get_chunk_video_segment(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let Some(path) = item_raw_path_for_chunk(&state.paths, &id)? else {
        return Ok(not_found("video segment not found"));
    };
    video_file_response(&path, headers.get(header::RANGE)).await
}

async fn get_chunk_video_clip(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Query(query): Query<VideoClipQuery>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let Some(source) = video_clip_source_for_chunk(&state.paths, &id)? else {
        return Ok(not_found("video clip not found"));
    };
    let (start_sec, duration_sec) = clip_window(
        source.start_sec,
        source.end_sec,
        query.padding_sec.unwrap_or(2.0),
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

async fn list_jobs(State(state): State<ApiState>) -> ApiResult<Json<Vec<JobRecord>>> {
    let conn = cerul_storage::sqlite::open(&state.paths)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT id, item_id, job_type, status, started_at, finished_at, error, progress, stage, stage_message
        FROM jobs
        ORDER BY COALESCE(started_at, 0) DESC, id ASC
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        let job_id: String = row.get(0)?;
        Ok(JobRecord {
            id: job_id.clone(),
            item_id: row.get(1)?,
            job_type: row.get(2)?,
            status: row.get(3)?,
            started_at: row.get(4)?,
            finished_at: row.get(5)?,
            error: row.get(6)?,
            progress: row.get(7)?,
            stage: row.get(8)?,
            stage_message: row.get(9)?,
            usage: cerul_storage::usage_totals_for_job(&state.paths, &job_id).unwrap_or_default(),
        })
    })?;

    Ok(Json(rows.collect::<Result<Vec<_>, _>>()?))
}

async fn usage_summary(
    State(state): State<ApiState>,
) -> ApiResult<Json<cerul_storage::UsageSummary>> {
    Ok(Json(cerul_storage::usage_summary(&state.paths)?))
}

#[derive(Debug, Deserialize)]
struct UsageEventsQuery {
    limit: Option<usize>,
}

async fn list_usage_events(
    State(state): State<ApiState>,
    Query(query): Query<UsageEventsQuery>,
) -> ApiResult<Json<Vec<cerul_storage::UsageEvent>>> {
    Ok(Json(cerul_storage::list_usage_events(
        &state.paths,
        query.limit.unwrap_or(50).min(500),
    )?))
}

async fn list_settings(State(state): State<ApiState>) -> ApiResult<Json<BTreeMap<String, Value>>> {
    let conn = cerul_storage::sqlite::open(&state.paths)?;
    remove_legacy_cloud_settings(&conn)?;
    let mut stmt = conn.prepare("SELECT key, value FROM settings ORDER BY key ASC")?;
    let rows = stmt.query_map([], |row| {
        let key: String = row.get(0)?;
        let value: String = row.get(1)?;
        Ok((key, parse_json(&value)))
    })?;

    Ok(Json(
        rows.collect::<Result<BTreeMap<_, _>, _>>()?
            .into_iter()
            .filter(|(key, _)| !is_hidden_setting(key))
            .map(|(key, value)| {
                let value = normalize_setting_value(&key, value);
                (key, value)
            })
            .collect(),
    ))
}

async fn update_settings(
    State(state): State<ApiState>,
    Json(settings): Json<BTreeMap<String, Value>>,
) -> ApiResult<Json<BTreeMap<String, Value>>> {
    let previous_inference_mode = configured_inference_mode(&state.paths)?;
    let requested_inference_mode = requested_inference_mode(&settings);
    let mut conn = cerul_storage::sqlite::open(&state.paths)?;
    let tx = conn.transaction()?;
    for (key, value) in &settings {
        if is_legacy_cloud_setting(key) {
            tx.execute("DELETE FROM settings WHERE key = ?1", [key])?;
            continue;
        }
        if is_internal_setting(key) {
            continue;
        }
        let value = normalize_setting_value(key, value.clone());
        tx.execute(
            r#"
            INSERT INTO settings (key, value, updated_at)
            VALUES (?1, ?2, strftime('%s','now'))
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = excluded.updated_at
            "#,
            (key, value.to_string()),
        )?;
    }
    tx.commit()?;

    if let Some(inference_mode) = requested_inference_mode.as_deref() {
        sync_inference_mode_side_effects(&state.paths, &previous_inference_mode, inference_mode)?;
    }

    Ok(Json(
        settings
            .into_iter()
            .filter(|(key, _)| !is_hidden_setting(key))
            .map(|(key, value)| {
                let value = normalize_setting_value(&key, value);
                (key, value)
            })
            .collect(),
    ))
}

fn remove_legacy_cloud_settings(conn: &rusqlite::Connection) -> anyhow::Result<usize> {
    let mut removed = 0;
    for key in LEGACY_CLOUD_SETTING_KEYS {
        removed += conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
    }
    Ok(removed)
}

fn is_legacy_cloud_setting(key: &str) -> bool {
    LEGACY_CLOUD_SETTING_KEYS.contains(&key)
}

fn is_internal_setting(key: &str) -> bool {
    INTERNAL_SETTING_KEYS.contains(&key)
}

fn is_hidden_setting(key: &str) -> bool {
    is_legacy_cloud_setting(key) || is_internal_setting(key)
}

fn normalize_setting_value(key: &str, value: Value) -> Value {
    if key == "inference_mode" {
        return Value::String(
            value
                .as_str()
                .map(normalize_inference_mode)
                .unwrap_or_else(|| "remote".to_string()),
        );
    }
    value
}

fn requested_inference_mode(settings: &BTreeMap<String, Value>) -> Option<String> {
    settings
        .get("inference_mode")
        .and_then(Value::as_str)
        .map(normalize_inference_mode)
}

fn configured_inference_mode(paths: &AppPaths) -> anyhow::Result<String> {
    Ok(setting_string(paths, "inference_mode")?
        .as_deref()
        .map(normalize_inference_mode)
        .unwrap_or_else(|| "remote".to_string()))
}

fn normalize_inference_mode(value: &str) -> String {
    if value.trim().eq_ignore_ascii_case("local") {
        "local".to_string()
    } else {
        "remote".to_string()
    }
}

fn sync_inference_mode_side_effects(
    paths: &AppPaths,
    previous_mode: &str,
    next_mode: &str,
) -> anyhow::Result<()> {
    let previous_mode = normalize_inference_mode(previous_mode);
    let next_mode = normalize_inference_mode(next_mode);
    cerul_storage::vectors::ensure_embedding_profile_for_inference_mode(paths, &next_mode)?;
    if next_mode != "local" {
        api_models::shutdown_local_query_sidecar();
    }

    let deferred_mode = setting_string(paths, DEFERRED_EMBEDDING_REBUILD_MODE_SETTING)?
        .as_deref()
        .map(normalize_inference_mode);
    let has_deferred_rebuild = deferred_mode.as_deref() == Some(next_mode.as_str());
    if previous_mode == next_mode && !has_deferred_rebuild {
        return Ok(());
    }

    if next_mode == "local" {
        let runtime = models::model_runtime_status(paths);
        if !runtime.local_runtime_ready {
            set_deferred_embedding_rebuild_mode(paths, &next_mode)?;
            tracing::warn!(
                previous_mode,
                next_mode,
                local_runtime_error = ?runtime.local_runtime_error,
                "local inference mode selected but runtime is not ready; deferred embedding profile rebuild"
            );
            return Ok(());
        }
    }

    let (rebuild_items, queued_jobs) = queue_items_for_embedding_mode_rebuild(paths)?;
    clear_deferred_embedding_rebuild_mode(paths)?;
    tracing::info!(
        previous_mode,
        next_mode,
        rebuild_items,
        queued_jobs,
        "inference mode changed; queued items for embedding profile rebuild"
    );
    Ok(())
}

pub(crate) fn sync_deferred_embedding_rebuild_if_ready(
    paths: &AppPaths,
    runtime: &models::ModelRuntimeStatus,
) -> anyhow::Result<()> {
    if !runtime.local_runtime_ready {
        return Ok(());
    }

    let inference_mode = configured_inference_mode(paths)?;
    if inference_mode != "local" {
        return Ok(());
    }

    let deferred_mode = setting_string(paths, DEFERRED_EMBEDDING_REBUILD_MODE_SETTING)?
        .as_deref()
        .map(normalize_inference_mode);
    if deferred_mode.as_deref() != Some("local") {
        return Ok(());
    }

    cerul_storage::vectors::ensure_embedding_profile_for_inference_mode(paths, "local")?;
    let (rebuild_items, queued_jobs) = queue_items_for_embedding_mode_rebuild(paths)?;
    clear_deferred_embedding_rebuild_mode(paths)?;
    tracing::info!(
        inference_mode,
        rebuild_items,
        queued_jobs,
        "local runtime is ready; queued deferred embedding profile rebuild"
    );
    Ok(())
}

fn set_deferred_embedding_rebuild_mode(paths: &AppPaths, mode: &str) -> anyhow::Result<()> {
    let conn = cerul_storage::sqlite::open(paths)?;
    conn.execute(
        r#"
        INSERT INTO settings (key, value, updated_at)
        VALUES (?1, ?2, strftime('%s','now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at
        "#,
        (
            DEFERRED_EMBEDDING_REBUILD_MODE_SETTING,
            Value::String(mode.to_string()).to_string(),
        ),
    )?;
    Ok(())
}

fn clear_deferred_embedding_rebuild_mode(paths: &AppPaths) -> anyhow::Result<()> {
    let conn = cerul_storage::sqlite::open(paths)?;
    conn.execute(
        "DELETE FROM settings WHERE key = ?1",
        [DEFERRED_EMBEDDING_REBUILD_MODE_SETTING],
    )?;
    Ok(())
}

fn sync_indexing_schema_side_effects(paths: &AppPaths) -> anyhow::Result<()> {
    let current = setting_string(paths, INDEXING_SCHEMA_VERSION_SETTING)?
        .and_then(|value| value.parse::<i32>().ok());
    if current == Some(INDEXING_SCHEMA_VERSION) {
        return Ok(());
    }

    let (rebuild_items, queued_jobs) = queue_items_for_embedding_mode_rebuild(paths)?;
    let conn = cerul_storage::sqlite::open(paths)?;
    conn.execute(
        r#"
        INSERT INTO settings (key, value, updated_at)
        VALUES (?1, ?2, strftime('%s','now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at
        "#,
        (
            INDEXING_SCHEMA_VERSION_SETTING,
            Value::from(INDEXING_SCHEMA_VERSION).to_string(),
        ),
    )?;
    tracing::info!(
        previous_version = ?current,
        version = INDEXING_SCHEMA_VERSION,
        rebuild_items,
        queued_jobs,
        "indexing schema changed; queued media rebuild"
    );
    Ok(())
}

fn queue_items_for_embedding_mode_rebuild(paths: &AppPaths) -> anyhow::Result<(usize, usize)> {
    let mut conn = cerul_storage::sqlite::open(paths)?;
    let tx = conn.transaction()?;
    let items = {
        let mut stmt = tx.prepare(
            r#"
            SELECT id, content_type, status
            FROM items
            WHERE status IN ('indexed', 'fetching', 'processing')
              AND content_type IN ('video', 'audio', 'image')
            ORDER BY id ASC
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let mut queued_jobs = 0;
    for (item_id, content_type, status) in &items {
        let content_type = parse_content_type(content_type)?;
        if status == "indexed" {
            tx.execute(
                r#"
                UPDATE items
                SET status = 'discovered',
                    indexed_at = NULL,
                    error = NULL
                WHERE id = ?1
                "#,
                [item_id.as_str()],
            )?;
        }
        if enqueue_embedding_rebuild_job(&tx, item_id, content_type)? {
            queued_jobs += 1;
        }
    }
    tx.commit()?;

    Ok((items.len(), queued_jobs))
}

fn enqueue_embedding_rebuild_job(
    tx: &Transaction<'_>,
    item_id: &str,
    content_type: ContentType,
) -> anyhow::Result<bool> {
    let job_type = index_job_type(content_type);
    let existing_queued: i64 = tx.query_row(
        r#"
        SELECT COUNT(*)
        FROM jobs
        WHERE item_id = ?1
          AND job_type = ?2
          AND status = 'queued'
        "#,
        (item_id, job_type),
        |row| row.get(0),
    )?;
    if existing_queued > 0 {
        return Ok(false);
    }

    tx.execute(
        r#"
        INSERT INTO jobs (id, item_id, job_type, status, progress)
        VALUES (?1, ?2, ?3, 'queued', 0)
        "#,
        (new_id("job"), item_id, job_type),
    )?;
    Ok(true)
}

fn set_source_status(paths: &AppPaths, id: &str, status: &str) -> anyhow::Result<()> {
    let conn = cerul_storage::sqlite::open(paths)?;
    let updated = conn.execute("UPDATE sources SET status = ?1 WHERE id = ?2", (status, id))?;
    anyhow::ensure!(updated == 1, "source not found: {id}");
    Ok(())
}

fn source_by_id(paths: &AppPaths, id: &str) -> anyhow::Result<SourceRecord> {
    let conn = cerul_storage::sqlite::open(paths)?;
    conn.query_row(
        r#"
        SELECT id, type, config, status, last_poll_at, created_at
        FROM sources
        WHERE id = ?1
        "#,
        [id],
        |row| {
            let config: String = row.get(2)?;
            Ok(SourceRecord {
                id: row.get(0)?,
                source_type: row.get(1)?,
                config: parse_json(&config),
                status: row.get(3)?,
                last_poll_at: row.get(4)?,
                created_at: row.get(5)?,
            })
        },
    )
    .map_err(Into::into)
}

fn primary_content_type(plugin: &dyn cerul_sources::SourcePlugin) -> anyhow::Result<ContentType> {
    plugin
        .content_types()
        .first()
        .copied()
        .ok_or_else(|| anyhow::anyhow!("source plugin {} has no content type", plugin.name()))
}

fn upsert_discovered_item(
    tx: &Transaction<'_>,
    source_id: &str,
    content_type: ContentType,
    item: &DiscoveredItem,
) -> anyhow::Result<String> {
    let item_id = new_id("item");
    let content_type = content_type_value(content_type);
    let raw_path = item.metadata.get("raw_path").and_then(Value::as_str);
    let metadata = item.metadata.to_string();

    tx.execute(
        r#"
        INSERT INTO items (
            id,
            source_id,
            content_type,
            external_id,
            title,
            duration_sec,
            raw_path,
            status,
            error,
            metadata
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'discovered', NULL, ?8)
        ON CONFLICT(source_id, external_id) DO UPDATE SET
            content_type = excluded.content_type,
            title = excluded.title,
            duration_sec = excluded.duration_sec,
            raw_path = excluded.raw_path,
            metadata = excluded.metadata,
            error = NULL,
            status = CASE
                WHEN items.status = 'indexed' THEN items.status
                ELSE excluded.status
            END
        "#,
        (
            item_id.as_str(),
            source_id,
            content_type,
            item.external_id.as_str(),
            item.title.as_deref(),
            item.duration_sec,
            raw_path,
            metadata.as_str(),
        ),
    )?;

    Ok(tx.query_row(
        "SELECT id FROM items WHERE source_id = ?1 AND external_id = ?2",
        (source_id, item.external_id.as_str()),
        |row| row.get(0),
    )?)
}

fn enqueue_index_job(
    tx: &Transaction<'_>,
    item_id: &str,
    content_type: ContentType,
) -> anyhow::Result<bool> {
    let status: String =
        tx.query_row("SELECT status FROM items WHERE id = ?1", [item_id], |row| {
            row.get(0)
        })?;
    if status == "indexed" {
        return Ok(false);
    }

    let job_type = index_job_type(content_type);
    let existing_active: i64 = tx.query_row(
        r#"
        SELECT COUNT(*)
        FROM jobs
        WHERE item_id = ?1
          AND job_type = ?2
          AND status IN ('queued', 'running')
        "#,
        (item_id, job_type),
        |row| row.get(0),
    )?;
    if existing_active > 0 {
        return Ok(false);
    }

    tx.execute(
        r#"
        INSERT INTO jobs (id, item_id, job_type, status, progress)
        VALUES (?1, ?2, ?3, 'queued', 0)
        "#,
        (new_id("job"), item_id, job_type),
    )?;
    Ok(true)
}

fn content_type_value(content_type: ContentType) -> &'static str {
    match content_type {
        ContentType::Video => "video",
        ContentType::Audio => "audio",
        ContentType::Image => "image",
    }
}

fn parse_content_type(value: &str) -> anyhow::Result<ContentType> {
    match value {
        "video" => Ok(ContentType::Video),
        "audio" => Ok(ContentType::Audio),
        "image" => Ok(ContentType::Image),
        other => Err(anyhow::anyhow!("unsupported content type: {other}")),
    }
}

fn index_job_type(content_type: ContentType) -> &'static str {
    match content_type {
        ContentType::Video => "index_video",
        ContentType::Audio => "index_audio",
        ContentType::Image => "index_image",
    }
}

fn item_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ItemRecord> {
    let metadata: Option<String> = row.get(10)?;

    Ok(ItemRecord {
        id: row.get(0)?,
        source_id: row.get(1)?,
        content_type: row.get(2)?,
        external_id: row.get(3)?,
        title: row.get(4)?,
        duration_sec: row.get(5)?,
        raw_path: row.get(6)?,
        indexed_at: row.get(7)?,
        status: row.get(8)?,
        error: row.get(9)?,
        metadata: metadata
            .as_deref()
            .map(parse_json)
            .unwrap_or_else(|| json!({})),
        thumbnail_chunk_id: row.get(11)?,
        usage: cerul_storage::UsageTotals::default(),
    })
}

fn attach_item_usage(paths: &AppPaths, items: &mut [ItemRecord]) {
    for item in items {
        item.usage = cerul_storage::usage_totals_for_item(paths, &item.id).unwrap_or_default();
    }
}

fn chunk_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChunkRecord> {
    let metadata: Option<String> = row.get(7)?;

    Ok(ChunkRecord {
        id: row.get(0)?,
        item_id: row.get(1)?,
        chunk_type: row.get(2)?,
        start_sec: row.get(3)?,
        end_sec: row.get(4)?,
        text: row.get(5)?,
        frame_path: row.get(6)?,
        metadata: metadata
            .as_deref()
            .map(parse_json)
            .unwrap_or_else(|| json!({})),
    })
}

fn chunk_path(paths: &AppPaths, chunk_id: &str, column: &str) -> anyhow::Result<Option<String>> {
    let conn = cerul_storage::sqlite::open(paths)?;
    let sql = format!("SELECT {column} FROM chunks WHERE id = ?1");
    match conn.query_row(&sql, [chunk_id], |row| row.get(0)) {
        Ok(path) => Ok(path),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn item_raw_path_for_chunk(paths: &AppPaths, chunk_id: &str) -> anyhow::Result<Option<String>> {
    let conn = cerul_storage::sqlite::open(paths)?;
    match conn.query_row(
        r#"
        SELECT i.raw_path
        FROM chunks c
        JOIN items i ON i.id = c.item_id
        WHERE c.id = ?1
        "#,
        [chunk_id],
        |row| row.get(0),
    ) {
        Ok(path) => Ok(path),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn video_clip_source_for_chunk(
    paths: &AppPaths,
    chunk_id: &str,
) -> anyhow::Result<Option<VideoClipSource>> {
    let conn = cerul_storage::sqlite::open(paths)?;
    match conn.query_row(
        r#"
        SELECT i.raw_path, i.title, c.start_sec, c.end_sec
        FROM chunks c
        JOIN items i ON i.id = c.item_id
        WHERE c.id = ?1
          AND i.content_type = 'video'
          AND i.raw_path IS NOT NULL
        "#,
        [chunk_id],
        |row| {
            let raw_path: Option<String> = row.get(0)?;
            let title: Option<String> = row.get(1)?;
            let start_sec: Option<f64> = row.get(2)?;
            let end_sec: Option<f64> = row.get(3)?;
            Ok(raw_path.map(|raw_path| VideoClipSource {
                raw_path,
                title,
                start_sec,
                end_sec,
            }))
        },
    ) {
        Ok(source) => Ok(source),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn clip_window(start_sec: Option<f64>, end_sec: Option<f64>, padding_sec: f64) -> (f64, f64) {
    let start = start_sec.unwrap_or(0.0).max(0.0);
    let fallback_end = start + 12.0;
    let end = end_sec
        .filter(|end| end.is_finite() && *end > start)
        .unwrap_or(fallback_end);
    let padding = if padding_sec.is_finite() {
        padding_sec.clamp(0.0, 10.0)
    } else {
        2.0
    };
    let clipped_start = (start - padding).max(0.0);
    let duration = (end + padding - clipped_start).clamp(1.0, 120.0);
    (clipped_start, duration)
}

fn video_clip_cache_path(
    paths: &AppPaths,
    chunk_id: &str,
    start_sec: f64,
    duration_sec: f64,
) -> PathBuf {
    paths.cache.join("clips").join(format!(
        "{}-{}-{}.mp4",
        safe_filename_part(chunk_id),
        (start_sec * 1000.0).round() as i64,
        (duration_sec * 1000.0).round() as i64
    ))
}

fn video_clip_filename(title: Option<&str>, chunk_id: &str, start_sec: f64) -> String {
    let base = title
        .map(safe_filename_part)
        .filter(|part| !part.is_empty())
        .unwrap_or_else(|| safe_filename_part(chunk_id));
    format!("{}-{}.mp4", base, start_sec.round() as i64)
}

fn safe_filename_part(value: &str) -> String {
    let mut part = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    while part.contains("--") {
        part = part.replace("--", "-");
    }
    part.trim_matches('-').chars().take(80).collect()
}

async fn video_file_response(path: &str, range: Option<&HeaderValue>) -> ApiResult<Response> {
    let mut file = tokio::fs::File::open(path).await?;
    let len = file.metadata().await?.len();
    let content_type = video_content_type(path);

    match parse_byte_range(range, len) {
        Ok(Some((start, end))) => {
            let byte_count = end - start + 1;
            let mut bytes = vec![0; byte_count as usize];
            file.seek(std::io::SeekFrom::Start(start)).await?;
            file.read_exact(&mut bytes).await?;

            let mut response = (StatusCode::PARTIAL_CONTENT, Body::from(bytes)).into_response();
            response
                .headers_mut()
                .insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
            response.headers_mut().insert(
                header::CONTENT_LENGTH,
                HeaderValue::from_str(&byte_count.to_string())
                    .map_err(|error| ApiError::internal(anyhow::anyhow!(error)))?,
            );
            response.headers_mut().insert(
                header::CONTENT_RANGE,
                HeaderValue::from_str(&format!("bytes {start}-{end}/{len}"))
                    .map_err(|error| ApiError::internal(anyhow::anyhow!(error)))?,
            );
            Ok(response)
        }
        Ok(None) => {
            let bytes = tokio::fs::read(path).await?;
            let mut response = Body::from(bytes).into_response();
            response
                .headers_mut()
                .insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
            response.headers_mut().insert(
                header::CONTENT_LENGTH,
                HeaderValue::from_str(&len.to_string())
                    .map_err(|error| ApiError::internal(anyhow::anyhow!(error)))?,
            );
            Ok(response)
        }
        Err(ByteRangeError::Unsatisfiable) => {
            let mut response = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
            response
                .headers_mut()
                .insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
            response.headers_mut().insert(
                header::CONTENT_RANGE,
                HeaderValue::from_str(&format!("bytes */{len}"))
                    .map_err(|error| ApiError::internal(anyhow::anyhow!(error)))?,
            );
            Ok(response)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ByteRangeError {
    Unsatisfiable,
}

fn parse_byte_range(
    range: Option<&HeaderValue>,
    len: u64,
) -> Result<Option<(u64, u64)>, ByteRangeError> {
    let Some(range) = range else {
        return Ok(None);
    };
    let Ok(range) = range.to_str() else {
        return Err(ByteRangeError::Unsatisfiable);
    };
    let Some(spec) = range.strip_prefix("bytes=") else {
        return Err(ByteRangeError::Unsatisfiable);
    };
    if spec.contains(',') || spec.is_empty() || len == 0 {
        return Err(ByteRangeError::Unsatisfiable);
    }

    let Some((start, end)) = spec.split_once('-') else {
        return Err(ByteRangeError::Unsatisfiable);
    };

    if start.is_empty() {
        let suffix_len = end
            .parse::<u64>()
            .map_err(|_| ByteRangeError::Unsatisfiable)?;
        if suffix_len == 0 {
            return Err(ByteRangeError::Unsatisfiable);
        }
        let start = len.saturating_sub(suffix_len);
        return Ok(Some((start, len - 1)));
    }

    let start = start
        .parse::<u64>()
        .map_err(|_| ByteRangeError::Unsatisfiable)?;
    if start >= len {
        return Err(ByteRangeError::Unsatisfiable);
    }

    let end = if end.is_empty() {
        len - 1
    } else {
        end.parse::<u64>()
            .map_err(|_| ByteRangeError::Unsatisfiable)?
            .min(len - 1)
    };
    if end < start {
        return Err(ByteRangeError::Unsatisfiable);
    }

    Ok(Some((start, end)))
}

fn video_content_type(path: &str) -> &'static str {
    match std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("mp4") | Some("m4v") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mov") => "video/quicktime",
        Some("mkv") => "video/x-matroska",
        _ => "application/octet-stream",
    }
}

fn image_content_type(path: &str) -> &'static str {
    match std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("heic") => "image/heic",
        _ => "application/octet-stream",
    }
}

pub(crate) fn setting_string(paths: &AppPaths, key: &str) -> anyhow::Result<Option<String>> {
    let conn = cerul_storage::sqlite::open(paths)?;
    let value: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?;

    Ok(value.and_then(|value| match parse_json(&value) {
        Value::String(value) => Some(value),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }))
}

fn parse_json(value: &str) -> Value {
    serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_string()))
}

fn new_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{prefix}-{nanos:x}")
}

fn not_found(message: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": message
        })),
    )
        .into_response()
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": self.error.to_string()
            })),
        )
            .into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self::internal(error)
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(error: rusqlite::Error) -> Self {
        Self::internal(error.into())
    }
}

impl From<std::io::Error> for ApiError {
    fn from(error: std::io::Error) -> Self {
        Self::internal(error.into())
    }
}

const API_PATHS: &[(&str, &[&str])] = &[
    ("/health", &["get"]),
    ("/metrics", &["get"]),
    ("/openapi.json", &["get"]),
    ("/search", &["post"]),
    ("/sources", &["get", "post"]),
    ("/sources/preview/rss", &["post"]),
    ("/sources/{id}", &["delete"]),
    ("/sources/{id}/pause", &["post"]),
    ("/sources/{id}/resume", &["post"]),
    ("/items", &["get"]),
    ("/items/{id}", &["get", "delete"]),
    ("/items/{id}/reindex", &["post"]),
    ("/items/{id}/chunks", &["get"]),
    ("/items/{id}/understanding", &["get", "post"]),
    ("/chunks/{id}/frame", &["get"]),
    ("/chunks/{id}/video-segment", &["get"]),
    ("/chunks/{id}/video-clip", &["get"]),
    ("/jobs", &["get"]),
    ("/usage/events", &["get"]),
    ("/usage/summary", &["get"]),
    ("/models/catalog", &["get"]),
    ("/models/whisper", &["get"]),
    ("/models/whisper/{id}/download", &["post"]),
    ("/models/whisper/auto-download-status", &["get"]),
    ("/models/embed/status", &["get"]),
    ("/models/embed/prepare", &["post"]),
    ("/providers", &["get", "post"]),
    ("/providers/{id}", &["patch", "delete"]),
    ("/providers/{id}/test", &["post"]),
    ("/settings", &["get", "patch"]),
];

const LEGACY_CLOUD_SETTING_KEYS: &[&str] = &[
    "cloud_api_key",
    "cloud_connected",
    "cloud_account_email",
    "cloud_email",
    "cloud_plan",
    "cloud_quota_percent",
];
const DEFERRED_EMBEDDING_REBUILD_MODE_SETTING: &str = "embedding_profile_rebuild_deferred_mode";
const INDEXING_SCHEMA_VERSION_SETTING: &str = "indexing_schema_version";
const INDEXING_SCHEMA_VERSION: i32 = 3;
const INTERNAL_SETTING_KEYS: &[&str] = &[
    DEFERRED_EMBEDDING_REBUILD_MODE_SETTING,
    INDEXING_SCHEMA_VERSION_SETTING,
];

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::{Method, Request},
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn router_serves_health_and_openapi() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let app = router_with_paths(paths);

        let health = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);
        let health_json = response_json(health).await;
        assert_eq!(health_json["status"], "ok");

        let openapi = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/openapi.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(openapi.status(), StatusCode::OK);
        let openapi_json = response_json(openapi).await;
        assert!(openapi_json["paths"].as_object().unwrap().len() >= 19);
    }

    #[tokio::test]
    async fn remote_api_requires_bearer_key_only_for_non_loopback_clients() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                r#"
                INSERT INTO settings (key, value, updated_at)
                VALUES ('remote_api_key', '"remote-secret"', strftime('%s','now'))
                "#,
                [],
            )
            .unwrap();
        }
        let app = router_with_paths(paths);

        let loopback = app
            .clone()
            .oneshot(remote_request("127.0.0.1:4000", None))
            .await
            .unwrap();
        assert_eq!(loopback.status(), StatusCode::OK);

        let remote_without_key = app
            .clone()
            .oneshot(remote_request("192.0.2.10:4000", None))
            .await
            .unwrap();
        assert_eq!(remote_without_key.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            remote_without_key
                .headers()
                .get(header::WWW_AUTHENTICATE)
                .unwrap(),
            "Bearer"
        );

        let remote_with_wrong_key = app
            .clone()
            .oneshot(remote_request("192.0.2.10:4000", Some("wrong")))
            .await
            .unwrap();
        assert_eq!(remote_with_wrong_key.status(), StatusCode::UNAUTHORIZED);

        let remote_with_key = app
            .oneshot(remote_request("192.0.2.10:4000", Some("remote-secret")))
            .await
            .unwrap();
        assert_eq!(remote_with_key.status(), StatusCode::OK);
    }

    #[test]
    fn configured_addr_defaults_to_loopback_and_reads_binding_setting() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        assert_eq!(configured_addr(&paths).unwrap(), default_addr());

        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        conn.execute(
            r#"
            INSERT INTO settings (key, value, updated_at)
            VALUES ('api_binding', '"0"', strftime('%s','now'))
            "#,
            [],
        )
        .unwrap();

        assert_eq!(
            configured_addr(&paths).unwrap(),
            "0.0.0.0:7777".parse::<SocketAddr>().unwrap()
        );
    }

    #[tokio::test]
    async fn router_serves_whisper_model_manifest() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let app = router_with_paths(paths);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/models/whisper")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let models = response_json(response).await;
        assert_eq!(models.as_array().unwrap().len(), 3);
        assert_eq!(models[0]["id"], "base.en");
        assert_eq!(models[0]["installed"], false);
    }

    #[tokio::test]
    async fn router_serves_model_catalog_with_default_profile() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let app = router_with_paths(paths);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/models/catalog")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let catalog = response_json(response).await;
        assert_eq!(
            catalog["active_embedding_profile"]["id"],
            cerul_storage::vectors::DEFAULT_EMBEDDING_PROFILE_ID
        );
        assert!(catalog["models"]
            .as_array()
            .unwrap()
            .iter()
            .any(|model| model["id"] == "whisper-1"));
        assert!(catalog["models"]
            .as_array()
            .unwrap()
            .iter()
            .any(|model| model["id"] == "gemini-embedding-2"));
    }

    #[tokio::test]
    async fn settings_endpoint_removes_legacy_cloud_token_keys() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                r#"
                INSERT INTO settings (key, value, updated_at)
                VALUES ('cloud_api_key', '"stale-token"', strftime('%s','now')),
                       ('cloud_email', '"user@example.com"', strftime('%s','now')),
                       ('cloud_quota_percent', '42', strftime('%s','now')),
                       ('inference_mode', '"byok"', strftime('%s','now')),
                       ('theme', '"Dark"', strftime('%s','now'))
                "#,
                [],
            )
            .unwrap();
        }
        let app = router_with_paths(paths.clone());

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/settings")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let settings = response_json(response).await;
        assert!(settings.get("cloud_api_key").is_none());
        assert!(settings.get("cloud_email").is_none());
        assert!(settings.get("cloud_quota_percent").is_none());
        assert_eq!(settings["inference_mode"], "remote");
        assert_eq!(settings["theme"], "Dark");
        assert!(setting_string(&paths, "cloud_api_key").unwrap().is_none());
        assert!(setting_string(&paths, "cloud_email").unwrap().is_none());
        assert!(setting_string(&paths, "cloud_quota_percent")
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn mode_switch_resets_profile_and_requeues_indexed_items() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'folder_video', '{}', 'active')",
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO items (
                    id, source_id, content_type, external_id, title, indexed_at, status, metadata
                )
                VALUES ('item-1', 'source-1', 'video', 'video.mp4', 'Video', 100, 'indexed', '{}')
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO settings (key, value, updated_at)
                VALUES
                    ('inference_mode', '"local"', strftime('%s','now')),
                    ('active_embedding_profile', '"qwen3-vl-local-2048"', strftime('%s','now'))
                "#,
                [],
            )
            .unwrap();
        }
        let app = router_with_paths(paths.clone());

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri("/settings")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({ "inference_mode": "remote" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let profile = cerul_storage::vectors::ensure_active_embedding_profile(&paths).unwrap();
        assert_eq!(
            profile.id,
            cerul_storage::vectors::DEFAULT_EMBEDDING_PROFILE_ID
        );
        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        let item_status: String = conn
            .query_row("SELECT status FROM items WHERE id = 'item-1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        let queued_jobs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM jobs WHERE item_id = 'item-1' AND status = 'queued'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(item_status, "discovered");
        assert_eq!(queued_jobs, 1);
    }

    #[tokio::test]
    async fn mode_switch_queues_followup_for_processing_items() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'folder_video', '{}', 'active')",
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO items (
                    id, source_id, content_type, external_id, title, indexed_at, status, metadata
                )
                VALUES ('item-1', 'source-1', 'video', 'video.mp4', 'Video', 100, 'processing', '{}')
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO jobs (id, item_id, job_type, status, progress)
                VALUES ('job-running', 'item-1', 'index_video', 'running', 0.5)
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO settings (key, value, updated_at)
                VALUES
                    ('inference_mode', '"local"', strftime('%s','now')),
                    ('active_embedding_profile', '"qwen3-vl-local-2048"', strftime('%s','now'))
                "#,
                [],
            )
            .unwrap();
        }
        let app = router_with_paths(paths.clone());

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri("/settings")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({ "inference_mode": "remote" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        let item_status: String = conn
            .query_row("SELECT status FROM items WHERE id = 'item-1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        let (running_jobs, queued_jobs): (i64, i64) = conn
            .query_row(
                r#"
                SELECT
                    SUM(CASE WHEN status = 'running' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN status = 'queued' THEN 1 ELSE 0 END)
                FROM jobs
                WHERE item_id = 'item-1'
                  AND job_type = 'index_video'
                "#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(item_status, "processing");
        assert_eq!(running_jobs, 1);
        assert_eq!(queued_jobs, 1);
    }

    #[test]
    fn deferred_local_rebuild_runs_after_runtime_becomes_ready() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'folder_video', '{}', 'active')",
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO items (
                    id, source_id, content_type, external_id, title, indexed_at, status, metadata
                )
                VALUES ('item-1', 'source-1', 'video', 'video.mp4', 'Video', 100, 'indexed', '{}')
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO settings (key, value, updated_at)
                VALUES
                    ('inference_mode', '"local"', strftime('%s','now')),
                    ('active_embedding_profile', '"qwen3-vl-local-2048"', strftime('%s','now')),
                    ('embedding_profile_rebuild_deferred_mode', '"local"', strftime('%s','now'))
                "#,
                [],
            )
            .unwrap();
        }

        sync_deferred_embedding_rebuild_if_ready(
            &paths,
            &models::ModelRuntimeStatus {
                platform: "test".to_string(),
                api_runtime_ready: false,
                local_runtime_ready: true,
                openai_ready: false,
                gemini_ready: false,
                last_error: Some(
                    "Connect OpenAI ASR provider and Gemini Embedding 2 provider before indexing."
                        .to_string(),
                ),
                local_runtime_error: None,
            },
        )
        .unwrap();

        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        let item_status: String = conn
            .query_row("SELECT status FROM items WHERE id = 'item-1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        let queued_jobs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM jobs WHERE item_id = 'item-1' AND status = 'queued'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(item_status, "discovered");
        assert_eq!(queued_jobs, 1);
        assert!(
            setting_string(&paths, DEFERRED_EMBEDDING_REBUILD_MODE_SETTING)
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn router_serves_provider_connections_and_protects_local() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let app = router_with_paths(paths);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/providers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let providers = response_json(response).await;
        let local_provider = providers
            .as_array()
            .unwrap()
            .iter()
            .find(|provider| provider["id"] == "local")
            .unwrap();
        assert_eq!(local_provider["has_key"], false);

        let create_body = json!({
            "type": "openai",
            "label": "OpenAI"
        });
        let created = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/providers")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(create_body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::OK);
        let created_json = response_json(created).await;
        assert_eq!(created_json["type"], "openai");
        assert_eq!(created_json["base_url"], "https://api.openai.com/v1");
        assert_eq!(created_json["has_key"], false);

        let local_patch = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri("/providers/local")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({"label": "Other"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(local_patch.status(), StatusCode::BAD_REQUEST);

        let local_delete = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri("/providers/local")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(local_delete.status(), StatusCode::BAD_REQUEST);

        let local_test = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/providers/local/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(local_test.status(), StatusCode::OK);
        let local_test_json = response_json(local_test).await;
        assert_eq!(local_test_json["status"], "ready");
    }

    #[tokio::test]
    async fn routed_api_models_expose_provider_info_for_usage_metering() {
        use cerul_pipeline::run::{Embedder, Transcriber};

        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let app = router_with_paths(paths.clone());

        let asr_provider = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/providers")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "type": "openai-compatible",
                            "label": "Groq ASR",
                            "base_url": "https://api.groq.com/openai/v1",
                            "api_key": "test-key"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(asr_provider.status(), StatusCode::OK);
        let asr_provider = response_json(asr_provider).await;

        let embedding_provider = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/providers")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "type": "gemini",
                            "label": "Gemini Embedding",
                            "base_url": "https://generativelanguage.googleapis.com/v1beta",
                            "api_key": "test-key"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(embedding_provider.status(), StatusCode::OK);
        let embedding_provider = response_json(embedding_provider).await;

        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            for (key, value) in [
                ("asr_provider_id", asr_provider["id"].as_str().unwrap()),
                (
                    "embedding_provider_id",
                    embedding_provider["id"].as_str().unwrap(),
                ),
                ("asr_model", "whisper-1"),
            ] {
                conn.execute(
                    r#"
                    INSERT INTO settings (key, value, updated_at)
                    VALUES (?1, ?2, strftime('%s','now'))
                    ON CONFLICT(key) DO UPDATE SET
                        value = excluded.value,
                        updated_at = excluded.updated_at
                    "#,
                    (key, Value::String(value.to_string()).to_string()),
                )
                .unwrap();
            }
        }

        let asr_info = crate::api_models::routed_transcriber(paths.clone())
            .inference_provider()
            .unwrap();
        assert_eq!(asr_info.provider_mode, "remote");
        assert_eq!(asr_info.provider_id.as_deref(), asr_provider["id"].as_str());
        assert_eq!(
            asr_info.base_url.as_deref(),
            Some("https://api.groq.com/openai/v1")
        );

        let embedding_info = crate::api_models::selected_embedder(&paths)
            .unwrap()
            .inference_provider()
            .unwrap();
        assert_eq!(embedding_info.provider_mode, "remote");
        assert_eq!(
            embedding_info.provider_id.as_deref(),
            embedding_provider["id"].as_str()
        );
        assert_eq!(
            embedding_info.model_id.as_deref(),
            Some("gemini-embedding-2")
        );
    }

    #[tokio::test]
    async fn source_lifecycle_updates_sqlite() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let videos = temp.path().join("videos");
        std::fs::create_dir(&videos).unwrap();
        let sample = videos.join("sample.mp4");
        std::fs::write(&sample, b"video").unwrap();
        let app = router_with_paths(paths);
        let body = json!({
            "type": "folder_video",
            "config": {
                "path": videos,
            },
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/sources")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let source = response_json(response).await;
        let id = source["id"].as_str().unwrap();
        assert_eq!(source["status"], "active");
        assert!(source["last_poll_at"].as_i64().is_some());

        let items = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/items")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(items.status(), StatusCode::OK);
        let items = response_json(items).await;
        assert_eq!(items.as_array().unwrap().len(), 1);
        assert_eq!(items[0]["source_id"], id);
        assert_eq!(items[0]["content_type"], "video");
        assert_eq!(items[0]["status"], "discovered");
        assert_eq!(
            items[0]["raw_path"].as_str().unwrap(),
            sample.to_string_lossy().as_ref()
        );

        let jobs = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/jobs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(jobs.status(), StatusCode::OK);
        let jobs = response_json(jobs).await;
        assert_eq!(jobs.as_array().unwrap().len(), 1);
        assert_eq!(jobs[0]["job_type"], "index_video");
        assert_eq!(jobs[0]["status"], "queued");
        assert_eq!(jobs[0]["progress"], 0.0);

        let pause = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/sources/{id}/pause"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(pause.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn item_delete_and_reindex_update_storage() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'folder_video', '{}', 'active')",
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO items (
                    id, source_id, content_type, external_id, title, indexed_at, status, metadata
                )
                VALUES ('item-1', 'source-1', 'video', 'clip.mp4', 'Clip', 10, 'indexed', '{}')
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, text, metadata)
                VALUES ('chunk-1', 'item-1', 'transcript', 0, 5, 'hello', '{}')
                "#,
                [],
            )
            .unwrap();
        }
        let app = router_with_paths(paths.clone());

        let reindex = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/items/item-1/reindex")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(reindex.status(), StatusCode::OK);
        let reindex = response_json(reindex).await;
        assert_eq!(reindex["status"], "queued");
        assert_eq!(reindex["queued_job"], true);

        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            let item: (String, Option<i64>) = conn
                .query_row(
                    "SELECT status, indexed_at FROM items WHERE id = 'item-1'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(item, ("discovered".to_string(), None));
            let jobs: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM jobs WHERE item_id = 'item-1' AND job_type = 'index_video' AND status = 'queued'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(jobs, 1);
        }

        let delete = app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri("/items/item-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(delete.status(), StatusCode::OK);

        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        for table in ["items", "chunks", "jobs"] {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0, "{table} should be empty after deleting item");
        }
    }

    #[tokio::test]
    async fn concurrent_reindex_requests_queue_without_sqlite_locking() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'folder_video', '{}', 'active')",
                [],
            )
            .unwrap();
            for index in 0..5 {
                let item_id = format!("item-{index}");
                let external_id = format!("clip-{index}.mp4");
                let title = format!("Clip {index}");
                conn.execute(
                    r#"
                    INSERT INTO items (
                        id, source_id, content_type, external_id, title, indexed_at, status, metadata
                    )
                    VALUES (?1, 'source-1', 'video', ?2, ?3, 10, 'indexed', '{}')
                    "#,
                    (&item_id, &external_id, &title),
                )
                .unwrap();
            }
        }
        let app = router_with_paths(paths.clone());

        let request = |item_id: &str| {
            Request::builder()
                .method(Method::POST)
                .uri(format!("/items/{item_id}/reindex"))
                .body(Body::empty())
                .unwrap()
        };
        let (r0, r1, r2, r3, r4) = tokio::join!(
            app.clone().oneshot(request("item-0")),
            app.clone().oneshot(request("item-1")),
            app.clone().oneshot(request("item-2")),
            app.clone().oneshot(request("item-3")),
            app.oneshot(request("item-4")),
        );

        for response in [r0, r1, r2, r3, r4] {
            let response = response.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body = response_json(response).await;
            assert_eq!(body["status"], "queued");
            assert_eq!(body["queued_job"], true);
        }

        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        let queued_jobs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM jobs WHERE job_type = 'index_video' AND status = 'queued'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(queued_jobs, 5);
    }

    #[tokio::test]
    async fn list_items_includes_first_frame_thumbnail_chunk() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'folder_video', '{}', 'active')",
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO items (
                    id, source_id, content_type, external_id, title, indexed_at, status, metadata
                )
                VALUES ('item-1', 'source-1', 'video', 'clip.mp4', 'Clip', 10, 'indexed', '{}')
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO chunks (id, item_id, chunk_type, start_sec, frame_path, metadata)
                VALUES ('chunk-late', 'item-1', 'keyframe', 20, '/tmp/frame-late.jpg', '{}')
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO chunks (id, item_id, chunk_type, start_sec, frame_path, metadata)
                VALUES ('chunk-early', 'item-1', 'keyframe', 5, '/tmp/frame-early.jpg', '{}')
                "#,
                [],
            )
            .unwrap();
        }
        let app = router_with_paths(paths);

        let items = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/items")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(items.status(), StatusCode::OK);
        let items = response_json(items).await;
        assert_eq!(items[0]["thumbnail_chunk_id"], "chunk-early");

        let item = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/items/item-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(item.status(), StatusCode::OK);
        let item = response_json(item).await;
        assert_eq!(item["thumbnail_chunk_id"], "chunk-early");
    }

    #[tokio::test]
    async fn chunk_frame_endpoint_serves_source_image_content_types() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let image = temp.path().join("diagram.PNG");
        let missing = temp.path().join("missing.webp");
        std::fs::write(&image, b"png-bytes").unwrap();
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'folder_image', '{}', 'active')",
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO items (
                    id, source_id, content_type, external_id, title, raw_path, indexed_at, status, metadata
                )
                VALUES ('item-1', 'source-1', 'image', 'diagram.PNG', 'Diagram', ?1, 10, 'indexed', '{}')
                "#,
                [image.to_string_lossy().as_ref()],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO chunks (id, item_id, chunk_type, frame_path, metadata)
                VALUES ('chunk-png', 'item-1', 'image', ?1, '{}')
                "#,
                [image.to_string_lossy().as_ref()],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO chunks (id, item_id, chunk_type, frame_path, metadata)
                VALUES ('chunk-missing', 'item-1', 'image', ?1, '{}')
                "#,
                [missing.to_string_lossy().as_ref()],
            )
            .unwrap();
        }
        let app = router_with_paths(paths);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/chunks/chunk-png/frame")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static("image/png"))
        );
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL),
            Some(&HeaderValue::from_static("public, max-age=3600"))
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&body[..], b"png-bytes");

        let missing_response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/chunks/chunk-missing/frame")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn video_segment_endpoint_serves_byte_ranges() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let video = temp.path().join("clip.mp4");
        std::fs::write(&video, b"0123456789abcdef").unwrap();
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'folder_video', '{}', 'active')",
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO items (
                    id, source_id, content_type, external_id, title, raw_path, indexed_at, status, metadata
                )
                VALUES ('item-1', 'source-1', 'video', 'clip.mp4', 'Clip', ?1, 10, 'indexed', '{}')
                "#,
                [video.to_string_lossy().as_ref()],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, text, metadata)
                VALUES ('chunk-1', 'item-1', 'transcript', 2, 5, 'hello', '{}')
                "#,
                [],
            )
            .unwrap();
        }
        let app = router_with_paths(paths);

        let partial = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/chunks/chunk-1/video-segment")
                    .header(header::RANGE, "bytes=2-5")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(partial.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            partial.headers().get(header::CONTENT_RANGE),
            Some(&HeaderValue::from_static("bytes 2-5/16"))
        );
        assert_eq!(
            partial.headers().get(header::ACCEPT_RANGES),
            Some(&HeaderValue::from_static("bytes"))
        );
        assert_eq!(
            partial.headers().get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static("video/mp4"))
        );
        let partial_body = to_bytes(partial.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&partial_body[..], b"2345");

        let full = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/chunks/chunk-1/video-segment")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(full.status(), StatusCode::OK);
        assert_eq!(
            full.headers().get(header::CONTENT_LENGTH),
            Some(&HeaderValue::from_static("16"))
        );
        let full_body = to_bytes(full.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&full_body[..], b"0123456789abcdef");

        let unsatisfiable = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/chunks/chunk-1/video-segment")
                    .header(header::RANGE, "bytes=20-30")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unsatisfiable.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(
            unsatisfiable.headers().get(header::CONTENT_RANGE),
            Some(&HeaderValue::from_static("bytes */16"))
        );
    }

    #[test]
    fn clip_window_adds_padding_and_caps_duration() {
        assert_eq!(clip_window(Some(10.0), Some(20.0), 2.0), (8.0, 14.0));
        assert_eq!(clip_window(Some(1.0), Some(3.0), 5.0), (0.0, 8.0));
        assert_eq!(clip_window(Some(0.0), None, 2.0), (0.0, 14.0));
        assert_eq!(clip_window(Some(10.0), Some(400.0), 10.0), (0.0, 120.0));
    }

    #[test]
    fn video_clip_source_requires_video_item() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let image = temp.path().join("image.png");
        let video = temp.path().join("video.mp4");
        std::fs::write(&image, b"image").unwrap();
        std::fs::write(&video, b"video").unwrap();
        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        conn.execute(
            "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'folder_video', '{}', 'active')",
            [],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO items (
                id, source_id, content_type, external_id, title, raw_path, indexed_at, status, metadata
            )
            VALUES ('image-1', 'source-1', 'image', 'image.png', 'Image', ?1, 10, 'indexed', '{}')
            "#,
            [image.to_string_lossy().as_ref()],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO items (
                id, source_id, content_type, external_id, title, raw_path, indexed_at, status, metadata
            )
            VALUES ('video-1', 'source-1', 'video', 'video.mp4', 'Video', ?1, 10, 'indexed', '{}')
            "#,
            [video.to_string_lossy().as_ref()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chunks (id, item_id, chunk_type, frame_path, metadata) VALUES ('image-chunk', 'image-1', 'image', ?1, '{}')",
            [image.to_string_lossy().as_ref()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, text, metadata) VALUES ('video-chunk', 'video-1', 'transcript', 2, 5, 'hello', '{}')",
            [],
        )
        .unwrap();

        assert!(video_clip_source_for_chunk(&paths, "image-chunk")
            .unwrap()
            .is_none());
        assert!(video_clip_source_for_chunk(&paths, "video-chunk")
            .unwrap()
            .is_some());
    }

    #[test]
    fn frame_content_type_matches_supported_image_sources() {
        assert_eq!(image_content_type("keyframe.jpg"), "image/jpeg");
        assert_eq!(image_content_type("photo.jpeg"), "image/jpeg");
        assert_eq!(image_content_type("diagram.PNG"), "image/png");
        assert_eq!(image_content_type("capture.webp"), "image/webp");
        assert_eq!(image_content_type("iphone.heic"), "image/heic");
        assert_eq!(
            image_content_type("unknown.bin"),
            "application/octet-stream"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn add_youtube_source_discovers_items_and_queues_video_jobs() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let summary = add_source_to_paths(
            &paths,
            AddSourceRequest {
                source_type: "youtube".to_string(),
                config: json!({
                    "url": "https://www.youtube.com/@cerul",
                    "max_videos": 2,
                    "ytdlp_path": fake_ytdlp(&temp),
                    "cache_dir": temp.path().join("cache"),
                }),
            },
        )
        .await
        .unwrap();

        assert_eq!(summary.source.source_type, "youtube");
        assert_eq!(summary.items.len(), 2);
        assert_eq!(summary.items[0].external_id.as_deref(), Some("abc123"));
        assert_eq!(summary.queued_jobs, 2);

        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        let video_items: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM items WHERE source_id = ?1 AND content_type = 'video' AND status = 'discovered'",
                [summary.source.id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        let video_jobs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM jobs WHERE job_type = 'index_video' AND status = 'queued'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(video_items, 2);
        assert_eq!(video_jobs, 2);
    }

    #[tokio::test]
    async fn add_rss_source_discovers_limited_items_and_queues_audio_jobs() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let audio = temp.path().join("episode.mp3");
        std::fs::write(&audio, b"audio").unwrap();
        let feed = temp.path().join("feed.xml");
        std::fs::write(
            &feed,
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Cerul Podcast</title>
    <item>
      <guid>episode-1</guid>
      <title>Episode One</title>
      <enclosure url="file://{}" type="audio/mpeg" length="5" />
    </item>
    <item>
      <guid>episode-2</guid>
      <title>Episode Two</title>
      <enclosure url="file://{}" type="audio/mpeg" length="5" />
    </item>
  </channel>
</rss>"#,
                audio.display(),
                audio.display()
            ),
        )
        .unwrap();

        let summary = add_source_to_paths(
            &paths,
            AddSourceRequest {
                source_type: "rss_podcast".to_string(),
                config: json!({
                    "url": feed,
                    "max_episodes": 1,
                    "cache_dir": temp.path().join("cache"),
                }),
            },
        )
        .await
        .unwrap();

        assert_eq!(summary.source.source_type, "rss_podcast");
        assert_eq!(summary.items.len(), 1);
        assert_eq!(summary.items[0].external_id.as_deref(), Some("episode-1"));
        assert_eq!(summary.queued_jobs, 1);

        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        let audio_jobs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM jobs WHERE job_type = 'index_audio' AND status = 'queued'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(audio_jobs, 1);
    }

    #[tokio::test]
    async fn preview_rss_source_returns_title_image_and_episode_count() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let app = router_with_paths(paths);
        let feed = temp.path().join("feed.xml");
        std::fs::write(
            &feed,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Cerul Podcast</title>
    <image>
      <url>https://example.com/art.jpg</url>
      <title>Cerul Podcast</title>
      <link>https://example.com</link>
    </image>
    <item>
      <guid>episode-1</guid>
      <title>Episode One</title>
    </item>
    <item>
      <guid>episode-2</guid>
      <title>Episode Two</title>
    </item>
  </channel>
</rss>"#,
        )
        .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/sources/preview/rss")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({ "url": feed.to_string_lossy() }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let preview = response_json(response).await;
        assert_eq!(preview["title"], "Cerul Podcast");
        assert_eq!(preview["image_url"], "https://example.com/art.jpg");
        assert_eq!(preview["episode_count"], 2);
    }

    #[tokio::test]
    async fn usage_routes_and_records_include_usage_totals() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                "INSERT INTO sources (id, type, config, status) VALUES ('source-usage', 'folder_video', '{}', 'active')",
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO items (
                    id, source_id, content_type, external_id, title, duration_sec,
                    raw_path, indexed_at, status, metadata
                )
                VALUES (
                    'item-usage', 'source-usage', 'video', 'usage.mp4', 'Usage clip',
                    60, '/tmp/usage.mp4', 100, 'indexed', '{}'
                )
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO jobs (id, item_id, job_type, status, started_at, finished_at, progress)
                VALUES ('job-usage', 'item-usage', 'index_video', 'succeeded', 90, 100, 1)
                "#,
                [],
            )
            .unwrap();
        }

        let mut usage = cerul_storage::NewUsageEvent::new("remote", "asr");
        usage.provider_id = Some("env-asr".to_string());
        usage.provider_type = Some("groq".to_string());
        usage.model_id = Some("whisper-large-v3-turbo".to_string());
        usage.item_id = Some("item-usage".to_string());
        usage.job_id = Some("job-usage".to_string());
        usage.audio_seconds = Some(60.0);
        usage.estimated_usd = Some(0.000_666_666_7);
        usage.price_snapshot_id = Some("groq-whisper-large-v3-turbo-2026-05".to_string());
        cerul_storage::record_usage_event(&paths, usage).unwrap();

        let app = router_with_paths(paths);

        let summary = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/usage/summary")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(summary.status(), StatusCode::OK);
        let summary = response_json(summary).await;
        assert_eq!(summary["remote"]["event_count"], 1);
        assert_eq!(summary["remote"]["audio_seconds"], 60.0);
        assert!(
            (summary["remote"]["estimated_usd"].as_f64().unwrap() - 0.000_666_666_7).abs()
                < f64::EPSILON
        );
        assert_eq!(summary["by_capability"][0]["key"], "asr");

        let events = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/usage/events?limit=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(events.status(), StatusCode::OK);
        let events = response_json(events).await;
        assert_eq!(events.as_array().unwrap().len(), 1);
        assert_eq!(events[0]["item_id"], "item-usage");
        assert_eq!(events[0]["job_id"], "job-usage");

        let items = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/items")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(items.status(), StatusCode::OK);
        let items = response_json(items).await;
        assert_eq!(items[0]["usage"]["event_count"], 1);
        assert_eq!(items[0]["usage"]["audio_seconds"], 60.0);

        let jobs = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/jobs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(jobs.status(), StatusCode::OK);
        let jobs = response_json(jobs).await;
        assert_eq!(jobs[0]["usage"]["event_count"], 1);
        assert_eq!(jobs[0]["usage"]["audio_seconds"], 60.0);
    }

    #[tokio::test]
    async fn cors_allows_desktop_frontend_origin() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let app = router_with_paths(paths);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/sources")
                    .header(header::ORIGIN, "http://127.0.0.1:1420")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            Some(&HeaderValue::from_static("*"))
        );
    }

    #[cfg(unix)]
    fn fake_ytdlp(temp: &tempfile::TempDir) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let script = temp.path().join("yt-dlp");
        std::fs::write(
            &script,
            r#"#!/bin/sh
if printf '%s\n' "$@" | grep -q -- '--flat-playlist'; then
  printf '{"id":"abc123","title":"First video","duration":12}\n'
  printf '{"id":"def456","title":"Second video","duration":34}\n'
else
  out=""
  while [ "$#" -gt 0 ]; do
    if [ "$1" = "-o" ]; then
      shift
      out="$1"
    fi
    shift
  done
  mkdir -p "$(dirname "$out")"
  printf 'video' > "$out"
fi
"#,
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();
        script
    }

    fn remote_request(remote_addr: &str, token: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .method(Method::GET)
            .uri("/health")
            .extension(ConnectInfo(
                remote_addr
                    .parse::<SocketAddr>()
                    .expect("valid remote addr"),
            ));

        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }

        builder.body(Body::empty()).unwrap()
    }

    async fn response_json(response: Response) -> Value {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }
}
