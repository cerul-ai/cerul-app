//! On-device (MLX) model preparation: capability self-check, a background
//! weight download driven by the sidecar's `--prepare` one-shot, and disk-scan
//! progress. Backs the first-run "run on this Mac" consent flow.
//!
//! Progress is measured from the model cache on disk rather than streamed from
//! the downloader. The scanner recognizes both the native Hugging Face cache
//! and Cerul's R2/CDN mirror cache, so progress survives restarts and works for
//! fallback downloads.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::Instant,
};

use anyhow::Context;
use axum::{extract::State, Json};
use cerul_pipeline::mlx_sidecar::{runtime_config, runtime_status, MlxSidecarConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ApiError, ApiResult, ApiState};

/// A single weights download must reach this fraction of its expected size
/// before we call it ready. Downloads are exact (safetensors only), so the
/// threshold mainly guards against a size estimate that is a touch high.
const READY_RATIO: f64 = 0.98;
const USER_MANAGED_MODEL_IDS: &[&str] = &["embed", "asr"];

/// Minimum installed RAM (GiB) before we recommend on-device inference.
const MIN_LOCAL_RAM_GB: u32 = 8;

static PREPARE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static PREPARE_CANCEL_REQUESTED: AtomicBool = AtomicBool::new(false);
static PREPARE_LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);
static PREPARE_STARTED_AT: Mutex<Option<Instant>> = Mutex::new(None);
static PREPARE_PID: Mutex<Option<u32>> = Mutex::new(None);
/// The model ids in the current prepare run, so status can mark the right rows
/// as "downloading" (not just the first incomplete one — matters when only one
/// capability's model is being fetched from Settings).
static PREPARE_ACTIVE_IDS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

/// One user-facing on-device model, which may map to more than one HF repo
/// (transcription needs the ASR model *and* the forced aligner).
struct LocalModelGroup {
    id: &'static str,
    label: &'static str,
    repos: Vec<String>,
    size_mb: u64,
}

/// Approximate download sizes (safetensors + configs, MB) measured from the
/// Hugging Face repos. Used for the progress bar and the "~N GB" consent copy;
/// the true ready signal is the on-disk byte count, so an estimate that is a
/// little off only affects bar smoothness, never correctness.
fn model_groups(cfg: &MlxSidecarConfig) -> Vec<LocalModelGroup> {
    vec![
        LocalModelGroup {
            id: "embed",
            label: "Multimodal embedding",
            repos: vec![cfg.embedding_model.clone()],
            size_mb: 2226,
        },
        LocalModelGroup {
            id: "asr",
            label: "Speech-to-text",
            repos: vec![cfg.asr_model.clone(), cfg.forced_aligner_model.clone()],
            size_mb: 3721,
        },
        LocalModelGroup {
            id: "ocr",
            label: "On-screen text",
            repos: vec![cfg.ocr_det_model.clone(), cfg.ocr_rec_model.clone()],
            size_mb: 30,
        },
    ]
}

fn is_user_managed_model(id: &str) -> bool {
    USER_MANAGED_MODEL_IDS.contains(&id)
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalModelBrief {
    pub id: &'static str,
    pub label: &'static str,
    pub size_mb: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalModelCapability {
    /// Apple Silicon + MLX runtime ready + enough RAM.
    pub can_run_local: bool,
    pub apple_silicon: bool,
    /// Human label for the chip family, e.g. "Apple Silicon" or the raw arch.
    pub arch: String,
    pub ram_gb: u32,
    /// "local" when the machine can run on-device comfortably, else "remote".
    pub recommended: &'static str,
    pub total_mb: u64,
    pub models: Vec<LocalModelBrief>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalModelInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub size_mb: u64,
    /// "pending" | "downloading" | "ready".
    pub status: &'static str,
    pub progress: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalPrepareStatus {
    /// "idle" | "downloading" | "ready" | "error".
    pub phase: &'static str,
    pub overall_progress: u32,
    pub done_mb: u64,
    pub total_mb: u64,
    pub eta_seconds: Option<u64>,
    pub active_source: Option<String>,
    pub source_label: Option<String>,
    pub download_bps: Option<u64>,
    pub can_pause: bool,
    pub can_cancel: bool,
    pub last_source_error: Option<String>,
    /// Source used by the most recent run, kept after it finishes so the UI can
    /// show "last used ModelScope" once `active_source` has gone null.
    pub last_source: Option<String>,
    pub last_source_label: Option<String>,
    /// Peak observed speed (B/s) of the most recent run.
    pub last_download_bps: Option<u64>,
    /// Per-source probe results from the most recent auto-selection.
    pub probes: Option<Value>,
    pub models: Vec<LocalModelInfo>,
    pub error: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct PrepareRequest {
    /// Restrict the download to these model ids; `null`/absent downloads all.
    #[serde(default)]
    pub models: Option<Vec<String>>,
}

/// GET /models/local/capability
pub async fn local_capability(
    State(state): State<ApiState>,
) -> ApiResult<Json<LocalModelCapability>> {
    let cfg = runtime_config(&state.paths).map_err(ApiError::internal)?;
    // Resilient: a missing Python/runtime must report "cannot run local", not 500.
    let status = runtime_status(&state.paths).ok();
    let apple_silicon = status
        .as_ref()
        .map(|s| s.apple_silicon)
        .unwrap_or_else(|| cfg!(target_arch = "aarch64") && cfg!(target_os = "macos"));
    let runtime_ready = status.as_ref().map(|s| s.ok).unwrap_or(false);
    let arch = status
        .as_ref()
        .map(|s| s.platform.machine.clone())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| std::env::consts::ARCH.to_string());
    let ram_gb = detect_ram_gb();

    let groups = model_groups(&cfg);
    let total_mb = groups
        .iter()
        .filter(|g| is_user_managed_model(g.id))
        .map(|g| g.size_mb)
        .sum();
    let can_run_local = apple_silicon && runtime_ready && ram_gb >= MIN_LOCAL_RAM_GB;

    Ok(Json(LocalModelCapability {
        can_run_local,
        apple_silicon,
        arch: if apple_silicon {
            "Apple Silicon".to_string()
        } else {
            arch
        },
        ram_gb,
        recommended: if can_run_local { "local" } else { "remote" },
        total_mb,
        models: groups
            .iter()
            .map(|g| LocalModelBrief {
                id: g.id,
                label: g.label,
                size_mb: g.size_mb,
            })
            .collect(),
    }))
}

/// POST /models/local/prepare — kick off a background weight download. Idempotent:
/// concurrent calls coalesce via `PREPARE_IN_PROGRESS`. Returns the current status.
pub async fn prepare_local_models(
    State(state): State<ApiState>,
    body: Option<Json<PrepareRequest>>,
) -> ApiResult<Json<LocalPrepareStatus>> {
    let cfg = runtime_config(&state.paths).map_err(ApiError::internal)?;
    let groups = model_groups(&cfg);
    let wanted = body.and_then(|Json(b)| b.models);
    let active_ids: Vec<&'static str> = match wanted.as_ref() {
        Some(ids) => groups
            .iter()
            .filter(|g| ids.iter().any(|id| id == g.id))
            .map(|g| g.id)
            .collect(),
        None => groups
            .iter()
            .filter(|g| is_user_managed_model(g.id))
            .map(|g| g.id)
            .collect(),
    };
    let repos: Vec<String> = groups
        .iter()
        .filter(|g| active_ids.contains(&g.id))
        .flat_map(|g| g.repos.clone())
        .collect();

    if !repos.is_empty() && !PREPARE_IN_PROGRESS.swap(true, Ordering::AcqRel) {
        PREPARE_CANCEL_REQUESTED.store(false, Ordering::Release);
        if let Ok(mut guard) = PREPARE_LAST_ERROR.lock() {
            *guard = None;
        }
        if let Ok(mut guard) = PREPARE_STARTED_AT.lock() {
            *guard = Some(Instant::now());
        }
        if let Ok(mut guard) = PREPARE_ACTIVE_IDS.lock() {
            *guard = active_ids.clone();
        }
        let python = cfg.python.clone();
        let script = cfg.script.clone();
        let cache = cfg.models_cache.clone();
        let download_source =
            model_download_source_setting(&state.paths).unwrap_or_else(|_| "auto".to_string());
        tokio::task::spawn_blocking(move || {
            let result = Command::new(&python)
                .arg(&script)
                .arg("--models-cache")
                .arg(&cache)
                .arg("--prepare")
                .args(&repos)
                .env("PYTHONUNBUFFERED", "1")
                .env("HF_HUB_DISABLE_XET", "1")
                .env("CERUL_MODEL_DOWNLOAD_SOURCE", &download_source)
                .spawn();
            match result {
                Ok(child) => {
                    if let Ok(mut guard) = PREPARE_PID.lock() {
                        *guard = Some(child.id());
                    }
                    match child.wait_with_output() {
                        Ok(output) if output.status.success() => {
                            tracing::info!("local model prepare complete ({} repos)", repos.len());
                        }
                        Ok(output) => {
                            let cancelled = PREPARE_CANCEL_REQUESTED.swap(false, Ordering::AcqRel);
                            if cancelled {
                                tracing::info!("local model prepare cancelled");
                            } else {
                                let stderr = String::from_utf8_lossy(&output.stderr);
                                let message = stderr
                                    .lines()
                                    .rev()
                                    .find(|line| !line.trim().is_empty())
                                    .unwrap_or("local model download failed")
                                    .to_string();
                                tracing::warn!(error = %message, "local model prepare failed");
                                if let Ok(mut guard) = PREPARE_LAST_ERROR.lock() {
                                    *guard = Some(message);
                                }
                            }
                        }
                        Err(error) => {
                            let cancelled = PREPARE_CANCEL_REQUESTED.swap(false, Ordering::AcqRel);
                            if cancelled {
                                tracing::info!("local model prepare cancelled");
                            } else {
                                tracing::warn!(error = %error, "local model prepare wait failed");
                                if let Ok(mut guard) = PREPARE_LAST_ERROR.lock() {
                                    *guard = Some(error.to_string());
                                }
                            }
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(error = %error, "local model prepare could not start");
                    if let Ok(mut guard) = PREPARE_LAST_ERROR.lock() {
                        *guard = Some(error.to_string());
                    }
                }
            }
            PREPARE_IN_PROGRESS.store(false, Ordering::Release);
            if let Ok(mut guard) = PREPARE_PID.lock() {
                *guard = None;
            }
            if let Ok(mut guard) = PREPARE_ACTIVE_IDS.lock() {
                guard.clear();
            }
        });
    }

    Ok(Json(compute_status(&cfg)))
}

/// POST /models/local/prepare-cancel — stop the active one-shot downloader.
/// Partial files remain on disk so a later prepare can resume or reuse cache.
pub async fn cancel_local_prepare(
    State(state): State<ApiState>,
) -> ApiResult<Json<LocalPrepareStatus>> {
    PREPARE_CANCEL_REQUESTED.store(true, Ordering::Release);
    let pid = PREPARE_PID.lock().ok().and_then(|guard| *guard);
    if let Some(pid) = pid {
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .status();
    }
    let cfg = runtime_config(&state.paths).map_err(ApiError::internal)?;
    Ok(Json(compute_status(&cfg)))
}

#[derive(Debug, Default, Deserialize)]
pub struct DeleteRequest {
    /// Restrict deletion to these model group ids; `null`/absent deletes all
    /// downloadable groups (OCR is bundled and never deleted).
    #[serde(default)]
    pub models: Option<Vec<String>>,
}

/// POST /models/local/delete — remove downloaded weights for the given on-device
/// model groups (embed/asr). Only touches the model cache (HF hub, Cerul mirror,
/// ModelScope); never the bundled OCR weights, the user's library, or originals.
pub async fn delete_local_models(
    State(state): State<ApiState>,
    body: Option<Json<DeleteRequest>>,
) -> ApiResult<Json<LocalPrepareStatus>> {
    if PREPARE_IN_PROGRESS.load(Ordering::Acquire) {
        return Err(ApiError::bad_request(
            "a model download is in progress; pause it before deleting",
        ));
    }
    let cfg = runtime_config(&state.paths).map_err(ApiError::internal)?;
    let wanted = body.and_then(|Json(b)| b.models);
    let hub = cfg.models_cache.join("huggingface").join("hub");
    let mirror = cfg.models_cache.join("cerul-mirror");
    let modelscope = cfg.models_cache.join("modelscope");

    for group in model_groups(&cfg) {
        // OCR ships inside the installer — there is no user-deletable copy.
        if group.id == "ocr" {
            continue;
        }
        let selected = wanted
            .as_ref()
            .map(|ids| ids.iter().any(|id| id == group.id))
            .unwrap_or(true);
        if !selected {
            continue;
        }
        for repo in &group.repos {
            let name = cache_dir_name(repo);
            for root in [&hub, &mirror, &modelscope] {
                let dir = root.join(&name);
                if dir.is_dir() {
                    fs::remove_dir_all(&dir)
                        .with_context(|| format!("failed to delete {}", dir.display()))
                        .map_err(ApiError::internal)?;
                }
            }
        }
        tracing::info!(group = group.id, "deleted local model weights");
    }

    Ok(Json(compute_status(&cfg)))
}

/// POST /models/local/repair — remove interrupted download artifacts for the
/// given model groups. This is intentionally narrower than delete: it only
/// removes temporary files/locks so a later prepare can resume cleanly.
pub async fn repair_local_models(
    State(state): State<ApiState>,
    body: Option<Json<DeleteRequest>>,
) -> ApiResult<Json<LocalPrepareStatus>> {
    if PREPARE_IN_PROGRESS.load(Ordering::Acquire) {
        return Err(ApiError::bad_request(
            "a model download is in progress; pause it before repairing",
        ));
    }
    let cfg = runtime_config(&state.paths).map_err(ApiError::internal)?;
    let wanted = body.and_then(|Json(b)| b.models);
    let hub = cfg.models_cache.join("huggingface").join("hub");
    let hf_locks = cfg.models_cache.join("huggingface").join(".locks");
    let mirror = cfg.models_cache.join("cerul-mirror");
    let modelscope = cfg.models_cache.join("modelscope");
    let mut removed = 0usize;

    for group in model_groups(&cfg) {
        if group.id == "ocr" {
            continue;
        }
        let selected = wanted
            .as_ref()
            .map(|ids| ids.iter().any(|id| id == group.id))
            .unwrap_or(true);
        if !selected {
            continue;
        }
        for repo in &group.repos {
            let name = cache_dir_name(repo);
            for root in [&hub, &mirror, &modelscope] {
                removed +=
                    remove_temporary_model_files(&root.join(&name)).map_err(ApiError::internal)?;
            }
            removed +=
                remove_temporary_model_files(&hf_locks.join(&name)).map_err(ApiError::internal)?;
        }
    }

    tracing::info!(removed, "repaired local model cache");
    Ok(Json(compute_status(&cfg)))
}

/// GET /models/local/prepare-status
pub async fn local_prepare_status(
    State(state): State<ApiState>,
) -> ApiResult<Json<LocalPrepareStatus>> {
    let cfg = runtime_config(&state.paths).map_err(ApiError::internal)?;
    Ok(Json(compute_status(&cfg)))
}

fn compute_status(cfg: &MlxSidecarConfig) -> LocalPrepareStatus {
    let hub = cfg.models_cache.join("huggingface").join("hub");
    let mirror = cfg.models_cache.join("cerul-mirror");
    let modelscope = cfg.models_cache.join("modelscope");
    let bundled = bundled_models_root();
    let sidecar_status = read_sidecar_prepare_status(&cfg.models_cache);
    let in_progress = PREPARE_IN_PROGRESS.load(Ordering::Acquire);
    let error = PREPARE_LAST_ERROR.lock().ok().and_then(|g| g.clone());
    let active_ids: Vec<&'static str> = PREPARE_ACTIVE_IDS
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default();

    let groups = model_groups(cfg);
    let progress_ids = if !active_ids.is_empty() {
        active_ids.clone()
    } else {
        groups
            .iter()
            .filter(|g| is_user_managed_model(g.id))
            .map(|g| g.id)
            .collect()
    };
    let mut models = Vec::with_capacity(groups.len());
    let mut done_mb = 0u64;
    let mut total_mb = 0u64;
    let mut downloading_assigned = false;

    for group in &groups {
        let on_disk_mb = group_cached_mb(&hub, &mirror, &modelscope, bundled.as_deref(), group);
        let capped = on_disk_mb.min(group.size_mb);
        if progress_ids.contains(&group.id) {
            total_mb += group.size_mb;
            done_mb += capped;
        }
        let progress = ((capped as f64 / group.size_mb as f64) * 100.0).round() as u32;
        let ready = group_weights_ready(&hub, &mirror, &modelscope, bundled.as_deref(), group);
        // Only mark a model "downloading" if it's actually in the active prepare
        // run (and is the first such not-yet-ready one) — so downloading one
        // capability's model from Settings doesn't light up a different row.
        let is_active = in_progress && active_ids.contains(&group.id);
        let status = if ready {
            "ready"
        } else if is_active && !downloading_assigned {
            downloading_assigned = true;
            "downloading"
        } else {
            "pending"
        };
        models.push(LocalModelInfo {
            id: group.id,
            label: group.label,
            size_mb: group.size_mb,
            status,
            progress: progress.min(100),
        });
    }

    let all_ready = models
        .iter()
        .filter(|m| progress_ids.contains(&m.id))
        .all(|m| m.status == "ready");
    let overall_progress = if total_mb == 0 {
        0
    } else {
        (((done_mb as f64 / total_mb as f64) * 100.0).round() as u32).min(100)
    };
    let phase = if error.is_some() {
        "error"
    } else if all_ready {
        "ready"
    } else if in_progress {
        "downloading"
    } else {
        "idle"
    };

    // ETA from the smoothed lifetime rate (done bytes over elapsed time).
    let eta_seconds = if in_progress && done_mb > 0 && done_mb < total_mb {
        PREPARE_STARTED_AT
            .lock()
            .ok()
            .and_then(|g| *g)
            .map(|started| started.elapsed().as_secs_f64())
            .filter(|secs| *secs > 1.0)
            .map(|secs| {
                let rate = done_mb as f64 / secs; // MB/s
                ((total_mb - done_mb) as f64 / rate.max(0.1)).round() as u64
            })
    } else {
        None
    };

    LocalPrepareStatus {
        phase,
        overall_progress,
        done_mb,
        total_mb,
        eta_seconds,
        active_source: sidecar_status.active_source,
        source_label: sidecar_status.source_label,
        download_bps: sidecar_status.download_bps,
        can_pause: in_progress,
        can_cancel: in_progress,
        last_source_error: sidecar_status.last_source_error,
        last_source: sidecar_status.last_source,
        last_source_label: sidecar_status.last_source_label,
        last_download_bps: sidecar_status.last_download_bps,
        probes: sidecar_status.probes,
        models,
        error,
    }
}

#[derive(Debug, Default)]
struct SidecarPrepareStatus {
    active_source: Option<String>,
    source_label: Option<String>,
    download_bps: Option<u64>,
    last_source_error: Option<String>,
    last_source: Option<String>,
    last_source_label: Option<String>,
    last_download_bps: Option<u64>,
    probes: Option<Value>,
}

fn read_sidecar_prepare_status(models_cache: &Path) -> SidecarPrepareStatus {
    let path = models_cache.join("prepare-status.json");
    let Ok(raw) = fs::read_to_string(path) else {
        return SidecarPrepareStatus::default();
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return SidecarPrepareStatus::default();
    };
    SidecarPrepareStatus {
        active_source: value
            .get("active_source")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        source_label: value
            .get("source_label")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        download_bps: value.get("download_bps").and_then(Value::as_u64),
        last_source_error: value
            .get("last_source_error")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        last_source: value
            .get("last_source")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        last_source_label: value
            .get("last_source_label")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        last_download_bps: value.get("last_download_bps").and_then(Value::as_u64),
        probes: value.get("probes").filter(|v| !v.is_null()).cloned(),
    }
}

/// Hugging Face cache directory name for a repo id, e.g.
/// `Qwen/Qwen3-ASR-0.6B` -> `models--Qwen--Qwen3-ASR-0.6B`.
fn cache_dir_name(repo: &str) -> String {
    format!("models--{}", repo.replace('/', "--"))
}

fn bundled_models_root() -> Option<PathBuf> {
    std::env::var_os("CERUL_BUNDLED_MODELS_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}

fn repo_cached_bytes(
    hf_hub: &Path,
    mirror_root: &Path,
    modelscope_root: &Path,
    bundled_root: Option<&Path>,
    repo: &str,
) -> u64 {
    repo_cached_bytes_with_filter(
        hf_hub,
        mirror_root,
        modelscope_root,
        bundled_root,
        repo,
        false,
    )
}

fn repo_complete_cached_bytes(
    hf_hub: &Path,
    mirror_root: &Path,
    modelscope_root: &Path,
    bundled_root: Option<&Path>,
    repo: &str,
) -> u64 {
    repo_cached_bytes_with_filter(
        hf_hub,
        mirror_root,
        modelscope_root,
        bundled_root,
        repo,
        true,
    )
}

fn repo_cached_bytes_with_filter(
    hf_hub: &Path,
    mirror_root: &Path,
    modelscope_root: &Path,
    bundled_root: Option<&Path>,
    repo: &str,
    complete_only: bool,
) -> u64 {
    let name = cache_dir_name(repo);
    dir_size_bytes_filtered(&hf_hub.join(&name), complete_only)
        + dir_size_bytes_filtered(&mirror_root.join(&name), complete_only)
        + dir_size_bytes_filtered(&modelscope_root.join(&name), complete_only)
        + bundled_root
            .map(|root| dir_size_bytes_filtered(&root.join(name), complete_only))
            .unwrap_or(0)
}

fn group_cached_mb(
    hf_hub: &Path,
    mirror_root: &Path,
    modelscope_root: &Path,
    bundled_root: Option<&Path>,
    group: &LocalModelGroup,
) -> u64 {
    group
        .repos
        .iter()
        .map(|repo| {
            repo_cached_bytes(hf_hub, mirror_root, modelscope_root, bundled_root, repo) / 1_000_000
        })
        .sum()
}

fn group_complete_cached_mb(
    hf_hub: &Path,
    mirror_root: &Path,
    modelscope_root: &Path,
    bundled_root: Option<&Path>,
    group: &LocalModelGroup,
) -> u64 {
    group
        .repos
        .iter()
        .map(|repo| {
            repo_complete_cached_bytes(hf_hub, mirror_root, modelscope_root, bundled_root, repo)
                / 1_000_000
        })
        .sum()
}

fn group_weights_ready(
    hf_hub: &Path,
    mirror_root: &Path,
    modelscope_root: &Path,
    bundled_root: Option<&Path>,
    group: &LocalModelGroup,
) -> bool {
    let every_repo_has_bytes = group.repos.iter().all(|repo| {
        repo_complete_cached_bytes(hf_hub, mirror_root, modelscope_root, bundled_root, repo) > 0
    });
    if !every_repo_has_bytes {
        return false;
    }

    group_complete_cached_mb(hf_hub, mirror_root, modelscope_root, bundled_root, group) as f64
        >= group.size_mb as f64 * READY_RATIO
}

/// True if the full local model group containing `repo` is ready on disk.
/// Transcription is one catalog row but two weight repos (ASR + forced aligner),
/// so the model catalog must not report it installed after seeing only one
/// partial repo cache.
pub fn local_model_weights_ready(paths: &cerul_storage::AppPaths, repo: &str) -> bool {
    const MIN_WEIGHT_BYTES: u64 = 64 * 1_000_000;
    let Ok(cfg) = runtime_config(paths) else {
        return false;
    };
    let hub = cfg.models_cache.join("huggingface").join("hub");
    let mirror = cfg.models_cache.join("cerul-mirror");
    let modelscope = cfg.models_cache.join("modelscope");
    let bundled = bundled_models_root();
    let groups = model_groups(&cfg);
    if let Some(group) = groups
        .iter()
        .find(|group| group.repos.iter().any(|candidate| candidate == repo))
    {
        return group_weights_ready(&hub, &mirror, &modelscope, bundled.as_deref(), group);
    }

    repo_complete_cached_bytes(&hub, &mirror, &modelscope, bundled.as_deref(), repo)
        >= MIN_WEIGHT_BYTES
}

fn model_download_source_setting(paths: &cerul_storage::AppPaths) -> anyhow::Result<String> {
    let conn = cerul_storage::sqlite::open(paths)?;
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'model_download_source'",
            [],
            |row| row.get(0),
        )
        .ok();
    Ok(value
        .and_then(|raw| serde_json::from_str::<String>(&raw).ok())
        .unwrap_or_else(|| "auto".to_string()))
}

/// Sum of real file bytes under `path`. Skips symlinks, so the HF `snapshots/`
/// symlink tree is not double-counted against the `blobs/` it points at.
fn dir_size_bytes_filtered(path: &Path, skip_temporary_downloads: bool) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                stack.push(entry.path());
            } else if file_type.is_file() {
                if skip_temporary_downloads && is_temporary_download_file(&entry.path()) {
                    continue;
                }
                if let Ok(meta) = entry.metadata() {
                    total += meta.len();
                }
            }
        }
    }
    total
}

fn is_temporary_download_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("incomplete" | "partial")
    )
}

fn is_repairable_model_file(path: &Path) -> bool {
    if is_temporary_download_file(path) {
        return true;
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.ends_with(".lock"))
        .unwrap_or(false)
}

fn remove_temporary_model_files(root: &Path) -> anyhow::Result<usize> {
    if !root.exists() {
        return Ok(0);
    }
    let mut removed = 0usize;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("failed to read {}", dir.display()));
            }
        };
        for entry in entries {
            let entry =
                entry.with_context(|| format!("failed to read entry under {}", dir.display()))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .with_context(|| format!("failed to inspect {}", path.display()))?;
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() && is_repairable_model_file(&path) {
                fs::remove_file(&path)
                    .with_context(|| format!("failed to remove {}", path.display()))?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}

/// Installed physical RAM in GiB via `sysctl hw.memsize` (macOS). 0 if unknown.
fn detect_ram_gb() -> u32 {
    if !cfg!(target_os = "macos") {
        return 0;
    }
    Command::new("sysctl")
        .arg("-n")
        .arg("hw.memsize")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|bytes| (bytes as f64 / 1024.0 / 1024.0 / 1024.0).round() as u32)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_dir_name_matches_hf_layout() {
        assert_eq!(
            cache_dir_name("Qwen/Qwen3-ASR-0.6B"),
            "models--Qwen--Qwen3-ASR-0.6B"
        );
        assert_eq!(
            cache_dir_name("PaddlePaddle/PP-OCRv6_small_det_onnx"),
            "models--PaddlePaddle--PP-OCRv6_small_det_onnx"
        );
        assert_eq!(
            cache_dir_name("PaddlePaddle/PP-OCRv6_small_rec_onnx"),
            "models--PaddlePaddle--PP-OCRv6_small_rec_onnx"
        );
    }

    #[test]
    fn missing_cache_dir_is_zero_bytes() {
        assert_eq!(
            dir_size_bytes_filtered(Path::new("/nonexistent/cerul/cache"), false),
            0
        );
    }

    #[test]
    fn repo_cached_bytes_counts_hf_mirror_and_bundled_roots() {
        let unique = format!(
            "cerul-model-cache-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        let hf = root.join("hf");
        let mirror = root.join("mirror");
        let modelscope = root.join("modelscope");
        let bundled = root.join("bundled");
        let repo = "Qwen/Qwen3-ASR-0.6B";
        let name = cache_dir_name(repo);
        std::fs::create_dir_all(hf.join(&name)).unwrap();
        std::fs::create_dir_all(mirror.join(&name)).unwrap();
        std::fs::create_dir_all(modelscope.join(&name)).unwrap();
        std::fs::create_dir_all(bundled.join(&name)).unwrap();
        std::fs::write(hf.join(&name).join("a.bin"), vec![0u8; 3]).unwrap();
        std::fs::write(mirror.join(&name).join("b.bin"), vec![0u8; 5]).unwrap();
        std::fs::write(modelscope.join(&name).join("c.bin"), vec![0u8; 7]).unwrap();
        std::fs::write(bundled.join(&name).join("c.bin"), vec![0u8; 7]).unwrap();

        assert_eq!(
            repo_cached_bytes(&hf, &mirror, &modelscope, Some(&bundled), repo),
            22
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn complete_cached_bytes_ignore_temporary_downloads() {
        let temp = tempfile::tempdir().unwrap();
        let hf = temp.path().join("hf");
        let mirror = temp.path().join("mirror");
        let modelscope = temp.path().join("modelscope");
        let repo = "Qwen/Qwen3-ASR-0.6B";
        let repo_dir = hf.join(cache_dir_name(repo));
        std::fs::create_dir_all(&repo_dir).unwrap();
        std::fs::write(repo_dir.join("model.safetensors"), vec![0u8; 3]).unwrap();
        std::fs::write(repo_dir.join("model.safetensors.incomplete"), vec![0u8; 5]).unwrap();
        std::fs::write(repo_dir.join("archive.tar.gz.partial"), vec![0u8; 7]).unwrap();

        assert_eq!(repo_cached_bytes(&hf, &mirror, &modelscope, None, repo), 15);
        assert_eq!(
            repo_complete_cached_bytes(&hf, &mirror, &modelscope, None, repo),
            3
        );
    }

    #[test]
    fn repair_removes_only_temporary_model_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("models--repo");
        let nested = root.join("snapshots").join("rev");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("model.safetensors"), vec![0u8; 3]).unwrap();
        std::fs::write(nested.join("model.safetensors.incomplete"), vec![0u8; 3]).unwrap();
        std::fs::write(nested.join("archive.tar.gz.partial"), vec![0u8; 3]).unwrap();
        std::fs::write(nested.join("download.lock"), vec![0u8; 3]).unwrap();

        assert_eq!(remove_temporary_model_files(&root).unwrap(), 3);
        assert!(nested.join("model.safetensors").exists());
        assert!(!nested.join("model.safetensors.incomplete").exists());
        assert!(!nested.join("archive.tar.gz.partial").exists());
        assert!(!nested.join("download.lock").exists());
    }

    #[test]
    fn group_ready_requires_every_repo_and_complete_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let hf = temp.path().join("hf");
        let mirror = temp.path().join("mirror");
        let modelscope = temp.path().join("modelscope");
        let asr = "Qwen/Qwen3-ASR-0.6B";
        let aligner = "Qwen/Qwen3-ForcedAligner-0.6B";
        let group = LocalModelGroup {
            id: "asr",
            label: "Speech-to-text",
            repos: vec![asr.to_string(), aligner.to_string()],
            size_mb: 2,
        };

        let asr_dir = hf.join(cache_dir_name(asr));
        std::fs::create_dir_all(&asr_dir).unwrap();
        std::fs::write(asr_dir.join("model.safetensors"), vec![0u8; 1_000_000]).unwrap();
        assert!(!group_weights_ready(
            &hf,
            &mirror,
            &modelscope,
            None,
            &group
        ));

        let aligner_dir = hf.join(cache_dir_name(aligner));
        std::fs::create_dir_all(&aligner_dir).unwrap();
        std::fs::write(
            aligner_dir.join("model.safetensors.incomplete"),
            vec![0u8; 1_000_000],
        )
        .unwrap();
        assert!(!group_weights_ready(
            &hf,
            &mirror,
            &modelscope,
            None,
            &group
        ));

        std::fs::write(aligner_dir.join("model.safetensors"), vec![0u8; 1_000_000]).unwrap();
        assert!(group_weights_ready(&hf, &mirror, &modelscope, None, &group));
    }
}
