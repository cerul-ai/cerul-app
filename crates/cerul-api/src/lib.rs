use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs,
    net::SocketAddr,
    path::{Path as FsPath, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    body::Body,
    extract::{ConnectInfo, Path, Query, Request, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
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
use tokio::io::AsyncSeekExt;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    trace::TraceLayer,
};

mod api_models;
pub mod jobs;
pub mod models;
pub mod providers;
pub mod video_understanding;

const QUERY_EMBEDDING_TIMEOUT: Duration = Duration::from_secs(8);

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
    pub error_info: Option<JobErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobErrorInfo {
    pub code: String,
    pub capability: String,
    pub settings_section: String,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MomentRecord {
    pub id: String,
    pub item_id: String,
    pub chunk_id: Option<String>,
    pub start_sec: Option<f64>,
    pub end_sec: Option<f64>,
    pub timestamp: String,
    pub title: String,
    pub quote: String,
    pub note: Option<String>,
    pub created_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateMomentRequest {
    pub item_id: String,
    pub chunk_id: Option<String>,
    pub start_sec: Option<f64>,
    pub end_sec: Option<f64>,
    pub title: Option<String>,
    pub quote: String,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AskRequest {
    pub q: String,
    pub limit: Option<usize>,
    pub locale: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AskResponse {
    pub answer: String,
    pub citations: Vec<AskCitation>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AskCitation {
    pub chunk_id: String,
    pub item_id: String,
    pub title: String,
    pub timestamp: String,
    pub start_sec: Option<f64>,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitySummary {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub mention_count: usize,
    pub item_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityMention {
    pub entity_id: String,
    pub label: String,
    pub kind: String,
    pub item_id: String,
    pub item_title: String,
    pub chunk_id: Option<String>,
    pub timestamp: String,
    pub start_sec: Option<f64>,
    pub quote: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EntityDetail {
    pub entity: EntitySummary,
    pub mentions: Vec<EntityMention>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WeeklyReviewResponse {
    pub week_start: i64,
    pub indexed_items: usize,
    pub indexed_seconds: f64,
    pub watched_percent: u8,
    pub topics: Vec<WeeklyTopic>,
    pub has_data: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WeeklyTopic {
    pub id: String,
    pub label: String,
    pub count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlaybackPositionRecord {
    pub item_id: String,
    pub position_sec: f64,
    pub timestamp: String,
    pub chunk_id: Option<String>,
    pub updated_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct UpdatePlaybackPositionRequest {
    position_sec: f64,
    chunk_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StorageUsageResponse {
    pub data_dir: String,
    pub total_bytes: u64,
    pub categories: Vec<StorageUsageCategory>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StorageUsageCategory {
    pub key: String,
    pub label: String,
    pub bytes: u64,
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
        .route("/ask", post(ask_library))
        .route("/sources", get(list_sources).post(add_source))
        .route("/sources/preview/rss", post(preview_rss_source))
        .route("/sources/:id", delete(remove_source))
        .route("/sources/:id/pause", post(pause_source))
        .route("/sources/:id/resume", post(resume_source))
        .route("/moments", get(list_moments).post(create_moment))
        .route("/moments/:id", delete(remove_moment))
        .route("/entities", get(list_entities))
        .route("/entities/:id", get(get_entity))
        .route("/weekly-review", get(weekly_review))
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
        .route("/jobs", get(list_jobs))
        .route("/usage/events", get(list_usage_events))
        .route("/usage/summary", get(usage_summary))
        .route("/storage/usage", get(storage_usage))
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
        .route(
            "/providers/:id/models",
            get(providers::discover_provider_models),
        )
        .route("/settings", get(list_settings).patch(update_settings))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_remote_auth,
        ))
        .layer(
            // Browsers enforce CORS per-origin: only the packaged app shell and
            // local dev servers may read responses. Never use `permissive()`
            // here — combined with the loopback auth exemption it would let any
            // website read and mutate the whole library via fetch().
            CorsLayer::new()
                .allow_origin(AllowOrigin::predicate(|origin, _| {
                    origin.to_str().map(browser_origin_allowed).unwrap_or(false)
                }))
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PATCH,
                    Method::PUT,
                    Method::DELETE,
                ])
                .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]),
        )
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
    // Loopback requests skip key auth, so requests that originate from a
    // browser context must prove they come from the app itself: a malicious
    // website always carries its own `Origin`, and a DNS-rebinding page
    // carries a foreign `Host`.
    if let Some(origin) = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
    {
        if !browser_origin_allowed(origin) {
            return forbidden_cross_origin();
        }
    }

    let remote_addr = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0);
    if remote_addr
        .map(|addr| addr.ip().is_loopback())
        .unwrap_or(true)
    {
        if !host_header_allowed(req.headers()) {
            return forbidden_cross_origin();
        }
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

/// Origins allowed to talk to the local API from a browser-like context:
/// the packaged Electron shell (`app://…`) and loopback-hosted dev servers.
fn browser_origin_allowed(origin: &str) -> bool {
    if origin.starts_with("app://") {
        return true;
    }
    let Some(rest) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    else {
        return false;
    };
    loopback_host(rest)
}

/// Reject loopback requests whose `Host` is not a loopback name —
/// the signature of a DNS-rebinding attack.
fn host_header_allowed(headers: &HeaderMap) -> bool {
    match headers.get(header::HOST).and_then(|v| v.to_str().ok()) {
        None => true,
        Some(host) => loopback_host(host),
    }
}

fn loopback_host(host_port: &str) -> bool {
    if host_port == "[::1]" || host_port.starts_with("[::1]:") {
        return true;
    }
    let host = host_port.split(':').next().unwrap_or(host_port);
    matches!(host, "127.0.0.1" | "localhost")
}

fn forbidden_cross_origin() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "error": "cross-origin requests to the Cerul API are not allowed"
        })),
    )
        .into_response()
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
    Json(mut req): Json<cerul_search::SearchRequest>,
) -> ApiResult<Json<Vec<cerul_search::SearchResult>>> {
    // The limit fans out 4x into vector + FTS retrieval and pre-allocates
    // buffers; an unclamped client value is a one-request memory DoS.
    req.limit = req.limit.clamp(1, 50);
    Ok(Json(search_records(&state.paths, req).await?))
}

async fn search_records(
    paths: &AppPaths,
    req: cerul_search::SearchRequest,
) -> anyhow::Result<Vec<cerul_search::SearchResult>> {
    let query = req.q.clone();
    let paths_for_embedding = paths.clone();
    let query_embedding = tokio::time::timeout(
        QUERY_EMBEDDING_TIMEOUT,
        tokio::task::spawn_blocking(move || api_models::embed_query(&paths_for_embedding, &query)),
    )
    .await;

    match query_embedding {
        Ok(Ok(Ok(embedding))) => {
            cerul_search::search_with_vector_for_profile(
                paths,
                req,
                embedding.vector,
                &embedding.profile,
            )
            .await
        }
        Ok(Ok(Err(error))) => {
            tracing::warn!(%error, "API semantic query embedding unavailable; falling back to FTS");
            cerul_search::search_fts_only(paths, req).await
        }
        Ok(Err(error)) => {
            tracing::warn!(%error, "API query embedding task failed; falling back to FTS");
            cerul_search::search_fts_only(paths, req).await
        }
        Err(error) => {
            tracing::warn!(
                %error,
                timeout_sec = QUERY_EMBEDDING_TIMEOUT.as_secs(),
                "API query embedding timed out; falling back to FTS"
            );
            cerul_search::search_fts_only(paths, req).await
        }
    }
}

async fn ask_library(
    State(state): State<ApiState>,
    Json(req): Json<AskRequest>,
) -> ApiResult<Json<AskResponse>> {
    let query = req.q.trim();
    if query.is_empty() {
        return Err(ApiError::bad_request("question cannot be empty"));
    }

    let limit = req.limit.unwrap_or(6).clamp(1, 8);
    let results = search_records(
        &state.paths,
        cerul_search::SearchRequest {
            q: query.to_string(),
            limit,
        },
    )
    .await?;
    let citations = results
        .into_iter()
        .filter(|result| !result.snippet.trim().is_empty())
        .take(limit)
        .map(|result| AskCitation {
            chunk_id: result.chunk_id,
            item_id: result.item_id,
            title: result
                .item_title
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| "Untitled media".to_string()),
            timestamp: format_playback_timestamp(result.start_sec.unwrap_or(0.0)),
            start_sec: result.start_sec,
            snippet: trim_for_answer(&result.snippet, 280),
        })
        .collect::<Vec<_>>();

    let answer_in_english = req
        .locale
        .as_deref()
        .is_some_and(|locale| locale.to_ascii_lowercase().starts_with("en"));
    let answer = if citations.is_empty() {
        if answer_in_english {
            format!(
                "I couldn't find a directly related moment for \"{}\" in the local index. Try another keyword or wait for current indexing jobs to finish.",
                query
            )
        } else {
            format!(
                "没有在本地索引里找到和「{}」直接相关的片段。可以先换一个关键词，或等当前索引任务完成后再问。",
                query
            )
        }
    } else {
        let mut sentences = Vec::new();
        for citation in citations.iter().take(3) {
            if answer_in_english {
                sentences.push(format!(
                    "Around {} in \"{}\", the index says: {}",
                    citation.timestamp, citation.title, citation.snippet
                ));
            } else {
                sentences.push(format!(
                    "在《{}》{} 附近，索引里提到：{}",
                    citation.title, citation.timestamp, citation.snippet
                ));
            }
        }
        if answer_in_english {
            format!(
                "{} This answer is grounded only in the local search hits below, and each citation can jump back to the original moment.",
                sentences.join(" ")
            )
        } else {
            format!(
                "{} 这不是云端幻觉式回答；它只基于当前本地检索到的片段生成，下面每条引用都能跳回原始时刻。",
                sentences.join(" ")
            )
        }
    };

    Ok(Json(AskResponse { answer, citations }))
}

async fn list_moments(State(state): State<ApiState>) -> ApiResult<Json<Vec<MomentRecord>>> {
    Ok(Json(read_moments(&state.paths)?))
}

async fn create_moment(
    State(state): State<ApiState>,
    Json(req): Json<CreateMomentRequest>,
) -> ApiResult<Json<MomentRecord>> {
    let quote = req.quote.trim();
    if quote.is_empty() {
        return Err(ApiError::bad_request("quote cannot be empty"));
    }

    let conn = cerul_storage::sqlite::open(&state.paths)?;
    let item_title: Option<String> = conn
        .query_row(
            "SELECT title FROM items WHERE id = ?1",
            [req.item_id.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    let Some(item_title) = item_title else {
        return Err(ApiError::not_found(format!(
            "item not found: {}",
            req.item_id
        )));
    };

    if let Some(chunk_id) = req
        .chunk_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let chunk_exists: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM chunks WHERE id = ?1 AND item_id = ?2",
                (chunk_id, req.item_id.as_str()),
                |row| row.get(0),
            )
            .optional()?;
        if chunk_exists.is_none() {
            return Err(ApiError::bad_request("chunk does not belong to item"));
        }
    }

    let id = new_id("moment");
    let title = req
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(item_title.as_str());
    conn.execute(
        r#"
        INSERT INTO moments (id, item_id, chunk_id, start_sec, end_sec, title, quote, note)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        (
            id.as_str(),
            req.item_id.as_str(),
            req.chunk_id
                .as_deref()
                .filter(|value| !value.trim().is_empty()),
            req.start_sec,
            req.end_sec,
            title,
            quote,
            req.note.as_deref().filter(|value| !value.trim().is_empty()),
        ),
    )?;

    read_moment(&state.paths, &id)?
        .map(Json)
        .ok_or_else(|| ApiError::internal(anyhow::anyhow!("moment was not recorded")))
}

async fn remove_moment(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let conn = cerul_storage::sqlite::open(&state.paths)?;
    let removed = conn.execute("DELETE FROM moments WHERE id = ?1", [id.as_str()])?;
    if removed != 1 {
        return Err(ApiError::not_found(format!("moment not found: {id}")));
    }
    Ok(Json(json!({ "status": "removed", "id": id })))
}

async fn list_entities(State(state): State<ApiState>) -> ApiResult<Json<Vec<EntitySummary>>> {
    let mentions = collect_entity_mentions(&state.paths)?;
    Ok(Json(entity_summaries(&mentions)))
}

async fn get_entity(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> ApiResult<Json<EntityDetail>> {
    let mentions = collect_entity_mentions(&state.paths)?;
    let mut summaries = entity_summaries(&mentions);
    let Some(entity) = summaries.iter_mut().find(|entity| entity.id == id).cloned() else {
        return Err(ApiError::not_found(format!("entity not found: {id}")));
    };
    let entity_mentions = mentions
        .into_iter()
        .filter(|mention| mention.entity_id == id)
        .collect::<Vec<_>>();

    Ok(Json(EntityDetail {
        entity,
        mentions: entity_mentions,
    }))
}

async fn weekly_review(State(state): State<ApiState>) -> ApiResult<Json<WeeklyReviewResponse>> {
    Ok(Json(weekly_review_for_paths(&state.paths)?))
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
        if !FsPath::new(trimmed).is_file() {
            return Err(ApiError::bad_request(format!("file not found: {trimmed}")));
        }
        let conn = cerul_storage::sqlite::open(&state.paths)?;
        let updated = conn.execute(
            "UPDATE items SET raw_path = ?2, error = NULL WHERE id = ?1",
            rusqlite::params![id.as_str(), trimmed],
        )?;
        if updated == 0 {
            return Err(ApiError::not_found("item not found"));
        }
    }
    get_item(State(state), Path(id)).await
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
    if let Err(error) = cerul_storage::vectors::delete_item_embeddings(paths, &item.id).await {
        tracing::warn!(
            item_id = %item.id,
            %error,
            "failed to delete item embeddings; continuing item cleanup"
        );
    }
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
    // Downloaded source media (youtube / web_video / rss enclosures) lives in
    // the cache too and used to be left behind forever after deletion. Only
    // delete raw_path when it actually sits inside our cache directory —
    // never a user's own file.
    if matches!(
        item.source_type.as_str(),
        "youtube" | "web_video" | "rss_podcast"
    ) {
        if let Some(raw_path) = item.raw_path.as_deref() {
            let raw = FsPath::new(raw_path);
            if raw.starts_with(&paths.cache) {
                remove_file_if_exists(raw.to_path_buf()).await?;
            }
        }
    }
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
    let mut usage_by_job = cerul_storage::usage_totals_by_job(&state.paths).unwrap_or_default();
    let rows = stmt.query_map([], |row| {
        let job_id: String = row.get(0)?;
        let job_type: String = row.get(2)?;
        let error: Option<String> = row.get(6)?;
        Ok(JobRecord {
            id: job_id.clone(),
            item_id: row.get(1)?,
            job_type: job_type.clone(),
            status: row.get(3)?,
            started_at: row.get(4)?,
            finished_at: row.get(5)?,
            error: error.clone(),
            progress: row.get(7)?,
            stage: row.get(8)?,
            stage_message: row.get(9)?,
            usage: cerul_storage::UsageTotals::default(),
            error_info: error
                .as_deref()
                .and_then(|message| classify_job_error(&job_type, message)),
        })
    })?;

    let mut jobs = rows.collect::<Result<Vec<_>, _>>()?;
    for job in &mut jobs {
        job.usage = usage_by_job.remove(&job.id).unwrap_or_default();
    }
    Ok(Json(jobs))
}

async fn usage_summary(
    State(state): State<ApiState>,
) -> ApiResult<Json<cerul_storage::UsageSummary>> {
    Ok(Json(cerul_storage::usage_summary(&state.paths)?))
}

async fn storage_usage(State(state): State<ApiState>) -> ApiResult<Json<StorageUsageResponse>> {
    Ok(Json(storage_usage_for_paths(&state.paths)?))
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

fn read_moments(paths: &AppPaths) -> anyhow::Result<Vec<MomentRecord>> {
    let conn = cerul_storage::sqlite::open(paths)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT m.id, m.item_id, m.chunk_id, m.start_sec, m.end_sec,
               COALESCE(NULLIF(m.title, ''), i.title, 'Untitled media') AS title,
               m.quote, m.note, m.created_at
        FROM moments m
        JOIN items i ON i.id = m.item_id
        ORDER BY m.created_at DESC, m.id DESC
        "#,
    )?;
    let rows = stmt.query_map([], moment_from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn read_moment(paths: &AppPaths, id: &str) -> anyhow::Result<Option<MomentRecord>> {
    let conn = cerul_storage::sqlite::open(paths)?;
    conn.query_row(
        r#"
        SELECT m.id, m.item_id, m.chunk_id, m.start_sec, m.end_sec,
               COALESCE(NULLIF(m.title, ''), i.title, 'Untitled media') AS title,
               m.quote, m.note, m.created_at
        FROM moments m
        JOIN items i ON i.id = m.item_id
        WHERE m.id = ?1
        "#,
        [id],
        moment_from_row,
    )
    .optional()
    .map_err(Into::into)
}

fn moment_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MomentRecord> {
    let start_sec: Option<f64> = row.get(3)?;
    Ok(MomentRecord {
        id: row.get(0)?,
        item_id: row.get(1)?,
        chunk_id: row.get(2)?,
        start_sec,
        end_sec: row.get(4)?,
        timestamp: format_playback_timestamp(start_sec.unwrap_or(0.0)),
        title: row.get(5)?,
        quote: row.get(6)?,
        note: row.get(7)?,
        created_at: row.get(8)?,
    })
}

fn collect_entity_mentions(paths: &AppPaths) -> anyhow::Result<Vec<EntityMention>> {
    let conn = cerul_storage::sqlite::open(paths)?;
    let mut mentions = Vec::new();

    let mut understanding_stmt = conn.prepare(
        r#"
        SELECT iu.item_id, COALESCE(i.title, 'Untitled media'), iu.result
        FROM item_understandings iu
        JOIN items i ON i.id = iu.item_id
        WHERE iu.status = 'completed'
        ORDER BY COALESCE(i.indexed_at, 0) DESC, iu.item_id ASC
        "#,
    )?;
    let understanding_rows = understanding_stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for row in understanding_rows {
        let (item_id, item_title, raw_result) = row?;
        let result = parse_json(&raw_result);
        if let Some(topics) = result.get("topics").and_then(Value::as_array) {
            for topic in topics {
                if let Some(label) = topic.as_str() {
                    push_entity_mention(
                        &mut mentions,
                        label,
                        "topic",
                        &item_id,
                        &item_title,
                        None,
                        None,
                        label,
                    );
                }
            }
        }
        if let Some(events) = result.get("events").and_then(Value::as_array) {
            for event in events {
                let start_sec = event.get("start_sec").and_then(Value::as_f64);
                let quote = event
                    .get("caption")
                    .and_then(Value::as_str)
                    .or_else(|| event.get("visual").and_then(Value::as_str))
                    .unwrap_or("")
                    .trim();
                if let Some(entities) = event.get("entities").and_then(Value::as_array) {
                    for entity in entities {
                        if let Some(label) = entity.as_str() {
                            push_entity_mention(
                                &mut mentions,
                                label,
                                "person_or_entity",
                                &item_id,
                                &item_title,
                                None,
                                start_sec,
                                if quote.is_empty() { label } else { quote },
                            );
                        }
                    }
                }
            }
        }
    }

    let mut chunk_stmt = conn.prepare(
        r#"
        SELECT c.id, c.item_id, COALESCE(i.title, 'Untitled media'), c.start_sec, c.text
        FROM chunks c
        JOIN items i ON i.id = c.item_id
        WHERE c.text IS NOT NULL
          AND c.chunk_type IN ('transcript', 'transcript_line', 'understanding', 'ocr')
        ORDER BY COALESCE(i.indexed_at, 0) DESC, COALESCE(c.start_sec, 0), c.id
        LIMIT 1000
        "#,
    )?;
    let chunk_rows = chunk_stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<f64>>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    for row in chunk_rows {
        let (chunk_id, item_id, item_title, start_sec, text) = row?;
        for label in extract_candidate_entities(&text).into_iter().take(4) {
            let kind = entity_kind(&label);
            push_entity_mention(
                &mut mentions,
                &label,
                kind,
                &item_id,
                &item_title,
                Some(&chunk_id),
                start_sec,
                &text,
            );
        }
    }

    let mut seen = BTreeSet::new();
    mentions.retain(|mention| {
        seen.insert(format!(
            "{}:{}:{}",
            mention.entity_id,
            mention.item_id,
            mention.chunk_id.as_deref().unwrap_or("")
        ))
    });
    Ok(mentions)
}

#[allow(clippy::too_many_arguments)]
fn push_entity_mention(
    mentions: &mut Vec<EntityMention>,
    label: &str,
    kind: &str,
    item_id: &str,
    item_title: &str,
    chunk_id: Option<&str>,
    start_sec: Option<f64>,
    quote: &str,
) {
    let Some(label) = normalize_entity_label(label) else {
        return;
    };
    let entity_id = entity_slug(&label);
    if entity_id.is_empty() {
        return;
    }
    mentions.push(EntityMention {
        entity_id,
        label,
        kind: kind.to_string(),
        item_id: item_id.to_string(),
        item_title: item_title.to_string(),
        chunk_id: chunk_id.map(ToString::to_string),
        timestamp: format_playback_timestamp(start_sec.unwrap_or(0.0)),
        start_sec,
        quote: trim_for_answer(quote, 220),
    });
}

fn entity_summaries(mentions: &[EntityMention]) -> Vec<EntitySummary> {
    let mut by_id: BTreeMap<String, (String, usize, BTreeSet<String>)> = BTreeMap::new();
    for mention in mentions {
        let entry = by_id
            .entry(mention.entity_id.clone())
            .or_insert_with(|| (mention.label.clone(), 0, BTreeSet::<String>::new()));
        entry.1 += 1;
        entry.2.insert(mention.item_id.clone());
    }
    let mut summaries = by_id
        .into_iter()
        .map(|(id, (label, mention_count, item_ids))| EntitySummary {
            kind: entity_kind(&label).to_string(),
            id,
            label,
            mention_count,
            item_count: item_ids.len(),
        })
        .collect::<Vec<_>>();
    summaries.sort_by(|left, right| {
        right
            .mention_count
            .cmp(&left.mention_count)
            .then_with(|| left.label.cmp(&right.label))
    });
    summaries.truncate(30);
    summaries
}

fn weekly_review_for_paths(paths: &AppPaths) -> anyhow::Result<WeeklyReviewResponse> {
    let now = current_unix_seconds();
    let week_start = now - 7 * 24 * 60 * 60;
    let conn = cerul_storage::sqlite::open(paths)?;
    let (indexed_items, indexed_seconds, watched_seconds): (i64, f64, f64) = conn.query_row(
        r#"
        SELECT COUNT(*),
               COALESCE(SUM(duration_sec), 0),
               COALESCE(SUM(
                 MIN(
                   COALESCE(json_extract(metadata, '$.playback_position.position_sec'), 0),
                   COALESCE(duration_sec, 0)
                 )
               ), 0)
        FROM items
        WHERE indexed_at IS NOT NULL
          AND indexed_at >= ?1
        "#,
        [week_start],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let watched_percent = if indexed_seconds > 0.0 {
        ((watched_seconds / indexed_seconds) * 100.0)
            .round()
            .clamp(0.0, 100.0) as u8
    } else {
        0
    };
    let current_week_item_ids = conn
        .prepare(
            r#"
            SELECT id
            FROM items
            WHERE indexed_at IS NOT NULL
              AND indexed_at >= ?1
            "#,
        )?
        .query_map([week_start], |row| row.get::<_, String>(0))?
        .collect::<Result<HashSet<_>, _>>()?;
    let current_week_mentions = collect_entity_mentions(paths)?
        .into_iter()
        .filter(|mention| current_week_item_ids.contains(&mention.item_id))
        .collect::<Vec<_>>();
    let topics = entity_summaries(&current_week_mentions)
        .into_iter()
        .take(3)
        .map(|entity| WeeklyTopic {
            id: entity.id,
            label: entity.label,
            count: entity.mention_count,
        })
        .collect::<Vec<_>>();

    Ok(WeeklyReviewResponse {
        week_start,
        indexed_items: indexed_items.max(0) as usize,
        indexed_seconds,
        watched_percent,
        has_data: indexed_items > 0 || !topics.is_empty(),
        topics,
    })
}

fn classify_job_error(job_type: &str, message: &str) -> Option<JobErrorInfo> {
    let normalized = message.to_ascii_lowercase();
    let capability = capability_for_job_type(job_type).to_string();
    let (code, friendly) = if normalized.contains("api key")
        || normalized.contains("missing key")
        || normalized.contains("no key")
        || normalized.contains("unauthorized")
        || normalized.contains("401")
    {
        (
            "missing_api_key",
            format!("{capability} 连接缺少可用 API 密钥。"),
        )
    } else if normalized.contains("model")
        && (normalized.contains("not found")
            || normalized.contains("does not exist")
            || normalized.contains("unsupported")
            || normalized.contains("404"))
    {
        (
            "model_not_found",
            format!("{capability} 当前选择的模型不可用，请换一个模型或连接。"),
        )
    } else if normalized.contains("ffmpeg") {
        (
            "ffmpeg_unavailable",
            "本机视频处理运行时不可用，需要修复本地工具链。".to_string(),
        )
    } else if normalized.contains("yt-dlp")
        || normalized.contains("video unavailable")
        || normalized.contains("private")
        || normalized.contains("geo")
    {
        (
            "source_unavailable",
            "来源暂时不可访问，可能是私有、地区限制或下载器失效。".to_string(),
        )
    } else if normalized.trim().is_empty() {
        return None;
    } else {
        (
            "unknown_processing_error",
            format!("{capability} 处理失败，需要查看技术详情。"),
        )
    };

    Some(JobErrorInfo {
        code: code.to_string(),
        capability,
        settings_section: "Models".to_string(),
        message: friendly,
    })
}

fn capability_for_job_type(job_type: &str) -> &'static str {
    match job_type {
        "index_audio" => "转录",
        "index_image" => "图像索引",
        "index_video" => "视频索引",
        _ => "索引",
    }
}

fn extract_candidate_entities(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let lower = text.to_ascii_lowercase();
    for phrase in [
        "test-time compute",
        "retrieval quality",
        "media memory",
        "semantic retrieval",
        "video understanding",
        "prompt engineering",
        "agent",
        "agents",
    ] {
        if lower.contains(phrase) {
            out.push(phrase.to_string());
        }
    }

    let words = text
        .split(|ch: char| !(ch.is_alphanumeric() || ch == '-' || ch == '\''))
        .filter(|word| word.len() > 1)
        .collect::<Vec<_>>();
    let mut current = Vec::new();
    for word in words {
        if word
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_uppercase())
            && !matches!(word, "I" | "The" | "This" | "That" | "And" | "But")
        {
            current.push(word);
            if current.len() >= 4 {
                out.push(current.join(" "));
                current.clear();
            }
        } else {
            if current.len() >= 2 {
                out.push(current.join(" "));
            }
            current.clear();
        }
    }
    if current.len() >= 2 {
        out.push(current.join(" "));
    }

    let mut seen = BTreeSet::new();
    out.into_iter()
        .filter_map(|label| normalize_entity_label(&label))
        .filter(|label| seen.insert(label.to_ascii_lowercase()))
        .take(12)
        .collect()
}

fn normalize_entity_label(label: &str) -> Option<String> {
    let cleaned = label
        .trim()
        .trim_matches(|ch: char| !ch.is_alphanumeric() && ch != '-' && ch != ' ')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if cleaned.len() < 3 || cleaned.len() > 80 {
        return None;
    }
    let lower = cleaned.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "the" | "and" | "this" | "that" | "with" | "from" | "your" | "you"
    ) {
        return None;
    }
    Some(cleaned)
}

fn entity_slug(label: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in label.chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            slug.push(ch);
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}

fn entity_kind(label: &str) -> &'static str {
    if label
        .split_whitespace()
        .next()
        .and_then(|word| word.chars().next())
        .is_some_and(|ch| ch.is_ascii_uppercase())
    {
        "person_or_entity"
    } else {
        "topic"
    }
}

fn trim_for_answer(value: &str, max_chars: usize) -> String {
    let text = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= max_chars {
        return text;
    }
    let mut out = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    out.push('…');
    out
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

    let all = rows.collect::<Result<BTreeMap<_, _>, _>>()?;
    let remote_key_set = all
        .get("remote_api_key")
        .and_then(|value| value.as_str())
        .map(|key| !key.trim().is_empty())
        .unwrap_or(false);

    let mut visible: BTreeMap<String, Value> = all
        .into_iter()
        .filter(|(key, _)| !is_hidden_setting(key))
        .map(|(key, value)| {
            let value = normalize_setting_value(&key, value);
            (key, value)
        })
        .collect();
    // The key itself is write-only; expose only whether one is configured.
    visible.insert(
        "remote_api_key_set".to_string(),
        Value::Bool(remote_key_set),
    );

    Ok(Json(visible))
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

fn is_secret_setting(key: &str) -> bool {
    SECRET_SETTING_KEYS.contains(&key)
}

fn is_hidden_setting(key: &str) -> bool {
    is_legacy_cloud_setting(key) || is_internal_setting(key) || is_secret_setting(key)
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
        .unwrap_or_else(|| "auto".to_string()))
}

fn normalize_inference_mode(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" => "auto".to_string(),
        "local" => "local".to_string(),
        _ => "remote".to_string(),
    }
}

fn sync_inference_mode_side_effects(
    paths: &AppPaths,
    previous_mode: &str,
    next_mode: &str,
) -> anyhow::Result<()> {
    let previous_mode = normalize_inference_mode(previous_mode);
    let next_mode = normalize_inference_mode(next_mode);
    let runtime = models::model_runtime_status(paths);
    let previous_effective = effective_inference_mode_for_runtime(&previous_mode, &runtime);
    let next_effective = effective_inference_mode_for_runtime(&next_mode, &runtime);
    cerul_storage::vectors::ensure_embedding_profile_for_inference_mode(paths, &next_effective)?;
    if next_effective != "local" {
        api_models::shutdown_local_query_sidecar();
    }

    let deferred_mode = setting_string(paths, DEFERRED_EMBEDDING_REBUILD_MODE_SETTING)?
        .as_deref()
        .map(normalize_inference_mode);
    let has_deferred_rebuild = deferred_mode.as_deref() == Some(next_mode.as_str());
    if previous_mode == next_mode && previous_effective == next_effective && !has_deferred_rebuild {
        return Ok(());
    }

    if next_mode == "local" && !runtime.local_runtime_ready {
        set_deferred_embedding_rebuild_mode(paths, &next_mode)?;
        tracing::warn!(
            previous_mode,
            next_mode,
            local_runtime_error = ?runtime.local_runtime_error,
            "local-only inference mode selected but runtime is not ready; deferred embedding profile rebuild"
        );
        return Ok(());
    }

    if next_mode == "auto" && !runtime.local_runtime_ready {
        set_deferred_embedding_rebuild_mode(paths, &next_mode)?;
        if previous_effective == next_effective && !has_deferred_rebuild {
            return Ok(());
        }
    }

    let (rebuild_items, queued_jobs) = queue_items_for_embedding_mode_rebuild(paths)?;
    if next_mode == "auto" && !runtime.local_runtime_ready {
        set_deferred_embedding_rebuild_mode(paths, &next_mode)?;
    } else {
        clear_deferred_embedding_rebuild_mode(paths)?;
    }
    tracing::info!(
        previous_mode,
        next_mode,
        previous_effective,
        next_effective,
        rebuild_items,
        queued_jobs,
        "inference mode changed; queued items for embedding profile rebuild"
    );
    Ok(())
}

fn effective_inference_mode_for_runtime(
    mode: &str,
    runtime: &models::ModelRuntimeStatus,
) -> String {
    match normalize_inference_mode(mode).as_str() {
        "local" => "local".to_string(),
        "auto" if runtime.local_runtime_ready => "local".to_string(),
        _ => "remote".to_string(),
    }
}

pub(crate) fn sync_deferred_embedding_rebuild_if_ready(
    paths: &AppPaths,
    runtime: &models::ModelRuntimeStatus,
) -> anyhow::Result<()> {
    if !runtime.local_runtime_ready {
        return Ok(());
    }

    let inference_mode = configured_inference_mode(paths)?;
    if inference_mode != "local" && inference_mode != "auto" {
        return Ok(());
    }

    let deferred_mode = setting_string(paths, DEFERRED_EMBEDDING_REBUILD_MODE_SETTING)?
        .as_deref()
        .map(normalize_inference_mode);
    if deferred_mode.as_deref() != Some(inference_mode.as_str()) {
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
    // Single GROUP BY query; per-item lookups opened one SQLite connection
    // per row and made GET /items O(n) connections.
    let mut totals = cerul_storage::usage_totals_by_item(paths).unwrap_or_default();
    for item in items {
        item.usage = totals.remove(&item.id).unwrap_or_default();
    }
}

fn playback_position_from_metadata(item_id: &str, metadata: &Value) -> PlaybackPositionRecord {
    let position = metadata.get("playback_position").unwrap_or(&Value::Null);
    let position_sec = position
        .get("position_sec")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(0.0);
    let timestamp = position
        .get("timestamp")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format_playback_timestamp(position_sec));
    let chunk_id = position
        .get("chunk_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string);
    let updated_at = position.get("updated_at").and_then(Value::as_i64);

    PlaybackPositionRecord {
        item_id: item_id.to_string(),
        position_sec,
        timestamp,
        chunk_id,
        updated_at,
    }
}

fn format_playback_timestamp(position_sec: f64) -> String {
    let total_seconds = position_sec.max(0.0).floor() as u64;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
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

fn storage_usage_for_paths(paths: &AppPaths) -> anyhow::Result<StorageUsageResponse> {
    let total_bytes = path_size(&paths.data)?;
    let database_bytes = file_size(&paths.db)?;
    let models_bytes = path_size(&paths.models)?;
    let index_bytes = path_size(&paths.qdrant)?;
    let cache_bytes = path_size(&paths.cache)?;
    let known_bytes = database_bytes
        .saturating_add(models_bytes)
        .saturating_add(index_bytes)
        .saturating_add(cache_bytes);
    let other_bytes = total_bytes.saturating_sub(known_bytes);

    Ok(StorageUsageResponse {
        data_dir: paths.data.to_string_lossy().to_string(),
        total_bytes,
        categories: vec![
            storage_category("database", "Database", database_bytes),
            storage_category("models", "Models", models_bytes),
            storage_category("index", "Search index", index_bytes),
            storage_category("cache", "Cache", cache_bytes),
            storage_category("other", "Other", other_bytes),
        ],
    })
}

fn storage_category(key: &str, label: &str, bytes: u64) -> StorageUsageCategory {
    StorageUsageCategory {
        key: key.to_string(),
        label: label.to_string(),
        bytes,
    }
}

fn file_size(path: &FsPath) -> anyhow::Result<u64> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(metadata.len()),
        Ok(metadata) if metadata.is_dir() => path_size(path),
        Ok(_) => Ok(0),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error.into()),
    }
}

fn path_size(path: &FsPath) -> anyhow::Result<u64> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => return Ok(metadata.len()),
        Ok(metadata) if !metadata.is_dir() => return Ok(0),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.into()),
    }

    let mut total = 0_u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(current)? {
            let entry = entry?;
            let metadata = fs::symlink_metadata(entry.path())?;
            if metadata.is_dir() {
                stack.push(entry.path());
            } else if metadata.is_file() {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    Ok(total)
}

fn current_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

async fn video_file_response(path: &str, range: Option<&HeaderValue>) -> ApiResult<Response> {
    let mut file = tokio::fs::File::open(path).await?;
    let len = file.metadata().await?.len();
    let content_type = video_content_type(path);

    match parse_byte_range(range, len) {
        Ok(Some((start, end))) => {
            let byte_count = end - start + 1;
            // Stream instead of buffering: a wide range used to allocate the
            // whole span (potentially gigabytes) in memory.
            file.seek(std::io::SeekFrom::Start(start)).await?;
            let stream =
                tokio_util::io::ReaderStream::new(tokio::io::AsyncReadExt::take(file, byte_count));

            let mut response =
                (StatusCode::PARTIAL_CONTENT, Body::from_stream(stream)).into_response();
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
            let stream = tokio_util::io::ReaderStream::new(file);
            let mut response = Body::from_stream(stream).into_response();
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
    use std::sync::atomic::{AtomicU64, Ordering};
    // Timestamp alone can collide when ids are minted in a tight loop
    // (same-nanosecond inserts abort the whole batch on the PRIMARY KEY).
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{nanos:x}-{seq:x}")
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
    ("/ask", &["post"]),
    ("/sources", &["get", "post"]),
    ("/sources/preview/rss", &["post"]),
    ("/sources/{id}", &["delete"]),
    ("/sources/{id}/pause", &["post"]),
    ("/sources/{id}/resume", &["post"]),
    ("/moments", &["get", "post"]),
    ("/moments/{id}", &["delete"]),
    ("/entities", &["get"]),
    ("/entities/{id}", &["get"]),
    ("/weekly-review", &["get"]),
    ("/items", &["get"]),
    ("/items/{id}", &["get", "delete"]),
    ("/items/{id}/playback", &["get", "patch"]),
    ("/items/{id}/reindex", &["post"]),
    ("/items/{id}/chunks", &["get"]),
    ("/items/{id}/understanding", &["get", "post"]),
    ("/chunks/{id}/frame", &["get"]),
    ("/chunks/{id}/video-segment", &["get"]),
    ("/chunks/{id}/video-clip", &["get"]),
    ("/jobs", &["get"]),
    ("/usage/events", &["get"]),
    ("/usage/summary", &["get"]),
    ("/storage/usage", &["get"]),
    ("/models/catalog", &["get"]),
    ("/models/whisper", &["get"]),
    ("/models/whisper/{id}/download", &["post"]),
    ("/models/whisper/auto-download-status", &["get"]),
    ("/models/embed/status", &["get"]),
    ("/models/embed/prepare", &["post"]),
    ("/providers", &["get", "post"]),
    ("/providers/{id}", &["patch", "delete"]),
    ("/providers/{id}/test", &["post"]),
    ("/providers/{id}/models", &["get"]),
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
    // Computed flag returned by list_settings; never persisted.
    "remote_api_key_set",
];
/// Settings that clients may write but must never read back in plaintext.
const SECRET_SETTING_KEYS: &[&str] = &["remote_api_key"];

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::{Method, Request},
    };
    use tower::ServiceExt;

    fn seed_indexing_schema_version(paths: &AppPaths) {
        let conn = cerul_storage::sqlite::open(paths).unwrap();
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
        )
        .unwrap();
    }

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
    async fn router_serves_model_catalog_with_remote_profile() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                r#"
                INSERT INTO settings (key, value, updated_at)
                VALUES ('inference_mode', '"remote"', strftime('%s','now'))
                "#,
                [],
            )
            .unwrap();
        }
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

        let models_without_key = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!(
                        "/providers/{}/models",
                        created_json["id"].as_str().unwrap()
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(models_without_key.status(), StatusCode::BAD_REQUEST);

        let local_models = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/providers/local/models")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(local_models.status(), StatusCode::BAD_REQUEST);

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

        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                r#"
                INSERT INTO settings (key, value, updated_at)
                VALUES ('asr_model', ?1, strftime('%s','now'))
                ON CONFLICT(key) DO UPDATE SET
                    value = excluded.value,
                    updated_at = excluded.updated_at
                "#,
                [Value::String("gemini-2.5-flash".to_string()).to_string()],
            )
            .unwrap();
        }

        let gateway_named_model_info = crate::api_models::routed_transcriber(paths.clone())
            .inference_provider()
            .unwrap();
        assert_eq!(
            gateway_named_model_info.provider_id.as_deref(),
            asr_provider["id"].as_str()
        );
        assert_eq!(
            gateway_named_model_info.model_id.as_deref(),
            Some("gemini-2.5-flash")
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
        seed_indexing_schema_version(&paths);
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
        let reindex_status = reindex.status();
        let reindex = response_json(reindex).await;
        assert_eq!(reindex_status, StatusCode::OK, "reindex failed: {reindex}");
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
        let delete_status = delete.status();
        let delete = response_json(delete).await;
        assert_eq!(delete_status, StatusCode::OK, "delete failed: {delete}");

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
    async fn item_playback_position_persists_in_metadata() {
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
        }
        let app = router_with_paths(paths.clone());

        let update = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri("/items/item-1/playback")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({ "position_sec": 75.4, "chunk_id": "chunk-1" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(update.status(), StatusCode::OK);
        let update = response_json(update).await;
        assert_eq!(update["position_sec"], 75.4);
        assert_eq!(update["timestamp"], "1:15");
        assert_eq!(update["chunk_id"], "chunk-1");
        assert!(update["updated_at"].as_i64().unwrap() > 0);

        let get = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/items/item-1/playback")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get.status(), StatusCode::OK);
        let get = response_json(get).await;
        assert_eq!(get["timestamp"], "1:15");

        let items = app
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
        assert_eq!(
            items[0]["metadata"]["playback_position"]["timestamp"],
            "1:15"
        );
    }

    #[tokio::test]
    async fn storage_usage_reports_data_directory_breakdown() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let _ = cerul_storage::sqlite::open(&paths).unwrap();
        std::fs::write(paths.models.join("model.bin"), b"model").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            paths.models.join("model.bin"),
            paths.models.join("snapshot.bin"),
        )
        .unwrap();
        std::fs::write(paths.cache.join("cache.bin"), b"cache-data").unwrap();
        std::fs::write(paths.qdrant.join("index.bin"), b"idx").unwrap();
        let app = router_with_paths(paths);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/storage/usage")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let usage = response_json(response).await;
        assert!(usage["total_bytes"].as_u64().unwrap() >= 18);
        let categories = usage["categories"].as_array().unwrap();
        let bytes_for = |key: &str| {
            categories
                .iter()
                .find(|category| category["key"] == key)
                .and_then(|category| category["bytes"].as_u64())
                .unwrap()
        };
        assert_eq!(bytes_for("models"), 5);
        assert_eq!(bytes_for("cache"), 10);
        assert_eq!(bytes_for("index"), 3);
        assert!(bytes_for("database") > 0);
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
        seed_indexing_schema_version(&paths);
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
        std::env::set_var("CERUL_ALLOW_LOCAL_FEEDS", "1");
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
        std::env::set_var("CERUL_ALLOW_LOCAL_FEEDS", "1");
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
            Some(&HeaderValue::from_static("http://127.0.0.1:1420"))
        );
    }

    #[tokio::test]
    async fn cors_blocks_foreign_web_origins() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let app = router_with_paths(paths);

        // Preflight from a malicious website must not be granted CORS.
        let preflight = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/sources")
                    .header(header::ORIGIN, "https://evil.example")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(preflight
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none());

        // Simple (no-preflight) requests carrying a foreign Origin are
        // rejected outright, even from loopback.
        let simple = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/items")
                    .header(header::ORIGIN, "https://evil.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(simple.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn settings_never_return_remote_api_key_plaintext() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                r#"INSERT INTO settings (key, value, updated_at)
                   VALUES ('remote_api_key', '"super-secret"', strftime('%s','now'))"#,
                [],
            )
            .unwrap();
        }
        let app = router_with_paths(paths);

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
        assert!(settings.get("remote_api_key").is_none());
        assert_eq!(settings["remote_api_key_set"], true);
    }

    #[cfg(unix)]
    fn fake_ytdlp(temp: &tempfile::TempDir) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let script = temp.path().join("yt-dlp");
        std::fs::write(
            &script,
            r#"#!/bin/sh
for arg in "$@"; do
  if [ "$arg" = "--flat-playlist" ]; then
  printf '{"id":"abc123","title":"First video","duration":12}\n'
  printf '{"id":"def456","title":"Second video","duration":34}\n'
  exit 0
  fi
done
out=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "-o" ]; then
    shift
    out="$1"
  fi
  shift
done
if [ -z "$out" ]; then
  exit 1
fi
mkdir -p "$(dirname "$out")"
printf 'video' > "$out"
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
