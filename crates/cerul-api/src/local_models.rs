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

use axum::{extract::State, Json};
use cerul_pipeline::mlx_sidecar::{runtime_config, runtime_status, MlxSidecarConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ApiError, ApiResult, ApiState};

/// A single weights download must reach this fraction of its expected size
/// before we call it ready. Downloads are exact (safetensors only), so the
/// threshold mainly guards against a size estimate that is a touch high.
const READY_RATIO: f64 = 0.98;

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
    let total_mb = groups.iter().map(|g| g.size_mb).sum();
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
    let active_ids: Vec<&'static str> = groups
        .iter()
        .filter(|g| {
            wanted
                .as_ref()
                .map(|ids| ids.iter().any(|id| id == g.id))
                .unwrap_or(true)
        })
        .map(|g| g.id)
        .collect();
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
                    if let Err(error) = fs::remove_dir_all(&dir) {
                        tracing::warn!(error = %error, dir = %dir.display(), "failed to delete local model cache");
                    }
                }
            }
        }
        tracing::info!(group = group.id, "deleted local model weights");
    }

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
    let mut models = Vec::with_capacity(groups.len());
    let mut done_mb = 0u64;
    let mut total_mb = 0u64;
    let mut downloading_assigned = false;

    for group in &groups {
        total_mb += group.size_mb;
        let on_disk_mb: u64 = group
            .repos
            .iter()
            .map(|repo| {
                repo_cached_bytes(&hub, &mirror, &modelscope, bundled.as_deref(), repo) / 1_000_000
            })
            .sum();
        let capped = on_disk_mb.min(group.size_mb);
        done_mb += capped;
        let progress = ((capped as f64 / group.size_mb as f64) * 100.0).round() as u32;
        let ready = capped as f64 >= group.size_mb as f64 * READY_RATIO;
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

    let all_ready = models.iter().all(|m| m.status == "ready");
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
    let name = cache_dir_name(repo);
    dir_size_bytes(&hf_hub.join(&name))
        + dir_size_bytes(&mirror_root.join(&name))
        + dir_size_bytes(&modelscope_root.join(&name))
        + bundled_root
            .map(|root| dir_size_bytes(&root.join(name)))
            .unwrap_or(0)
}

/// True if a non-trivial amount of weights for `repo` are already cached on
/// disk (HF hub, Cerul mirror, ModelScope, or bundled). The model catalog uses
/// this so a "local" model only reports installed once its weights actually
/// exist — a ready MLX runtime alone does not mean the weights are downloaded.
pub fn repo_weights_present(paths: &cerul_storage::AppPaths, repo: &str) -> bool {
    // 64 MB floor distinguishes real weights from a config-only / empty cache;
    // the precise per-model "ready" signal lives in the prepare-status scan.
    const MIN_WEIGHT_BYTES: u64 = 64 * 1_000_000;
    let Ok(cfg) = runtime_config(paths) else {
        return false;
    };
    let hub = cfg.models_cache.join("huggingface").join("hub");
    let mirror = cfg.models_cache.join("cerul-mirror");
    let modelscope = cfg.models_cache.join("modelscope");
    let bundled = bundled_models_root();
    repo_cached_bytes(&hub, &mirror, &modelscope, bundled.as_deref(), repo) >= MIN_WEIGHT_BYTES
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
fn dir_size_bytes(path: &Path) -> u64 {
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
                if let Ok(meta) = entry.metadata() {
                    total += meta.len();
                }
            }
        }
    }
    total
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
        assert_eq!(dir_size_bytes(Path::new("/nonexistent/cerul/cache")), 0);
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
}
