use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use axum::{extract::State, Json};
use cerul_pipeline::run::Embedder;
use cerul_storage::AppPaths;
use serde::Serialize;
use serde_json::json;
use tokio::io::AsyncWriteExt;

use crate::{jobs, setting_string, ApiResult, ApiState};

pub const DEFAULT_ASR_MODEL_ID: &str = "whisper-1";
pub const LOCAL_ASR_MODEL_ID: &str = "qwen3-asr-0.6b-local";
pub const DEFAULT_EMBEDDING_MODEL_ID: &str = "gemini-embedding-2";
pub const DEFAULT_VIDEO_UNDERSTANDING_MODEL_ID: &str = "gemini-3.5-flash";

/// Default legacy fallback model when the selected ASR path is Whisper.
pub const DEFAULT_WHISPER_MODEL_ID: &str = "base.en";

/// Tracks whether an auto-download is currently in progress so we don't
/// double-spawn when multiple sources are added quickly.
static AUTO_DOWNLOAD_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static AUTO_DOWNLOAD_LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

/// Live byte-level progress for the active legacy Whisper model download.
/// Updated by `download_model_file`; read by `auto_download_status`.
/// 0 means "unknown / not started".
static WHISPER_DOWNLOAD_BYTES: AtomicU64 = AtomicU64::new(0);
static WHISPER_DOWNLOAD_TOTAL: AtomicU64 = AtomicU64::new(0);
static WHISPER_DOWNLOAD_STARTED_AT_MS: AtomicU64 = AtomicU64::new(0);
static LOCAL_RUNTIME_STATUS_CACHE: Mutex<Option<CachedLocalRuntimeStatus>> = Mutex::new(None);

const LOCAL_RUNTIME_STATUS_TTL: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
struct CachedLocalRuntimeStatus {
    checked_at: Instant,
    ready: bool,
    error: Option<String>,
}

const WHISPER_MODELS: &[WhisperModelSpec] = &[
    WhisperModelSpec {
        id: "base.en",
        aliases: &["base", "fast"],
        label: "Fast",
        filename: "ggml-base.en.bin",
        size_bytes: 142 * 1024 * 1024,
        size_label: "142 MB",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin",
    },
    WhisperModelSpec {
        id: "small.en",
        aliases: &["small", "balanced"],
        label: "Balanced",
        filename: "ggml-small.en.bin",
        size_bytes: 466 * 1024 * 1024,
        size_label: "466 MB",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin",
    },
    WhisperModelSpec {
        id: "large-v3",
        aliases: &["best"],
        label: "Best",
        filename: "ggml-large-v3.bin",
        size_bytes: 2_900 * 1024 * 1024,
        size_label: "2.9 GB",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin",
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WhisperModelSpec {
    id: &'static str,
    aliases: &'static [&'static str],
    label: &'static str,
    filename: &'static str,
    size_bytes: u64,
    size_label: &'static str,
    url: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ModelSpec {
    id: &'static str,
    label: &'static str,
    capability: &'static str,
    tier: &'static str,
    format: &'static str,
    source: &'static str,
    size_label: &'static str,
    install_behavior: &'static str,
    required_for_first_search: bool,
    recommended: bool,
}

const MODEL_CATALOG: &[ModelSpec] = &[
    ModelSpec {
        id: "whisper-1",
        label: "OpenAI Whisper",
        capability: "asr",
        tier: "default",
        format: "api",
        source: "openai/whisper-1",
        size_label: "usage-based",
        install_behavior: "api-openai",
        required_for_first_search: true,
        recommended: true,
    },
    ModelSpec {
        id: "gpt-4o-mini-transcribe",
        label: "OpenAI GPT-4o mini transcribe",
        capability: "asr",
        tier: "fast",
        format: "api",
        source: "openai/gpt-4o-mini-transcribe",
        size_label: "usage-based",
        install_behavior: "api-openai",
        required_for_first_search: false,
        recommended: false,
    },
    ModelSpec {
        id: "gpt-4o-transcribe",
        label: "OpenAI GPT-4o transcribe",
        capability: "asr",
        tier: "quality",
        format: "api",
        source: "openai/gpt-4o-transcribe",
        size_label: "usage-based",
        install_behavior: "api-openai",
        required_for_first_search: false,
        recommended: false,
    },
    ModelSpec {
        id: "gemini-2.5-flash",
        label: "Gemini 2.5 Flash Audio API",
        capability: "asr",
        tier: "optional",
        format: "api",
        source: "google/gemini-2.5-flash",
        size_label: "usage-based",
        install_behavior: "api-gemini",
        required_for_first_search: false,
        recommended: false,
    },
    ModelSpec {
        id: LOCAL_ASR_MODEL_ID,
        label: "Qwen3 ASR local",
        capability: "asr",
        tier: "local",
        format: "mlx",
        source: "Qwen/Qwen3-ASR-0.6B",
        size_label: "local runtime",
        install_behavior: "local-mlx",
        required_for_first_search: false,
        recommended: true,
    },
    ModelSpec {
        id: "gemini-embedding-2",
        label: "Gemini Embedding 2",
        capability: "multimodal_embedding",
        tier: "default",
        format: "api",
        source: "google/gemini-embedding-2",
        size_label: "3072 dimensions",
        install_behavior: "api-gemini",
        required_for_first_search: true,
        recommended: true,
    },
    ModelSpec {
        id: "qwen3-vl-embedding-2b-local",
        label: "Qwen3-VL Embedding local",
        capability: "multimodal_embedding",
        tier: "local",
        format: "mlx",
        source: "mlx-community/Qwen3-VL-Embedding-2B-6bit",
        size_label: "2048 dimensions",
        install_behavior: "local-mlx",
        required_for_first_search: false,
        recommended: true,
    },
    ModelSpec {
        id: "gemini-3.5-flash",
        label: "Gemini 3.5 Flash Video",
        capability: "video_understanding",
        tier: "beta",
        format: "api",
        source: "google/gemini-3.5-flash",
        size_label: "usage-based",
        install_behavior: "api-gemini",
        required_for_first_search: false,
        recommended: false,
    },
];

#[derive(Debug, Clone, Serialize)]
pub struct WhisperModelRecord {
    pub id: String,
    pub label: String,
    pub filename: String,
    pub size_bytes: u64,
    pub size_label: String,
    pub url: String,
    pub installed: bool,
    pub selected: bool,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelCatalogResponse {
    pub models: Vec<ModelCatalogRecord>,
    pub active_embedding_profile: cerul_storage::vectors::EmbeddingProfile,
    pub embedding_profiles: Vec<cerul_storage::vectors::EmbeddingProfile>,
    pub runtime: ModelRuntimeStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelCatalogRecord {
    pub id: String,
    pub label: String,
    pub capability: String,
    pub tier: String,
    pub format: String,
    pub source: String,
    pub size_label: String,
    pub install_behavior: String,
    pub required_for_first_search: bool,
    pub recommended: bool,
    pub installed: bool,
    pub selected: bool,
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelRuntimeStatus {
    pub platform: String,
    pub api_runtime_ready: bool,
    pub local_runtime_ready: bool,
    pub openai_ready: bool,
    pub gemini_ready: bool,
    pub last_error: Option<String>,
    pub local_runtime_error: Option<String>,
}

pub async fn model_catalog(State(state): State<ApiState>) -> ApiResult<Json<ModelCatalogResponse>> {
    Ok(Json(model_catalog_for_paths(&state.paths)?))
}

pub fn model_catalog_for_paths(paths: &AppPaths) -> anyhow::Result<ModelCatalogResponse> {
    let configured_inference_mode = selected_inference_mode(paths);
    let runtime = model_runtime_status(paths);
    crate::sync_deferred_embedding_rebuild_if_ready(paths, &runtime)?;
    let inference_mode = effective_inference_mode_for_runtime(&configured_inference_mode, &runtime);
    let active_embedding_profile =
        cerul_storage::vectors::ensure_embedding_profile_for_inference_mode(
            paths,
            &inference_mode,
        )?;
    let embedding_profiles = cerul_storage::vectors::list_embedding_profiles(paths)?;
    let selected_asr = selected_remote_asr_model_id(paths);
    let selected_video_understanding = selected_video_understanding_model_id(paths)
        .unwrap_or_else(|| DEFAULT_VIDEO_UNDERSTANDING_MODEL_ID.to_string());

    let models = MODEL_CATALOG
        .iter()
        .map(|spec| {
            let selected = match spec.capability {
                "asr" if inference_mode == "local" => spec.id == LOCAL_ASR_MODEL_ID,
                "asr" => spec.tier != "local" && selected_asr == spec.id,
                "multimodal_embedding" => embedding_model_selected(spec, &active_embedding_profile),
                "video_understanding" => selected_video_understanding == spec.id,
                _ => false,
            };
            let installed = model_installed(paths, spec, &active_embedding_profile, &runtime);
            let blocked_reason = model_blocked_reason(spec, &runtime, installed);
            ModelCatalogRecord {
                id: spec.id.to_string(),
                label: spec.label.to_string(),
                capability: spec.capability.to_string(),
                tier: spec.tier.to_string(),
                format: spec.format.to_string(),
                source: spec.source.to_string(),
                size_label: spec.size_label.to_string(),
                install_behavior: spec.install_behavior.to_string(),
                required_for_first_search: spec.required_for_first_search,
                recommended: spec.recommended,
                installed,
                selected,
                blocked_reason,
            }
        })
        .collect();

    Ok(ModelCatalogResponse {
        models,
        active_embedding_profile,
        embedding_profiles,
        runtime,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelDownloadResponse {
    pub id: String,
    pub installed: bool,
    pub path: String,
    pub size_bytes: u64,
}

pub async fn list_whisper_models(
    State(state): State<ApiState>,
) -> ApiResult<Json<Vec<WhisperModelRecord>>> {
    Ok(Json(whisper_model_records(&state.paths)?))
}

pub async fn download_whisper_model(
    State(state): State<ApiState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<ModelDownloadResponse>> {
    let spec = whisper_model_by_id(&id)?;
    let path = whisper_model_path(&state.paths, spec);

    // Mark in-progress so the banner / Indexing settings show live progress
    // for manual downloads too. Skip if another download is already running.
    let was_idle = !AUTO_DOWNLOAD_IN_PROGRESS.swap(true, Ordering::AcqRel);
    if was_idle {
        if let Ok(mut guard) = AUTO_DOWNLOAD_LAST_ERROR.lock() {
            *guard = None;
        }
    }
    let download = model_download_config(&state.paths);
    let result = download_model_file(spec.url, &path, &download).await;
    if was_idle {
        AUTO_DOWNLOAD_IN_PROGRESS.store(false, Ordering::Release);
    }
    if let Err(error) = &result {
        if let Ok(mut guard) = AUTO_DOWNLOAD_LAST_ERROR.lock() {
            *guard = Some(error.to_string());
        }
    }
    result?;
    select_whisper_model(&state.paths, spec.id)?;
    let _worker = jobs::spawn_default_job_worker(state.paths.clone());

    Ok(Json(ModelDownloadResponse {
        id: spec.id.to_string(),
        installed: true,
        path: path_to_string(&path),
        size_bytes: spec.size_bytes,
    }))
}

#[derive(Debug, Clone, Serialize)]
pub struct AutoDownloadStatus {
    pub in_progress: bool,
    pub model_id: &'static str,
    pub size_label: &'static str,
    pub last_error: Option<String>,
    /// True when *some* Whisper model is on disk, regardless of which one.
    pub any_model_installed: bool,
    /// Bytes already written to disk for the in-progress download. 0 when idle.
    pub downloaded_bytes: u64,
    /// Total bytes expected, taken from the response Content-Length when
    /// available; 0 means unknown (fall back to the model spec size).
    pub total_bytes: u64,
    /// Bytes per second across the lifetime of the current download (smoothed
    /// by being computed over total elapsed time, not a rolling window — keeps
    /// the UI stable and is good enough for an ETA).
    pub bytes_per_second: u64,
    /// Estimated seconds remaining. 0 when speed/total are unknown.
    pub eta_seconds: u64,
}

pub async fn get_auto_download_status(
    State(state): State<ApiState>,
) -> ApiResult<Json<AutoDownloadStatus>> {
    Ok(Json(auto_download_status(&state.paths)))
}

#[derive(Debug, Clone, Serialize)]
pub struct EmbeddingStatus {
    pub ready: bool,
    pub preparing: bool,
    pub cached_mb: f64,
    pub last_error: Option<String>,
    pub download_source: String,
    pub download_proxy_configured: bool,
}

static EMBED_PREPARING: AtomicBool = AtomicBool::new(false);
static EMBED_LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

pub async fn get_embedding_status(
    State(state): State<ApiState>,
) -> ApiResult<Json<EmbeddingStatus>> {
    let last_error = EMBED_LAST_ERROR.lock().ok().and_then(|guard| guard.clone());
    Ok(Json(EmbeddingStatus {
        ready: gemini_provider_ready(&state.paths) && last_error.is_none(),
        preparing: EMBED_PREPARING.load(Ordering::Acquire),
        cached_mb: 0.0,
        last_error,
        download_source: "api".to_string(),
        download_proxy_configured: false,
    }))
}

/// Trigger a background warm-up of the embedding models. fastembed downloads
/// model files on first init; this gives users a way to do that explicitly
/// without waiting for the first index job. Idempotent — concurrent calls
/// coalesce into a single background task via `EMBED_PREPARING`.
pub async fn prepare_embedding_models(
    State(state): State<ApiState>,
) -> ApiResult<Json<EmbeddingStatus>> {
    if !EMBED_PREPARING.swap(true, Ordering::AcqRel) {
        if let Ok(mut guard) = EMBED_LAST_ERROR.lock() {
            *guard = None;
        }
        let paths = state.paths.clone();

        tokio::task::spawn_blocking(move || {
            let outcome = crate::api_models::selected_embedder(&paths)
                .and_then(|embedder| {
                    embedder.embed_texts(&["Cerul API embedding test".to_string()])
                })
                .map(|_| ());
            if let Err(error) = &outcome {
                let message = explain_api_embedding_error(error);
                tracing::warn!(error = %message, "embedding provider test failed");
                if let Ok(mut guard) = EMBED_LAST_ERROR.lock() {
                    *guard = Some(message);
                }
            } else {
                tracing::info!("embedding provider test complete");
            }
            EMBED_PREPARING.store(false, Ordering::Release);
        });
    }

    let last_error = EMBED_LAST_ERROR.lock().ok().and_then(|guard| guard.clone());
    Ok(Json(EmbeddingStatus {
        ready: gemini_provider_ready(&state.paths) && last_error.is_none(),
        preparing: EMBED_PREPARING.load(Ordering::Acquire),
        cached_mb: 0.0,
        last_error,
        download_source: "api".to_string(),
        download_proxy_configured: false,
    }))
}

pub(crate) fn model_download_config(paths: &AppPaths) -> cerul_embed::ModelDownloadConfig {
    let source = setting_string(paths, "model_download_source")
        .ok()
        .flatten()
        .map(|value| cerul_embed::ModelDownloadSource::parse(&value))
        .unwrap_or(cerul_embed::ModelDownloadSource::Auto);
    let proxy_url = setting_string(paths, "model_download_proxy")
        .ok()
        .flatten()
        .and_then(clean_optional_setting);
    cerul_embed::ModelDownloadConfig::default()
        .with_source(source)
        .with_proxy_url(proxy_url)
}

fn clean_optional_setting(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn explain_api_embedding_error(error: &anyhow::Error) -> String {
    let original = error.to_string();
    if original.contains("API key") || original.contains("provider") {
        return original;
    }
    format!("Could not reach Gemini Embedding 2. Check provider key, quota, and network. Original error: {original}")
}

pub(crate) fn model_runtime_status(paths: &AppPaths) -> ModelRuntimeStatus {
    let openai_ready = provider_ready(paths, &["openai", "openai-compatible"]);
    let gemini_ready = gemini_provider_ready(paths);
    let api_runtime_ready = openai_ready && gemini_ready;
    let (local_runtime_ready, local_runtime_error) = local_runtime_readiness(paths);
    let last_error = if api_runtime_ready {
        None
    } else {
        let mut missing = Vec::new();
        if !openai_ready {
            missing.push("OpenAI ASR provider");
        }
        if !gemini_ready {
            missing.push("Gemini Embedding 2 provider");
        }
        Some(format!(
            "Connect {} before indexing.",
            missing.join(" and ")
        ))
    };

    ModelRuntimeStatus {
        platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        api_runtime_ready,
        local_runtime_ready,
        openai_ready,
        gemini_ready,
        last_error,
        local_runtime_error,
    }
}

fn local_runtime_readiness(paths: &AppPaths) -> (bool, Option<String>) {
    if let Ok(guard) = LOCAL_RUNTIME_STATUS_CACHE.lock() {
        if let Some(cached) = guard.as_ref() {
            if cached.checked_at.elapsed() < LOCAL_RUNTIME_STATUS_TTL {
                return (cached.ready, cached.error.clone());
            }
        }
    }

    let (ready, error) = match cerul_pipeline::mlx_sidecar::runtime_status(paths) {
        Ok(status) if status.ok => (true, None),
        Ok(status) => {
            let message = if !status.apple_silicon {
                "Local MLX runtime requires Apple Silicon macOS.".to_string()
            } else if !status.missing.is_empty() {
                format!(
                    "Install MLX runtime packages: {}.",
                    status.missing.join(", ")
                )
            } else {
                "Local MLX runtime is not ready.".to_string()
            };
            (false, Some(message))
        }
        Err(error) => (false, Some(error.to_string())),
    };

    if let Ok(mut guard) = LOCAL_RUNTIME_STATUS_CACHE.lock() {
        *guard = Some(CachedLocalRuntimeStatus {
            checked_at: Instant::now(),
            ready,
            error: error.clone(),
        });
    }

    (ready, error)
}

fn gemini_provider_ready(paths: &AppPaths) -> bool {
    provider_ready(paths, &["gemini"])
}

fn provider_ready(paths: &AppPaths, provider_types: &[&str]) -> bool {
    cerul_storage::providers::list_providers(paths)
        .map(|providers| {
            providers.into_iter().any(|provider| {
                provider.id != cerul_storage::providers::LOCAL_PROVIDER_ID
                    && provider_types.contains(&provider.provider_type.as_str())
                    && crate::providers::has_provider_key_for_provider(paths, &provider)
            })
        })
        .unwrap_or(false)
}

fn model_installed(
    paths: &AppPaths,
    spec: &ModelSpec,
    active_embedding_profile: &cerul_storage::vectors::EmbeddingProfile,
    runtime: &ModelRuntimeStatus,
) -> bool {
    match spec.install_behavior {
        "api-openai" => provider_ready(paths, &["openai", "openai-compatible"]),
        "api-gemini" => gemini_provider_ready(paths),
        "local-mlx" => runtime.local_runtime_ready,
        "embedding-profile" => {
            spec.id == DEFAULT_EMBEDDING_MODEL_ID
                && cerul_storage::vectors::is_default_embedding_profile_id(
                    &active_embedding_profile.id,
                )
        }
        "manual-fallback" => selected_whisper_model_path(paths).is_some(),
        _ => paths.models.join(spec.capability).join(spec.id).exists(),
    }
}

fn embedding_model_selected(
    spec: &ModelSpec,
    active_embedding_profile: &cerul_storage::vectors::EmbeddingProfile,
) -> bool {
    match spec.id {
        DEFAULT_EMBEDDING_MODEL_ID => {
            cerul_storage::vectors::is_default_embedding_profile_id(&active_embedding_profile.id)
        }
        _ => active_embedding_profile.model_id.ends_with(spec.source),
    }
}

fn model_blocked_reason(
    spec: &ModelSpec,
    runtime: &ModelRuntimeStatus,
    installed: bool,
) -> Option<String> {
    match spec.install_behavior {
        "api-openai" if !installed => {
            return Some("Connect an OpenAI or OpenAI-compatible provider".to_string());
        }
        "api-gemini" if !installed => {
            return Some("Connect a Gemini provider".to_string());
        }
        "local-mlx" if !installed => {
            return Some(
                runtime
                    .local_runtime_error
                    .clone()
                    .unwrap_or_else(|| "Local MLX runtime is not available".to_string()),
            );
        }
        _ => {}
    }
    let _ = runtime;
    None
}

pub(crate) fn selected_asr_model_id(paths: &AppPaths) -> Option<String> {
    selected_setting(paths, "asr_model").or_else(|| env_setting("CERUL_ASR_MODEL"))
}

fn selected_remote_asr_model_id(paths: &AppPaths) -> String {
    selected_asr_model_id(paths)
        .filter(|model| !is_local_asr_model_id(model))
        .unwrap_or_else(|| DEFAULT_ASR_MODEL_ID.to_string())
}

pub(crate) fn is_local_asr_model_id(model_id: &str) -> bool {
    model_id == LOCAL_ASR_MODEL_ID
}

pub(crate) fn selected_video_understanding_model_id(paths: &AppPaths) -> Option<String> {
    selected_setting(paths, "video_understanding_model")
        .or_else(|| env_setting("CERUL_VIDEO_UNDERSTANDING_MODEL"))
}

fn selected_inference_mode(paths: &AppPaths) -> String {
    selected_setting(paths, "inference_mode")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| value == "remote" || value == "local" || value == "auto")
        .unwrap_or_else(|| "auto".to_string())
}

fn effective_inference_mode_for_runtime(mode: &str, runtime: &ModelRuntimeStatus) -> String {
    match mode {
        "local" => "local".to_string(),
        "auto" if runtime.local_runtime_ready => "local".to_string(),
        _ => "remote".to_string(),
    }
}

fn env_setting(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn selected_setting(paths: &AppPaths, key: &str) -> Option<String> {
    let conn = cerul_storage::sqlite::open(paths).ok()?;
    let value: String = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .ok()?;
    serde_json::from_str::<serde_json::Value>(&value)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .or(Some(value))
}

pub fn auto_download_status(paths: &AppPaths) -> AutoDownloadStatus {
    let any_installed = selected_whisper_model_path(paths).is_some();
    let last_error = AUTO_DOWNLOAD_LAST_ERROR
        .lock()
        .ok()
        .and_then(|guard| guard.clone());
    let spec = auto_download_whisper_model_spec(paths).unwrap_or_else(|_| {
        whisper_model_by_id(DEFAULT_WHISPER_MODEL_ID).expect("default model exists")
    });

    let in_progress = AUTO_DOWNLOAD_IN_PROGRESS.load(Ordering::Acquire);
    let downloaded_bytes = WHISPER_DOWNLOAD_BYTES.load(Ordering::Acquire);
    let total_bytes = WHISPER_DOWNLOAD_TOTAL.load(Ordering::Acquire);
    let started_at_ms = WHISPER_DOWNLOAD_STARTED_AT_MS.load(Ordering::Acquire);

    let (bytes_per_second, eta_seconds) = if in_progress && started_at_ms > 0 {
        let now_ms = current_unix_millis();
        let elapsed_ms = now_ms.saturating_sub(started_at_ms).max(1);
        let bps = (downloaded_bytes.saturating_mul(1_000)) / elapsed_ms;
        let eta = if bps > 0 && total_bytes > downloaded_bytes {
            (total_bytes - downloaded_bytes) / bps.max(1)
        } else {
            0
        };
        (bps, eta)
    } else {
        (0, 0)
    };

    AutoDownloadStatus {
        in_progress,
        model_id: spec.id,
        size_label: spec.size_label,
        last_error,
        any_model_installed: any_installed,
        downloaded_bytes,
        total_bytes,
        bytes_per_second,
        eta_seconds,
    }
}

fn current_unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// v1 no longer auto-downloads local Whisper models. The endpoint remains for
/// legacy/manual compatibility, but startup and source import never call it.
pub fn ensure_default_whisper_model_in_background(paths: AppPaths) {
    let _ = paths;
}

pub fn selected_whisper_model_path(paths: &AppPaths) -> Option<PathBuf> {
    if let Ok(path) = std::env::var("CERUL_WHISPER_MODEL_PATH") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }

    let spec = whisper_model_by_id("large-v3").ok()?;
    let path = whisper_model_path(paths, spec);
    if path.is_file() {
        return Some(path);
    }

    if let Some(spec) = selected_whisper_model_spec(paths) {
        let path = whisper_model_path(paths, spec);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

fn auto_download_whisper_model_spec(paths: &AppPaths) -> anyhow::Result<&'static WhisperModelSpec> {
    if selected_asr_model_id(paths).as_deref() == Some("whisper-large-v3-turbo") {
        return whisper_model_by_id("large-v3");
    }
    if let Some(spec) = selected_whisper_model_spec(paths) {
        return Ok(spec);
    }
    whisper_model_by_id(DEFAULT_WHISPER_MODEL_ID)
}

fn whisper_model_records(paths: &AppPaths) -> anyhow::Result<Vec<WhisperModelRecord>> {
    let selected = selected_whisper_model_id(paths);

    Ok(WHISPER_MODELS
        .iter()
        .map(|spec| {
            let path = whisper_model_path(paths, spec);
            let installed = path.is_file();
            WhisperModelRecord {
                id: spec.id.to_string(),
                label: spec.label.to_string(),
                filename: spec.filename.to_string(),
                size_bytes: spec.size_bytes,
                size_label: spec.size_label.to_string(),
                url: spec.url.to_string(),
                installed,
                selected: selected.as_deref() == Some(spec.id),
                path: installed.then(|| path_to_string(&path)),
            }
        })
        .collect())
}

fn whisper_model_by_id(id: &str) -> anyhow::Result<&'static WhisperModelSpec> {
    WHISPER_MODELS
        .iter()
        .find(|spec| spec.matches(id))
        .ok_or_else(|| anyhow::anyhow!("unknown Whisper model: {id}"))
}

fn selected_whisper_model_spec(paths: &AppPaths) -> Option<&'static WhisperModelSpec> {
    let selected = selected_whisper_model_id(paths)?;
    whisper_model_by_id(&selected).ok()
}

fn selected_whisper_model_id(paths: &AppPaths) -> Option<String> {
    let conn = cerul_storage::sqlite::open(paths).ok()?;
    let value: String = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'whisper_model'",
            [],
            |row| row.get(0),
        )
        .ok()?;
    let parsed = serde_json::from_str::<serde_json::Value>(&value).unwrap_or_else(|_| json!(value));
    let configured = parsed.as_str()?;
    whisper_model_by_id(configured)
        .ok()
        .map(|spec| spec.id.to_string())
}

fn select_whisper_model(paths: &AppPaths, id: &str) -> anyhow::Result<()> {
    let spec = whisper_model_by_id(id)?;
    let conn = cerul_storage::sqlite::open(paths)?;
    conn.execute(
        r#"
        INSERT INTO settings (key, value, updated_at)
        VALUES ('whisper_model', ?1, strftime('%s','now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at
        "#,
        [serde_json::Value::String(spec.id.to_string()).to_string()],
    )?;
    Ok(())
}

async fn download_model_file(
    url: &str,
    path: &PathBuf,
    download: &cerul_embed::ModelDownloadConfig,
) -> anyhow::Result<()> {
    if path.is_file() {
        return Ok(());
    }

    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid model path: {}", path.display()))?;
    tokio::fs::create_dir_all(parent).await?;
    let temp_path = path.with_extension("download");
    let client = async_download_client(download)?;
    let mut last_error = None;
    let mut response = None;
    for candidate in whisper_download_urls(url, download) {
        match client
            .get(&candidate)
            .send()
            .await
            .and_then(|res| res.error_for_status())
        {
            Ok(res) => {
                response = Some(res);
                break;
            }
            Err(error) => {
                last_error = Some(format!("{candidate}: {error}"));
            }
        }
    }
    let response = response.ok_or_else(|| {
        anyhow::anyhow!(
            "could not download model file{}",
            last_error
                .as_deref()
                .map(|error| format!(": {error}"))
                .unwrap_or_default()
        )
    })?;
    let total = response.content_length().unwrap_or(0);

    // Snapshot total + start time so the status endpoint can compute speed/ETA.
    // Reset bytes to 0 (a previous failed download may have left stale state).
    WHISPER_DOWNLOAD_TOTAL.store(total, Ordering::Release);
    WHISPER_DOWNLOAD_BYTES.store(0, Ordering::Release);
    WHISPER_DOWNLOAD_STARTED_AT_MS.store(current_unix_millis(), Ordering::Release);

    let mut response = response;
    let mut file = tokio::fs::File::create(&temp_path).await?;
    let mut downloaded: u64 = 0;

    while let Some(chunk) = response.chunk().await? {
        file.write_all(&chunk).await?;
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        WHISPER_DOWNLOAD_BYTES.store(downloaded, Ordering::Release);
    }

    file.flush().await?;
    drop(file);
    tokio::fs::rename(&temp_path, path).await?;

    // Keep totals around briefly so a final poll sees 100% — they get cleared
    // when the next download starts or when in_progress flips to false.
    Ok(())
}

fn async_download_client(
    download: &cerul_embed::ModelDownloadConfig,
) -> anyhow::Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .connect_timeout(std::time::Duration::from_secs(30))
        .user_agent(format!(
            "Cerul/{} model-downloader",
            env!("CARGO_PKG_VERSION")
        ));
    if let Some(proxy_url) = &download.proxy_url {
        builder = builder.proxy(reqwest::Proxy::all(proxy_url)?);
    }
    Ok(builder.build()?)
}

fn whisper_download_urls(
    original: &str,
    download: &cerul_embed::ModelDownloadConfig,
) -> Vec<String> {
    let mirror = original.replace("https://huggingface.co", "https://hf-mirror.com");
    match download.source {
        cerul_embed::ModelDownloadSource::HuggingFace => vec![original.to_string()],
        cerul_embed::ModelDownloadSource::HuggingFaceMirror => vec![mirror],
        // The archived local Whisper compatibility path is hosted by
        // ggerganov on Hugging Face. Keep Hugging Face first and the mirror
        // as a fallback.
        cerul_embed::ModelDownloadSource::ModelScope | cerul_embed::ModelDownloadSource::Auto => {
            if mirror == original {
                vec![original.to_string()]
            } else {
                vec![original.to_string(), mirror]
            }
        }
    }
}

fn whisper_model_path(paths: &AppPaths, spec: &WhisperModelSpec) -> PathBuf {
    paths.models.join("whisper").join(spec.filename)
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

impl WhisperModelSpec {
    fn matches(&self, id: &str) -> bool {
        self.id == id || self.aliases.contains(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_whisper_models_reports_installed_selected_model() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        write_string_setting(&paths, "asr_model", "whisper-large-v3-turbo");
        let spec = whisper_model_by_id("base.en").unwrap();
        let path = whisper_model_path(&paths, spec);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"model").unwrap();
        select_whisper_model(&paths, "fast").unwrap();

        let records = whisper_model_records(&paths).unwrap();
        let base = records
            .iter()
            .find(|record| record.id == "base.en")
            .unwrap();

        assert!(base.installed);
        assert!(base.selected);
        assert_eq!(selected_whisper_model_path(&paths), Some(path));
    }

    #[test]
    fn unknown_whisper_model_is_rejected() {
        let error = whisper_model_by_id("tiny").unwrap_err().to_string();

        assert!(error.contains("unknown Whisper model"));
    }

    #[tokio::test]
    async fn ensure_default_whisper_model_skips_when_model_installed() {
        // If the selected legacy fallback model is already on disk, the helper must not spawn
        // any background work. Defensive: reset the shared atomic in case a
        // parallel test left it flipped.
        AUTO_DOWNLOAD_IN_PROGRESS.store(false, Ordering::Release);

        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        write_string_setting(&paths, "asr_model", "whisper-large-v3-turbo");
        let spec = whisper_model_by_id("large-v3").unwrap();
        let path = whisper_model_path(&paths, spec);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"model").unwrap();

        ensure_default_whisper_model_in_background(paths.clone());

        let status = auto_download_status(&paths);
        assert!(
            !status.in_progress,
            "helper must not flip flag when model already installed"
        );
        assert!(status.any_model_installed);
        assert_eq!(status.model_id, "large-v3");
    }

    #[test]
    fn auto_download_status_reports_no_model_when_empty() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let status = auto_download_status(&paths);
        assert!(!status.any_model_installed);
        assert_eq!(status.model_id, DEFAULT_WHISPER_MODEL_ID);
    }

    #[test]
    fn auto_inference_mode_resolves_to_local_when_runtime_ready() {
        let runtime = ModelRuntimeStatus {
            platform: "test".to_string(),
            api_runtime_ready: true,
            local_runtime_ready: true,
            openai_ready: true,
            gemini_ready: true,
            last_error: None,
            local_runtime_error: None,
        };

        assert_eq!(
            effective_inference_mode_for_runtime("auto", &runtime),
            "local"
        );
        assert_eq!(
            effective_inference_mode_for_runtime("remote", &runtime),
            "remote"
        );
    }

    #[test]
    fn auto_whisper_download_prefers_hugging_face_first() {
        let urls = whisper_download_urls(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin",
            &cerul_embed::ModelDownloadConfig::default(),
        );

        assert_eq!(
            urls,
            vec![
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin",
                "https://hf-mirror.com/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin"
            ]
        );
    }

    #[test]
    fn turbo_fallback_requires_large_v3_whisper_path() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        write_string_setting(&paths, "asr_model", "whisper-large-v3-turbo");

        let base = whisper_model_by_id("base.en").unwrap();
        let base_path = whisper_model_path(&paths, base);
        std::fs::create_dir_all(base_path.parent().unwrap()).unwrap();
        std::fs::write(&base_path, b"base").unwrap();

        let status = auto_download_status(&paths);
        assert_eq!(status.model_id, "large-v3");
        assert!(!status.any_model_installed);
        assert_eq!(selected_whisper_model_path(&paths), None);

        let large = whisper_model_by_id("large-v3").unwrap();
        let large_path = whisper_model_path(&paths, large);
        std::fs::write(&large_path, b"large").unwrap();

        assert_eq!(selected_whisper_model_path(&paths), Some(large_path));
    }

    #[test]
    fn auto_download_skipped_by_env_flag() {
        // CERUL_AUTO_DOWNLOAD_MODEL=0 should make the helper a no-op.
        AUTO_DOWNLOAD_IN_PROGRESS.store(false, Ordering::Release);
        std::env::set_var("CERUL_AUTO_DOWNLOAD_MODEL", "0");
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        ensure_default_whisper_model_in_background(paths.clone());
        std::env::remove_var("CERUL_AUTO_DOWNLOAD_MODEL");

        let status = auto_download_status(&paths);
        assert!(!status.in_progress);
    }

    fn write_string_setting(paths: &AppPaths, key: &str, value: &str) {
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
                key,
                serde_json::Value::String(value.to_string()).to_string(),
            ),
        )
        .unwrap();
    }
}
