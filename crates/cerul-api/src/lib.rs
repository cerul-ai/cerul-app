#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{self, Write},
    net::SocketAddr,
    path::{Path as FsPath, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
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
use rusqlite::{types::Value as SqlValue, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::AsyncSeekExt;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    trace::TraceLayer,
};
use tracing_subscriber::fmt::MakeWriter;

mod api_models;
pub mod jobs;
pub mod local_models;
pub mod local_runtime;
pub mod models;
pub mod providers;
pub mod video_understanding;

const QUERY_EMBEDDING_TIMEOUT: Duration = Duration::from_secs(8);
const DEFAULT_LIST_LIMIT: usize = 250;
const MAX_LIST_LIMIT: usize = 1_000;
const CORE_LOG_FILE: &str = "cerul-core.log";
const DEFAULT_API_PORT: u16 = 23785;
const API_PORT_SETTING: &str = "api_port";
const API_ENDPOINT_FILE: &str = "endpoint.json";

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
    pub raw_path_exists: Option<bool>,
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
    /// Symmetric padding (legacy / fallback). Used for both sides when
    /// before_sec/after_sec are absent.
    padding_sec: Option<f64>,
    /// Seconds to extend before the chunk start (overrides padding_sec).
    before_sec: Option<f64>,
    /// Seconds to extend after the chunk end (overrides padding_sec).
    after_sec: Option<f64>,
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
    pub playback_chunk_id: String,
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
    pub total_apparent_bytes: u64,
    pub categories: Vec<StorageUsageCategory>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StorageLocationsResponse {
    pub data_dir: String,
    pub database_path: String,
    pub models_dir: String,
    pub index_dir: String,
    pub cache_dir: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StorageUsageCategory {
    pub key: String,
    pub label: String,
    pub bytes: u64,
    pub apparent_bytes: u64,
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
    if let Err(error) = repair_indexed_item_status_from_artifacts(&paths) {
        tracing::warn!(%error, "failed to repair indexed item status from artifacts");
    }
    if let Err(error) = sync_indexing_schema_side_effects(&paths) {
        tracing::warn!(%error, "failed to sync indexing schema side effects");
    }
    let state = ApiState { paths };

    let internal_routes = Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics))
        .route("/openapi.json", get(openapi_json))
        .route("/diagnostics", get(diagnostics_bundle))
        .route("/diagnostics/indexing", get(indexing_diagnostics))
        .route("/search", post(search))
        .route("/search/diagnostics", get(search_diagnostics))
        .route("/search/rebuild", post(rebuild_search_index))
        .route("/ask", post(ask_library))
        .route("/sources", get(list_sources).post(add_source))
        .route("/sources/preview/rss", post(preview_rss_source))
        .route("/sources/:id", delete(remove_source))
        .route("/sources/:id/pause", post(pause_source))
        .route("/sources/:id/resume", post(resume_source))
        .route("/sources/:id/retry-failed", post(retry_failed_source_items))
        .route("/sources/:id/retry-discovery", post(retry_source_discovery))
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
        .route("/jobs/:id/cancel", post(cancel_job))
        .route("/usage/events", get(list_usage_events))
        .route("/usage/summary", get(usage_summary))
        .route("/storage/usage", get(storage_usage))
        .route("/storage/locations", get(storage_locations))
        .route("/storage/reset-library", post(reset_local_library))
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
            "/models/local/capability",
            get(local_models::local_capability),
        )
        .route(
            "/models/local/prepare",
            post(local_models::prepare_local_models),
        )
        .route(
            "/models/local/prepare-status",
            get(local_models::local_prepare_status),
        )
        .route(
            "/models/local/prepare-cancel",
            post(local_models::cancel_local_prepare),
        )
        .route(
            "/models/local/delete",
            post(local_models::delete_local_models),
        )
        .route(
            "/models/local/repair",
            post(local_models::repair_local_models),
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
        .route("/settings", get(list_settings).patch(update_settings));
    let v1_routes = Router::new()
        .route("/status", get(v1_status))
        .route("/openapi.json", get(v1_openapi_json))
        .route("/search", post(v1_search))
        .route("/ask", post(v1_ask))
        .route("/items", get(v1_list_items))
        .route("/items/:id", get(v1_get_item))
        .route("/items/:id/chunks", get(v1_list_item_chunks))
        .route("/chunks/:id/frame", get(get_chunk_frame))
        .route("/chunks/:id/video-segment", get(get_chunk_video_segment))
        .route("/chunks/:id/video-clip", get(get_chunk_video_clip));

    Router::new()
        .nest("/internal", internal_routes)
        .nest("/v1", v1_routes)
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
    init_core_file_logging(&paths);
    if let Err(error) = providers::bootstrap_env_providers(&paths) {
        tracing::warn!(%error, "failed to bootstrap env providers");
    }
    if let Err(error) = jobs::cleanup_deleting_items(&paths).await {
        tracing::warn!(%error, "failed to clean interrupted Cerul deletes");
    }
    if let Err(error) = jobs::requeue_interrupted_jobs(&paths) {
        tracing::warn!(%error, "failed to requeue interrupted Cerul jobs");
    }
    resume_interrupted_source_discovery(&paths);
    let _worker = jobs::spawn_default_job_worker(paths.clone());
    let _vector_index_shutdown = VectorIndexShutdownGuard;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    if let Err(error) = write_api_endpoint_file(&paths, addr.port()) {
        tracing::warn!(%error, "failed to write Cerul API endpoint file");
    }
    axum::serve(
        listener,
        router_with_paths(paths).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

static CORE_LOGGING_INIT: OnceLock<()> = OnceLock::new();

fn init_core_file_logging(paths: &AppPaths) {
    if CORE_LOGGING_INIT.get().is_some() {
        return;
    }

    let log_dir = paths.logs_dir();
    let result = fs::create_dir_all(&log_dir)
        .and_then(|_| {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_dir.join(CORE_LOG_FILE))
        })
        .map(|file| CoreLogWriter {
            file: Arc::new(Mutex::new(file)),
        });

    let Ok(writer) = result else {
        eprintln!(
            "failed to initialize Cerul core log at {}",
            log_dir.join(CORE_LOG_FILE).display()
        );
        let _ = CORE_LOGGING_INIT.set(());
        return;
    };

    match tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(true)
        .with_level(true)
        .try_init()
    {
        Ok(()) => {
            let _ = CORE_LOGGING_INIT.set(());
            tracing::info!(
                log_path = %cerul_storage::log_file_path(paths, CORE_LOG_FILE).display(),
                "Cerul core file logging initialized"
            );
        }
        Err(error) => {
            eprintln!("failed to install Cerul core tracing subscriber: {error}");
            let _ = CORE_LOGGING_INIT.set(());
        }
    }
}

#[derive(Clone)]
struct CoreLogWriter {
    file: Arc<Mutex<File>>,
}

impl<'a> MakeWriter<'a> for CoreLogWriter {
    type Writer = CoreLogGuard;

    fn make_writer(&'a self) -> Self::Writer {
        CoreLogGuard {
            file: Arc::clone(&self.file),
        }
    }
}

struct CoreLogGuard {
    file: Arc<Mutex<File>>,
}

impl Write for CoreLogGuard {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut file = self
            .file
            .lock()
            .map_err(|_| io::Error::other("core log lock poisoned"))?;
        file.write_all(buf)?;
        io::stderr().write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut file = self
            .file
            .lock()
            .map_err(|_| io::Error::other("core log lock poisoned"))?;
        file.flush()?;
        io::stderr().flush()
    }
}

struct VectorIndexShutdownGuard;

impl Drop for VectorIndexShutdownGuard {
    fn drop(&mut self) {
        api_models::shutdown_local_query_sidecar();
        cerul_storage::vectors::shutdown_vector_index();
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
    format!("127.0.0.1:{DEFAULT_API_PORT}")
        .parse()
        .expect("default Cerul API address is valid")
}

pub fn configured_addr(paths: &AppPaths) -> anyhow::Result<SocketAddr> {
    let host = match setting_string(paths, "api_binding")?.as_deref() {
        Some("0") | Some("0.0.0.0") => "0.0.0.0",
        _ => "127.0.0.1",
    };
    let port = configured_api_port(paths)?;

    Ok(format!("{host}:{port}").parse()?)
}

fn configured_api_port(paths: &AppPaths) -> anyhow::Result<u16> {
    if let Ok(value) = std::env::var("CERUL_API_PORT") {
        return parse_api_port(&value).ok_or_else(|| {
            anyhow::anyhow!("CERUL_API_PORT must be an integer from 1024 to 65535")
        });
    }
    match setting_string(paths, API_PORT_SETTING)? {
        Some(value) => parse_api_port(&value)
            .ok_or_else(|| anyhow::anyhow!("api_port must be an integer from 1024 to 65535")),
        None => Ok(DEFAULT_API_PORT),
    }
}

fn parse_api_port(value: &str) -> Option<u16> {
    let port = value.trim().parse::<u16>().ok()?;
    (1024..=65535).contains(&port).then_some(port)
}

fn write_api_endpoint_file(paths: &AppPaths, port: u16) -> anyhow::Result<()> {
    fs::create_dir_all(&paths.data)?;
    let base_url = format!("http://127.0.0.1:{port}");
    let payload = json!({
        "base_url": base_url,
        "v1_base_url": format!("{base_url}/v1"),
        "internal_base_url": format!("{base_url}/internal"),
        "port": port,
        "version": env!("CARGO_PKG_VERSION"),
    });
    fs::write(
        paths.data.join(API_ENDPOINT_FILE),
        serde_json::to_vec_pretty(&payload)?,
    )?;
    Ok(())
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
    Json(openapi_document("Cerul Internal API", API_PATHS))
}

async fn v1_openapi_json() -> Json<Value> {
    Json(openapi_document("Cerul Agent API", V1_API_PATHS))
}

fn openapi_document(title: &str, api_paths: &[(&str, &[&str])]) -> Value {
    let mut paths = serde_json::Map::new();
    for (path, methods) in api_paths {
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

    json!({
        "openapi": "3.1.0",
        "info": {
            "title": title,
            "version": env!("CARGO_PKG_VERSION")
        },
        "paths": paths
    })
}

#[derive(Debug, Serialize)]
struct V1Execution {
    target: &'static str,
    account_id: Option<String>,
    privacy: &'static str,
}

#[derive(Debug, Serialize)]
struct V1StatusResponse {
    request_id: String,
    status: &'static str,
    version: &'static str,
    execution: V1Execution,
    library: V1StatusLibrary,
    search: V1StatusSearch,
    indexing: V1StatusIndexing,
    account: V1StatusAccount,
    capabilities: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct V1StatusLibrary {
    total_items: u64,
    indexed_items: u64,
    processing_items: u64,
    failed_items: u64,
    chunk_count: usize,
}

#[derive(Debug, Serialize)]
struct V1StatusSearch {
    ready: bool,
    retrieval_mode: &'static str,
    text_ready: bool,
    vector_ready: bool,
}

#[derive(Debug, Serialize)]
struct V1StatusIndexing {
    paused: bool,
    active_jobs: u64,
    queued_jobs: u64,
}

#[derive(Debug, Serialize)]
struct V1StatusAccount {
    signed_in: bool,
    plan: Option<String>,
    credits_remaining: Option<i64>,
}

async fn v1_status(State(state): State<ApiState>) -> ApiResult<Json<V1StatusResponse>> {
    let indexing = jobs::indexing_diagnostics(&state.paths)?;
    let search = search_health_diagnostics(&state.paths).await?;
    let text_ready = search.fts_row_count > 0 || search.retrieval_unit_fts_row_count > 0;
    let vector_ready =
        search.vector_index_error.is_none() && search.vector_index_point_count.unwrap_or(0) > 0;
    let retrieval_mode = match (text_ready, vector_ready) {
        (true, true) => "hybrid",
        (true, false) => "text",
        (false, true) => "vector",
        (false, false) => "empty",
    };
    let counts = &indexing.counts;

    Ok(Json(V1StatusResponse {
        request_id: new_id("req"),
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        execution: V1Execution {
            target: "local",
            account_id: None,
            privacy: "local_only",
        },
        library: V1StatusLibrary {
            total_items: counts.total_items,
            indexed_items: counts.indexed_items,
            processing_items: counts.processing_items,
            failed_items: counts.failed_items,
            chunk_count: search.chunk_count,
        },
        search: V1StatusSearch {
            ready: text_ready || vector_ready,
            retrieval_mode,
            text_ready,
            vector_ready,
        },
        indexing: V1StatusIndexing {
            paused: indexing.paused,
            active_jobs: counts.running_jobs,
            queued_jobs: counts.queued_jobs,
        },
        account: V1StatusAccount {
            signed_in: false,
            plan: None,
            credits_remaining: None,
        },
        capabilities: vec!["status", "openapi", "search", "ask", "items", "chunks"],
    }))
}

#[derive(Debug, Deserialize)]
struct V1ListItemsQuery {
    limit: Option<usize>,
    cursor: Option<String>,
    status: Option<String>,
    source_id: Option<String>,
    source_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct V1ItemChunksQuery {
    limit: Option<usize>,
    cursor: Option<String>,
    from_sec: Option<f64>,
    to_sec: Option<f64>,
    #[serde(rename = "type")]
    chunk_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct V1SearchRequest {
    query: Option<String>,
    q: Option<String>,
    max_results: Option<usize>,
    limit: Option<usize>,
    target: Option<String>,
}

#[derive(Debug, Deserialize)]
struct V1AskRequest {
    question: Option<String>,
    query: Option<String>,
    q: Option<String>,
    max_results: Option<usize>,
    limit: Option<usize>,
    locale: Option<String>,
    mode: Option<String>,
    target: Option<String>,
}

#[derive(Debug, Serialize)]
struct V1SearchResponse {
    request_id: String,
    execution: V1Execution,
    results: Vec<V1SearchResult>,
    diagnostics: V1SearchDiagnostics,
    usage: V1Usage,
}

#[derive(Debug, Serialize)]
struct V1AskResponse {
    request_id: String,
    execution: V1Execution,
    mode: &'static str,
    answer: String,
    citations: Vec<V1SearchResult>,
    warnings: Vec<String>,
    usage: V1Usage,
}

#[derive(Debug, Serialize)]
struct V1ItemsResponse {
    request_id: String,
    execution: V1Execution,
    items: Vec<V1Item>,
    page: V1Page,
}

#[derive(Debug, Serialize)]
struct V1ItemResponse {
    request_id: String,
    execution: V1Execution,
    item: V1Item,
}

#[derive(Debug, Serialize)]
struct V1ItemChunksResponse {
    request_id: String,
    execution: V1Execution,
    item: V1Item,
    chunks: Vec<V1ItemChunk>,
    page: V1Page,
}

#[derive(Debug, Serialize)]
struct V1Page {
    limit: usize,
    next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
struct V1Item {
    id: String,
    title: String,
    content_type: String,
    source_type: String,
    source_url: Option<String>,
    status: String,
    duration_sec: Option<f64>,
    indexed_at: Option<i64>,
    chunk_count: usize,
    thumbnail: Option<V1Locator>,
    open_in_cerul: String,
}

#[derive(Debug)]
struct V1ItemRow {
    id: String,
    title: String,
    content_type: String,
    external_id: Option<String>,
    duration_sec: Option<f64>,
    indexed_at: Option<i64>,
    status: String,
    metadata: Value,
    source_type: String,
    source_config: Value,
    thumbnail_chunk_id: Option<String>,
    thumbnail_frame_path: Option<String>,
    chunk_count: usize,
    source_file_exists: bool,
}

#[derive(Debug, Serialize)]
struct V1ItemChunk {
    id: String,
    #[serde(rename = "type")]
    chunk_type: String,
    source: &'static str,
    time: V1SearchTime,
    text: V1ChunkText,
    evidence: V1Evidence,
}

#[derive(Debug, Serialize)]
struct V1ChunkText {
    content: Option<String>,
    snippet: Option<String>,
}

#[derive(Debug, Serialize)]
struct V1SearchResult {
    id: String,
    #[serde(rename = "type")]
    result_type: &'static str,
    source: &'static str,
    item: V1SearchItem,
    time: V1SearchTime,
    text: V1SearchText,
    evidence: V1Evidence,
    score: V1Score,
}

#[derive(Debug, Clone, Serialize)]
struct V1SearchItem {
    id: String,
    title: String,
    content_type: String,
    source_type: String,
    duration_sec: Option<f64>,
}

#[derive(Debug, Clone)]
struct V1SearchItemMetadata {
    item: V1SearchItem,
    source_file_exists: bool,
}

#[derive(Debug, Serialize)]
struct V1SearchTime {
    start_sec: Option<f64>,
    end_sec: Option<f64>,
    timestamp: Option<String>,
}

#[derive(Debug, Serialize)]
struct V1SearchText {
    snippet: String,
    quote: String,
}

#[derive(Debug, Serialize)]
struct V1Evidence {
    id: String,
    kind: &'static str,
    clip: Option<V1Locator>,
    preview: Option<V1Locator>,
    open_in_cerul: String,
}

#[derive(Debug, Serialize)]
struct V1Locator {
    #[serde(rename = "type")]
    locator_type: &'static str,
    url: String,
}

#[derive(Debug, Serialize)]
struct V1Score {
    #[serde(rename = "match")]
    match_score: f32,
    exact_match: bool,
    similarity: Option<f32>,
}

#[derive(Debug, Serialize)]
struct V1SearchDiagnostics {
    retrieval_mode: String,
    fallback_reason: Option<String>,
    vector_hits: usize,
    text_hits: usize,
    result_count: usize,
}

#[derive(Debug, Serialize)]
struct V1Usage {
    billable: bool,
    metered_events: Vec<V1MeteredEvent>,
    credits_used: i64,
}

#[derive(Debug, Serialize)]
struct V1MeteredEvent {
    capability: &'static str,
    quantity: u64,
    credits: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum V1QueryExecution {
    LocalOnly,
    RemoteEmbedding,
}

impl V1QueryExecution {
    fn execution(self) -> V1Execution {
        V1Execution {
            target: "local",
            account_id: None,
            privacy: match self {
                Self::LocalOnly => "local_only",
                Self::RemoteEmbedding => "local_library_remote_query",
            },
        }
    }

    fn search_events(self) -> Vec<V1MeteredEvent> {
        let mut events = vec![V1MeteredEvent {
            capability: "local_search",
            quantity: 1,
            credits: 0,
        }];
        if self == Self::RemoteEmbedding {
            events.push(V1MeteredEvent {
                capability: "remote_embedding_query",
                quantity: 1,
                credits: 0,
            });
        }
        events
    }

    fn ask_events(self) -> Vec<V1MeteredEvent> {
        let mut events = vec![V1MeteredEvent {
            capability: "local_ask_extractive",
            quantity: 1,
            credits: 0,
        }];
        if self == Self::RemoteEmbedding {
            events.push(V1MeteredEvent {
                capability: "remote_embedding_query",
                quantity: 1,
                credits: 0,
            });
        }
        events
    }
}

async fn v1_search(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<V1SearchRequest>,
) -> ApiResult<Json<V1SearchResponse>> {
    let query = first_non_empty_text([req.query, req.q])
        .ok_or_else(|| ApiError::bad_request("query cannot be empty"))?;
    validate_v1_local_target(req.target.as_deref())?;
    let query_execution = v1_query_execution(&state.paths);
    let limit = req.max_results.or(req.limit).unwrap_or(10).clamp(1, 50);
    let response = search_records(
        &state.paths,
        cerul_search::SearchRequest { q: query, limit },
    )
    .await?;
    let item_metadata = v1_search_item_metadata(&state.paths, &response.results)?;
    let existing_preview_chunk_ids =
        v1_existing_preview_chunk_ids(&state.paths, &response.results)?;
    let base_url = v1_base_url(&headers, &state.paths);
    let results = response
        .results
        .iter()
        .map(|result| {
            v1_search_result(
                result,
                &item_metadata,
                &existing_preview_chunk_ids,
                &base_url,
            )
        })
        .collect::<Vec<_>>();
    let result_count = results.len();

    Ok(Json(V1SearchResponse {
        request_id: new_id("req"),
        execution: query_execution.execution(),
        results,
        diagnostics: V1SearchDiagnostics {
            retrieval_mode: response.diagnostics.retrieval_mode,
            fallback_reason: response.diagnostics.fallback_reason,
            vector_hits: response.diagnostics.vector_hits_count,
            text_hits: response.diagnostics.fts_hits_count,
            result_count,
        },
        usage: V1Usage {
            billable: false,
            metered_events: query_execution.search_events(),
            credits_used: 0,
        },
    }))
}

async fn v1_ask(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<V1AskRequest>,
) -> ApiResult<Json<V1AskResponse>> {
    let question = first_non_empty_text([req.question, req.query, req.q])
        .ok_or_else(|| ApiError::bad_request("question cannot be empty"))?;
    validate_v1_local_target(req.target.as_deref())?;
    let query_execution = v1_query_execution(&state.paths);
    let requested_mode = req
        .mode
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("extractive")
        .to_ascii_lowercase();
    if !matches!(requested_mode.as_str(), "extractive" | "auto") {
        return Err(ApiError::bad_request(
            "only extractive mode is currently supported by /v1/ask",
        ));
    }
    let limit = req.max_results.or(req.limit).unwrap_or(6).clamp(1, 8);
    let response = search_records(
        &state.paths,
        cerul_search::SearchRequest {
            q: question.clone(),
            limit,
        },
    )
    .await?;
    let filtered_results = response
        .results
        .into_iter()
        .filter(|result| !result.snippet.trim().is_empty())
        .take(limit)
        .collect::<Vec<_>>();
    let item_metadata = v1_search_item_metadata(&state.paths, &filtered_results)?;
    let existing_preview_chunk_ids =
        v1_existing_preview_chunk_ids(&state.paths, &filtered_results)?;
    let base_url = v1_base_url(&headers, &state.paths);
    let citations = filtered_results
        .iter()
        .map(|result| {
            v1_search_result(
                result,
                &item_metadata,
                &existing_preview_chunk_ids,
                &base_url,
            )
        })
        .collect::<Vec<_>>();
    let answer = v1_extractive_answer(&question, &citations, req.locale.as_deref());

    Ok(Json(V1AskResponse {
        request_id: new_id("req"),
        execution: query_execution.execution(),
        mode: "extractive",
        answer,
        citations,
        warnings: Vec::new(),
        usage: V1Usage {
            billable: false,
            metered_events: query_execution.ask_events(),
            credits_used: 0,
        },
    }))
}

async fn v1_list_items(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<V1ListItemsQuery>,
) -> ApiResult<Json<V1ItemsResponse>> {
    let limit = v1_page_limit(query.limit, 50, 100);
    let offset = v1_cursor_offset(query.cursor.as_deref())?;
    let fetch_limit = limit + 1;
    let statuses = split_filter_values(query.status.as_deref());
    let base_url = v1_base_url(&headers, &state.paths);
    let conn = cerul_storage::sqlite::open(&state.paths)?;
    let mut params: Vec<SqlValue> = Vec::new();
    let mut sql = v1_item_select_sql();
    sql.push_str(" WHERE i.status != 'deleting'");

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
    if let Some(source_type) = query.source_type.filter(|value| !value.trim().is_empty()) {
        sql.push_str(" AND s.type = ?");
        params.push(SqlValue::from(source_type));
    }
    sql.push_str(
        r#"
        ORDER BY COALESCE(i.indexed_at, 0) DESC, i.id ASC
        LIMIT ? OFFSET ?
        "#,
    );
    params.push(SqlValue::from(fetch_limit as i64));
    params.push(SqlValue::from(offset as i64));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        rusqlite::params_from_iter(params.iter()),
        v1_item_row_from_row,
    )?;
    let mut rows = rows.collect::<Result<Vec<_>, _>>()?;
    let next_cursor = if rows.len() > limit {
        rows.truncate(limit);
        Some((offset + limit).to_string())
    } else {
        None
    };
    let items = rows
        .iter()
        .map(|item| v1_item_from_row(item, &base_url))
        .collect::<Vec<_>>();

    Ok(Json(V1ItemsResponse {
        request_id: new_id("req"),
        execution: V1Execution {
            target: "local",
            account_id: None,
            privacy: "local_only",
        },
        items,
        page: V1Page { limit, next_cursor },
    }))
}

async fn v1_get_item(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<V1ItemResponse>> {
    let base_url = v1_base_url(&headers, &state.paths);
    let item = v1_load_item(&state.paths, &id)?
        .ok_or_else(|| ApiError::not_found(format!("item not found: {id}")))?;

    Ok(Json(V1ItemResponse {
        request_id: new_id("req"),
        execution: V1Execution {
            target: "local",
            account_id: None,
            privacy: "local_only",
        },
        item: v1_item_from_row(&item, &base_url),
    }))
}

async fn v1_list_item_chunks(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<V1ItemChunksQuery>,
) -> ApiResult<Json<V1ItemChunksResponse>> {
    let limit = v1_page_limit(query.limit, 100, 250);
    let offset = v1_cursor_offset(query.cursor.as_deref())?;
    let fetch_limit = limit + 1;
    let base_url = v1_base_url(&headers, &state.paths);
    let item = v1_load_item(&state.paths, &id)?
        .ok_or_else(|| ApiError::not_found(format!("item not found: {id}")))?;

    if let Some(from_sec) = query.from_sec {
        if !from_sec.is_finite() || from_sec < 0.0 {
            return Err(ApiError::bad_request(
                "from_sec must be a finite non-negative number",
            ));
        }
    }
    if let Some(to_sec) = query.to_sec {
        if !to_sec.is_finite() || to_sec < 0.0 {
            return Err(ApiError::bad_request(
                "to_sec must be a finite non-negative number",
            ));
        }
    }
    if let (Some(from_sec), Some(to_sec)) = (query.from_sec, query.to_sec) {
        if to_sec < from_sec {
            return Err(ApiError::bad_request(
                "to_sec must be greater than or equal to from_sec",
            ));
        }
    }

    let mut params: Vec<SqlValue> = vec![SqlValue::from(id.clone())];
    let mut sql = String::from(
        r#"
        SELECT id, item_id, chunk_type, start_sec, end_sec, text, frame_path, metadata
        FROM chunks
        WHERE item_id = ?
        "#,
    );
    if let Some(chunk_type) = query.chunk_type.filter(|value| !value.trim().is_empty()) {
        let chunk_types = v1_chunk_type_filter_values(&chunk_type);
        if chunk_types.len() == 1 {
            sql.push_str(" AND chunk_type = ?");
        } else {
            sql.push_str(" AND chunk_type IN (");
            sql.push_str(
                &std::iter::repeat_n("?", chunk_types.len())
                    .collect::<Vec<_>>()
                    .join(","),
            );
            sql.push(')');
        }
        for chunk_type in chunk_types {
            params.push(SqlValue::from(chunk_type));
        }
    }
    if let Some(from_sec) = query.from_sec {
        sql.push_str(" AND COALESCE(end_sec, start_sec, 0) >= ?");
        params.push(SqlValue::from(from_sec));
    }
    if let Some(to_sec) = query.to_sec {
        sql.push_str(" AND COALESCE(start_sec, end_sec, 0) <= ?");
        params.push(SqlValue::from(to_sec));
    }
    sql.push_str(
        r#"
        ORDER BY COALESCE(start_sec, 0), id ASC
        LIMIT ? OFFSET ?
        "#,
    );
    params.push(SqlValue::from(fetch_limit as i64));
    params.push(SqlValue::from(offset as i64));

    let conn = cerul_storage::sqlite::open(&state.paths)?;
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), chunk_from_row)?;
    let mut rows = rows.collect::<Result<Vec<_>, _>>()?;
    let next_cursor = if rows.len() > limit {
        rows.truncate(limit);
        Some((offset + limit).to_string())
    } else {
        None
    };
    let chunks = rows
        .iter()
        .map(|chunk| v1_item_chunk(chunk, &item, &base_url))
        .collect::<Vec<_>>();

    Ok(Json(V1ItemChunksResponse {
        request_id: new_id("req"),
        execution: V1Execution {
            target: "local",
            account_id: None,
            privacy: "local_only",
        },
        item: v1_item_from_row(&item, &base_url),
        chunks,
        page: V1Page { limit, next_cursor },
    }))
}

fn v1_page_limit(limit: Option<usize>, default: usize, max: usize) -> usize {
    limit.unwrap_or(default).clamp(1, max)
}

fn v1_cursor_offset(cursor: Option<&str>) -> ApiResult<usize> {
    let Some(cursor) = cursor.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(0);
    };
    cursor
        .parse::<usize>()
        .map_err(|_| ApiError::bad_request("cursor must be a non-negative integer offset"))
}

fn first_non_empty_text(values: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    values
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

fn v1_chunk_type_filter_values(value: &str) -> Vec<String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "transcript" => vec!["transcript".to_string(), "transcript_line".to_string()],
        "visual" => vec![
            "keyframe".to_string(),
            "image".to_string(),
            "ocr".to_string(),
        ],
        "summary" => vec!["understanding".to_string()],
        raw => vec![raw.to_string()],
    }
}

fn local_source_file_exists(raw_path: &str) -> bool {
    let raw_path = raw_path.trim();
    !raw_path.is_empty() && FsPath::new(raw_path).is_file()
}

fn v1_query_execution(paths: &AppPaths) -> V1QueryExecution {
    match api_models::effective_query_inference_mode(paths) {
        Ok(mode) if mode == "remote" => V1QueryExecution::RemoteEmbedding,
        Ok(_) => V1QueryExecution::LocalOnly,
        Err(error) => {
            tracing::debug!(%error, "could not resolve v1 query execution mode; assuming local-only fallback");
            V1QueryExecution::LocalOnly
        }
    }
}

fn v1_load_item(paths: &AppPaths, id: &str) -> anyhow::Result<Option<V1ItemRow>> {
    let conn = cerul_storage::sqlite::open(paths)?;
    let sql = format!(
        r#"
        {}
        WHERE i.id = ?1
          AND i.status != 'deleting'
        "#,
        v1_item_select_sql()
    );
    conn.query_row(&sql, [id], v1_item_row_from_row)
        .optional()
        .map_err(Into::into)
}

fn v1_item_select_sql() -> String {
    r#"
        SELECT i.id, i.content_type, i.external_id, i.title,
               COALESCE(i.duration_sec, (
                   SELECT MAX(c2.end_sec)
                   FROM chunks c2
                   WHERE c2.item_id = i.id
               )) AS duration_sec,
               i.indexed_at, i.status, i.metadata,
               s.type AS source_type, s.config AS source_config,
               (
                   SELECT c.id
                   FROM chunks c
                   WHERE c.item_id = i.id
                     AND c.frame_path IS NOT NULL
                     AND TRIM(c.frame_path) <> ''
                   ORDER BY COALESCE(c.start_sec, 0), c.id
                   LIMIT 1
               ) AS thumbnail_chunk_id,
               (
                   SELECT c.frame_path
                   FROM chunks c
                   WHERE c.item_id = i.id
                     AND c.frame_path IS NOT NULL
                     AND TRIM(c.frame_path) <> ''
                   ORDER BY COALESCE(c.start_sec, 0), c.id
                   LIMIT 1
               ) AS thumbnail_frame_path,
               (
                   SELECT COUNT(*)
                   FROM chunks c
                   WHERE c.item_id = i.id
               ) AS chunk_count,
               i.raw_path
        FROM items i
        JOIN sources s ON s.id = i.source_id
    "#
    .to_string()
}

fn v1_item_row_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<V1ItemRow> {
    let title = row
        .get::<_, Option<String>>(3)?
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Untitled media".to_string());
    let metadata = row
        .get::<_, Option<String>>(7)?
        .as_deref()
        .map(parse_json)
        .unwrap_or_else(|| json!({}));
    let source_config = row
        .get::<_, Option<String>>(9)?
        .as_deref()
        .map(parse_json)
        .unwrap_or_else(|| json!({}));
    let thumbnail_chunk_id: Option<String> = row.get(10)?;
    let thumbnail_frame_path: Option<String> = row.get(11)?;
    let chunk_count = row.get::<_, i64>(12)?.max(0) as usize;
    let raw_path: Option<String> = row.get(13)?;
    let source_file_exists = raw_path.as_deref().is_some_and(local_source_file_exists);

    Ok(V1ItemRow {
        id: row.get(0)?,
        content_type: row.get(1)?,
        external_id: row.get(2)?,
        title,
        duration_sec: row.get(4)?,
        indexed_at: row.get(5)?,
        status: row.get(6)?,
        metadata,
        source_type: row.get(8)?,
        source_config,
        thumbnail_chunk_id,
        thumbnail_frame_path,
        chunk_count,
        source_file_exists,
    })
}

fn v1_item_from_row(item: &V1ItemRow, base_url: &str) -> V1Item {
    let thumbnail = item
        .thumbnail_chunk_id
        .as_deref()
        .zip(item.thumbnail_frame_path.as_deref())
        .filter(|(_, frame_path)| local_source_file_exists(frame_path))
        .map(|(chunk_id, _)| V1Locator {
            locator_type: "local",
            url: format!(
                "{}/chunks/{}/frame",
                base_url,
                encode_path_segment(chunk_id)
            ),
        });
    V1Item {
        id: item.id.clone(),
        title: item.title.clone(),
        content_type: item.content_type.clone(),
        source_type: item.source_type.clone(),
        source_url: v1_item_source_url(item),
        status: item.status.clone(),
        duration_sec: item.duration_sec,
        indexed_at: item.indexed_at,
        chunk_count: item.chunk_count,
        thumbnail,
        open_in_cerul: v1_open_item_in_cerul_link(&item.id),
    }
}

fn v1_item_chunk(chunk: &ChunkRecord, item: &V1ItemRow, base_url: &str) -> V1ItemChunk {
    V1ItemChunk {
        id: chunk.id.clone(),
        chunk_type: v1_result_type(&chunk.chunk_type).to_string(),
        source: "local_library",
        time: V1SearchTime {
            start_sec: chunk.start_sec.filter(|value| value.is_finite()),
            end_sec: chunk.end_sec.filter(|value| value.is_finite()),
            timestamp: chunk.start_sec.map(format_playback_timestamp),
        },
        text: V1ChunkText {
            content: chunk.text.clone(),
            snippet: chunk.text.as_deref().map(|text| trim_for_answer(text, 360)),
        },
        evidence: v1_chunk_evidence(
            &chunk.id,
            item,
            chunk.start_sec,
            chunk.frame_path.as_deref(),
            base_url,
        ),
    }
}

fn has_timed_video_clip_start(start_sec: Option<f64>) -> bool {
    start_sec.is_some_and(|value| value.is_finite() && value >= 0.0)
}

fn v1_chunk_evidence(
    chunk_id: &str,
    item: &V1ItemRow,
    start_sec: Option<f64>,
    frame_path: Option<&str>,
    base_url: &str,
) -> V1Evidence {
    let clip = if item.source_file_exists
        && item.content_type == "video"
        && has_timed_video_clip_start(start_sec)
    {
        Some(V1Locator {
            locator_type: "local",
            url: format!(
                "{}/chunks/{}/video-clip?before_sec=3&after_sec=5",
                base_url,
                encode_path_segment(chunk_id)
            ),
        })
    } else {
        None
    };
    let preview = frame_path
        .map(str::trim)
        .filter(|path| local_source_file_exists(path))
        .map(|_| V1Locator {
            locator_type: "local",
            url: format!(
                "{}/chunks/{}/frame",
                base_url,
                encode_path_segment(chunk_id)
            ),
        });
    let evidence_kind = match (clip.is_some(), preview.is_some()) {
        (true, _) => "video_clip",
        (false, true) => "frame",
        (false, false) => "chunk",
    };

    V1Evidence {
        id: chunk_id.to_string(),
        kind: evidence_kind,
        clip,
        preview,
        open_in_cerul: v1_open_in_cerul_link(&item.id, chunk_id, start_sec),
    }
}

fn v1_item_source_url(item: &V1ItemRow) -> Option<String> {
    for key in [
        "webpage_url",
        "original_url",
        "source_url",
        "url",
        "enclosure_url",
        "feed_url",
        "channel_url",
    ] {
        if let Some(url) = item
            .metadata
            .get(key)
            .and_then(Value::as_str)
            .and_then(v1_http_url)
        {
            return Some(url);
        }
    }
    if item.source_type == "youtube" {
        if let Some(external_id) = item
            .external_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(format!("https://www.youtube.com/watch?v={external_id}"));
        }
    }
    for key in ["url", "feed_url", "channel_url"] {
        if let Some(url) = item
            .source_config
            .get(key)
            .and_then(Value::as_str)
            .and_then(v1_http_url)
        {
            return Some(url);
        }
    }
    None
}

fn v1_http_url(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
        Some(trimmed.to_string())
    } else {
        None
    }
}

fn validate_v1_local_target(target: Option<&str>) -> ApiResult<()> {
    let target = target
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("local")
        .to_ascii_lowercase();
    if matches!(target.as_str(), "local" | "auto") {
        return Ok(());
    }
    Err(ApiError::bad_request(
        "only local or auto target is currently supported by /v1",
    ))
}

fn v1_extractive_answer(
    question: &str,
    citations: &[V1SearchResult],
    locale: Option<&str>,
) -> String {
    let answer_in_english =
        !locale.is_some_and(|locale| locale.trim().to_ascii_lowercase().starts_with("zh"));
    if citations.is_empty() {
        if answer_in_english {
            format!(
                "I couldn't find a directly related moment for \"{}\" in the local index. Try another keyword or wait for current indexing jobs to finish.",
                question
            )
        } else {
            format!(
                "没有在本地索引里找到和「{}」直接相关的片段。可以先换一个关键词，或等当前索引任务完成后再问。",
                question
            )
        }
    } else {
        let mut sentences = Vec::new();
        for citation in citations.iter().take(3) {
            let timestamp = citation.time.timestamp.as_deref().unwrap_or("0:00");
            if answer_in_english {
                sentences.push(format!(
                    "Around {} in \"{}\", the index says: {}",
                    timestamp, citation.item.title, citation.text.snippet
                ));
            } else {
                sentences.push(format!(
                    "在《{}》{} 附近，索引里提到：{}",
                    citation.item.title, timestamp, citation.text.snippet
                ));
            }
        }
        if answer_in_english {
            format!(
                "{} This answer is extractive and grounded only in the local search hits below.",
                sentences.join(" ")
            )
        } else {
            format!(
                "{} 本回答是抽取式回答，只基于下面这些本地检索命中。",
                sentences.join(" ")
            )
        }
    }
}

fn v1_search_result(
    result: &cerul_search::SearchResult,
    item_metadata: &HashMap<String, V1SearchItemMetadata>,
    existing_preview_chunk_ids: &HashSet<String>,
    base_url: &str,
) -> V1SearchResult {
    let metadata = item_metadata
        .get(&result.item_id)
        .cloned()
        .unwrap_or_else(|| fallback_v1_search_item_metadata(result));
    let start_sec = result.start_sec.filter(|value| value.is_finite());
    let end_sec = result.end_sec.filter(|value| value.is_finite());
    let preview_chunk_id = result.nearest_frame_chunk_id.as_deref().or_else(|| {
        result
            .frame_path
            .as_ref()
            .map(|_| result.playback_chunk_id.as_str())
    });
    let clip = if metadata.source_file_exists
        && metadata.item.content_type == "video"
        && has_timed_video_clip_start(start_sec)
    {
        Some(V1Locator {
            locator_type: "local",
            url: format!(
                "{}/chunks/{}/video-clip?before_sec=3&after_sec=5",
                base_url,
                encode_path_segment(&result.playback_chunk_id)
            ),
        })
    } else {
        None
    };
    let preview = preview_chunk_id.and_then(|chunk_id| {
        existing_preview_chunk_ids
            .contains(chunk_id)
            .then(|| V1Locator {
                locator_type: "local",
                url: format!(
                    "{}/chunks/{}/frame",
                    base_url,
                    encode_path_segment(chunk_id)
                ),
            })
    });
    let evidence_kind = match (clip.is_some(), preview.is_some()) {
        (true, _) => "video_clip",
        (false, true) => "frame",
        (false, false) => "chunk",
    };

    V1SearchResult {
        id: result.playback_chunk_id.clone(),
        result_type: v1_result_type(&result.chunk_type),
        source: "local_library",
        item: metadata.item,
        time: V1SearchTime {
            start_sec,
            end_sec,
            timestamp: start_sec.map(format_playback_timestamp),
        },
        text: V1SearchText {
            snippet: trim_for_answer(&result.snippet, 360),
            quote: trim_for_answer(&result.snippet, 240),
        },
        evidence: V1Evidence {
            id: result.playback_chunk_id.clone(),
            kind: evidence_kind,
            clip,
            preview,
            open_in_cerul: v1_open_in_cerul_link(
                &result.item_id,
                &result.playback_chunk_id,
                start_sec,
            ),
        },
        score: V1Score {
            match_score: result.match_score,
            exact_match: result.exact_match,
            similarity: result.similarity_score,
        },
    }
}

fn v1_search_item_metadata(
    paths: &AppPaths,
    results: &[cerul_search::SearchResult],
) -> anyhow::Result<HashMap<String, V1SearchItemMetadata>> {
    let mut item_ids = results
        .iter()
        .map(|result| result.item_id.as_str())
        .collect::<Vec<_>>();
    item_ids.sort_unstable();
    item_ids.dedup();
    if item_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders = std::iter::repeat_n("?", item_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        r#"
        SELECT i.id, i.title, i.content_type, s.type, i.duration_sec,
               i.raw_path
        FROM items i
        JOIN sources s ON s.id = i.source_id
        WHERE i.id IN ({placeholders})
        "#
    );
    let conn = cerul_storage::sqlite::open(paths)?;
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(item_ids.iter()), |row| {
        let id: String = row.get(0)?;
        let title = row
            .get::<_, Option<String>>(1)?
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "Untitled media".to_string());
        let raw_path: Option<String> = row.get(5)?;
        let source_file_exists = raw_path.as_deref().is_some_and(local_source_file_exists);
        Ok((
            id.clone(),
            V1SearchItemMetadata {
                item: V1SearchItem {
                    id,
                    title,
                    content_type: row.get(2)?,
                    source_type: row.get(3)?,
                    duration_sec: row.get(4)?,
                },
                source_file_exists,
            },
        ))
    })?;
    let mut metadata = HashMap::with_capacity(item_ids.len());
    for row in rows {
        let (id, item) = row?;
        metadata.insert(id, item);
    }
    Ok(metadata)
}

fn v1_existing_preview_chunk_ids(
    paths: &AppPaths,
    results: &[cerul_search::SearchResult],
) -> anyhow::Result<HashSet<String>> {
    let mut chunk_ids = results
        .iter()
        .filter_map(|result| {
            result.nearest_frame_chunk_id.as_deref().or_else(|| {
                result
                    .frame_path
                    .as_ref()
                    .map(|_| result.playback_chunk_id.as_str())
            })
        })
        .collect::<Vec<_>>();
    chunk_ids.sort_unstable();
    chunk_ids.dedup();
    if chunk_ids.is_empty() {
        return Ok(HashSet::new());
    }

    let placeholders = std::iter::repeat_n("?", chunk_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        r#"
        SELECT id, frame_path
        FROM chunks
        WHERE id IN ({placeholders})
          AND frame_path IS NOT NULL
          AND TRIM(frame_path) <> ''
        "#
    );
    let conn = cerul_storage::sqlite::open(paths)?;
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(chunk_ids.iter()), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut existing = HashSet::new();
    for row in rows {
        let (id, frame_path) = row?;
        if local_source_file_exists(&frame_path) {
            existing.insert(id);
        }
    }
    Ok(existing)
}

fn fallback_v1_search_item_metadata(result: &cerul_search::SearchResult) -> V1SearchItemMetadata {
    V1SearchItemMetadata {
        item: V1SearchItem {
            id: result.item_id.clone(),
            title: result
                .item_title
                .clone()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "Untitled media".to_string()),
            content_type: "unknown".to_string(),
            source_type: "unknown".to_string(),
            duration_sec: None,
        },
        source_file_exists: false,
    }
}

fn v1_result_type(chunk_type: &str) -> &'static str {
    match chunk_type {
        "keyframe" | "image" | "ocr" => "visual",
        "understanding" => "summary",
        _ => "transcript",
    }
}

fn v1_base_url(headers: &HeaderMap, paths: &AppPaths) -> String {
    if let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.contains('/'))
    {
        return format!("http://{host}/v1");
    }
    let port = configured_addr(paths)
        .map(|addr| addr.port())
        .unwrap_or(DEFAULT_API_PORT);
    format!("http://127.0.0.1:{port}/v1")
}

fn v1_open_in_cerul_link(item_id: &str, chunk_id: &str, start_sec: Option<f64>) -> String {
    let mut link = format!(
        "cerul-app://item/{}?playbackChunkId={}",
        encode_path_segment(item_id),
        encode_path_segment(chunk_id)
    );
    if let Some(start_sec) = start_sec.filter(|value| value.is_finite() && *value >= 0.0) {
        link.push_str("&t=");
        link.push_str(&format_seconds_param(start_sec));
    }
    link
}

fn v1_open_item_in_cerul_link(item_id: &str) -> String {
    format!("cerul-app://item/{}", encode_path_segment(item_id))
}

fn encode_path_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            out.push('%');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    out
}

fn format_seconds_param(value: f64) -> String {
    let mut formatted = format!("{:.3}", value.max(0.0));
    while formatted.contains('.') && formatted.ends_with('0') {
        formatted.pop();
    }
    if formatted.ends_with('.') {
        formatted.pop();
    }
    formatted
}

async fn search(
    State(state): State<ApiState>,
    Json(mut req): Json<cerul_search::SearchRequest>,
) -> ApiResult<Json<cerul_search::SearchResponse>> {
    // The limit fans out 4x into vector + FTS retrieval and pre-allocates
    // buffers; an unclamped client value is a one-request memory DoS.
    req.limit = req.limit.clamp(1, 50);
    Ok(Json(search_records(&state.paths, req).await?))
}

async fn search_records(
    paths: &AppPaths,
    req: cerul_search::SearchRequest,
) -> anyhow::Result<cerul_search::SearchResponse> {
    let query = req.q.clone();
    let paths_for_embedding = paths.clone();
    let embedding_started = Instant::now();
    let query_embedding = tokio::time::timeout(
        QUERY_EMBEDDING_TIMEOUT,
        tokio::task::spawn_blocking(move || api_models::embed_query(&paths_for_embedding, &query)),
    )
    .await;

    match query_embedding {
        Ok(Ok(Ok(embedding))) => {
            let embedding_elapsed = embedding_started.elapsed();
            tracing::info!(
                embedding_profile_id = %embedding.profile.id,
                query_embedding_ms = embedding_elapsed.as_millis(),
                "API semantic query embedding completed"
            );
            let fallback_req = req.clone();
            let search_started = Instant::now();
            match cerul_search::search_with_vector_for_profile_diagnostics(
                paths,
                req,
                embedding.vector,
                &embedding.profile,
            )
            .await
            {
                Ok(response) => {
                    tracing::info!(
                        retrieval_mode = %response.diagnostics.retrieval_mode,
                        vector_hits_count = response.diagnostics.vector_hits_count,
                        fts_hits_count = response.diagnostics.fts_hits_count,
                        vector_index_collection = ?response.diagnostics.vector_index_collection,
                        vector_index_point_count = ?response.diagnostics.vector_index_point_count,
                        search_ms = search_started.elapsed().as_millis(),
                        "API search completed"
                    );
                    Ok(response)
                }
                Err(error) => {
                    tracing::warn!(%error, "API vector search failed; falling back to FTS");
                    search_fts_fallback(paths, fallback_req, "vector_search_failed").await
                }
            }
        }
        Ok(Ok(Err(error))) => {
            tracing::warn!(%error, "API semantic query embedding unavailable; falling back to FTS");
            search_fts_fallback(paths, req, "query_embedding_failed").await
        }
        Ok(Err(error)) => {
            tracing::warn!(%error, "API query embedding task failed; falling back to FTS");
            search_fts_fallback(paths, req, "query_embedding_task_failed").await
        }
        Err(error) => {
            tracing::warn!(
                %error,
                timeout_sec = QUERY_EMBEDDING_TIMEOUT.as_secs(),
                "API query embedding timed out; falling back to FTS"
            );
            search_fts_fallback(paths, req, "query_embedding_timeout").await
        }
    }
}

async fn search_fts_fallback(
    paths: &AppPaths,
    req: cerul_search::SearchRequest,
    fallback_reason: &str,
) -> anyhow::Result<cerul_search::SearchResponse> {
    let started = Instant::now();
    let response = cerul_search::search_fts_only_with_diagnostics(
        paths,
        req,
        Some(fallback_reason.to_string()),
    )
    .await?;
    tracing::info!(
        retrieval_mode = %response.diagnostics.retrieval_mode,
        fallback_reason,
        fts_hits_count = response.diagnostics.fts_hits_count,
        search_ms = started.elapsed().as_millis(),
        "API search completed with FTS fallback"
    );
    Ok(response)
}

#[derive(Debug, Serialize)]
struct SearchHealthDiagnostics {
    item_count: usize,
    indexed_item_count: usize,
    search_index_version: i32,
    retrieval_unit_count: usize,
    unified_indexed_item_count: usize,
    items_needing_rebuild: usize,
    chunk_count: usize,
    searchable_text_chunk_count: usize,
    image_chunk_count: usize,
    fts_row_count: usize,
    retrieval_unit_fts_row_count: usize,
    orphan_job_count: usize,
    missing_raw_path_count: usize,
    embedding_profile_id: Option<String>,
    vector_index_collection: Option<String>,
    vector_index_point_count: Option<usize>,
    vector_index_text_collection: Option<String>,
    vector_index_image_collection: Option<String>,
    vector_index_text_points: Option<usize>,
    vector_index_image_points: Option<usize>,
    embedded_text_chunk_count: Option<usize>,
    embedded_image_chunk_count: Option<usize>,
    text_embedding_gap_count: Option<usize>,
    image_embedding_gap_count: Option<usize>,
    vector_index_error: Option<String>,
}

#[derive(Debug, Serialize)]
struct IndexingDiagnosticsResponse {
    #[serde(flatten)]
    indexing: jobs::IndexingDiagnostics,
    vector_index: IndexingVectorIndexDiagnostics,
}

#[derive(Debug, Serialize)]
struct IndexingVectorIndexDiagnostics {
    ready: bool,
    collection: Option<String>,
    point_count: Option<usize>,
    error: Option<String>,
}

async fn indexing_diagnostics(
    State(state): State<ApiState>,
) -> ApiResult<Json<IndexingDiagnosticsResponse>> {
    let indexing = jobs::indexing_diagnostics(&state.paths)?;
    let search = search_health_diagnostics(&state.paths).await?;
    Ok(Json(IndexingDiagnosticsResponse {
        indexing,
        vector_index: IndexingVectorIndexDiagnostics {
            ready: search.vector_index_error.is_none(),
            collection: search.vector_index_collection,
            point_count: search.vector_index_point_count,
            error: search.vector_index_error,
        },
    }))
}

async fn search_diagnostics(
    State(state): State<ApiState>,
) -> ApiResult<Json<SearchHealthDiagnostics>> {
    Ok(Json(search_health_diagnostics(&state.paths).await?))
}

#[derive(Debug, Serialize)]
struct SearchRebuildResponse {
    rebuild_queued_items: usize,
    queued_jobs: usize,
    diagnostics: SearchHealthDiagnostics,
}

async fn rebuild_search_index(
    State(state): State<ApiState>,
) -> ApiResult<Json<SearchRebuildResponse>> {
    let (rebuild_queued_items, queued_jobs) = queue_items_for_embedding_mode_rebuild(&state.paths)?;
    Ok(Json(SearchRebuildResponse {
        rebuild_queued_items,
        queued_jobs,
        diagnostics: search_health_diagnostics(&state.paths).await?,
    }))
}

async fn search_health_diagnostics(paths: &AppPaths) -> anyhow::Result<SearchHealthDiagnostics> {
    let conn = cerul_storage::sqlite::open(paths)?;
    let search_index_version = cerul_storage::SEARCH_INDEX_VERSION;
    let item_count = count_query(&conn, "SELECT COUNT(*) FROM items")?;
    let indexed_item_count =
        count_query(&conn, "SELECT COUNT(*) FROM items WHERE status = 'indexed'")?;
    let chunk_count = count_query(&conn, "SELECT COUNT(*) FROM chunks")?;
    let searchable_text_chunk_count = count_query(
        &conn,
        "SELECT COUNT(*) FROM chunks WHERE text IS NOT NULL AND TRIM(text) <> ''",
    )?;
    let image_chunk_count = count_query(
        &conn,
        "SELECT COUNT(*) FROM chunks WHERE frame_path IS NOT NULL AND TRIM(frame_path) <> ''",
    )?;
    let fts_row_count = count_query(&conn, "SELECT COUNT(*) FROM chunks_fts")?;
    let retrieval_unit_count = count_query(
        &conn,
        &format!(
            "SELECT COUNT(*) FROM retrieval_units WHERE index_version = {search_index_version}"
        ),
    )?;
    let retrieval_unit_fts_row_count =
        count_query(&conn, "SELECT COUNT(*) FROM retrieval_units_fts")?;
    let unified_indexed_item_count = count_query(
        &conn,
        &format!(
            "SELECT COUNT(*) FROM items WHERE search_index_version = {search_index_version} AND search_index_status = 'indexed'"
        ),
    )?;
    let items_needing_rebuild = count_query(
        &conn,
        &format!(
            r#"
        SELECT COUNT(*)
        FROM items
        WHERE status = 'indexed'
          AND (
            search_index_version IS NULL
            OR search_index_version != {search_index_version}
            OR search_index_status IS NULL
            OR search_index_status != 'indexed'
          )
        "#
        ),
    )?;
    let orphan_job_count = count_query(
        &conn,
        "SELECT COUNT(*) FROM jobs AS j LEFT JOIN items AS i ON i.id = j.item_id WHERE i.id IS NULL",
    )?;
    let missing_raw_path_count = count_missing_raw_paths(&conn)?;
    drop(conn);

    let mut diagnostics = SearchHealthDiagnostics {
        item_count,
        indexed_item_count,
        search_index_version: cerul_storage::SEARCH_INDEX_VERSION,
        retrieval_unit_count,
        unified_indexed_item_count,
        items_needing_rebuild,
        chunk_count,
        searchable_text_chunk_count,
        image_chunk_count,
        fts_row_count,
        retrieval_unit_fts_row_count,
        orphan_job_count,
        missing_raw_path_count,
        embedding_profile_id: None,
        vector_index_collection: None,
        vector_index_point_count: None,
        vector_index_text_collection: None,
        vector_index_image_collection: None,
        vector_index_text_points: None,
        vector_index_image_points: None,
        embedded_text_chunk_count: None,
        embedded_image_chunk_count: None,
        text_embedding_gap_count: None,
        image_embedding_gap_count: None,
        vector_index_error: None,
    };

    let profile = match cerul_storage::vectors::ensure_active_embedding_profile(paths) {
        Ok(profile) => profile,
        Err(error) => {
            tracing::warn!(%error, "failed to load active embedding profile for search diagnostics");
            diagnostics.vector_index_error = Some("embedding_profile_unavailable".to_string());
            return Ok(diagnostics);
        }
    };
    let collection = cerul_storage::vectors::unified_collection_name(
        paths,
        &profile,
        cerul_storage::SEARCH_INDEX_VERSION,
    );
    diagnostics.embedding_profile_id = Some(profile.id.clone());
    diagnostics.vector_index_collection = Some(collection.clone());

    let unified_points =
        cerul_storage::vectors::collection_point_count_for_profile(paths, &collection, &profile)
            .await;
    match unified_points {
        Ok(count) => {
            diagnostics.vector_index_point_count = Some(count);
            diagnostics.vector_index_text_points = Some(count);
            diagnostics.embedded_text_chunk_count = Some(count);
            diagnostics.text_embedding_gap_count = Some(retrieval_unit_count.saturating_sub(count));
        }
        Err(error) => {
            tracing::warn!(%error, collection, "failed to count vector index unified points for search diagnostics");
            diagnostics.vector_index_error = Some("vector_index_count_failed".to_string());
        }
    }
    diagnostics.vector_index_image_points = Some(0);
    diagnostics.embedded_image_chunk_count = Some(0);
    diagnostics.image_embedding_gap_count = Some(0);

    Ok(diagnostics)
}

fn count_missing_raw_paths(conn: &rusqlite::Connection) -> anyhow::Result<usize> {
    let mut stmt = conn.prepare(
        r#"
        SELECT raw_path
        FROM items
        WHERE raw_path IS NOT NULL
          AND TRIM(raw_path) <> ''
        "#,
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut missing = 0usize;
    for row in rows {
        let raw_path = row?;
        if !FsPath::new(&raw_path).exists() {
            missing += 1;
        }
    }
    Ok(missing)
}

fn count_query(conn: &rusqlite::Connection, sql: &str) -> rusqlite::Result<usize> {
    conn.query_row(sql, [], |row| row.get::<_, i64>(0))
        .map(|count| count.max(0) as usize)
}

#[derive(Debug, Serialize)]
struct DiagnosticsBundle {
    generated_at: u64,
    app_version: &'static str,
    runtime: DiagnosticsRuntime,
    settings: BTreeMap<String, Value>,
    local_models: Option<local_models::LocalPrepareStatus>,
    local_models_error: Option<String>,
    search: SearchHealthDiagnostics,
    jobs: Vec<DiagnosticsJob>,
    recent_errors: Vec<DiagnosticsItemError>,
}

#[derive(Debug, Serialize)]
struct DiagnosticsRuntime {
    platform: String,
    api_runtime_ready: bool,
    local_runtime_ready: bool,
    openai_ready: bool,
    gemini_ready: bool,
    configured_inference_mode: String,
    effective_inference_mode: String,
    last_error: Option<String>,
    local_runtime_error: Option<String>,
}

#[derive(Debug, Serialize)]
struct DiagnosticsJob {
    id: String,
    item_id: Option<String>,
    job_type: String,
    status: String,
    started_at: Option<i64>,
    finished_at: Option<i64>,
    progress: f64,
    stage: Option<String>,
    stage_message: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct DiagnosticsItemError {
    item_id: String,
    title: Option<String>,
    status: String,
    error: String,
}

const DIAGNOSTIC_SETTING_KEYS: &[&str] = &[
    "api_binding",
    API_PORT_SETTING,
    "asr_model",
    "concurrent_jobs",
    "inference_mode",
    "log_level",
    "model_download_source",
    "telemetry",
    "video_understanding_model",
    "whisper_model",
];

async fn diagnostics_bundle(State(state): State<ApiState>) -> ApiResult<Json<DiagnosticsBundle>> {
    let generated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let runtime_status = models::model_runtime_status(&state.paths);
    let configured_inference_mode = configured_inference_mode(&state.paths)?;
    let effective_inference_mode =
        effective_inference_mode_for_runtime(&configured_inference_mode, &runtime_status);
    let settings = diagnostics_settings_snapshot(
        &state.paths,
        &configured_inference_mode,
        &effective_inference_mode,
    )?;
    let (local_models, local_models_error) =
        match local_models::local_prepare_status_snapshot(&state.paths) {
            Ok(status) => (Some(status), None),
            Err(error) => (None, Some(redact_diagnostic_text(&error.to_string()))),
        };

    Ok(Json(DiagnosticsBundle {
        generated_at,
        app_version: env!("CARGO_PKG_VERSION"),
        runtime: DiagnosticsRuntime {
            platform: runtime_status.platform,
            api_runtime_ready: runtime_status.api_runtime_ready,
            local_runtime_ready: runtime_status.local_runtime_ready,
            openai_ready: runtime_status.openai_ready,
            gemini_ready: runtime_status.gemini_ready,
            configured_inference_mode,
            effective_inference_mode,
            last_error: runtime_status
                .last_error
                .map(|error| redact_diagnostic_text(&error)),
            local_runtime_error: runtime_status
                .local_runtime_error
                .map(|error| redact_diagnostic_text(&error)),
        },
        settings,
        local_models,
        local_models_error,
        search: search_health_diagnostics(&state.paths).await?,
        jobs: diagnostics_recent_jobs(&state.paths)?,
        recent_errors: diagnostics_recent_item_errors(&state.paths)?,
    }))
}

fn diagnostics_settings_snapshot(
    paths: &AppPaths,
    configured_inference_mode: &str,
    effective_inference_mode: &str,
) -> anyhow::Result<BTreeMap<String, Value>> {
    let conn = cerul_storage::sqlite::open(paths)?;
    let mut settings = BTreeMap::new();
    for key in DIAGNOSTIC_SETTING_KEYS {
        let value: Option<String> = conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        if let Some(value) = value {
            settings.insert(
                (*key).to_string(),
                normalize_setting_value(key, parse_json(&value)),
            );
        }
    }

    settings.insert(
        "configured_inference_mode".to_string(),
        Value::String(configured_inference_mode.to_string()),
    );
    settings.insert(
        "effective_inference_mode".to_string(),
        Value::String(effective_inference_mode.to_string()),
    );
    settings.insert(
        "remote_api_key_set".to_string(),
        Value::Bool(secret_setting_present(&conn, "remote_api_key")?),
    );

    Ok(settings)
}

fn secret_setting_present(conn: &rusqlite::Connection, key: &str) -> anyhow::Result<bool> {
    let value: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(value
        .and_then(|raw| parse_json(&raw).as_str().map(str::to_string))
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false))
}

fn diagnostics_recent_jobs(paths: &AppPaths) -> anyhow::Result<Vec<DiagnosticsJob>> {
    let conn = cerul_storage::sqlite::open(paths)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT id, item_id, job_type, status, started_at, finished_at, progress,
               stage, stage_message, error
        FROM jobs
        ORDER BY COALESCE(started_at, finished_at, 0) DESC, id DESC
        LIMIT 20
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(DiagnosticsJob {
            id: row.get(0)?,
            item_id: row.get(1)?,
            job_type: row.get(2)?,
            status: row.get(3)?,
            started_at: row.get(4)?,
            finished_at: row.get(5)?,
            progress: row.get(6)?,
            stage: row.get(7)?,
            stage_message: row
                .get::<_, Option<String>>(8)?
                .map(|message| redact_diagnostic_text(&message)),
            error: row
                .get::<_, Option<String>>(9)?
                .map(|error| redact_diagnostic_text(&error)),
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn diagnostics_recent_item_errors(paths: &AppPaths) -> anyhow::Result<Vec<DiagnosticsItemError>> {
    let conn = cerul_storage::sqlite::open(paths)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT id, title, status, error
        FROM items
        WHERE error IS NOT NULL
          AND TRIM(error) <> ''
        ORDER BY COALESCE(indexed_at, 0) DESC, id DESC
        LIMIT 20
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        let error: String = row.get(3)?;
        Ok(DiagnosticsItemError {
            item_id: row.get(0)?,
            title: row.get(1)?,
            status: row.get(2)?,
            error: redact_diagnostic_text(&error),
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn redact_diagnostic_text(value: &str) -> String {
    let mut redacted = value.to_string();
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            redacted = redacted.replace(&home, "~");
        }
    }
    redact_users_path_segments(&redacted)
}

fn redact_users_path_segments(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(index) = rest.find("/Users/") {
        output.push_str(&rest[..index]);
        let after_prefix = &rest[index + "/Users/".len()..];
        if let Some(next_slash) = after_prefix.find('/') {
            output.push_str("~/");
            rest = &after_prefix[next_slash + 1..];
        } else {
            output.push('~');
            rest = "";
        }
    }
    output.push_str(rest);
    output
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
        .results
        .into_iter()
        .filter(|result| !result.snippet.trim().is_empty())
        .take(limit)
        .map(|result| AskCitation {
            playback_chunk_id: result.playback_chunk_id,
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
    if should_discover_source_async(&req.source_type) {
        let source = create_syncing_source(&state.paths, req)?;
        start_source_discovery(state.paths.clone(), source.id.clone())?;
        return Ok(Json(source));
    }

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
    let plugin = cerul_sources::build(
        &req.source_type,
        source_config_with_web_access_settings(paths, &req.source_type, req.config.clone()),
    )?;
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

    persist_discovered_items(
        &tx,
        &id,
        content_type,
        &discovered_items,
        &mut items,
        &mut queued_jobs,
    )?;

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

fn should_discover_source_async(source_type: &str) -> bool {
    matches!(source_type, "youtube" | "web_video" | "rss_podcast")
}

fn create_syncing_source(paths: &AppPaths, req: AddSourceRequest) -> anyhow::Result<SourceRecord> {
    let id = new_id("source");
    let plugin = cerul_sources::build(&req.source_type, req.config.clone())?;
    primary_content_type(&*plugin)?;
    let config = req.config.to_string();
    let conn = cerul_storage::sqlite::open(paths)?;
    conn.execute(
        "INSERT INTO sources (id, type, config, status) VALUES (?1, ?2, ?3, 'syncing')",
        (&id, &req.source_type, &config),
    )?;
    source_by_id(paths, &id)
}

// Each discovery attempt stamps a fresh token into the source config before its
// async task is spawned; only the attempt whose token still matches may write
// the source's terminal status. Stamping synchronously keeps request order and
// attempt order aligned, so a late-starting older task cannot overwrite a newer
// retry token.
fn rotate_discovery_token(paths: &AppPaths, source_id: &str) -> anyhow::Result<String> {
    let token = new_id("disc");
    let conn = cerul_storage::sqlite::open(paths)?;
    conn.execute(
        "UPDATE sources SET config = json_set(config, '$.discovery_token', ?2) WHERE id = ?1",
        (source_id, &token),
    )?;
    Ok(token)
}

fn start_source_discovery(paths: AppPaths, source_id: String) -> anyhow::Result<()> {
    let token = rotate_discovery_token(&paths, &source_id)?;
    spawn_source_discovery(paths, source_id, token);
    Ok(())
}

fn spawn_source_discovery(paths: AppPaths, source_id: String, token: String) {
    tokio::spawn(async move {
        if let Err(error) = discover_source_items_to_paths(&paths, &source_id, &token).await {
            let message = error.to_string();
            if let Err(mark_error) =
                mark_source_discovery_error(&paths, &source_id, &token, &message)
            {
                tracing::warn!(
                    source_id,
                    error = %mark_error,
                    "failed to mark source discovery error"
                );
            }
            tracing::warn!(source_id, error = %message, "source discovery failed");
        }
    });
}

// After a restart, source discovery (unlike index jobs) is not otherwise resumed,
// so a source left mid-discovery would sit in `syncing` forever with no task
// running. Re-spawn discovery for every still-syncing source at startup.
fn resume_interrupted_source_discovery(paths: &AppPaths) {
    let ids = match syncing_source_ids(paths) {
        Ok(ids) => ids,
        Err(error) => {
            tracing::warn!(%error, "failed to list interrupted Cerul source discovery");
            return;
        }
    };
    for id in ids {
        tracing::info!(source_id = %id, "resuming interrupted source discovery after restart");
        if let Err(error) = start_source_discovery(paths.clone(), id.clone()) {
            tracing::warn!(source_id = %id, error = %error, "failed to resume source discovery");
        }
    }
}

fn syncing_source_ids(paths: &AppPaths) -> anyhow::Result<Vec<String>> {
    let conn = cerul_storage::sqlite::open(paths)?;
    let mut stmt = conn.prepare("SELECT id FROM sources WHERE status = 'syncing'")?;
    let ids = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

async fn discover_source_items_to_paths(
    paths: &AppPaths,
    source_id: &str,
    token: &str,
) -> anyhow::Result<()> {
    let source = source_by_id(paths, source_id)?;
    if source.status != "syncing" {
        return Ok(());
    }

    let plugin = cerul_sources::build(
        &source.source_type,
        source_config_with_web_access_settings(paths, &source.source_type, source.config.clone()),
    )?;
    let content_type = primary_content_type(&*plugin)?;
    let discovered_items = plugin.discover().await?;
    let mut conn = cerul_storage::sqlite::open(paths)?;
    let tx = conn.transaction()?;
    // Bail if this attempt is no longer the current one: a newer retry rotated the
    // token (so persisting our discovery would clobber theirs), or the source has
    // already left `syncing`.
    let current = tx
        .query_row(
            "SELECT status, json_extract(config, '$.discovery_token') FROM sources WHERE id = ?1",
            [source_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .optional()?;
    let is_current_attempt = matches!(
        &current,
        Some((status, tok)) if status == "syncing" && tok.as_deref() == Some(token)
    );
    if !is_current_attempt {
        tx.commit()?;
        return Ok(());
    }

    let mut items = Vec::with_capacity(discovered_items.len());
    let mut queued_jobs = 0;
    persist_discovered_items(
        &tx,
        source_id,
        content_type,
        &discovered_items,
        &mut items,
        &mut queued_jobs,
    )?;
    tx.execute(
        "UPDATE sources SET status = 'active', last_poll_at = strftime('%s','now') \
         WHERE id = ?1 AND status = 'syncing' AND json_extract(config, '$.discovery_token') = ?2",
        (source_id, token),
    )?;
    tx.commit()?;
    tracing::info!(
        source_id,
        discovered_items = items.len(),
        queued_jobs,
        "source discovery completed"
    );
    Ok(())
}

const WEB_VIDEO_COOKIE_MODE_SETTING: &str = "web_video_cookie_mode";
const WEB_VIDEO_COOKIE_BROWSER_SETTING: &str = "web_video_cookie_browser";
const WEB_VIDEO_COOKIES_PATH_SETTING: &str = "web_video_cookies_path";

fn source_config_with_web_access_settings(
    paths: &AppPaths,
    source_type: &str,
    config: Value,
) -> Value {
    if !matches!(source_type, "youtube" | "web_video") {
        return config;
    }
    let mut object = match config {
        Value::Object(object) => object,
        other => return other,
    };
    if has_source_cookie_config(&object) {
        return Value::Object(object);
    }

    let mode = setting_string(paths, WEB_VIDEO_COOKIE_MODE_SETTING)
        .ok()
        .flatten()
        .unwrap_or_else(|| "browser".to_string())
        .trim()
        .to_ascii_lowercase();
    match mode.as_str() {
        "browser" => {
            let browser = setting_string(paths, WEB_VIDEO_COOKIE_BROWSER_SETTING)
                .ok()
                .flatten()
                .unwrap_or_else(|| "chrome".to_string());
            let browser = browser.trim();
            if !browser.is_empty() {
                object.insert(
                    "cookies_from_browser".to_string(),
                    Value::String(browser.to_string()),
                );
            }
        }
        "file" => {
            if let Some(path) = setting_string(paths, WEB_VIDEO_COOKIES_PATH_SETTING)
                .ok()
                .flatten()
            {
                let path = path.trim();
                if !path.is_empty() {
                    object.insert("cookies_path".to_string(), Value::String(path.to_string()));
                }
            }
        }
        _ => {}
    }
    Value::Object(object)
}

fn has_source_cookie_config(object: &serde_json::Map<String, Value>) -> bool {
    [
        "cookies_from_browser",
        "cookie_browser",
        "ytdlp_cookies_from_browser",
        "ytdlp_cookie_browser",
        "cookies_path",
        "cookies_file",
        "ytdlp_cookies_path",
        "ytdlp_cookies_file",
    ]
    .iter()
    .any(|key| {
        object
            .get(*key)
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    })
}

fn persist_discovered_items(
    tx: &Transaction<'_>,
    source_id: &str,
    content_type: ContentType,
    discovered_items: &[DiscoveredItem],
    items: &mut Vec<AddedSourceItem>,
    queued_jobs: &mut usize,
) -> anyhow::Result<()> {
    for item in discovered_items {
        let Some(item_id) = upsert_discovered_item(tx, source_id, content_type, item)? else {
            continue;
        };
        let queued_job = enqueue_index_job(tx, &item_id, content_type)?;
        if queued_job {
            *queued_jobs += 1;
        }
        items.push(AddedSourceItem {
            id: item_id,
            external_id: Some(item.external_id.clone()),
            title: item.title.clone(),
            status: "discovered".to_string(),
            queued_job,
        });
    }
    Ok(())
}

fn mark_source_discovery_error(
    paths: &AppPaths,
    source_id: &str,
    token: &str,
    error: &str,
) -> anyhow::Result<()> {
    let conn = cerul_storage::sqlite::open(paths)?;
    let row = conn
        .query_row(
            "SELECT type, config FROM sources WHERE id = ?1",
            [source_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((source_type, config)) = row else {
        return Ok(());
    };
    let mut config = parse_json(&config);
    if !config.is_object() {
        config = json!({});
    }
    if let Some(config) = config.as_object_mut() {
        if matches!(source_type.as_str(), "youtube" | "web_video") {
            if let Some(info) = classify_job_error("index_video", error) {
                config.insert("last_error_code".to_string(), Value::String(info.code));
                config.insert(
                    "last_error_settings_section".to_string(),
                    Value::String(info.settings_section),
                );
            }
        }
        config.insert("last_error".to_string(), Value::String(error.to_string()));
        config.insert(
            "last_error_detail".to_string(),
            Value::String(error.to_string()),
        );
    }
    // Only fail a source that is still this discovery attempt: if a newer retry
    // rotated the token (or already moved the source out of `syncing`), a stale
    // failure from an earlier task must not clobber that result — mirrors the
    // token-guarded success path in discover_source_items_to_paths.
    conn.execute(
        "UPDATE sources SET status = 'error', config = ?2 \
         WHERE id = ?1 AND status = 'syncing' AND json_extract(config, '$.discovery_token') = ?3",
        (source_id, config.to_string(), token),
    )?;
    Ok(())
}

async fn remove_source(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let item_ids = cerul_storage::item_ids_for_source(&state.paths, &id)?;
    for item_id in item_ids {
        let item = cerul_storage::get_item(&state.paths, &item_id)?;
        if !item_has_running_jobs(&state.paths, &item.id)? {
            cleanup_item_artifacts(&state.paths, &item).await?;
        }
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

async fn retry_failed_source_items(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let mut conn = cerul_storage::sqlite::open(&state.paths)?;
    let source_exists: Option<String> = conn
        .query_row(
            "SELECT id FROM sources WHERE id = ?1",
            [id.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    if source_exists.is_none() {
        return Err(ApiError::not_found(format!("source not found: {id}")));
    }
    let failed_items = {
        let mut stmt = conn.prepare(
            r#"
            SELECT id, content_type
            FROM items
            WHERE source_id = ?1
              AND status = 'failed'
            ORDER BY title COLLATE NOCASE, id ASC
            "#,
        )?;
        let rows = stmt.query_map([id.as_str()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    for (item_id, _) in &failed_items {
        delete_item_embeddings_best_effort_async(&state.paths, item_id).await;
    }

    let tx = conn.transaction()?;
    let mut queued_jobs = 0usize;
    for (item_id, content_type) in &failed_items {
        let content_type = parse_content_type(content_type)?;
        tx.execute(
            r#"
            UPDATE items
            SET status = CASE
                    WHEN indexed_at IS NOT NULL THEN 'indexed'
                    ELSE 'discovered'
                END,
                error = NULL
            WHERE id = ?1
            "#,
            [item_id.as_str()],
        )?;
        tx.execute(
            "DELETE FROM item_understandings WHERE item_id = ?1",
            [item_id.as_str()],
        )?;
        tx.execute(
            "DELETE FROM chunks WHERE item_id = ?1 AND chunk_type = 'understanding'",
            [item_id.as_str()],
        )?;
        clear_generated_display_title_with_tx(&tx, item_id)?;
        clear_item_unified_search_index_with_tx(&tx, item_id)?;
        if enqueue_embedding_rebuild_job(&tx, item_id, content_type, true)? {
            queued_jobs += 1;
        }
    }
    tx.commit()?;

    Ok(Json(json!({
        "status": if queued_jobs > 0 { "queued" } else { "nothing_to_retry" },
        "id": id,
        "items": failed_items.len(),
        "queued_jobs": queued_jobs
    })))
}

async fn retry_source_discovery(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    queue_source_discovery_retry(&state.paths, &id)?;
    start_source_discovery(state.paths.clone(), id.clone())?;
    Ok(Json(json!({ "status": "syncing", "id": id })))
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

#[derive(Debug, Deserialize)]
struct ListJobsQuery {
    limit: Option<usize>,
    cursor: Option<usize>,
    status: Option<String>,
    source_id: Option<String>,
    item_id: Option<String>,
    scope: Option<String>,
    light: Option<bool>,
    include_usage: Option<bool>,
}

fn list_limit(limit: Option<usize>) -> usize {
    limit.unwrap_or(DEFAULT_LIST_LIMIT).clamp(1, MAX_LIST_LIMIT)
}

fn split_filter_values(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .take(32)
        .map(ToOwned::to_owned)
        .collect()
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
               i.raw_path, i.indexed_at, i.status, i.error, {metadata_expr} AS metadata,
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
        ORDER BY COALESCE(i.indexed_at, 0) DESC, i.id ASC
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
    let removed = tx.execute("DELETE FROM items WHERE id = ?1", [id.as_str()])?;
    if removed != 1 {
        return Err(ApiError::not_found(format!("item not found: {id}")));
    }
    tx.commit()?;

    Ok(Json(json!({ "status": "removed", "id": id })))
}

fn item_has_running_jobs(paths: &AppPaths, item_id: &str) -> anyhow::Result<bool> {
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

fn item_pipeline_cache_keys(item: &cerul_storage::StoredItem) -> Vec<String> {
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

async fn list_jobs(
    State(state): State<ApiState>,
    Query(query): Query<ListJobsQuery>,
) -> ApiResult<Json<Vec<JobRecord>>> {
    let limit = list_limit(query.limit);
    let offset = query.cursor.unwrap_or(0);
    let light = query.light.unwrap_or(false);
    let include_usage = query.include_usage.unwrap_or(!light);
    let statuses = split_filter_values(query.status.as_deref());
    let drawer_scope = statuses.is_empty()
        && query.scope.as_deref().map(str::trim).is_some_and(|scope| {
            scope.eq_ignore_ascii_case("drawer") || scope.eq_ignore_ascii_case("active")
        });
    let error_expr = if light { "NULL" } else { "j.error" };
    let stage_message_expr = if light { "NULL" } else { "j.stage_message" };
    let conn = cerul_storage::sqlite::open(&state.paths)?;
    let mut params: Vec<SqlValue> = Vec::new();
    let mut sql = format!(
        r#"
        SELECT j.id, j.item_id, j.job_type, j.status, j.started_at, j.finished_at,
               {error_expr} AS error, j.progress, j.stage, {stage_message_expr} AS stage_message
        FROM jobs j
        WHERE 1 = 1
        "#
    );
    if !statuses.is_empty() {
        sql.push_str(" AND j.status IN (");
        sql.push_str(
            &std::iter::repeat_n("?", statuses.len())
                .collect::<Vec<_>>()
                .join(", "),
        );
        sql.push(')');
        params.extend(statuses.into_iter().map(SqlValue::from));
    } else if drawer_scope {
        sql.push_str(
            r#"
            AND (
                j.status IN ('queued', 'running')
                OR (
                    j.status = 'completed'
                    AND COALESCE(j.finished_at, j.started_at, 0) >= strftime('%s','now') - 86400
                )
                OR (
                    j.status = 'failed'
                    AND COALESCE(j.finished_at, j.started_at, 0) >= strftime('%s','now') - 604800
                )
            )
            "#,
        );
    }
    if let Some(item_id) = query.item_id.filter(|value| !value.trim().is_empty()) {
        sql.push_str(" AND j.item_id = ?");
        params.push(SqlValue::from(item_id));
    }
    if let Some(source_id) = query.source_id.filter(|value| !value.trim().is_empty()) {
        sql.push_str(
            " AND EXISTS (SELECT 1 FROM items i WHERE i.id = j.item_id AND i.source_id = ?)",
        );
        params.push(SqlValue::from(source_id));
    }
    if drawer_scope {
        sql.push_str(
            r#"
            ORDER BY
                CASE
                    WHEN j.status = 'running' THEN 0
                    WHEN j.status = 'queued' THEN 1
                    ELSE 2
                END,
                COALESCE(j.finished_at, j.started_at, 0) DESC,
                j.id ASC
            LIMIT ? OFFSET ?
            "#,
        );
    } else {
        sql.push_str(
            r#"
            ORDER BY COALESCE(j.started_at, 0) DESC, j.id ASC
            LIMIT ? OFFSET ?
            "#,
        );
    }
    params.push(SqlValue::from(limit as i64));
    params.push(SqlValue::from(offset as i64));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), |row| {
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
    if include_usage {
        let job_ids = jobs.iter().map(|job| job.id.clone()).collect::<Vec<_>>();
        let mut usage_by_job =
            cerul_storage::usage_totals_by_job_ids(&state.paths, &job_ids).unwrap_or_default();
        for job in &mut jobs {
            job.usage = usage_by_job.remove(&job.id).unwrap_or_default();
        }
    }
    Ok(Json(jobs))
}

async fn cancel_job(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let cancelled = jobs::cancel_job(&state.paths, &id)?
        .ok_or_else(|| ApiError::not_found(format!("job not found: {id}")))?;
    if !cancelled.was_running {
        match cerul_storage::get_item(&state.paths, &cancelled.item_id) {
            Ok(item) if item.status == "indexed" => {
                tracing::info!(
                    item_id = %cancelled.item_id,
                    job_id = %id,
                    "skipped artifact cleanup for cancelled indexed-item rebuild"
                );
            }
            Ok(item) => cleanup_item_artifacts(&state.paths, &item).await?,
            Err(error) => tracing::warn!(
                %error,
                job_id = %id,
                item_id = %cancelled.item_id,
                "cancelled job item was not available for artifact cleanup"
            ),
        }
    }
    Ok(Json(json!({
        "status": "cancelled",
        "id": id,
        "item_id": cancelled.item_id,
        "cleanup_deferred": cancelled.was_running,
    })))
}

async fn usage_summary(
    State(state): State<ApiState>,
) -> ApiResult<Json<cerul_storage::UsageSummary>> {
    Ok(Json(cerul_storage::usage_summary(&state.paths)?))
}

async fn storage_usage(State(state): State<ApiState>) -> ApiResult<Json<StorageUsageResponse>> {
    Ok(Json(storage_usage_for_paths(&state.paths)?))
}

async fn storage_locations(
    State(state): State<ApiState>,
) -> ApiResult<Json<StorageLocationsResponse>> {
    Ok(Json(storage_locations_for_paths(&state.paths)))
}

async fn reset_local_library(State(state): State<ApiState>) -> ApiResult<Json<Value>> {
    let reset = reset_local_library_database(&state.paths)?;
    Ok(Json(json!({
        "status": "ok",
        "cleared": reset.cleared,
        "compacted": reset.compacted,
        "compaction_error": reset.compaction_error,
        "download_targets": reset.download_targets,
    })))
}

#[derive(Debug)]
struct LibraryResetResult {
    cleared: BTreeMap<String, usize>,
    compacted: bool,
    compaction_error: Option<String>,
    download_targets: Vec<String>,
}

fn reset_local_library_database(paths: &AppPaths) -> anyhow::Result<LibraryResetResult> {
    let mut conn = cerul_storage::sqlite::open(paths)?;
    let download_targets = local_library_download_targets(paths, &conn)?;
    let tx = conn.transaction()?;
    let mut cleared = BTreeMap::new();

    for (label, sql) in [
        (
            "usage_events",
            "DELETE FROM inference_usage_events WHERE item_id IS NOT NULL OR job_id IS NOT NULL",
        ),
        ("moments", "DELETE FROM moments"),
        ("retrieval_units", "DELETE FROM retrieval_units"),
        ("chunks", "DELETE FROM chunks"),
        ("item_understandings", "DELETE FROM item_understandings"),
        ("ignored_items", "DELETE FROM ignored_items"),
        ("jobs", "DELETE FROM jobs"),
        ("items", "DELETE FROM items"),
        ("sources", "DELETE FROM sources"),
    ] {
        let rows = tx.execute(sql, [])?;
        cleared.insert(label.to_string(), rows);
    }

    tx.commit()?;
    let compaction_error = compact_library_database(&conn).err().map(|error| {
        let message = error.to_string();
        tracing::warn!(%message, "failed to compact SQLite database after local library reset");
        message
    });
    Ok(LibraryResetResult {
        cleared,
        compacted: compaction_error.is_none(),
        compaction_error,
        download_targets,
    })
}

fn local_library_download_targets(
    paths: &AppPaths,
    conn: &rusqlite::Connection,
) -> anyhow::Result<Vec<String>> {
    let mut targets = BTreeSet::new();
    if let Some(media_dir) = read_setting_string(conn, "media_dir")? {
        targets.insert(PathBuf::from(media_dir).join("sources"));
    }

    let mut stmt = conn.prepare(
        r#"
        SELECT s.type, s.config, i.raw_path, i.metadata
        FROM items i
        JOIN sources s ON s.id = i.source_id
        WHERE s.type IN ('youtube', 'web_video', 'rss_podcast')
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;
    for row in rows {
        let (source_type, config, raw_path, metadata) = row?;
        if let Some(target) = download_target_from_source_config(&config, &source_type) {
            targets.insert(target);
        }
        let mut candidates = Vec::new();
        if let Some(raw_path) = raw_path {
            candidates.push(raw_path);
        }
        if let Some(raw_path) = metadata.as_deref().and_then(metadata_raw_path) {
            candidates.push(raw_path);
        }
        for candidate in candidates {
            if let Some(target) = download_target_from_raw_path(&candidate, &source_type) {
                targets.insert(target);
            }
        }
    }

    Ok(targets
        .into_iter()
        .filter(|target| target != &paths.cache)
        .filter(|target| !reset_target_conflicts_with_preserved_path(target, &paths.models))
        .map(|target| target.to_string_lossy().to_string())
        .collect())
}

fn read_setting_string(conn: &rusqlite::Connection, key: &str) -> anyhow::Result<Option<String>> {
    let raw: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(raw
        .and_then(|value| serde_json::from_str::<Value>(&value).ok())
        .and_then(|value| value.as_str().map(str::to_string))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty()))
}

fn metadata_raw_path(metadata: &str) -> Option<String> {
    serde_json::from_str::<Value>(metadata)
        .ok()
        .and_then(|value| {
            value
                .get("raw_path")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

fn download_target_from_source_config(config: &str, source_type: &str) -> Option<PathBuf> {
    let cache_dir = serde_json::from_str::<Value>(config)
        .ok()
        .and_then(|value| {
            value
                .get("cache_dir")
                .and_then(Value::as_str)
                .map(str::to_string)
        })?;
    download_target_from_cache_dir(&cache_dir, source_type)
}

fn download_target_from_cache_dir(cache_dir: &str, source_type: &str) -> Option<PathBuf> {
    let cache_dir = FsPath::new(cache_dir.trim());
    if cache_dir.as_os_str().is_empty() {
        return None;
    }
    if file_name_eq(cache_dir, "sources") {
        return Some(cache_dir.to_path_buf());
    }
    if file_name_eq(cache_dir, source_type) {
        let parent = cache_dir.parent()?;
        if file_name_eq(parent, "sources") {
            return Some(parent.to_path_buf());
        }
    }
    None
}

fn download_target_from_raw_path(raw_path: &str, source_type: &str) -> Option<PathBuf> {
    let mut current = FsPath::new(raw_path.trim()).parent();
    while let Some(dir) = current {
        if file_name_eq(dir, source_type) {
            let parent = dir.parent()?;
            if file_name_eq(parent, "sources") {
                return Some(parent.to_path_buf());
            }
        }
        current = dir.parent();
    }
    None
}

fn file_name_eq(path: &FsPath, expected: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == expected)
}

fn reset_target_conflicts_with_preserved_path(target: &FsPath, preserved: &FsPath) -> bool {
    path_contains(target, preserved) || path_contains(preserved, target)
}

fn path_contains(parent: &FsPath, candidate: &FsPath) -> bool {
    candidate == parent || candidate.starts_with(parent)
}

fn compact_library_database(conn: &rusqlite::Connection) -> anyhow::Result<()> {
    checkpoint_wal(conn)?;
    conn.execute_batch("VACUUM")?;
    checkpoint_wal(conn)?;
    Ok(())
}

fn checkpoint_wal(conn: &rusqlite::Connection) -> anyhow::Result<()> {
    let busy: i64 = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))?;
    anyhow::ensure!(busy == 0, "SQLite WAL checkpoint was busy");
    Ok(())
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
    let downloader_error = is_downloader_error(&normalized);
    let (code, friendly, settings_section) = if normalized.contains("sign in to confirm")
        && (normalized.contains("not a bot") || normalized.contains("cookies"))
    {
        (
            "browser_cookies_required",
            "该视频平台要求登录验证。连接浏览器登录状态后重试失败视频。".to_string(),
            "Indexing",
        )
    } else if downloader_error && is_browser_cookie_load_error_message(&normalized) {
        (
            "browser_cookies_unavailable",
            "无法读取所选浏览器的 Cookie。请选择已安装且可访问的浏览器，或改用 cookies.txt 后重试。"
                .to_string(),
            "Indexing",
        )
    } else if normalized.contains("members-only")
        || normalized.contains("available to this channel's members")
        || normalized.contains("channel's members")
    {
        (
            "members_only",
            "这是会员专享视频。只有连接的浏览器账号具备会员权限时才能下载。".to_string(),
            "Indexing",
        )
    } else if downloader_error
        && (normalized.contains("captcha")
            || normalized.contains("geetest")
            || normalized.contains("risk control")
            || normalized.contains("risk-control")
            || normalized.contains("http error 412")
            || normalized.contains("412: precondition failed")
            || normalized.contains("precondition failed")
            || normalized.contains("风控")
            || normalized.contains("验证码")
            || normalized.contains("v_voucher")
            || normalized.contains("verification required")
            || normalized.contains("verify you are human"))
    {
        (
            "platform_verification_required",
            "平台触发了风控或验证码。使用浏览器登录态后稍后重试。".to_string(),
            "Indexing",
        )
    } else if downloader_error
        && (normalized.contains("http error 429")
            || normalized.contains("429: too many requests")
            || normalized.contains("too many requests")
            || normalized.contains("rate limit")
            || normalized.contains("rate-limit")
            || normalized.contains("限流"))
    {
        (
            "rate_limited",
            "平台暂时限流下载请求。稍后重试，或减少作者主页导入数量。".to_string(),
            "Indexing",
        )
    } else if normalized.contains("yt-dlp")
        && (normalized.contains("update")
            || normalized.contains("out of date")
            || normalized.contains("outdated")
            || normalized.contains("please update"))
    {
        (
            "downloader_outdated",
            "视频下载器可能过旧，需要更新后重试。".to_string(),
            "About",
        )
    } else if (downloader_error
        && (normalized.contains("http error 401")
            || normalized.contains("401: unauthorized")
            || normalized.contains("unauthorized")
            || normalized.contains("401")))
        || normalized.contains("http error 403")
        || normalized.contains("403: forbidden")
    {
        (
            "download_forbidden",
            "平台拒绝下载请求。连接浏览器登录状态，稍后再重试失败视频。".to_string(),
            "Indexing",
        )
    } else if normalized.contains("this video is not available")
        || normalized.contains("video unavailable")
    {
        (
            "video_unavailable",
            "该视频已不可用或对当前地区不可见。".to_string(),
            "",
        )
    } else if normalized.contains("no supported javascript runtime") {
        (
            "downloader_runtime_missing",
            "下载器缺少 YouTube 需要的 JavaScript 运行时，部分视频可能无法下载。".to_string(),
            "Indexing",
        )
    } else if !downloader_error
        && (normalized.contains("api key")
            || normalized.contains("missing key")
            || normalized.contains("no key")
            || normalized.contains("unauthorized")
            || normalized.contains("401"))
    {
        (
            "missing_api_key",
            format!("{capability} 连接缺少可用 API 密钥。"),
            "Models",
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
            "Models",
        )
    } else if normalized.contains("ffmpeg") {
        (
            "ffmpeg_unavailable",
            "本机视频处理运行时不可用，需要修复本地工具链。".to_string(),
            "Models",
        )
    } else if normalized.contains("yt-dlp")
        || normalized.contains("video unavailable")
        || normalized.contains("private")
        || normalized.contains("geo")
    {
        (
            "source_unavailable",
            "来源暂时不可访问，可能是私有、地区限制或下载器失效。".to_string(),
            "Sources",
        )
    } else if normalized.trim().is_empty() {
        return None;
    } else {
        (
            "unknown_processing_error",
            format!("{capability} 处理失败，需要查看技术详情。"),
            "",
        )
    };

    Some(JobErrorInfo {
        code: code.to_string(),
        capability,
        settings_section: settings_section.to_string(),
        message: friendly,
    })
}

fn is_browser_cookie_load_error_message(normalized: &str) -> bool {
    normalized.contains("browser cookie load failed")
        || normalized.contains("cookie database")
        || normalized.contains("cookies database")
        || normalized.contains("failed to decrypt")
        || normalized.contains("unsupported browser")
        || normalized.contains("keyring")
        || (normalized.contains("browser cookies")
            && (normalized.contains("could not")
                || normalized.contains("cannot")
                || normalized.contains("can't")
                || normalized.contains("failed")
                || normalized.contains("permission denied")
                || normalized.contains("no such file")
                || normalized.contains("unable")))
        || (normalized.contains("cookies from browser")
            && (normalized.contains("could not")
                || normalized.contains("cannot")
                || normalized.contains("can't")
                || normalized.contains("failed")
                || normalized.contains("permission denied")
                || normalized.contains("no such file")
                || normalized.contains("unable")))
}

fn is_downloader_error(normalized: &str) -> bool {
    normalized.contains("yt-dlp")
        || normalized.contains("[bilibili]")
        || normalized.contains("[youtube]")
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
        let value = validate_write_setting_value(key, normalize_setting_value(key, value.clone()))?;
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

fn validate_write_setting_value(key: &str, value: Value) -> ApiResult<Value> {
    if key == API_PORT_SETTING {
        let port = match &value {
            Value::Number(number) => number.as_u64().and_then(|value| u16::try_from(value).ok()),
            Value::String(value) => parse_api_port(value),
            _ => None,
        }
        .filter(|port| (1024..=65535).contains(port))
        .ok_or_else(|| ApiError::bad_request("api_port must be an integer from 1024 to 65535"))?;
        return Ok(Value::from(port));
    }
    Ok(value)
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
    let current_vector_backend = setting_string(paths, VECTOR_INDEX_BACKEND_SETTING)?;
    if current == Some(INDEXING_SCHEMA_VERSION)
        && current_vector_backend.as_deref() == Some(ACTIVE_VECTOR_INDEX_BACKEND)
    {
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
    conn.execute(
        r#"
        INSERT INTO settings (key, value, updated_at)
        VALUES (?1, ?2, strftime('%s','now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at
        "#,
        (
            VECTOR_INDEX_BACKEND_SETTING,
            Value::from(ACTIVE_VECTOR_INDEX_BACKEND).to_string(),
        ),
    )?;
    tracing::info!(
        previous_version = ?current,
        version = INDEXING_SCHEMA_VERSION,
        previous_vector_backend = ?current_vector_backend,
        vector_backend = ACTIVE_VECTOR_INDEX_BACKEND,
        rebuild_items,
        queued_jobs,
        "indexing schema or vector backend changed; queued media rebuild"
    );
    Ok(())
}

fn repair_indexed_item_status_from_artifacts(paths: &AppPaths) -> anyhow::Result<usize> {
    let conn = cerul_storage::sqlite::open(paths)?;
    let item_ids = {
        let mut stmt = conn.prepare(
            r#"
            SELECT id, metadata
            FROM items
            WHERE status IN ('discovered', 'fetching', 'processing', 'failed')
              AND (indexed_at IS NULL OR status = 'failed')
              AND metadata IS NOT NULL
            ORDER BY id ASC
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter_map(|(id, metadata)| {
                metadata
                    .as_deref()
                    .filter(|value| metadata_has_indexed_artifacts(value))
                    .map(|_| id)
            })
            .collect::<Vec<_>>()
    };

    for item_id in &item_ids {
        conn.execute(
            r#"
            UPDATE items
            SET status = 'indexed',
                indexed_at = COALESCE(
                    indexed_at,
                    (
                        SELECT MAX(finished_at)
                        FROM jobs
                        WHERE item_id = ?1
                          AND status = 'completed'
                          AND finished_at IS NOT NULL
                    ),
                    strftime('%s','now')
                ),
                error = NULL
            WHERE id = ?1
              AND status IN ('discovered', 'fetching', 'processing', 'failed')
              AND (indexed_at IS NULL OR status = 'failed')
            "#,
            [item_id.as_str()],
        )?;
    }

    Ok(item_ids.len())
}

fn metadata_has_indexed_artifacts(metadata: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(metadata) else {
        return false;
    };
    [
        "embedding_index_status",
        "transcript_index_status",
        "visual_index_status",
        "ocr_index_status",
    ]
    .into_iter()
    .any(|key| value.get(key).and_then(Value::as_str) == Some("indexed"))
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
                "UPDATE items SET error = NULL WHERE id = ?1",
                [item_id.as_str()],
            )?;
        }
        if enqueue_embedding_rebuild_job(&tx, item_id, content_type, false)? {
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
    dedupe_running: bool,
) -> anyhow::Result<bool> {
    let job_type = index_job_type(content_type);
    let active_statuses = if dedupe_running {
        &["queued", "running"][..]
    } else {
        &["queued"][..]
    };
    let mut params = vec![
        SqlValue::from(item_id.to_string()),
        SqlValue::from(job_type.to_string()),
    ];
    params.extend(
        active_statuses
            .iter()
            .map(|status| SqlValue::from((*status).to_string())),
    );
    let status_placeholders = std::iter::repeat_n("?", active_statuses.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        r#"
        SELECT COUNT(*)
        FROM jobs
        WHERE item_id = ?
          AND job_type = ?
          AND status IN ({status_placeholders})
        "#
    );
    let existing_active: i64 =
        tx.query_row(&sql, rusqlite::params_from_iter(params.iter()), |row| {
            row.get(0)
        })?;
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

fn clear_item_unified_search_index_with_tx(
    tx: &Transaction<'_>,
    item_id: &str,
) -> anyhow::Result<()> {
    tx.execute(
        "DELETE FROM retrieval_units WHERE item_id = ?1 AND index_version = ?2",
        (item_id, cerul_storage::SEARCH_INDEX_VERSION),
    )?;
    tx.execute(
        r#"
        UPDATE items
        SET search_index_version = ?2,
            search_index_status = 'pending',
            search_index_error = NULL,
            search_index_unit_count = 0,
            search_index_vector_count = 0
        WHERE id = ?1
        "#,
        (item_id, cerul_storage::SEARCH_INDEX_VERSION),
    )?;
    Ok(())
}

fn clear_generated_display_title_with_tx(
    tx: &Transaction<'_>,
    item_id: &str,
) -> anyhow::Result<()> {
    let current: Option<String> = tx.query_row(
        "SELECT metadata FROM items WHERE id = ?1",
        [item_id],
        |row| row.get(0),
    )?;
    let Some(raw_metadata) = current.filter(|value| !value.trim().is_empty()) else {
        return Ok(());
    };
    let mut metadata: Value = serde_json::from_str(&raw_metadata)?;
    let Some(object) = metadata.as_object_mut() else {
        return Ok(());
    };
    if object.remove("display_title").is_none() {
        return Ok(());
    }
    tx.execute(
        "UPDATE items SET metadata = ?2 WHERE id = ?1",
        (item_id, serde_json::to_string(&metadata)?),
    )?;
    Ok(())
}

pub(crate) fn refresh_item_retrieval_units_after_understanding_update(
    paths: &AppPaths,
    item_id: &str,
    dedupe_running: bool,
    delete_embeddings: bool,
    queue_rebuild: bool,
) -> anyhow::Result<bool> {
    if delete_embeddings {
        delete_item_embeddings_best_effort(paths, item_id);
    }
    let profile = cerul_storage::vectors::ensure_active_embedding_profile(paths)?;
    let units = cerul_storage::rebuild_item_retrieval_units(paths, item_id, &profile.id)?;

    let mut conn = cerul_storage::sqlite::open(paths)?;
    let tx = conn.transaction()?;
    let queued_job = if queue_rebuild {
        let content_type: String = tx.query_row(
            "SELECT content_type FROM items WHERE id = ?1",
            [item_id],
            |row| row.get(0),
        )?;
        let content_type = parse_content_type(&content_type)?;
        let vector_count = if delete_embeddings {
            0
        } else {
            tx.query_row(
                "SELECT COALESCE(search_index_vector_count, 0) FROM items WHERE id = ?1",
                [item_id],
                |row| row.get::<_, i64>(0),
            )?
            .max(0)
        };
        tx.execute(
            r#"
            UPDATE items
            SET search_index_version = ?2,
                search_index_status = 'pending',
                search_index_error = NULL,
                search_index_unit_count = ?3,
                search_index_vector_count = ?4
            WHERE id = ?1
            "#,
            (
                item_id,
                cerul_storage::SEARCH_INDEX_VERSION,
                units.len() as i64,
                vector_count,
            ),
        )?;
        enqueue_embedding_rebuild_job(&tx, item_id, content_type, dedupe_running)?
    } else {
        let vector_count = tx
            .query_row(
                "SELECT COALESCE(search_index_vector_count, 0) FROM items WHERE id = ?1",
                [item_id],
                |row| row.get::<_, i64>(0),
            )?
            .max(0);
        tx.execute(
            r#"
            UPDATE items
            SET search_index_version = ?2,
                search_index_status = 'pending',
                search_index_error = NULL,
                search_index_unit_count = ?3,
                search_index_vector_count = ?4
            WHERE id = ?1
            "#,
            (
                item_id,
                cerul_storage::SEARCH_INDEX_VERSION,
                units.len() as i64,
                vector_count,
            ),
        )?;
        false
    };
    tx.commit()?;
    Ok(queued_job)
}

fn delete_item_embeddings_best_effort(paths: &AppPaths, item_id: &str) {
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => {
            if let Err(error) = runtime.block_on(cerul_storage::vectors::delete_item_embeddings(
                paths, item_id,
            )) {
                tracing::warn!(
                    item_id,
                    %error,
                    "failed to delete stale item vectors before retrieval refresh"
                );
            }
        }
        Err(error) => {
            tracing::warn!(
                item_id,
                %error,
                "failed to create runtime for stale vector cleanup"
            );
        }
    }
}

async fn delete_item_embeddings_best_effort_async(paths: &AppPaths, item_id: &str) {
    if let Err(error) = cerul_storage::vectors::delete_item_embeddings(paths, item_id).await {
        tracing::warn!(
            item_id,
            %error,
            "failed to delete stale item vectors before retrieval refresh"
        );
    }
}

fn set_source_status(paths: &AppPaths, id: &str, status: &str) -> anyhow::Result<()> {
    let conn = cerul_storage::sqlite::open(paths)?;
    let updated = conn.execute("UPDATE sources SET status = ?1 WHERE id = ?2", (status, id))?;
    anyhow::ensure!(updated == 1, "source not found: {id}");
    Ok(())
}

fn queue_source_discovery_retry(paths: &AppPaths, id: &str) -> anyhow::Result<()> {
    let conn = cerul_storage::sqlite::open(paths)?;
    let row = conn
        .query_row(
            "SELECT type, config FROM sources WHERE id = ?1",
            [id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((source_type, config)) = row else {
        anyhow::bail!("source not found: {id}");
    };
    anyhow::ensure!(
        should_discover_source_async(&source_type),
        "source type does not support discovery retry: {source_type}"
    );

    let mut config = parse_json(&config);
    if let Some(object) = config.as_object_mut() {
        object.remove("last_error");
        object.remove("last_error_detail");
        object.remove("last_error_code");
        object.remove("last_error_settings_section");
    }
    conn.execute(
        "UPDATE sources SET status = 'syncing', config = ?2 WHERE id = ?1",
        (id, config.to_string()),
    )?;
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
) -> anyhow::Result<Option<String>> {
    if is_discovered_item_ignored(tx, source_id, item)? {
        return Ok(None);
    }

    let content_type = content_type_value(content_type);
    let raw_path = item.metadata.get("raw_path").and_then(Value::as_str);
    let metadata = item.metadata.to_string();
    let has_exact_existing = existing_item_for_source_external(tx, source_id, &item.external_id)?;
    if !has_exact_existing {
        if let Some(existing) = existing_item_for_raw_path(tx, source_id, raw_path)? {
            let external_id_changed =
                existing.external_id.as_deref() != Some(item.external_id.as_str());
            if external_id_changed {
                tx.execute(
                    "DELETE FROM chunks WHERE item_id = ?1",
                    [existing.id.as_str()],
                )?;
                clear_item_unified_search_index_with_tx(tx, &existing.id)?;
            }
            tx.execute(
                r#"
            UPDATE items
            SET content_type = ?2,
                external_id = ?3,
                title = ?4,
                duration_sec = ?5,
                raw_path = ?6,
                metadata = ?7,
                error = NULL,
                indexed_at = CASE
                    WHEN status = 'indexed' AND external_id = ?3 THEN indexed_at
                    WHEN status IN ('fetching', 'processing') THEN indexed_at
                    ELSE NULL
                END,
                status = CASE
                    WHEN status = 'indexed' AND external_id = ?3 THEN status
                    WHEN status IN ('fetching', 'processing') THEN status
                    ELSE 'discovered'
                END
            WHERE id = ?1
              AND status != 'deleting'
            "#,
                (
                    existing.id.as_str(),
                    content_type,
                    item.external_id.as_str(),
                    item.title.as_deref(),
                    item.duration_sec,
                    raw_path,
                    metadata.as_str(),
                ),
            )?;
            return Ok(Some(existing.id));
        }
    }

    let item_id = new_id("item");

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

    Ok(Some(tx.query_row(
        "SELECT id FROM items WHERE source_id = ?1 AND external_id = ?2",
        (source_id, item.external_id.as_str()),
        |row| row.get(0),
    )?))
}

#[derive(Debug)]
struct ExistingItemForRawPath {
    id: String,
    external_id: Option<String>,
}

fn existing_item_for_raw_path(
    tx: &Transaction<'_>,
    source_id: &str,
    raw_path: Option<&str>,
) -> anyhow::Result<Option<ExistingItemForRawPath>> {
    let Some(raw_path) = raw_path.map(str::trim).filter(|path| !path.is_empty()) else {
        return Ok(None);
    };
    Ok(tx
        .query_row(
            r#"
            SELECT id, external_id
            FROM items
            WHERE raw_path = ?1
              AND source_id = ?2
              AND status != 'deleting'
            ORDER BY
                CASE status
                    WHEN 'indexed' THEN 0
                    WHEN 'processing' THEN 1
                    WHEN 'fetching' THEN 2
                    ELSE 3
                END,
                id ASC
            LIMIT 1
            "#,
            (raw_path, source_id),
            |row| {
                Ok(ExistingItemForRawPath {
                    id: row.get(0)?,
                    external_id: row.get(1)?,
                })
            },
        )
        .optional()?)
}

fn existing_item_for_source_external(
    tx: &Transaction<'_>,
    source_id: &str,
    external_id: &str,
) -> anyhow::Result<bool> {
    let count: i64 = tx.query_row(
        r#"
        SELECT COUNT(*)
        FROM items
        WHERE source_id = ?1
          AND external_id = ?2
          AND status != 'deleting'
        "#,
        (source_id, external_id),
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn is_discovered_item_ignored(
    tx: &Transaction<'_>,
    source_id: &str,
    item: &DiscoveredItem,
) -> anyhow::Result<bool> {
    let raw_path = item
        .metadata
        .get("raw_path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty());
    let ignored: i64 = tx.query_row(
        r#"
        SELECT COUNT(*)
        FROM ignored_items
        WHERE source_id = ?1
          AND (
              external_id = ?2
              OR (
                  ?3 IS NOT NULL
                  AND raw_path = ?3
              )
          )
        "#,
        (source_id, item.external_id.as_str(), raw_path),
        |row| row.get(0),
    )?;
    Ok(ignored > 0)
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
    let raw_path: Option<String> = row.get(6)?;

    Ok(ItemRecord {
        id: row.get(0)?,
        source_id: row.get(1)?,
        content_type: row.get(2)?,
        external_id: row.get(3)?,
        title: row.get(4)?,
        duration_sec: row.get(5)?,
        raw_path,
        raw_path_exists: None,
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

fn attach_raw_path_exists(item: &mut ItemRecord) {
    item.raw_path_exists = item
        .raw_path
        .as_deref()
        .map(|path| FsPath::new(path).is_file());
}

fn attach_item_usage(paths: &AppPaths, items: &mut [ItemRecord]) {
    // Single GROUP BY query; per-item lookups opened one SQLite connection
    // per row and made GET /items O(n) connections.
    let item_ids = items.iter().map(|item| item.id.clone()).collect::<Vec<_>>();
    let mut totals = cerul_storage::usage_totals_by_item_ids(paths, &item_ids).unwrap_or_default();
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
            if !has_timed_video_clip_start(start_sec) {
                return Ok(None);
            }
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

fn clip_window(
    start_sec: Option<f64>,
    end_sec: Option<f64>,
    before_sec: f64,
    after_sec: f64,
) -> (f64, f64) {
    let start = start_sec.unwrap_or(0.0).max(0.0);
    let fallback_end = start + 12.0;
    let end = end_sec
        .filter(|end| end.is_finite() && *end > start)
        .unwrap_or(fallback_end);
    // Per-side extension capped at 30s; total duration capped at 120s so a
    // stray request can't ask ffmpeg for a giant clip.
    let before = if before_sec.is_finite() {
        before_sec.clamp(0.0, 30.0)
    } else {
        2.0
    };
    let after = if after_sec.is_finite() {
        after_sec.clamp(0.0, 30.0)
    } else {
        2.0
    };
    let clipped_start = (start - before).max(0.0);
    let duration = (end + after - clipped_start).clamp(1.0, 120.0);
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
    let data = path_usage(&paths.data)?;
    let database = path_usage(&paths.db)?;
    let models = path_usage(&paths.models)?;
    let index = path_usage(&paths.vector_index)?;
    let cache = path_usage(&paths.cache)?;
    // Downloads can be redirected to an external disk via the `media_dir` setting.
    // When that path is outside the data dir, its media is invisible to the
    // data-dir scan — measure it explicitly so large external downloads don't
    // silently vanish from the usage total (the disk space is real even if it
    // isn't under the app data dir).
    let external_downloads = external_download_usage(paths)?;
    let known_bytes = database
        .bytes
        .saturating_add(models.bytes)
        .saturating_add(index.bytes)
        .saturating_add(cache.bytes);
    let known_apparent_bytes = database
        .apparent_bytes
        .saturating_add(models.apparent_bytes)
        .saturating_add(index.apparent_bytes)
        .saturating_add(cache.apparent_bytes);
    // "Other" is whatever inside the data dir we didn't attribute; external
    // downloads sit outside it and get their own category instead.
    let other = PathUsage {
        bytes: data.bytes.saturating_sub(known_bytes),
        apparent_bytes: data.apparent_bytes.saturating_sub(known_apparent_bytes),
    };
    let total = add_path_usage(data, external_downloads.unwrap_or_default());

    let mut categories = vec![
        storage_category("database", "Database", database),
        storage_category("models", "Models", models),
        storage_category("index", "Search index", index),
        storage_category("cache", "Cache", cache),
        storage_category("other", "Other", other),
    ];
    if let Some(downloads) = external_downloads {
        categories.push(storage_category("downloads", "Downloads", downloads));
    }

    Ok(StorageUsageResponse {
        data_dir: paths.data.to_string_lossy().to_string(),
        total_bytes: total.bytes,
        total_apparent_bytes: total.apparent_bytes,
        categories,
    })
}

fn storage_locations_for_paths(paths: &AppPaths) -> StorageLocationsResponse {
    StorageLocationsResponse {
        data_dir: paths.data.to_string_lossy().to_string(),
        database_path: paths.db.to_string_lossy().to_string(),
        models_dir: paths.models.to_string_lossy().to_string(),
        index_dir: paths.vector_index.to_string_lossy().to_string(),
        cache_dir: paths.cache.to_string_lossy().to_string(),
    }
}

// Measures downloaded media when it lives outside the data dir (the `media_dir`
// setting points at an external location). Returns None when downloads default to
// the in-data cache (already counted by the data scan) or the directory is
// missing/empty.
fn external_download_usage(paths: &AppPaths) -> anyhow::Result<Option<PathUsage>> {
    let media_dir = match cerul_storage::read_string_setting(paths, "media_dir") {
        Ok(Some(dir)) if !dir.trim().is_empty() => PathBuf::from(dir.trim()),
        _ => return Ok(None),
    };
    // Downloads are written under `<media_dir>/sources` (see source_download_dir).
    let downloads_root = media_dir.join("sources");
    if downloads_root.starts_with(&paths.data) {
        // Already inside the data dir, so the scan above counts it.
        return Ok(None);
    }
    let usage = path_usage(&downloads_root)?;
    if usage.bytes == 0 && usage.apparent_bytes == 0 {
        return Ok(None);
    }
    Ok(Some(usage))
}

fn storage_category(key: &str, label: &str, usage: PathUsage) -> StorageUsageCategory {
    StorageUsageCategory {
        key: key.to_string(),
        label: label.to_string(),
        bytes: usage.bytes,
        apparent_bytes: usage.apparent_bytes,
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct PathUsage {
    bytes: u64,
    apparent_bytes: u64,
}

fn path_usage(path: &FsPath) -> anyhow::Result<PathUsage> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(metadata_usage(&metadata)),
        Ok(metadata) if !metadata.is_dir() => Ok(PathUsage::default()),
        Ok(_metadata) => {
            let mut total = PathUsage::default();
            let mut stack = vec![path.to_path_buf()];
            while let Some(current) = stack.pop() {
                for entry in fs::read_dir(current)? {
                    let entry = entry?;
                    let metadata = fs::symlink_metadata(entry.path())?;
                    if metadata.is_dir() {
                        stack.push(entry.path());
                    } else if metadata.is_file() {
                        total = add_path_usage(total, metadata_usage(&metadata));
                    }
                }
            }
            Ok(total)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(PathUsage::default()),
        Err(error) => Err(error.into()),
    }
}

fn add_path_usage(left: PathUsage, right: PathUsage) -> PathUsage {
    PathUsage {
        bytes: left.bytes.saturating_add(right.bytes),
        apparent_bytes: left.apparent_bytes.saturating_add(right.apparent_bytes),
    }
}

fn metadata_usage(metadata: &fs::Metadata) -> PathUsage {
    PathUsage {
        bytes: allocated_bytes(metadata),
        apparent_bytes: metadata.len(),
    }
}

#[cfg(unix)]
fn allocated_bytes(metadata: &fs::Metadata) -> u64 {
    metadata.blocks().saturating_mul(512)
}

#[cfg(not(unix))]
fn allocated_bytes(metadata: &fs::Metadata) -> u64 {
    metadata.len()
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
    ("/internal/health", &["get"]),
    ("/internal/metrics", &["get"]),
    ("/internal/openapi.json", &["get"]),
    ("/internal/diagnostics", &["get"]),
    ("/internal/diagnostics/indexing", &["get"]),
    ("/internal/search", &["post"]),
    ("/internal/search/diagnostics", &["get"]),
    ("/internal/search/rebuild", &["post"]),
    ("/internal/ask", &["post"]),
    ("/internal/sources", &["get", "post"]),
    ("/internal/sources/preview/rss", &["post"]),
    ("/internal/sources/{id}", &["delete"]),
    ("/internal/sources/{id}/pause", &["post"]),
    ("/internal/sources/{id}/resume", &["post"]),
    ("/internal/sources/{id}/retry-failed", &["post"]),
    ("/internal/sources/{id}/retry-discovery", &["post"]),
    ("/internal/moments", &["get", "post"]),
    ("/internal/moments/{id}", &["delete"]),
    ("/internal/entities", &["get"]),
    ("/internal/entities/{id}", &["get"]),
    ("/internal/weekly-review", &["get"]),
    ("/internal/items", &["get"]),
    ("/internal/items/{id}", &["get", "patch", "delete"]),
    ("/internal/items/{id}/playback", &["get", "patch"]),
    ("/internal/items/{id}/reindex", &["post"]),
    ("/internal/items/{id}/chunks", &["get"]),
    ("/internal/items/{id}/understanding", &["get", "post"]),
    ("/internal/chunks/{id}/frame", &["get"]),
    ("/internal/chunks/{id}/video-segment", &["get"]),
    ("/internal/chunks/{id}/video-clip", &["get"]),
    ("/internal/jobs", &["get"]),
    ("/internal/jobs/{id}/cancel", &["post"]),
    ("/internal/usage/events", &["get"]),
    ("/internal/usage/summary", &["get"]),
    ("/internal/storage/usage", &["get"]),
    ("/internal/storage/locations", &["get"]),
    ("/internal/storage/reset-library", &["post"]),
    ("/internal/models/catalog", &["get"]),
    ("/internal/models/whisper", &["get"]),
    ("/internal/models/whisper/{id}/download", &["post"]),
    ("/internal/models/whisper/auto-download-status", &["get"]),
    ("/internal/models/embed/status", &["get"]),
    ("/internal/models/embed/prepare", &["post"]),
    ("/internal/models/local/capability", &["get"]),
    ("/internal/models/local/prepare", &["post"]),
    ("/internal/models/local/prepare-status", &["get"]),
    ("/internal/models/local/prepare-cancel", &["post"]),
    ("/internal/models/local/delete", &["post"]),
    ("/internal/models/local/repair", &["post"]),
    ("/internal/providers", &["get", "post"]),
    ("/internal/providers/{id}", &["patch", "delete"]),
    ("/internal/providers/{id}/test", &["post"]),
    ("/internal/providers/{id}/models", &["get"]),
    ("/internal/settings", &["get", "patch"]),
];

const V1_API_PATHS: &[(&str, &[&str])] = &[
    ("/v1/status", &["get"]),
    ("/v1/openapi.json", &["get"]),
    ("/v1/search", &["post"]),
    ("/v1/ask", &["post"]),
    ("/v1/items", &["get"]),
    ("/v1/items/{id}", &["get"]),
    ("/v1/items/{id}/chunks", &["get"]),
    ("/v1/chunks/{id}/frame", &["get"]),
    ("/v1/chunks/{id}/video-segment", &["get"]),
    ("/v1/chunks/{id}/video-clip", &["get"]),
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
const VECTOR_INDEX_BACKEND_SETTING: &str = "vector_index_backend";
const ACTIVE_VECTOR_INDEX_BACKEND: &str = "zvec";
const INDEXING_SCHEMA_VERSION: i32 = 5;
const INTERNAL_SETTING_KEYS: &[&str] = &[
    DEFERRED_EMBEDDING_REBUILD_MODE_SETTING,
    INDEXING_SCHEMA_VERSION_SETTING,
    VECTOR_INDEX_BACKEND_SETTING,
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
            VALUES
                (?1, ?2, strftime('%s','now')),
                (?3, ?4, strftime('%s','now'))
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = excluded.updated_at
            "#,
            (
                INDEXING_SCHEMA_VERSION_SETTING,
                Value::from(INDEXING_SCHEMA_VERSION).to_string(),
                VECTOR_INDEX_BACKEND_SETTING,
                Value::from(ACTIVE_VECTOR_INDEX_BACKEND).to_string(),
            ),
        )
        .unwrap();
    }

    #[test]
    fn classify_youtube_bot_check_as_browser_cookie_failure() {
        let info = classify_job_error(
            "index_video",
            "yt-dlp fetch failed: ERROR: [youtube] abc: Sign in to confirm you’re not a bot. Use --cookies-from-browser or --cookies for the authentication.",
        )
        .unwrap();

        assert_eq!(info.code, "browser_cookies_required");
        assert_eq!(info.settings_section, "Indexing");
    }

    #[test]
    fn classify_browser_cookie_load_failure_before_platform_fallback() {
        let info = classify_job_error(
            "index_video",
            "yt-dlp single discovery failed: Browser cookie load failed before retrying without browser cookies:\nERROR: could not find Chrome cookies database\n\nRetry without browser cookies also failed:\nERROR: [BiliBili] BV1xx: HTTP Error 412: Precondition Failed",
        )
        .unwrap();

        assert_eq!(info.code, "browser_cookies_unavailable");
        assert_eq!(info.settings_section, "Indexing");
    }

    #[test]
    fn classify_bilibili_risk_control_as_platform_verification() {
        let info = classify_job_error(
            "index_video",
            "yt-dlp fetch failed: ERROR: [BiliBili] BV1xx: risk control triggered; verification required with captcha.",
        )
        .unwrap();

        assert_eq!(info.code, "platform_verification_required");
        assert_eq!(info.settings_section, "Indexing");
    }

    #[test]
    fn classify_bilibili_precondition_failed_as_platform_verification() {
        let info = classify_job_error(
            "index_video",
            "yt-dlp fetch failed: ERROR: [BiliBili] BV1xx: Unable to download JSON metadata: HTTP Error 412: Precondition Failed",
        )
        .unwrap();

        assert_eq!(info.code, "platform_verification_required");
        assert_eq!(info.settings_section, "Indexing");
    }

    #[test]
    fn classify_downloader_unauthorized_as_download_forbidden() {
        let info = classify_job_error(
            "index_video",
            "yt-dlp author discovery failed: ERROR: [BiliBili] BV1xx: HTTP Error 401: Unauthorized",
        )
        .unwrap();

        assert_eq!(info.code, "download_forbidden");
        assert_eq!(info.settings_section, "Indexing");
    }

    #[test]
    fn classify_provider_unauthorized_as_missing_api_key() {
        let info = classify_job_error(
            "index_video",
            "embedding provider returned HTTP Error 401: Unauthorized; missing API key",
        )
        .unwrap();

        assert_eq!(info.code, "missing_api_key");
        assert_eq!(info.settings_section, "Models");
    }

    #[test]
    fn classify_rate_limit_as_rate_limited() {
        let info = classify_job_error(
            "index_video",
            "yt-dlp fetch failed: ERROR: HTTP Error 429: Too Many Requests",
        )
        .unwrap();

        assert_eq!(info.code, "rate_limited");
        assert_eq!(info.settings_section, "Indexing");
    }

    #[test]
    fn classify_provider_rate_limit_does_not_use_downloader_guidance() {
        let info = classify_job_error(
            "index_video",
            "embedding provider returned HTTP Error 429: Too Many Requests",
        )
        .unwrap();

        assert_ne!(info.code, "rate_limited");
    }

    #[test]
    fn classify_ytdlp_update_error_as_downloader_outdated() {
        let info = classify_job_error(
            "index_video",
            "yt-dlp fetch failed: ERROR: This extractor is out of date; please update yt-dlp.",
        )
        .unwrap();

        assert_eq!(info.code, "downloader_outdated");
        assert_eq!(info.settings_section, "About");
    }

    #[test]
    fn source_config_with_web_access_settings_defaults_to_browser_cookies() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let config = source_config_with_web_access_settings(
            &paths,
            "web_video",
            json!({ "url": "https://www.bilibili.com/video/BV1abc123456" }),
        );

        assert_eq!(config["cookies_from_browser"].as_str(), Some("chrome"));
    }

    #[test]
    fn source_discovery_error_stores_friendly_message_and_raw_detail() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        conn.execute(
            "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'web_video', '{}', 'syncing')",
            [],
        )
        .unwrap();
        let token = rotate_discovery_token(&paths, "source-1").unwrap();

        mark_source_discovery_error(
            &paths,
            "source-1",
            &token,
            "yt-dlp author discovery failed: ERROR: [BiliBili] BV1xx: HTTP Error 412: Precondition Failed",
        )
        .unwrap();

        let (status, raw_config): (String, String) = conn
            .query_row(
                "SELECT status, config FROM sources WHERE id = 'source-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let config: Value = serde_json::from_str(&raw_config).unwrap();

        assert_eq!(status, "error");
        assert_eq!(
            config["last_error_code"].as_str(),
            Some("platform_verification_required")
        );
        assert!(config["last_error"]
            .as_str()
            .is_some_and(|message| message.contains("HTTP Error 412")));
        assert!(config["last_error_detail"]
            .as_str()
            .is_some_and(|message| message.contains("HTTP Error 412")));
    }

    #[test]
    fn rss_source_discovery_error_keeps_raw_feed_error() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        conn.execute(
            "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'rss_podcast', '{}', 'syncing')",
            [],
        )
        .unwrap();
        let token = rotate_discovery_token(&paths, "source-1").unwrap();

        mark_source_discovery_error(
            &paths,
            "source-1",
            &token,
            "RSS feed discovery failed: HTTP Error 401: Unauthorized",
        )
        .unwrap();

        let raw_config: String = conn
            .query_row(
                "SELECT config FROM sources WHERE id = 'source-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let config: Value = serde_json::from_str(&raw_config).unwrap();

        assert!(config.get("last_error_code").is_none());
        assert!(config["last_error"]
            .as_str()
            .is_some_and(|message| message.contains("401")));
    }

    #[test]
    fn stale_discovery_failure_does_not_override_a_retried_source() {
        // Reproduces the retry race: an original discovery task that fails *after*
        // the user hits "retry" must not flip the still-syncing source to `error`
        // and strand the retry. The retry rotates the token, so the original task's
        // late failure is recognized as stale and ignored.
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        conn.execute(
            "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'web_video', '{}', 'syncing')",
            [],
        )
        .unwrap();

        let stale = rotate_discovery_token(&paths, "source-1").unwrap();
        // A retry supersedes the original attempt with a fresh token.
        let _fresh = rotate_discovery_token(&paths, "source-1").unwrap();

        mark_source_discovery_error(&paths, "source-1", &stale, "late discovery failure").unwrap();

        let status: String = conn
            .query_row(
                "SELECT status FROM sources WHERE id = 'source-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "syncing");
    }

    #[test]
    fn source_config_with_web_access_settings_injects_browser_cookie_setting() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        conn.execute(
            r#"
            INSERT INTO settings (key, value, updated_at)
            VALUES
              ('web_video_cookie_mode', '"browser"', strftime('%s','now')),
              ('web_video_cookie_browser', '"safari"', strftime('%s','now'))
            "#,
            [],
        )
        .unwrap();

        let config = source_config_with_web_access_settings(
            &paths,
            "web_video",
            json!({ "url": "https://www.youtube.com/watch?v=abc123" }),
        );

        assert_eq!(config["cookies_from_browser"].as_str(), Some("safari"));
    }

    #[test]
    fn queue_source_discovery_retry_marks_source_syncing_and_clears_error() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                r#"
                INSERT INTO sources (id, type, config, status)
                VALUES (
                    'source-1',
                    'web_video',
                    '{"url":"https://www.bilibili.com/video/BV1aa411c7mD","last_error":"old","last_error_detail":"old detail","last_error_code":"platform_verification_required","last_error_settings_section":"Indexing"}',
                    'error'
                )
                "#,
                [],
            )
            .unwrap();
        }

        queue_source_discovery_retry(&paths, "source-1").unwrap();

        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        let (status, raw_config): (String, String) = conn
            .query_row(
                "SELECT status, config FROM sources WHERE id = 'source-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let config = parse_json(&raw_config);

        assert_eq!(status, "syncing");
        assert_eq!(
            config["url"].as_str(),
            Some("https://www.bilibili.com/video/BV1aa411c7mD")
        );
        assert!(config.get("last_error").is_none());
        assert!(config.get("last_error_detail").is_none());
        assert!(config.get("last_error_code").is_none());
        assert!(config.get("last_error_settings_section").is_none());
    }

    #[tokio::test]
    async fn retry_failed_source_items_requeues_failed_items() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'web_video', '{}', 'active')",
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO items (
                    id, source_id, content_type, external_id, title, status, error, metadata
                )
                VALUES (
                    'item-1', 'source-1', 'video', 'video-1', 'Video 1', 'failed', 'bot check',
                    '{"display_title":"Old generated title"}'
                )
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO item_understandings (item_id, status, result, error)
                VALUES ('item-1', 'failed', '{}', 'old understanding failure')
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO chunks (id, item_id, chunk_type, text, metadata)
                VALUES ('item-1:understanding:summary', 'item-1', 'understanding', 'old understanding text', '{}')
                "#,
                [],
            )
            .unwrap();
        }
        let profile = cerul_storage::vectors::ensure_active_embedding_profile(&paths).unwrap();
        cerul_storage::replace_item_retrieval_units(
            &paths,
            "item-1",
            &[cerul_storage::StorageRetrievalUnit {
                id: "item-1:unit:v2:000000".to_string(),
                item_id: "item-1".to_string(),
                unit_index: 0,
                unit_kind: "summary".to_string(),
                start_sec: None,
                end_sec: None,
                content_text: "old understanding text".to_string(),
                transcript_text: None,
                ocr_text: None,
                visual_text: None,
                summary_text: Some("old understanding text".to_string()),
                representative_chunk_id: Some("item-1:understanding:summary".to_string()),
                representative_frame_path: None,
                embedding_profile_id: profile.id,
                index_version: cerul_storage::SEARCH_INDEX_VERSION,
                metadata: Default::default(),
            }],
        )
        .unwrap();
        cerul_storage::set_item_search_index_status(&paths, "item-1", "indexed", None, 1, 1)
            .unwrap();

        let app = router_with_paths(paths.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/internal/sources/source-1/retry-failed")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "retry failed: {body}");
        assert_eq!(body["status"], "queued");
        assert_eq!(body["items"], 1);
        assert_eq!(body["queued_jobs"], 1);

        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        let item: (String, Option<String>, String) = conn
            .query_row(
                "SELECT status, error, metadata FROM items WHERE id = 'item-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!((item.0, item.1), ("discovered".to_string(), None));
        let item_metadata = parse_json(&item.2);
        assert!(item_metadata.get("display_title").is_none());

        let understanding_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM item_understandings WHERE item_id = 'item-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(understanding_count, 0);
        let understanding_chunk_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE item_id = 'item-1' AND chunk_type = 'understanding'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(understanding_chunk_count, 0);
        let retrieval_unit_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM retrieval_units WHERE item_id = 'item-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let search_index_state: (String, i64, i64) = conn
            .query_row(
                r#"
                SELECT search_index_status, search_index_unit_count, search_index_vector_count
                FROM items
                WHERE id = 'item-1'
                "#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(retrieval_unit_count, 0);
        assert_eq!(search_index_state, ("pending".to_string(), 0, 0));

        let queued_jobs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM jobs WHERE item_id = 'item-1' AND job_type = 'index_video' AND status = 'queued'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(queued_jobs, 1);
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
                    .uri("/internal/health")
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
                    .uri("/internal/openapi.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(openapi.status(), StatusCode::OK);
        let openapi_json = response_json(openapi).await;
        let paths = openapi_json["paths"].as_object().unwrap();
        assert!(paths.len() >= 19);
        assert!(paths["/internal/items/{id}"].get("patch").is_some());
        assert!(paths["/internal/jobs/{id}/cancel"].get("post").is_some());
    }

    #[tokio::test]
    async fn router_serves_v1_status_and_openapi() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'local', '{}', 'active')",
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO items (
                    id, source_id, content_type, external_id, title, status, indexed_at, metadata
                )
                VALUES
                    ('item-indexed', 'source-1', 'video', 'video-1', 'Indexed', 'indexed', 10, '{}'),
                    ('item-processing', 'source-1', 'video', 'video-2', 'Processing', 'processing', NULL, '{}'),
                    ('item-failed', 'source-1', 'video', 'video-3', 'Failed', 'failed', NULL, '{}')
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO jobs (id, item_id, job_type, status, progress)
                VALUES ('job-queued', 'item-processing', 'index_video', 'queued', 0)
                "#,
                [],
            )
            .unwrap();
        }
        seed_indexing_schema_version(&paths);
        cerul_storage::set_item_search_index_status(&paths, "item-indexed", "indexed", None, 0, 0)
            .unwrap();
        let app = router_with_paths(paths);

        let status = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(status.status(), StatusCode::OK);
        let status_json = response_json(status).await;
        assert!(status_json["request_id"]
            .as_str()
            .unwrap()
            .starts_with("req-"));
        assert_eq!(status_json["status"], "ok");
        assert_eq!(status_json["execution"]["target"], "local");
        assert_eq!(status_json["execution"]["privacy"], "local_only");
        assert_eq!(status_json["library"]["total_items"], 3);
        assert_eq!(status_json["library"]["indexed_items"], 1);
        assert_eq!(status_json["library"]["processing_items"], 1);
        assert_eq!(status_json["library"]["failed_items"], 1);
        assert_eq!(status_json["indexing"]["queued_jobs"], 1);
        assert_eq!(status_json["account"]["signed_in"], false);
        assert_eq!(
            status_json["capabilities"],
            json!(["status", "openapi", "search", "ask", "items", "chunks"])
        );

        let openapi = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/openapi.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(openapi.status(), StatusCode::OK);
        let openapi_json = response_json(openapi).await;
        let paths = openapi_json["paths"].as_object().unwrap();
        assert!(paths.contains_key("/v1/status"));
        assert!(paths.contains_key("/v1/openapi.json"));
        assert!(paths.contains_key("/v1/search"));
        assert!(paths.contains_key("/v1/ask"));
        assert!(paths.contains_key("/v1/items"));
        assert!(paths.contains_key("/v1/items/{id}"));
        assert!(paths.contains_key("/v1/items/{id}/chunks"));
        assert!(paths.contains_key("/v1/chunks/{id}/frame"));
        assert!(paths.contains_key("/v1/chunks/{id}/video-segment"));
        assert!(paths.contains_key("/v1/chunks/{id}/video-clip"));
        assert!(!paths.contains_key("/internal/health"));
        assert!(!paths.contains_key("/health"));
    }

    fn seed_v1_agent_search_fixture(paths: &AppPaths, raw_path: &FsPath) {
        fs::write(raw_path, b"not a real video").unwrap();
        let raw_path_string = raw_path.to_string_lossy().to_string();
        let frame_path = raw_path.with_file_name("frame.jpg");
        fs::write(&frame_path, b"not a real frame").unwrap();
        let frame_path_string = frame_path.to_string_lossy().to_string();
        {
            let conn = cerul_storage::sqlite::open(paths).unwrap();
            conn.execute(
                "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'local', '{}', 'active')",
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
                    'item-1', 'source-1', 'video', 'video-1', 'Scaling Talk', 120.5,
                    ?1, 10, 'indexed', '{}'
                )
                "#,
                [raw_path_string.as_str()],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, text, metadata)
                VALUES (
                    'item-1:transcript:000000',
                    'item-1',
                    'transcript',
                    12.3,
                    18.0,
                    'The talk says scaling laws keep holding across larger training runs.',
                    '{}'
                )
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, frame_path, metadata)
                VALUES (
                    'item-1:keyframe:000012',
                    'item-1',
                    'keyframe',
                    12.0,
                    12.0,
                    ?1,
                    '{}'
                )
                "#,
                [frame_path_string.as_str()],
            )
            .unwrap();
        }
        seed_indexing_schema_version(paths);
        let profile = cerul_storage::vectors::ensure_active_embedding_profile(paths).unwrap();
        let units = vec![cerul_storage::StorageRetrievalUnit {
            id: "item-1:unit:v2:000000".to_string(),
            item_id: "item-1".to_string(),
            unit_index: 0,
            unit_kind: "transcript".to_string(),
            start_sec: Some(12.3),
            end_sec: Some(18.0),
            content_text: "The talk says scaling laws keep holding across larger training runs."
                .to_string(),
            transcript_text: Some("scaling laws keep holding".to_string()),
            ocr_text: None,
            visual_text: None,
            summary_text: None,
            representative_chunk_id: Some("item-1:transcript:000000".to_string()),
            representative_frame_path: None,
            embedding_profile_id: profile.id,
            index_version: cerul_storage::SEARCH_INDEX_VERSION,
            metadata: Default::default(),
        }];
        cerul_storage::replace_item_retrieval_units(paths, "item-1", &units).unwrap();
        cerul_storage::set_item_search_index_status(
            paths,
            "item-1",
            "indexed",
            None,
            units.len(),
            0,
        )
        .unwrap();
    }

    fn contract_shape(value: &Value) -> Value {
        match value {
            Value::Null => Value::Null,
            Value::Bool(_) => Value::from("boolean"),
            Value::Number(_) => Value::from("number"),
            Value::String(_) => Value::from("string"),
            Value::Array(values) => {
                Value::Array(values.iter().map(contract_shape).collect::<Vec<_>>())
            }
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(key, value)| (key.clone(), contract_shape(value)))
                    .collect(),
            ),
        }
    }

    fn assert_contract_shape(name: &str, actual: &Value, expected: Value) {
        assert_eq!(
            contract_shape(actual),
            expected,
            "{name} contract shape changed"
        );
    }

    #[tokio::test]
    async fn v1_golden_contract_shapes_cover_agent_endpoints() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let raw_path = temp.path().join("video.mp4");
        seed_v1_agent_search_fixture(&paths, &raw_path);
        let app = router_with_paths(paths);

        let openapi = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/openapi.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(openapi.status(), StatusCode::OK);
        let openapi = response_json(openapi).await;
        assert_eq!(openapi["openapi"], "3.1.0");
        assert_eq!(openapi["info"]["title"], "Cerul Agent API");
        assert_eq!(
            openapi["paths"],
            json!({
                "/v1/status": {"get": {"responses": {"200": {"description": "OK"}}}},
                "/v1/openapi.json": {"get": {"responses": {"200": {"description": "OK"}}}},
                "/v1/search": {"post": {"responses": {"200": {"description": "OK"}}}},
                "/v1/ask": {"post": {"responses": {"200": {"description": "OK"}}}},
                "/v1/items": {"get": {"responses": {"200": {"description": "OK"}}}},
                "/v1/items/{id}": {"get": {"responses": {"200": {"description": "OK"}}}},
                "/v1/items/{id}/chunks": {"get": {"responses": {"200": {"description": "OK"}}}},
                "/v1/chunks/{id}/frame": {"get": {"responses": {"200": {"description": "OK"}}}},
                "/v1/chunks/{id}/video-segment": {"get": {"responses": {"200": {"description": "OK"}}}},
                "/v1/chunks/{id}/video-clip": {"get": {"responses": {"200": {"description": "OK"}}}}
            })
        );

        let status = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(status.status(), StatusCode::OK);
        let status = response_json(status).await;
        assert_contract_shape(
            "v1 status",
            &status,
            json!({
                "request_id": "string",
                "status": "string",
                "version": "string",
                "execution": {"target": "string", "account_id": null, "privacy": "string"},
                "library": {
                    "total_items": "number",
                    "indexed_items": "number",
                    "processing_items": "number",
                    "failed_items": "number",
                    "chunk_count": "number"
                },
                "search": {
                    "ready": "boolean",
                    "retrieval_mode": "string",
                    "text_ready": "boolean",
                    "vector_ready": "boolean"
                },
                "indexing": {"paused": "boolean", "active_jobs": "number", "queued_jobs": "number"},
                "account": {"signed_in": "boolean", "plan": null, "credits_remaining": null},
                "capabilities": ["string", "string", "string", "string", "string", "string"]
            }),
        );

        let search = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/search")
                    .header(header::HOST, "127.0.0.1:25101")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"query": "scaling laws", "max_results": 1}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(search.status(), StatusCode::OK);
        let search = response_json(search).await;
        assert_contract_shape(
            "v1 search",
            &search,
            json!({
                "request_id": "string",
                "execution": {"target": "string", "account_id": null, "privacy": "string"},
                "results": [{
                    "id": "string",
                    "type": "string",
                    "source": "string",
                    "item": {
                        "id": "string",
                        "title": "string",
                        "content_type": "string",
                        "source_type": "string",
                        "duration_sec": "number"
                    },
                    "time": {"start_sec": "number", "end_sec": "number", "timestamp": "string"},
                    "text": {"snippet": "string", "quote": "string"},
                    "evidence": {
                        "id": "string",
                        "kind": "string",
                        "clip": {"type": "string", "url": "string"},
                        "preview": {"type": "string", "url": "string"},
                        "open_in_cerul": "string"
                    },
                    "score": {"match": "number", "exact_match": "boolean", "similarity": null}
                }],
                "diagnostics": {
                    "retrieval_mode": "string",
                    "fallback_reason": "string",
                    "vector_hits": "number",
                    "text_hits": "number",
                    "result_count": "number"
                },
                "usage": {
                    "billable": "boolean",
                    "metered_events": [
                        {"capability": "string", "quantity": "number", "credits": "number"},
                        {"capability": "string", "quantity": "number", "credits": "number"}
                    ],
                    "credits_used": "number"
                }
            }),
        );

        let ask = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/ask")
                    .header(header::HOST, "127.0.0.1:25102")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"question": "scaling laws", "max_results": 1}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ask.status(), StatusCode::OK);
        let ask = response_json(ask).await;
        assert_contract_shape(
            "v1 ask",
            &ask,
            json!({
                "request_id": "string",
                "execution": {"target": "string", "account_id": null, "privacy": "string"},
                "mode": "string",
                "answer": "string",
                "citations": [{
                    "id": "string",
                    "type": "string",
                    "source": "string",
                    "item": {
                        "id": "string",
                        "title": "string",
                        "content_type": "string",
                        "source_type": "string",
                        "duration_sec": "number"
                    },
                    "time": {"start_sec": "number", "end_sec": "number", "timestamp": "string"},
                    "text": {"snippet": "string", "quote": "string"},
                    "evidence": {
                        "id": "string",
                        "kind": "string",
                        "clip": {"type": "string", "url": "string"},
                        "preview": {"type": "string", "url": "string"},
                        "open_in_cerul": "string"
                    },
                    "score": {"match": "number", "exact_match": "boolean", "similarity": null}
                }],
                "warnings": [],
                "usage": {
                    "billable": "boolean",
                    "metered_events": [
                        {"capability": "string", "quantity": "number", "credits": "number"},
                        {"capability": "string", "quantity": "number", "credits": "number"}
                    ],
                    "credits_used": "number"
                }
            }),
        );

        let items = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/items?status=indexed&limit=1")
                    .header(header::HOST, "127.0.0.1:25103")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(items.status(), StatusCode::OK);
        let items = response_json(items).await;
        assert_contract_shape(
            "v1 items",
            &items,
            json!({
                "request_id": "string",
                "execution": {"target": "string", "account_id": null, "privacy": "string"},
                "items": [{
                    "id": "string",
                    "title": "string",
                    "content_type": "string",
                    "source_type": "string",
                    "source_url": null,
                    "status": "string",
                    "duration_sec": "number",
                    "indexed_at": "number",
                    "chunk_count": "number",
                    "thumbnail": {"type": "string", "url": "string"},
                    "open_in_cerul": "string"
                }],
                "page": {"limit": "number", "next_cursor": null}
            }),
        );

        let item = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/items/item-1")
                    .header(header::HOST, "127.0.0.1:25104")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(item.status(), StatusCode::OK);
        let item = response_json(item).await;
        assert_contract_shape(
            "v1 item",
            &item,
            json!({
                "request_id": "string",
                "execution": {"target": "string", "account_id": null, "privacy": "string"},
                "item": {
                    "id": "string",
                    "title": "string",
                    "content_type": "string",
                    "source_type": "string",
                    "source_url": null,
                    "status": "string",
                    "duration_sec": "number",
                    "indexed_at": "number",
                    "chunk_count": "number",
                    "thumbnail": {"type": "string", "url": "string"},
                    "open_in_cerul": "string"
                }
            }),
        );

        let chunks = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/items/item-1/chunks?type=transcript&limit=1")
                    .header(header::HOST, "127.0.0.1:25105")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(chunks.status(), StatusCode::OK);
        let chunks = response_json(chunks).await;
        assert_contract_shape(
            "v1 chunks",
            &chunks,
            json!({
                "request_id": "string",
                "execution": {"target": "string", "account_id": null, "privacy": "string"},
                "item": {
                    "id": "string",
                    "title": "string",
                    "content_type": "string",
                    "source_type": "string",
                    "source_url": null,
                    "status": "string",
                    "duration_sec": "number",
                    "indexed_at": "number",
                    "chunk_count": "number",
                    "thumbnail": {"type": "string", "url": "string"},
                    "open_in_cerul": "string"
                },
                "chunks": [{
                    "id": "string",
                    "type": "string",
                    "source": "string",
                    "time": {"start_sec": "number", "end_sec": "number", "timestamp": "string"},
                    "text": {"content": "string", "snippet": "string"},
                    "evidence": {
                        "id": "string",
                        "kind": "string",
                        "clip": {"type": "string", "url": "string"},
                        "preview": null,
                        "open_in_cerul": "string"
                    }
                }],
                "page": {"limit": "number", "next_cursor": null}
            }),
        );

        // This golden fixture uses placeholder video bytes, so segment/clip binary
        // behavior stays covered by `v1_chunk_binary_routes_resolve_agent_evidence_urls`;
        // here the media contract locks the deterministic frame endpoint and the
        // OpenAPI paths for all three evidence media routes.
        let frame = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/chunks/item-1:keyframe:000012/frame")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(frame.status(), StatusCode::OK);
        assert_eq!(
            frame.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/jpeg"
        );
        let bytes = to_bytes(frame.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&bytes[..], b"not a real frame");
    }

    fn seed_v1_untimed_summary_fixture(paths: &AppPaths, raw_path: &FsPath) {
        fs::write(raw_path, b"not a real video").unwrap();
        let raw_path_string = raw_path.to_string_lossy().to_string();
        {
            let conn = cerul_storage::sqlite::open(paths).unwrap();
            conn.execute(
                "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'local', '{}', 'active')",
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
                    'item-1', 'source-1', 'video', 'video-1', 'Summary Talk', 120.5,
                    ?1, 10, 'indexed', '{}'
                )
                "#,
                [raw_path_string.as_str()],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO chunks (id, item_id, chunk_type, text, metadata)
                VALUES (
                    'item-1:understanding:summary',
                    'item-1',
                    'understanding',
                    'Untimed executive summary about launch planning.',
                    '{}'
                )
                "#,
                [],
            )
            .unwrap();
        }
        seed_indexing_schema_version(paths);
        let profile = cerul_storage::vectors::ensure_active_embedding_profile(paths).unwrap();
        let units = vec![cerul_storage::StorageRetrievalUnit {
            id: "item-1:unit:v2:summary".to_string(),
            item_id: "item-1".to_string(),
            unit_index: 0,
            unit_kind: "understanding".to_string(),
            start_sec: None,
            end_sec: None,
            content_text: "Untimed executive summary about launch planning.".to_string(),
            transcript_text: None,
            ocr_text: None,
            visual_text: None,
            summary_text: Some("Untimed executive summary about launch planning.".to_string()),
            representative_chunk_id: Some("item-1:understanding:summary".to_string()),
            representative_frame_path: None,
            embedding_profile_id: profile.id,
            index_version: cerul_storage::SEARCH_INDEX_VERSION,
            metadata: Default::default(),
        }];
        cerul_storage::replace_item_retrieval_units(paths, "item-1", &units).unwrap();
        cerul_storage::set_item_search_index_status(
            paths,
            "item-1",
            "indexed",
            None,
            units.len(),
            0,
        )
        .unwrap();
    }

    #[tokio::test]
    async fn v1_items_omit_thumbnail_when_frame_file_is_missing() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let missing_frame = temp.path().join("missing-frame.jpg");
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'local', '{}', 'active')",
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO items (
                    id, source_id, content_type, external_id, title, duration_sec,
                    indexed_at, status, metadata
                )
                VALUES (
                    'item-1', 'source-1', 'video', 'video-1', 'Clip', 10,
                    10, 'indexed', '{}'
                )
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO chunks (id, item_id, chunk_type, start_sec, frame_path, metadata)
                VALUES ('item-1:keyframe:000000', 'item-1', 'keyframe', 0, ?1, '{}')
                "#,
                [missing_frame.to_string_lossy().as_ref()],
            )
            .unwrap();
        }
        let app = router_with_paths(paths);

        let items = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/items")
                    .header(header::HOST, "127.0.0.1:25001")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(items.status(), StatusCode::OK);
        let items = response_json(items).await;
        assert!(items["items"][0]["thumbnail"].is_null());

        let item = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/items/item-1")
                    .header(header::HOST, "127.0.0.1:25001")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(item.status(), StatusCode::OK);
        let item = response_json(item).await;
        assert!(item["item"]["thumbnail"].is_null());
    }

    #[tokio::test]
    async fn v1_search_returns_agent_friendly_results_with_evidence_urls() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let raw_path = temp.path().join("video.mp4");
        seed_v1_agent_search_fixture(&paths, &raw_path);
        let app = router_with_paths(paths);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/search")
                    .header(header::HOST, "127.0.0.1:25001")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"query": "scaling laws", "max_results": 2}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert!(body["request_id"].as_str().unwrap().starts_with("req-"));
        assert_eq!(body["execution"]["target"], "local");
        assert_eq!(body["results"][0]["id"], "item-1:transcript:000000");
        assert_eq!(body["results"][0]["type"], "transcript");
        assert_eq!(body["results"][0]["source"], "local_library");
        assert_eq!(body["results"][0]["item"]["id"], "item-1");
        assert_eq!(body["results"][0]["item"]["title"], "Scaling Talk");
        assert_eq!(body["results"][0]["item"]["content_type"], "video");
        assert_eq!(body["results"][0]["item"]["source_type"], "local");
        assert_eq!(body["results"][0]["item"]["duration_sec"], 120.5);
        assert_eq!(body["results"][0]["time"]["start_sec"], 12.3);
        assert_eq!(body["results"][0]["time"]["end_sec"], 18.0);
        assert_eq!(body["results"][0]["time"]["timestamp"], "0:12");
        assert!(body["results"][0]["text"]["snippet"]
            .as_str()
            .unwrap()
            .contains("scaling laws"));
        assert_eq!(body["results"][0]["evidence"]["kind"], "video_clip");
        assert_eq!(
            body["results"][0]["evidence"]["clip"]["url"],
            "http://127.0.0.1:25001/v1/chunks/item-1%3Atranscript%3A000000/video-clip?before_sec=3&after_sec=5"
        );
        assert_eq!(
            body["results"][0]["evidence"]["preview"]["url"],
            "http://127.0.0.1:25001/v1/chunks/item-1%3Akeyframe%3A000012/frame"
        );
        assert_eq!(
            body["results"][0]["evidence"]["open_in_cerul"],
            "cerul-app://item/item-1?playbackChunkId=item-1%3Atranscript%3A000000&t=12.3"
        );
        assert_eq!(body["results"][0]["score"]["exact_match"], true);
        assert_eq!(body["usage"]["billable"], false);
        assert_eq!(
            body["usage"]["metered_events"][0],
            json!({"capability": "local_search", "quantity": 1, "credits": 0})
        );
    }

    #[tokio::test]
    async fn v1_search_uses_q_alias_when_query_is_blank() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let raw_path = temp.path().join("video.mp4");
        seed_v1_agent_search_fixture(&paths, &raw_path);
        let app = router_with_paths(paths);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/search")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"query": "   ", "q": "scaling laws"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["results"][0]["id"], "item-1:transcript:000000");
    }

    #[tokio::test]
    async fn v1_search_marks_remote_embedding_privacy_when_remote_query_mode_is_selected() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let raw_path = temp.path().join("video.mp4");
        seed_v1_agent_search_fixture(&paths, &raw_path);
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
                    .method(Method::POST)
                    .uri("/v1/search")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({"query": "scaling laws"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["execution"]["target"], "local");
        assert_eq!(body["execution"]["privacy"], "local_library_remote_query");
        assert_eq!(
            body["usage"]["metered_events"],
            json!([
                {"capability": "local_search", "quantity": 1, "credits": 0},
                {"capability": "remote_embedding_query", "quantity": 1, "credits": 0}
            ])
        );
    }

    #[tokio::test]
    async fn v1_search_does_not_advertise_clip_when_source_file_is_missing() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let raw_path = temp.path().join("video.mp4");
        seed_v1_agent_search_fixture(&paths, &raw_path);
        fs::remove_file(&raw_path).unwrap();
        let app = router_with_paths(paths);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/search")
                    .header(header::HOST, "127.0.0.1:25005")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({"query": "scaling laws"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        let evidence = &body["results"][0]["evidence"];
        assert_eq!(evidence["kind"], "frame");
        assert_eq!(evidence["clip"], Value::Null);
        assert_eq!(
            evidence["preview"]["url"],
            "http://127.0.0.1:25005/v1/chunks/item-1%3Akeyframe%3A000012/frame"
        );
    }

    #[tokio::test]
    async fn v1_search_does_not_advertise_preview_when_frame_file_is_missing() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let raw_path = temp.path().join("video.mp4");
        let frame_path = temp.path().join("frame.jpg");
        seed_v1_agent_search_fixture(&paths, &raw_path);
        fs::remove_file(frame_path).unwrap();
        let app = router_with_paths(paths);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/search")
                    .header(header::HOST, "127.0.0.1:25006")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({"query": "scaling laws"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        let evidence = &body["results"][0]["evidence"];
        assert_eq!(evidence["kind"], "video_clip");
        assert!(evidence["clip"]["url"]
            .as_str()
            .unwrap()
            .contains("/v1/chunks/item-1%3Atranscript%3A000000/video-clip"));
        assert_eq!(evidence["preview"], Value::Null);
    }

    #[tokio::test]
    async fn v1_search_does_not_advertise_clip_for_untimed_summary_hit() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let raw_path = temp.path().join("video.mp4");
        seed_v1_untimed_summary_fixture(&paths, &raw_path);
        let app = router_with_paths(paths);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/search")
                    .header(header::HOST, "127.0.0.1:25007")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"query": "untimed executive summary"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        let evidence = &body["results"][0]["evidence"];
        assert_eq!(body["results"][0]["id"], "item-1:understanding:summary");
        assert_eq!(body["results"][0]["time"]["start_sec"], Value::Null);
        assert_eq!(evidence["kind"], "chunk");
        assert_eq!(evidence["clip"], Value::Null);
        assert_eq!(evidence["preview"], Value::Null);
        assert_eq!(
            evidence["open_in_cerul"],
            "cerul-app://item/item-1?playbackChunkId=item-1%3Aunderstanding%3Asummary"
        );
    }

    #[tokio::test]
    async fn v1_search_rejects_cloud_target_until_cloud_proxy_exists() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let app = router_with_paths(paths);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/search")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"query": "scaling laws", "target": "cloud"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_json(response).await;
        assert!(body["error"]
            .as_str()
            .unwrap()
            .contains("only local or auto target"));
    }

    #[tokio::test]
    async fn v1_ask_returns_extractive_answer_with_evidence_citations() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let raw_path = temp.path().join("video.mp4");
        seed_v1_agent_search_fixture(&paths, &raw_path);
        let app = router_with_paths(paths);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/ask")
                    .header(header::HOST, "127.0.0.1:25002")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "question": "scaling laws",
                            "max_results": 2,
                            "locale": "en-US"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert!(body["request_id"].as_str().unwrap().starts_with("req-"));
        assert_eq!(body["execution"]["target"], "local");
        assert_eq!(body["mode"], "extractive");
        assert!(body["answer"]
            .as_str()
            .unwrap()
            .contains("This answer is extractive"));
        assert_eq!(body["citations"][0]["id"], "item-1:transcript:000000");
        assert_eq!(body["citations"][0]["item"]["title"], "Scaling Talk");
        assert_eq!(
            body["citations"][0]["evidence"]["clip"]["url"],
            "http://127.0.0.1:25002/v1/chunks/item-1%3Atranscript%3A000000/video-clip?before_sec=3&after_sec=5"
        );
        assert_eq!(body["warnings"], json!([]));
        assert_eq!(body["usage"]["billable"], false);
        assert_eq!(
            body["usage"]["metered_events"][0],
            json!({"capability": "local_ask_extractive", "quantity": 1, "credits": 0})
        );
    }

    #[tokio::test]
    async fn v1_ask_defaults_to_english_without_locale() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let raw_path = temp.path().join("video.mp4");
        seed_v1_agent_search_fixture(&paths, &raw_path);
        let app = router_with_paths(paths);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/ask")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({"question": "scaling laws"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        let answer = body["answer"].as_str().unwrap();
        assert!(answer.contains("This answer is extractive"));
        assert!(!answer.contains("本回答"));
    }

    #[tokio::test]
    async fn v1_ask_uses_fallback_aliases_after_trimming_blanks() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let raw_path = temp.path().join("video.mp4");
        seed_v1_agent_search_fixture(&paths, &raw_path);
        let app = router_with_paths(paths);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/ask")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "question": "",
                            "query": "   ",
                            "q": "scaling laws"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["citations"][0]["id"], "item-1:transcript:000000");
    }

    #[tokio::test]
    async fn v1_ask_rejects_non_extractive_mode_until_rag_exists() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let app = router_with_paths(paths);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/ask")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"question": "scaling laws", "mode": "rag"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_json(response).await;
        assert!(body["error"]
            .as_str()
            .unwrap()
            .contains("only extractive mode"));
    }

    #[tokio::test]
    async fn v1_items_returns_agent_friendly_item_records() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let raw_path = temp.path().join("video.mp4");
        seed_v1_agent_search_fixture(&paths, &raw_path);
        let app = router_with_paths(paths);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/items?status=indexed&limit=1")
                    .header(header::HOST, "127.0.0.1:25003")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert!(body["request_id"].as_str().unwrap().starts_with("req-"));
        assert_eq!(body["execution"]["target"], "local");
        assert_eq!(body["page"], json!({"limit": 1, "next_cursor": null}));
        let item = &body["items"][0];
        assert_eq!(item["id"], "item-1");
        assert_eq!(item["title"], "Scaling Talk");
        assert_eq!(item["content_type"], "video");
        assert_eq!(item["source_type"], "local");
        assert_eq!(item["status"], "indexed");
        assert_eq!(item["duration_sec"], 120.5);
        assert_eq!(item["indexed_at"], 10);
        assert_eq!(item["chunk_count"], 2);
        assert_eq!(item["source_url"], Value::Null);
        assert_eq!(
            item["thumbnail"]["url"],
            "http://127.0.0.1:25003/v1/chunks/item-1%3Akeyframe%3A000012/frame"
        );
        assert_eq!(item["open_in_cerul"], "cerul-app://item/item-1");
        assert!(item.get("raw_path").is_none());
        assert!(item.get("metadata").is_none());
    }

    #[tokio::test]
    async fn v1_item_chunks_returns_agent_context_with_evidence() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let raw_path = temp.path().join("video.mp4");
        seed_v1_agent_search_fixture(&paths, &raw_path);
        let app = router_with_paths(paths);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/items/item-1/chunks?type=transcript&limit=5")
                    .header(header::HOST, "127.0.0.1:25004")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["execution"]["target"], "local");
        assert_eq!(body["item"]["id"], "item-1");
        assert_eq!(body["chunks"].as_array().unwrap().len(), 1);
        let chunk = &body["chunks"][0];
        assert_eq!(chunk["id"], "item-1:transcript:000000");
        assert_eq!(chunk["type"], "transcript");
        assert_eq!(chunk["source"], "local_library");
        assert_eq!(chunk["time"]["start_sec"], 12.3);
        assert_eq!(chunk["time"]["end_sec"], 18.0);
        assert_eq!(chunk["time"]["timestamp"], "0:12");
        assert_eq!(
            chunk["text"]["content"],
            "The talk says scaling laws keep holding across larger training runs."
        );
        assert_eq!(
            chunk["evidence"]["clip"]["url"],
            "http://127.0.0.1:25004/v1/chunks/item-1%3Atranscript%3A000000/video-clip?before_sec=3&after_sec=5"
        );
        assert_eq!(chunk["evidence"]["preview"], Value::Null);
        assert_eq!(
            chunk["evidence"]["open_in_cerul"],
            "cerul-app://item/item-1?playbackChunkId=item-1%3Atranscript%3A000000&t=12.3"
        );
        assert_eq!(body["page"], json!({"limit": 5, "next_cursor": null}));
    }

    #[tokio::test]
    async fn v1_item_chunks_do_not_advertise_clip_for_untimed_chunks() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let raw_path = temp.path().join("video.mp4");
        seed_v1_untimed_summary_fixture(&paths, &raw_path);
        let app = router_with_paths(paths);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/items/item-1/chunks?type=summary")
                    .header(header::HOST, "127.0.0.1:25008")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        let chunk = &body["chunks"][0];
        assert_eq!(chunk["id"], "item-1:understanding:summary");
        assert_eq!(chunk["time"]["start_sec"], Value::Null);
        assert_eq!(chunk["evidence"]["kind"], "chunk");
        assert_eq!(chunk["evidence"]["clip"], Value::Null);
        assert_eq!(chunk["evidence"]["preview"], Value::Null);
        assert_eq!(
            chunk["evidence"]["open_in_cerul"],
            "cerul-app://item/item-1?playbackChunkId=item-1%3Aunderstanding%3Asummary"
        );
    }

    #[tokio::test]
    async fn v1_item_chunks_translates_public_visual_type_filter() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let raw_path = temp.path().join("video.mp4");
        seed_v1_agent_search_fixture(&paths, &raw_path);
        let app = router_with_paths(paths);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/items/item-1/chunks?type=visual&limit=5")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["chunks"].as_array().unwrap().len(), 1);
        assert_eq!(body["chunks"][0]["id"], "item-1:keyframe:000012");
        assert_eq!(body["chunks"][0]["type"], "visual");
    }

    #[test]
    fn v1_chunk_type_filter_values_cover_public_aliases_and_raw_types() {
        assert_eq!(
            v1_chunk_type_filter_values("transcript"),
            vec!["transcript".to_string(), "transcript_line".to_string()]
        );
        assert_eq!(
            v1_chunk_type_filter_values("visual"),
            vec![
                "keyframe".to_string(),
                "image".to_string(),
                "ocr".to_string()
            ]
        );
        assert_eq!(
            v1_chunk_type_filter_values("summary"),
            vec!["understanding".to_string()]
        );
        assert_eq!(
            v1_chunk_type_filter_values("keyframe"),
            vec!["keyframe".to_string()]
        );
    }

    #[tokio::test]
    async fn root_routes_are_not_retained_as_compatibility_aliases() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let app = router_with_paths(paths);

        for (method, path) in [
            (Method::GET, "/health"),
            (Method::GET, "/metrics"),
            (Method::GET, "/diagnostics"),
            (Method::GET, "/diagnostics/indexing"),
            (Method::POST, "/search"),
            (Method::GET, "/search/diagnostics"),
            (Method::POST, "/search/rebuild"),
            (Method::POST, "/ask"),
            (Method::GET, "/sources"),
            (Method::GET, "/items"),
            (Method::GET, "/items/item-1"),
            (Method::GET, "/items/item-1/chunks"),
            (Method::GET, "/chunks/chunk-1/frame"),
            (Method::GET, "/chunks/chunk-1/video-segment"),
            (Method::GET, "/chunks/chunk-1/video-clip"),
            (Method::GET, "/jobs"),
            (Method::GET, "/usage/summary"),
            (Method::GET, "/storage/usage"),
            (Method::GET, "/providers"),
            (Method::GET, "/settings"),
            (Method::GET, "/openapi.json"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method.clone())
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {path}");
        }
    }

    #[tokio::test]
    async fn internal_product_routes_remain_available_after_root_migration() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let frame = temp.path().join("frame.jpg");
        std::fs::write(&frame, b"jpg-bytes").unwrap();
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
                INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, frame_path, metadata)
                VALUES ('chunk-frame', 'item-1', 'keyframe', 2, 2, ?1, '{}')
                "#,
                [frame.to_string_lossy().as_ref()],
            )
            .unwrap();
        }
        let app = router_with_paths(paths);

        let health = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/internal/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);

        let settings = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/internal/settings")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(settings.status(), StatusCode::OK);

        let items = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/internal/items")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(items.status(), StatusCode::OK);
        let items = response_json(items).await;
        assert_eq!(items[0]["id"], "item-1");

        let item = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/internal/items/item-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(item.status(), StatusCode::OK);

        let chunks = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/internal/items/item-1/chunks")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(chunks.status(), StatusCode::OK);
        let chunks = response_json(chunks).await;
        assert_eq!(chunks[0]["id"], "chunk-frame");

        let frame = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/internal/chunks/chunk-frame/frame")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(frame.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn diagnostics_bundle_redacts_private_values() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/tester".to_string());
        let missing_path = format!("{home}/Downloads/missing.mp4");
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                r#"
                INSERT INTO settings (key, value, updated_at) VALUES
                    ('remote_api_key', '"super-secret"', strftime('%s','now')),
                    ('inference_mode', '"auto"', strftime('%s','now')),
                    ('model_download_source', '"auto"', strftime('%s','now'))
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO sources (id, type, config, status)
                VALUES ('source-1', 'local', '{}', 'active')
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO items (
                    id, source_id, content_type, title, raw_path, indexed_at, status, error, metadata
                )
                VALUES ('item-1', 'source-1', 'video', 'Missing video', ?1, 10, 'failed', ?2, '{}')
                "#,
                (
                    &missing_path,
                    format!("source file does not exist: {missing_path}"),
                ),
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO jobs (
                    id, item_id, job_type, status, started_at, error, progress, stage, stage_message
                )
                VALUES ('job-1', 'item-1', 'index_video', 'failed', 11, ?1, 0.4, 'transcribing', ?2)
                "#,
                (
                    format!("failed to read {missing_path}"),
                    format!("Reading {missing_path}"),
                ),
            )
            .unwrap();
        }
        let app = router_with_paths(paths);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/internal/diagnostics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["settings"]["remote_api_key_set"], true);
        assert!(json["settings"].get("remote_api_key").is_none());
        assert_eq!(
            json["recent_errors"][0]["error"],
            "source file does not exist: ~/Downloads/missing.mp4"
        );
        assert_eq!(
            json["jobs"][0]["error"],
            "failed to read ~/Downloads/missing.mp4"
        );

        let serialized = serde_json::to_string(&json).unwrap();
        assert!(!serialized.contains("super-secret"));
        if !home.trim().is_empty() {
            assert!(!serialized.contains(&home));
        }
    }

    #[tokio::test]
    async fn indexing_diagnostics_route_reports_local_queue_pressure() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                r#"
                INSERT INTO settings (key, value, updated_at) VALUES
                    ('inference_mode', '"local"', strftime('%s','now')),
                    ('concurrent_jobs', '4', strftime('%s','now'))
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'folder_video', '{}', 'active')",
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO items (id, source_id, content_type, external_id, title, status, metadata)
                VALUES ('item-1', 'source-1', 'video', 'clip.mp4', 'Clip', 'processing', '{}')
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO jobs (id, item_id, job_type, status, started_at, progress, stage, stage_message)
                VALUES ('job-1', 'item-1', 'index_video', 'running', 10, 0.24, 'waiting_model', 'Waiting for local model')
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
                    .uri("/internal/diagnostics/indexing")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let diagnostics = response_json(response).await;
        assert_eq!(diagnostics["configured_concurrent_jobs"], 4);
        assert_eq!(diagnostics["effective_concurrent_jobs"], 1);
        assert_eq!(diagnostics["effective_inference_mode"], "local");
        assert_eq!(diagnostics["waiting_model_jobs"], 1);
        assert_eq!(diagnostics["counts"]["running_jobs"], 1);
        assert!(diagnostics["vector_index"]["ready"].is_boolean());
    }

    #[tokio::test]
    async fn local_capability_route_reports_models_and_total() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let app = router_with_paths(paths);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/internal/models/local/capability")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        // Three user-facing models (embed / asr / ocr); the exact total follows
        // the current user-managed model size estimates instead of a stale fixture
        // constant. OCR is bundled and reported for diagnostics, but it is not part
        // of the default user download total.
        let models = json["models"].as_array().unwrap();
        assert_eq!(models.len(), 3);
        let ids: Vec<&str> = models.iter().map(|m| m["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["embed", "asr", "ocr"]);
        let summed_model_sizes: u64 = models
            .iter()
            .filter(|m| matches!(m["id"].as_str(), Some("embed" | "asr")))
            .map(|m| m["size_mb"].as_u64().unwrap())
            .sum();
        assert_eq!(json["total_mb"].as_u64().unwrap(), summed_model_sizes);
        // recommended is one of the two known values; can_run_local is a bool.
        assert!(matches!(
            json["recommended"].as_str(),
            Some("local") | Some("remote")
        ));
        assert!(json["can_run_local"].is_boolean());
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
    fn configured_addr_defaults_to_loopback_and_reads_binding_and_port_settings() {
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
            "0.0.0.0:23785".parse::<SocketAddr>().unwrap()
        );

        conn.execute(
            r#"
            INSERT INTO settings (key, value, updated_at)
            VALUES ('api_port', '24001', strftime('%s','now'))
            "#,
            [],
        )
        .unwrap();

        assert_eq!(
            configured_addr(&paths).unwrap(),
            "0.0.0.0:24001".parse::<SocketAddr>().unwrap()
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
                    .uri("/internal/models/whisper")
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
                    .uri("/internal/models/catalog")
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
                    .uri("/internal/settings")
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
    async fn settings_endpoint_validates_api_port_without_changing_active_endpoint_file() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        write_api_endpoint_file(&paths, 23785).unwrap();
        let app = router_with_paths(paths.clone());

        let valid = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri("/internal/settings")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({ "api_port": 24001 }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(valid.status(), StatusCode::OK);
        let settings = response_json(valid).await;
        assert_eq!(settings["api_port"], 24001);
        assert_eq!(
            setting_string(&paths, "api_port").unwrap().as_deref(),
            Some("24001")
        );

        let endpoint: Value =
            serde_json::from_slice(&std::fs::read(paths.data.join(API_ENDPOINT_FILE)).unwrap())
                .unwrap();
        assert_eq!(endpoint["port"], 23785);
        assert_eq!(endpoint["base_url"], "http://127.0.0.1:23785");
        assert_eq!(endpoint["v1_base_url"], "http://127.0.0.1:23785/v1");
        assert_eq!(
            endpoint["internal_base_url"],
            "http://127.0.0.1:23785/internal"
        );

        let invalid = app
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri("/internal/settings")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({ "api_port": 80 }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn mode_switch_preserves_indexed_items_while_requeueing_rebuild() {
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
                    .uri("/internal/settings")
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
        assert_eq!(item_status, "indexed");
        assert_eq!(queued_jobs, 1);
    }

    #[test]
    fn vector_backend_change_requeues_indexed_items_when_schema_is_current() {
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
                    id, source_id, content_type, external_id, title, indexed_at, status, metadata,
                    search_index_version, search_index_status, search_index_unit_count, search_index_vector_count
                )
                VALUES (
                    'item-1', 'source-1', 'video', 'video.mp4', 'Video', 100, 'indexed', '{}',
                    ?1, 'indexed', 4, 4
                )
                "#,
                [cerul_storage::SEARCH_INDEX_VERSION],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO settings (key, value, updated_at)
                VALUES (?1, ?2, strftime('%s','now'))
                "#,
                (
                    INDEXING_SCHEMA_VERSION_SETTING,
                    Value::from(INDEXING_SCHEMA_VERSION).to_string(),
                ),
            )
            .unwrap();
        }

        sync_indexing_schema_side_effects(&paths).unwrap();

        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        let queued_jobs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM jobs WHERE item_id = 'item-1' AND job_type = 'index_video' AND status = 'queued'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(queued_jobs, 1);
        assert_eq!(
            setting_string(&paths, VECTOR_INDEX_BACKEND_SETTING)
                .unwrap()
                .as_deref(),
            Some(ACTIVE_VECTOR_INDEX_BACKEND)
        );
    }

    #[test]
    fn repairs_discovered_items_with_indexed_artifact_metadata() {
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
                    id, source_id, content_type, external_id, title, indexed_at, status, metadata, error
                )
                VALUES
                    (
                        'item-1',
                        'source-1',
                        'video',
                        'video.mp4',
                        'Video',
                        NULL,
                        'discovered',
                        '{"embedding_index_status":"indexed","transcript_index_status":"indexed"}',
                        NULL
                    ),
                    (
                        'item-2',
                        'source-1',
                        'video',
                        'failed-rebuild.mp4',
                        'Failed Rebuild',
                        NULL,
                        'failed',
                        '{"ocr_index_status":"indexed"}',
                        'stale rebuild failure'
                    )
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO jobs (id, item_id, job_type, status, started_at, finished_at, progress)
                VALUES
                    ('job-done', 'item-1', 'index_video', 'completed', 1000, 1234, 1),
                    ('job-done-2', 'item-2', 'index_video', 'completed', 2000, 2222, 1)
                "#,
                [],
            )
            .unwrap();
        }

        let repaired = repair_indexed_item_status_from_artifacts(&paths).unwrap();

        assert_eq!(repaired, 2);
        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        let item: (String, Option<i64>, Option<String>) = conn
            .query_row(
                "SELECT status, indexed_at, error FROM items WHERE id = 'item-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(item.0, "indexed");
        assert_eq!(item.1, Some(1234));
        assert_eq!(item.2, None);
        let failed_rebuild: (String, Option<i64>, Option<String>) = conn
            .query_row(
                "SELECT status, indexed_at, error FROM items WHERE id = 'item-2'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(failed_rebuild.0, "indexed");
        assert_eq!(failed_rebuild.1, Some(2222));
        assert_eq!(failed_rebuild.2, None);
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
                    .uri("/internal/settings")
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

        assert_eq!(item_status, "indexed");
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
                    .uri("/internal/providers")
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
                    .uri("/internal/providers")
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

        let retargeted = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri(format!(
                        "/internal/providers/{}",
                        created_json["id"].as_str().unwrap()
                    ))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "type": "openai-compatible",
                            "label": "Groq ASR",
                            "base_url": "https://api.groq.com/openai/v1/"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(retargeted.status(), StatusCode::OK);
        let retargeted_json = response_json(retargeted).await;
        assert_eq!(retargeted_json["type"], "openai-compatible");
        assert_eq!(
            retargeted_json["base_url"],
            "https://api.groq.com/openai/v1"
        );

        let models_without_key = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!(
                        "/internal/providers/{}/models",
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
                    .uri("/internal/providers/local/models")
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
                    .uri("/internal/providers/local")
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
                    .uri("/internal/providers/local")
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
                    .uri("/internal/providers/local/test")
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
                    .uri("/internal/providers")
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
                    .uri("/internal/providers")
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
                    .uri("/internal/sources")
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
                    .uri("/internal/items")
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
                    .uri("/internal/jobs")
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
                    .uri(format!("/internal/sources/{id}/pause"))
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
        let raw_path = temp.path().join("clip.mp4").to_string_lossy().into_owned();
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
                VALUES (
                    'item-1',
                    'source-1',
                    'video',
                    'clip.mp4',
                    'Clip',
                    ?1,
                    10,
                    'indexed',
                    '{"display_title":"Old generated title"}'
                )
                "#,
                [raw_path.as_str()],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, text, metadata)
                VALUES
                    ('chunk-1', 'item-1', 'transcript', 0, 5, 'hello', '{}'),
                    ('item-1:understanding:summary', 'item-1', 'understanding', NULL, NULL, 'old understanding text', '{}')
                "#,
                [],
            )
            .unwrap();
        }
        let profile = cerul_storage::vectors::ensure_active_embedding_profile(&paths).unwrap();
        cerul_storage::replace_item_retrieval_units(
            &paths,
            "item-1",
            &[cerul_storage::StorageRetrievalUnit {
                id: "item-1:unit:v2:000000".to_string(),
                item_id: "item-1".to_string(),
                unit_index: 0,
                unit_kind: "summary".to_string(),
                start_sec: None,
                end_sec: None,
                content_text: "old understanding text".to_string(),
                transcript_text: None,
                ocr_text: None,
                visual_text: None,
                summary_text: Some("old understanding text".to_string()),
                representative_chunk_id: Some("item-1:understanding:summary".to_string()),
                representative_frame_path: None,
                embedding_profile_id: profile.id,
                index_version: cerul_storage::SEARCH_INDEX_VERSION,
                metadata: Default::default(),
            }],
        )
        .unwrap();
        cerul_storage::set_item_search_index_status(&paths, "item-1", "indexed", None, 1, 1)
            .unwrap();
        seed_indexing_schema_version(&paths);
        let app = router_with_paths(paths.clone());

        let reindex = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/internal/items/item-1/reindex")
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
            let item: (String, Option<i64>, String) = conn
                .query_row(
                    "SELECT status, indexed_at, metadata FROM items WHERE id = 'item-1'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            assert_eq!((item.0, item.1), ("indexed".to_string(), Some(10)));
            let item_metadata = parse_json(&item.2);
            assert!(item_metadata.get("display_title").is_none());
            let jobs: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM jobs WHERE item_id = 'item-1' AND job_type = 'index_video' AND status = 'queued'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(jobs, 1);
            let understanding_chunks: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM chunks WHERE item_id = 'item-1' AND chunk_type = 'understanding'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            let retrieval_units: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM retrieval_units WHERE item_id = 'item-1'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            let search_index_state: (String, i64, i64) = conn
                .query_row(
                    r#"
                    SELECT search_index_status, search_index_unit_count, search_index_vector_count
                    FROM items
                    WHERE id = 'item-1'
                    "#,
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            assert_eq!(understanding_chunks, 0);
            assert_eq!(retrieval_units, 0);
            assert_eq!(search_index_state, ("pending".to_string(), 0, 0));
        }

        let delete = app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri("/internal/items/item-1")
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
        let ignored: i64 = conn
            .query_row("SELECT COUNT(*) FROM ignored_items", [], |row| row.get(0))
            .unwrap();
        assert_eq!(ignored, 1);
        drop(conn);

        let mut conn = cerul_storage::sqlite::open(&paths).unwrap();
        let tx = conn.transaction().unwrap();
        let rediscovered = upsert_discovered_item(
            &tx,
            "source-1",
            ContentType::Video,
            &DiscoveredItem {
                external_id: "clip.mp4".to_string(),
                title: Some("Clip".to_string()),
                duration_sec: Some(10.0),
                metadata: json!({ "raw_path": raw_path.as_str() }),
            },
        )
        .unwrap();
        assert_eq!(rediscovered, None);
        let rediscovered_with_changed_external_id = upsert_discovered_item(
            &tx,
            "source-1",
            ContentType::Video,
            &DiscoveredItem {
                external_id: "changed-metadata-id".to_string(),
                title: Some("Clip".to_string()),
                duration_sec: Some(10.0),
                metadata: json!({ "raw_path": raw_path.as_str() }),
            },
        )
        .unwrap();
        assert_eq!(
            rediscovered_with_changed_external_id, None,
            "raw_path tombstone should block rediscovery even if external_id changes"
        );
        tx.commit().unwrap();

        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        let items: i64 = conn
            .query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0))
            .unwrap();
        assert_eq!(items, 0, "ignored item should not be rediscovered");
    }

    #[tokio::test]
    async fn reindex_item_does_not_duplicate_running_rebuild() {
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
                INSERT INTO jobs (id, item_id, job_type, status, started_at, progress, stage)
                VALUES ('job-running', 'item-1', 'index_video', 'running', 100, 0.5, 'asr')
                "#,
                [],
            )
            .unwrap();
        }
        seed_indexing_schema_version(&paths);
        let app = router_with_paths(paths.clone());

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/internal/items/item-1/reindex")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["queued_job"], false);
        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        let queued_jobs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM jobs WHERE item_id = 'item-1' AND status = 'queued'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let running_jobs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM jobs WHERE item_id = 'item-1' AND status = 'running'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(queued_jobs, 0);
        assert_eq!(running_jobs, 1);
    }

    #[tokio::test]
    async fn cancelling_queued_rebuild_preserves_indexed_artifacts() {
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
        seed_indexing_schema_version(&paths);
        let item = cerul_storage::get_item(&paths, "item-1").unwrap();
        let cache_key = item_pipeline_cache_keys(&item).into_iter().next().unwrap();
        let audio_cache = paths
            .cache
            .join("pipeline")
            .join("audio")
            .join(format!("{cache_key}.wav"));
        std::fs::create_dir_all(audio_cache.parent().unwrap()).unwrap();
        std::fs::write(&audio_cache, b"cached audio").unwrap();
        let app = router_with_paths(paths.clone());

        let reindex = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/internal/items/item-1/reindex")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(reindex.status(), StatusCode::OK);
        let job_id: String = {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.query_row(
                "SELECT id FROM jobs WHERE item_id = 'item-1' AND status = 'queued'",
                [],
                |row| row.get(0),
            )
            .unwrap()
        };

        let cancel = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/internal/jobs/{job_id}/cancel"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(cancel.status(), StatusCode::OK);
        assert!(
            audio_cache.exists(),
            "indexed rebuild cancellation should not clear existing cache"
        );
        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        let item: (String, Option<i64>) = conn
            .query_row(
                "SELECT status, indexed_at FROM items WHERE id = 'item-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(item, ("indexed".to_string(), Some(10)));
    }

    #[tokio::test]
    async fn item_delete_records_raw_path_tombstone_without_external_id() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let raw_path = temp.path().join("clip.mp4").to_string_lossy().into_owned();
        seed_indexing_schema_version(&paths);
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
                VALUES ('item-1', 'source-1', 'video', NULL, 'Clip', ?1, 10, 'indexed', ?2)
                "#,
                (
                    raw_path.as_str(),
                    json!({ "raw_path": raw_path.as_str() }).to_string(),
                ),
            )
            .unwrap();
        }
        let app = router_with_paths(paths.clone());

        let delete = app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri("/internal/items/item-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(delete.status(), StatusCode::OK);

        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        let ignored: (String, Option<String>) = conn
            .query_row(
                "SELECT external_id, raw_path FROM ignored_items WHERE source_id = 'source-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(ignored.0, raw_path);
        assert_eq!(ignored.1.as_deref(), Some(ignored.0.as_str()));
        drop(conn);

        let mut conn = cerul_storage::sqlite::open(&paths).unwrap();
        let tx = conn.transaction().unwrap();
        let rediscovered = upsert_discovered_item(
            &tx,
            "source-1",
            ContentType::Video,
            &DiscoveredItem {
                external_id: "fresh-signature".to_string(),
                title: Some("Clip".to_string()),
                duration_sec: Some(10.0),
                metadata: json!({ "raw_path": ignored.0.as_str() }),
            },
        )
        .unwrap();
        assert_eq!(rediscovered, None);
        tx.commit().unwrap();
    }

    #[tokio::test]
    async fn rediscovering_changed_raw_path_reuses_item_and_requires_reindex() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let raw_path = temp.path().join("clip.mp4").to_string_lossy().into_owned();
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'file_video', '{}', 'active')",
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO items (
                    id, source_id, content_type, external_id, title, raw_path, indexed_at, status, metadata
                )
                VALUES ('item-existing', 'source-1', 'video', 'old-signature', 'Old', ?1, 10, 'indexed', '{}')
                "#,
                [raw_path.as_str()],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, text, metadata)
                VALUES ('chunk-old', 'item-existing', 'transcript', 0, 5, 'old searchable text', '{}')
                "#,
                [],
            )
            .unwrap();
        }
        let profile = cerul_storage::vectors::ensure_active_embedding_profile(&paths).unwrap();
        cerul_storage::replace_item_retrieval_units(
            &paths,
            "item-existing",
            &[cerul_storage::StorageRetrievalUnit {
                id: "item-existing:unit:v2:000000".to_string(),
                item_id: "item-existing".to_string(),
                unit_index: 0,
                unit_kind: "moment".to_string(),
                start_sec: Some(0.0),
                end_sec: Some(5.0),
                content_text: "Transcript: old searchable text".to_string(),
                transcript_text: Some("old searchable text".to_string()),
                ocr_text: None,
                visual_text: None,
                summary_text: None,
                representative_chunk_id: Some("chunk-old".to_string()),
                representative_frame_path: None,
                embedding_profile_id: profile.id,
                index_version: cerul_storage::SEARCH_INDEX_VERSION,
                metadata: Default::default(),
            }],
        )
        .unwrap();
        cerul_storage::set_item_search_index_status(&paths, "item-existing", "indexed", None, 1, 1)
            .unwrap();

        let mut conn = cerul_storage::sqlite::open(&paths).unwrap();
        let tx = conn.transaction().unwrap();
        let item_id = upsert_discovered_item(
            &tx,
            "source-1",
            ContentType::Video,
            &DiscoveredItem {
                external_id: "new-signature".to_string(),
                title: Some("New".to_string()),
                duration_sec: Some(12.0),
                metadata: json!({ "raw_path": raw_path.as_str() }),
            },
        )
        .unwrap();
        assert_eq!(item_id.as_deref(), Some("item-existing"));
        let queued =
            enqueue_index_job(&tx, item_id.as_deref().unwrap(), ContentType::Video).unwrap();
        tx.commit().unwrap();

        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        let row: (String, String, Option<i64>) = conn
            .query_row(
                "SELECT external_id, status, indexed_at FROM items WHERE id = 'item-existing'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let chunks: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE item_id = 'item-existing'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            row,
            ("new-signature".to_string(), "discovered".to_string(), None)
        );
        assert_eq!(chunks, 0, "old chunks should not remain searchable");
        let retrieval_units: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM retrieval_units WHERE item_id = 'item-existing'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let search_index_state: (String, i64, i64) = conn
            .query_row(
                r#"
                SELECT search_index_status, search_index_unit_count, search_index_vector_count
                FROM items
                WHERE id = 'item-existing'
                "#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            retrieval_units, 0,
            "old retrieval units should not remain searchable"
        );
        assert_eq!(search_index_state, ("pending".to_string(), 0, 0));
        assert!(
            queued,
            "changed raw_path signature should queue a fresh index job"
        );
    }

    #[tokio::test]
    async fn raw_path_reuse_is_scoped_to_source() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let raw_path = temp.path().join("clip.mp4").to_string_lossy().into_owned();
        let mut conn = cerul_storage::sqlite::open(&paths).unwrap();
        conn.execute(
            "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'folder_video', '{}', 'active'), ('source-2', 'folder_video', '{}', 'active')",
            [],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO items (
                id, source_id, content_type, external_id, title, raw_path, indexed_at, status, metadata
            )
            VALUES ('item-source-1', 'source-1', 'video', 'clip-a', 'A', ?1, 10, 'indexed', '{}')
            "#,
            [raw_path.as_str()],
        )
        .unwrap();

        let tx = conn.transaction().unwrap();
        let item_id = upsert_discovered_item(
            &tx,
            "source-2",
            ContentType::Video,
            &DiscoveredItem {
                external_id: "clip-b".to_string(),
                title: Some("B".to_string()),
                duration_sec: Some(12.0),
                metadata: json!({ "raw_path": raw_path.as_str() }),
            },
        )
        .unwrap();
        tx.commit().unwrap();

        assert_ne!(item_id.as_deref(), Some("item-source-1"));
        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        let source_2_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM items WHERE source_id = 'source-2' AND raw_path = ?1",
                [raw_path.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(source_2_count, 1);
    }

    #[tokio::test]
    async fn list_items_hides_items_pending_delete() {
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
                    id, source_id, content_type, external_id, title, status, metadata
                )
                VALUES
                    ('item-visible', 'source-1', 'video', 'visible.mp4', 'Visible', 'indexed', '{}'),
                    ('item-deleting', 'source-1', 'video', 'deleting.mp4', 'Deleting', 'deleting', '{}')
                "#,
                [],
            )
            .unwrap();
        }

        let response = router_with_paths(paths)
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/internal/items")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let items = response_json(response).await;
        let ids = items
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["id"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["item-visible"]);
    }

    #[tokio::test]
    async fn cancelling_running_job_defers_temp_artifact_cleanup() {
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
                    id, source_id, content_type, external_id, title, status, metadata
                )
                VALUES ('item-1', 'source-1', 'video', 'clip.mp4', 'Clip', 'processing', '{}')
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO jobs (id, item_id, job_type, status, progress)
                VALUES ('job-1', 'item-1', 'index_video', 'running', 0.5)
                "#,
                [],
            )
            .unwrap();
        }

        let audio_key = cerul_pipeline::run::cache_key_for_item("item-1", "clip.mp4");
        let audio_path = paths
            .cache
            .join("pipeline")
            .join("audio")
            .join(format!("{audio_key}.wav"));
        std::fs::create_dir_all(audio_path.parent().unwrap()).unwrap();
        std::fs::write(&audio_path, b"temporary audio").unwrap();

        let app = router_with_paths(paths.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/internal/jobs/job-1/cancel")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["status"], "cancelled");
        assert_eq!(body["item_id"], "item-1");
        assert_eq!(body["cleanup_deferred"], true);
        assert!(
            audio_path.exists(),
            "running job cancellation must not remove audio still in use by the sidecar"
        );
    }

    #[tokio::test]
    async fn item_raw_path_patch_syncs_metadata_without_reindexing() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let old_path = temp.path().join("old.mp4");
        let new_path = temp.path().join("new.mp4");
        std::fs::write(&new_path, b"video").unwrap();
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
                    id, source_id, content_type, external_id, title, raw_path,
                    indexed_at, status, error, metadata
                )
                VALUES (
                    'item-1', 'source-1', 'video', 'clip.mp4', 'Clip', ?1,
                    10, 'failed', ?2, ?3
                )
                "#,
                rusqlite::params![
                    old_path.to_string_lossy().as_ref(),
                    format!("source file does not exist: {}", old_path.display()),
                    json!({ "raw_path": old_path.to_string_lossy(), "kept": true }).to_string()
                ],
            )
            .unwrap();
        }
        let app = router_with_paths(paths.clone());
        let body = json!({ "raw_path": new_path.to_string_lossy() });

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri("/internal/items/item-1")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let item = response_json(response).await;
        assert_eq!(status, StatusCode::OK, "patch failed: {item}");
        assert_eq!(
            item["raw_path"].as_str().unwrap(),
            new_path.to_string_lossy().as_ref()
        );
        assert_eq!(
            item["metadata"]["raw_path"].as_str().unwrap(),
            new_path.to_string_lossy().as_ref()
        );
        assert_eq!(item["metadata"]["kept"], true);
        assert_eq!(item["raw_path_exists"], true);
        assert_eq!(item["status"], "indexed");
        assert!(item["error"].is_null());

        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        let queued_jobs: i64 = conn
            .query_row("SELECT COUNT(*) FROM jobs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(queued_jobs, 0);
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
                    .uri("/internal/items/item-1/playback")
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
                    .uri("/internal/items/item-1/playback")
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
                    .uri("/internal/items")
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
        std::fs::write(paths.vector_index.join("index.bin"), b"idx").unwrap();
        let app = router_with_paths(paths);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/internal/storage/usage")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let usage = response_json(response).await;
        assert!(usage["total_bytes"].as_u64().unwrap() >= 18);
        assert!(usage["total_apparent_bytes"].as_u64().unwrap() >= 18);
        let categories = usage["categories"].as_array().unwrap();
        let bytes_for = |key: &str| {
            categories
                .iter()
                .find(|category| category["key"] == key)
                .and_then(|category| category["bytes"].as_u64())
                .unwrap()
        };
        let apparent_bytes_for = |key: &str| {
            categories
                .iter()
                .find(|category| category["key"] == key)
                .and_then(|category| category["apparent_bytes"].as_u64())
                .unwrap()
        };
        assert_eq!(apparent_bytes_for("models"), 5);
        assert_eq!(apparent_bytes_for("cache"), 10);
        assert_eq!(apparent_bytes_for("index"), 3);
        assert!(bytes_for("models") > 0);
        assert!(bytes_for("cache") > 0);
        assert!(bytes_for("index") > 0);
        assert!(bytes_for("database") > 0);
    }

    #[tokio::test]
    async fn storage_locations_reports_paths_without_scanning_usage() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let app = router_with_paths(paths.clone());

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/internal/storage/locations")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let locations = response_json(response).await;
        assert_eq!(
            locations["data_dir"].as_str(),
            Some(paths.data.to_string_lossy().as_ref())
        );
        assert_eq!(
            locations["database_path"].as_str(),
            Some(paths.db.to_string_lossy().as_ref())
        );
        assert_eq!(
            locations["models_dir"].as_str(),
            Some(paths.models.to_string_lossy().as_ref())
        );
        assert_eq!(
            locations["index_dir"].as_str(),
            Some(paths.vector_index.to_string_lossy().as_ref())
        );
        assert_eq!(
            locations["cache_dir"].as_str(),
            Some(paths.cache.to_string_lossy().as_ref())
        );
    }

    #[tokio::test]
    async fn reset_local_library_clears_library_state_and_preserves_settings() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let current_media = temp.path().join("current-media");
        let old_media = temp.path().join("old-media");
        let old_downloaded_video = old_media.join("sources").join("web_video").join("clip.mp4");
        std::fs::write(paths.models.join("model.bin"), b"model").unwrap();
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                "INSERT INTO settings (key, value) VALUES ('media_dir', ?1)",
                [Value::String(current_media.to_string_lossy().into_owned()).to_string()],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO providers (id, type, label, status) VALUES ('remote-asr', 'openai-compatible', 'Remote ASR', 'ready')",
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO embedding_profiles (
                    id, model_id, output_dimension, distance_metric, index_version, status, provider_id
                )
                VALUES ('profile-1', 'local-embed', 4, 'cosine', 1, 'ready', 'local')
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'folder_video', '{}', 'active')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO sources (id, type, config, status) VALUES ('source-remote', 'web_video', '{}', 'active')",
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO items (id, source_id, content_type, external_id, title, status, metadata)
                VALUES ('item-1', 'source-1', 'video', 'clip.mp4', 'Clip', 'indexed', '{}')
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO items (
                    id, source_id, content_type, external_id, title, raw_path, status, metadata
                )
                VALUES ('item-remote', 'source-remote', 'video', 'web-clip', 'Web Clip', ?1, 'indexed', ?2)
                "#,
                (
                    old_downloaded_video.to_string_lossy().as_ref(),
                    json!({ "raw_path": old_downloaded_video.to_string_lossy() }).to_string(),
                ),
            )
            .unwrap();
            conn.execute(
                "INSERT INTO jobs (id, item_id, job_type, status, progress) VALUES ('job-1', 'item-1', 'index_video', 'failed', 1)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO chunks (id, item_id, chunk_type, text, metadata) VALUES ('chunk-1', 'item-1', 'transcript', 'hello', '{}')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO moments (id, item_id, chunk_id, title, quote) VALUES ('moment-1', 'item-1', 'chunk-1', 'Moment', 'hello')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO item_understandings (item_id, status, summary, result) VALUES ('item-1', 'completed', 'summary', '{}')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO ignored_items (source_id, external_id, reason) VALUES ('source-1', 'ignored.mp4', 'test')",
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO retrieval_units (
                    id, item_id, unit_index, unit_kind, content_text, embedding_profile_id, index_version
                )
                VALUES ('unit-1', 'item-1', 0, 'transcript', 'hello', 'profile-1', 1)
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO inference_usage_events (
                    id, provider_mode, capability, item_id, job_id, status, metadata
                )
                VALUES ('usage-bound', 'remote', 'asr', 'item-1', 'job-1', 'succeeded', '{}')
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO inference_usage_events (
                    id, provider_mode, capability, status, metadata
                )
                VALUES ('usage-unbound', 'remote', 'asr', 'succeeded', '{}')
                "#,
                [],
            )
            .unwrap();
        }

        let app = router_with_paths(paths.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/internal/storage/reset-library")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["status"], "ok");
        assert_eq!(body["compacted"], true);
        assert!(body["compaction_error"].is_null());
        let download_targets = body["download_targets"].as_array().unwrap();
        let includes_download_target = |path: PathBuf| {
            let expected = path.to_string_lossy();
            download_targets
                .iter()
                .any(|target| target.as_str() == Some(expected.as_ref()))
        };
        assert!(includes_download_target(current_media.join("sources")));
        assert!(includes_download_target(old_media.join("sources")));

        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        let count = |table: &str| -> i64 {
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
        };
        for table in [
            "sources",
            "items",
            "jobs",
            "chunks",
            "chunks_fts",
            "moments",
            "item_understandings",
            "ignored_items",
            "retrieval_units",
            "retrieval_units_fts",
        ] {
            assert_eq!(count(table), 0, "{table} should be empty after reset");
        }
        assert_eq!(count("providers"), 2);
        assert_eq!(count("embedding_profiles"), 1);
        assert_eq!(count("inference_usage_events"), 1);
        let media_dir = cerul_storage::read_string_setting(&paths, "media_dir")
            .unwrap()
            .unwrap();
        assert_eq!(media_dir, current_media.to_string_lossy().as_ref());
        let usage_id: String = conn
            .query_row("SELECT id FROM inference_usage_events", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(usage_id, "usage-unbound");
        assert!(paths.models.join("model.bin").is_file());
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
                .uri(format!("/internal/items/{item_id}/reindex"))
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
                    .uri("/internal/items")
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
                    .uri("/internal/items/item-1")
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
                    .uri("/internal/chunks/chunk-png/frame")
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
                    .uri("/internal/chunks/chunk-missing/frame")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn v1_chunk_binary_routes_resolve_agent_evidence_urls() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let image = temp.path().join("frame.PNG");
        let video = temp.path().join("clip.mp4");
        std::fs::write(&image, b"png-bytes").unwrap();
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
                INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, frame_path, metadata)
                VALUES ('chunk-frame', 'item-1', 'keyframe', 2, 2, ?1, '{}')
                "#,
                [image.to_string_lossy().as_ref()],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, text, metadata)
                VALUES ('chunk-video', 'item-1', 'transcript', 2, 5, 'hello', '{}')
                "#,
                [],
            )
            .unwrap();
        }
        let app = router_with_paths(paths);

        let frame = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/chunks/chunk-frame/frame")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(frame.status(), StatusCode::OK);
        assert_eq!(
            frame.headers().get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static("image/png"))
        );
        let frame_body = to_bytes(frame.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&frame_body[..], b"png-bytes");

        let segment = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/chunks/chunk-video/video-segment")
                    .header(header::RANGE, "bytes=1-3")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(segment.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            segment.headers().get(header::CONTENT_RANGE),
            Some(&HeaderValue::from_static("bytes 1-3/16"))
        );
        let segment_body = to_bytes(segment.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&segment_body[..], b"123");

        let missing_clip = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/chunks/missing/video-clip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_clip.status(), StatusCode::NOT_FOUND);
        let missing_clip = response_json(missing_clip).await;
        assert_eq!(missing_clip["error"], "video clip not found");
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
                    .uri("/internal/chunks/chunk-1/video-segment")
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
                    .uri("/internal/chunks/chunk-1/video-segment")
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
                    .uri("/internal/chunks/chunk-1/video-segment")
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
        // Symmetric (before == after) behaves like the old padding.
        assert_eq!(clip_window(Some(10.0), Some(20.0), 2.0, 2.0), (8.0, 14.0));
        assert_eq!(clip_window(Some(1.0), Some(3.0), 5.0, 5.0), (0.0, 8.0));
        assert_eq!(clip_window(Some(0.0), None, 2.0, 2.0), (0.0, 14.0));
        // Total duration capped at 120s.
        assert_eq!(
            clip_window(Some(10.0), Some(400.0), 10.0, 10.0),
            (0.0, 120.0)
        );
        // Asymmetric before/after.
        assert_eq!(clip_window(Some(60.0), Some(70.0), 10.0, 4.0), (50.0, 24.0));
        // Per-side extension capped at 30s each.
        assert_eq!(
            clip_window(Some(100.0), Some(110.0), 50.0, 50.0),
            (70.0, 70.0)
        );
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
        conn.execute(
            "INSERT INTO chunks (id, item_id, chunk_type, text, metadata) VALUES ('untimed-video-chunk', 'video-1', 'understanding', 'summary', '{}')",
            [],
        )
        .unwrap();

        assert!(video_clip_source_for_chunk(&paths, "image-chunk")
            .unwrap()
            .is_none());
        assert!(video_clip_source_for_chunk(&paths, "video-chunk")
            .unwrap()
            .is_some());
        assert!(video_clip_source_for_chunk(&paths, "untimed-video-chunk")
            .unwrap()
            .is_none());
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
    async fn add_remote_source_http_returns_syncing_before_discovery() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let app = router_with_paths(paths.clone());
        let body = json!({
            "type": "youtube",
            "config": {
                "url": "https://www.youtube.com/@cerul",
                "max_videos": 2,
                "ytdlp_path": fake_slow_ytdlp(&temp),
                "cache_dir": temp.path().join("cache"),
                "timeout_sec": 5
            }
        });

        let started = std::time::Instant::now();
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/internal/sources")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(response.status(), StatusCode::OK);
        let source = response_json(response).await;
        assert_eq!(source["status"], "syncing");
        let source_id = source["id"].as_str().unwrap();

        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        let item_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM items WHERE source_id = ?1",
                [source_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(item_count, 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn background_source_discovery_persists_items_and_activates_source() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let source = create_syncing_source(
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
        .unwrap();

        let token = rotate_discovery_token(&paths, &source.id).unwrap();
        discover_source_items_to_paths(&paths, &source.id, &token)
            .await
            .unwrap();

        let source = source_by_id(&paths, &source.id).unwrap();
        assert_eq!(source.status, "active");
        assert!(source.last_poll_at.is_some());
        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        let item_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM items WHERE source_id = ?1 AND status = 'discovered'",
                [source.id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        let job_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM jobs WHERE job_type = 'index_video' AND status = 'queued'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(item_count, 2);
        assert_eq!(job_count, 2);
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
                    .uri("/internal/sources/preview/rss")
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
                    .uri("/internal/usage/summary")
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
                    .uri("/internal/usage/events?limit=1")
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
                    .uri("/internal/items")
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
                    .uri("/internal/jobs")
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
    async fn list_items_supports_paging_filters_and_light_records() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let missing_raw_path = temp
            .path()
            .join("sleeping-disk-video.mp4")
            .to_string_lossy()
            .to_string();
        seed_indexing_schema_version(&paths);
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                "INSERT INTO sources (id, type, config, status) VALUES ('source-a', 'folder_video', '{}', 'active'), ('source-b', 'folder_video', '{}', 'active')",
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO items (
                    id, source_id, content_type, external_id, title, raw_path,
                    indexed_at, status, metadata
                )
                VALUES
                    ('item-new', 'source-a', 'video', 'new.mp4', 'New', NULL, 30, 'indexed', '{"channel":"heavy"}'),
                    ('item-old', 'source-a', 'video', 'old.mp4', 'Old', ?1, 10, 'indexed', '{"channel":"heavy"}'),
                    ('item-other', 'source-b', 'video', 'other.mp4', 'Other', NULL, 20, 'indexed', '{"channel":"heavy"}'),
                    ('item-running', 'source-a', 'video', 'running.mp4', 'Running', NULL, NULL, 'discovered', '{"channel":"heavy"}')
                "#,
                [missing_raw_path.as_str()],
            )
            .unwrap();
        }
        let app = router_with_paths(paths);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/internal/items?source_id=source-a&status=indexed&limit=1&cursor=1&light=true")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let items = response_json(response).await;
        let items = items.as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], "item-old");
        assert_eq!(items[0]["metadata"], json!({}));
        assert!(items[0]["raw_path_exists"].is_null());
        assert_eq!(items[0]["usage"]["event_count"], 0);
    }

    #[tokio::test]
    async fn list_jobs_supports_paging_filters_and_light_records() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                "INSERT INTO sources (id, type, config, status) VALUES ('source-a', 'folder_video', '{}', 'active'), ('source-b', 'folder_video', '{}', 'active')",
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO items (id, source_id, content_type, external_id, title, status, metadata)
                VALUES
                    ('item-a', 'source-a', 'video', 'a.mp4', 'A', 'discovered', '{}'),
                    ('item-b', 'source-b', 'video', 'b.mp4', 'B', 'discovered', '{}')
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO jobs (
                    id, item_id, job_type, status, started_at, finished_at, error, progress, stage, stage_message
                )
                VALUES
                    ('job-a-running', 'item-a', 'index_video', 'running', 30, NULL, 'verbose error', 0.5, 'asr', 'verbose stage'),
                    ('job-a-done', 'item-a', 'index_video', 'succeeded', 20, 25, NULL, 1, 'done', NULL),
                    ('job-b-running', 'item-b', 'index_video', 'running', 40, NULL, NULL, 0.25, 'asr', NULL)
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
                    .uri("/internal/jobs?source_id=source-a&status=queued,running&limit=1&light=true")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let jobs = response_json(response).await;
        let jobs = jobs.as_array().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0]["id"], "job-a-running");
        assert_eq!(jobs[0]["error"], Value::Null);
        assert_eq!(jobs[0]["stage_message"], Value::Null);
        assert_eq!(jobs[0]["usage"]["event_count"], 0);
    }

    #[tokio::test]
    async fn list_jobs_drawer_scope_returns_active_and_recent_terminal_jobs() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        {
            let conn = cerul_storage::sqlite::open(&paths).unwrap();
            conn.execute(
                "INSERT INTO sources (id, type, config, status) VALUES ('source-a', 'folder_video', '{}', 'active')",
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO items (id, source_id, content_type, external_id, title, status, metadata)
                VALUES
                    ('item-a', 'source-a', 'video', 'a.mp4', 'A', 'discovered', '{}'),
                    ('item-b', 'source-a', 'video', 'b.mp4', 'B', 'discovered', '{}')
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO jobs (
                    id, item_id, job_type, status, started_at, finished_at, error, progress, stage, stage_message
                )
                VALUES
                    ('job-completed-old', 'item-a', 'index_video', 'completed', 10, 20, NULL, 1, 'completed', NULL),
                    ('job-old-failed', 'item-a', 'index_video', 'failed', 30, 40, 'old fail', 1, 'failed', NULL),
                    ('job-running', 'item-a', 'index_video', 'running', 50, NULL, NULL, 0.5, 'asr', NULL),
                    ('job-queued', 'item-b', 'index_video', 'queued', NULL, NULL, NULL, 0, 'queued', NULL)
                "#,
                [],
            )
            .unwrap();
            conn.execute(
                r#"
                INSERT INTO jobs (
                    id, item_id, job_type, status, started_at, finished_at, error, progress, stage, stage_message
                )
                VALUES
                    (
                        'job-recent-failed',
                        'item-b',
                        'index_video',
                        'failed',
                        strftime('%s','now') - 20,
                        strftime('%s','now') - 10,
                        'recent fail',
                        1,
                        'failed',
                        NULL
                    ),
                    (
                        'job-stale-completed',
                        'item-a',
                        'index_video',
                        'completed',
                        strftime('%s','now') - 90000,
                        strftime('%s','now') - 90000,
                        NULL,
                        1,
                        'completed',
                        NULL
                    ),
                    (
                        'job-recent-completed',
                        'item-a',
                        'index_video',
                        'completed',
                        strftime('%s','now') - 3600,
                        strftime('%s','now') - 20,
                        NULL,
                        1,
                        'completed',
                        NULL
                    )
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
                    .uri("/internal/jobs?scope=drawer&light=true&limit=10")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let jobs = response_json(response).await;
        let ids = jobs
            .as_array()
            .unwrap()
            .iter()
            .map(|job| job["id"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "job-running".to_string(),
                "job-queued".to_string(),
                "job-recent-failed".to_string(),
                "job-recent-completed".to_string()
            ]
        );
        assert!(!ids.contains(&"job-completed-old".to_string()));
        assert!(!ids.contains(&"job-stale-completed".to_string()));
        assert!(!ids.contains(&"job-old-failed".to_string()));
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
                    .uri("/internal/sources")
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
                    .uri("/internal/sources")
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
                    .uri("/internal/items")
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
                    .uri("/internal/settings")
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
for arg in "$@"; do
  if [ "$arg" = "--dump-single-json" ]; then
  url=""
  for value in "$@"; do
    url="$value"
  done
  id="${url##*=}"
  printf '{"id":"%s","title":"Checked video","duration":12}\n' "$id"
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

    #[cfg(unix)]
    fn fake_slow_ytdlp(temp: &tempfile::TempDir) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let script = temp.path().join("yt-dlp-slow");
        std::fs::write(
            &script,
            r#"#!/bin/sh
for arg in "$@"; do
  if [ "$arg" = "--flat-playlist" ]; then
  sleep 2
  printf '{"id":"abc123","title":"First video","duration":12}\n'
  exit 0
  fi
done
exit 1
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
            .uri("/internal/health")
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
