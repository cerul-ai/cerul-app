use std::{
    env, fs,
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver},
        Mutex,
    },
    thread,
    time::Duration,
};

use anyhow::Context;
use cerul_storage::AppPaths;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    run::{
        Embedder, InferenceProviderInfo, ModelReleaseScope, ModelRuntimeControl, OcrEngine,
        OcrFrame, Transcriber,
    },
    whisper::{Segment, TranscriptionProgress},
};

pub const DEFAULT_EMBEDDING_MODEL: &str = "mlx-community/Qwen3-VL-Embedding-2B-6bit";
pub const DEFAULT_ASR_MODEL: &str = "Qwen/Qwen3-ASR-0.6B";
pub const DEFAULT_FORCED_ALIGNER_MODEL: &str = "Qwen/Qwen3-ForcedAligner-0.6B";
pub const DEFAULT_OCR_DET_MODEL: &str = "PaddlePaddle/PP-OCRv6_small_det_onnx";
pub const DEFAULT_OCR_REC_MODEL: &str = "PaddlePaddle/PP-OCRv6_small_rec_onnx";
pub const DEFAULT_WHISPER_MODEL: &str = "mlx-community/whisper-large-v3-turbo";
/// In-memory quantization for the official Qwen3-ASR + ForcedAligner weights.
/// "4bit" minimises RAM (~-70%); "8bit" is near-lossless; "none" keeps fp16.
pub const DEFAULT_ASR_QUANTIZATION: &str = "4bit";
const EXTERNAL_RUNTIME_READY_MARKER: &str = ".cerul-mlx-runtime-ready.json";

#[derive(Debug, Clone, Deserialize)]
pub struct ExternalMlxRuntimeManifest {
    pub archive: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub platform: String,
}

#[derive(Debug, Deserialize)]
struct ExternalRuntimeReadyMarker {
    archive_sha256: String,
}

/// Restart the sidecar if it emits no output at all for this long. The Python
/// side sends heartbeats every few seconds while it is genuinely working, so
/// total silence for this window means the process is wedged, not just slow.
const SIDECAR_IDLE_TIMEOUT: Duration = Duration::from_secs(180);
const RUNTIME_STATUS_PROBE: &str = r#"
import importlib.metadata
import json
import os
import platform
import sys

def package_version(name):
    try:
        return importlib.metadata.version(name)
    except importlib.metadata.PackageNotFoundError:
        return None

packages = {
    "mlx": package_version("mlx"),
    "mlx-embeddings": package_version("mlx-embeddings"),
    "mlx-qwen3-asr": package_version("mlx-qwen3-asr"),
    "mlx-vlm": package_version("mlx-vlm"),
    "mlx-whisper": package_version("mlx-whisper"),
    "numpy": package_version("numpy"),
    "opencv-python": package_version("opencv-python"),
    "onnxruntime": package_version("onnxruntime"),
    "Pillow": package_version("Pillow"),
    "pyclipper": package_version("pyclipper"),
    "PyYAML": package_version("PyYAML"),
    "huggingface-hub": package_version("huggingface-hub"),
}
required = ["mlx", "mlx-embeddings", "mlx-qwen3-asr", "mlx-vlm", "opencv-python", "onnxruntime", "pyclipper", "PyYAML"]
missing = [name for name in required if packages.get(name) is None]
apple_silicon = platform.system() == "Darwin" and platform.machine() == "arm64"
print(json.dumps({
    "ok": apple_silicon and not missing,
    "platform": {
        "system": platform.system(),
        "machine": platform.machine(),
        "python": sys.version.split()[0],
    },
    "apple_silicon": apple_silicon,
    "packages": packages,
    "missing": missing,
    "models": {},
    "cache": {"HF_HOME": os.environ.get("HF_HOME")},
    "loaded": {"embedding": False, "ocr": False, "asr": False, "forced_aligner": False},
}))
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MlxSidecarConfig {
    pub python: PathBuf,
    pub script: PathBuf,
    pub models_cache: PathBuf,
    pub embedding_model: String,
    pub asr_model: String,
    pub forced_aligner_model: String,
    pub asr_quantization: String,
    pub ocr_det_model: String,
    pub ocr_rec_model: String,
    pub whisper_model: String,
}

impl MlxSidecarConfig {
    pub fn for_paths(paths: &AppPaths) -> anyhow::Result<Self> {
        let repo_root = repo_root();
        let python = env::var_os("CERUL_MLX_PYTHON")
            .map(PathBuf::from)
            .or_else(|| prepared_external_runtime_python(paths))
            .unwrap_or_else(|| default_python_path(&repo_root));
        let script = env::var_os("CERUL_MLX_SIDECAR")
            .map(PathBuf::from)
            .unwrap_or_else(|| default_sidecar_script(&repo_root));

        anyhow::ensure!(
            script.is_file(),
            "MLX sidecar script does not exist: {}",
            script.display()
        );
        let models_cache = env::var_os("CERUL_MLX_MODELS_CACHE")
            .map(PathBuf::from)
            .unwrap_or_else(|| paths.models.join("mlx"));

        Ok(Self {
            python,
            script,
            models_cache,
            embedding_model: env::var("CERUL_MLX_EMBEDDING_MODEL")
                .unwrap_or_else(|_| DEFAULT_EMBEDDING_MODEL.to_string()),
            asr_model: env::var("CERUL_MLX_ASR_MODEL")
                .unwrap_or_else(|_| DEFAULT_ASR_MODEL.to_string()),
            forced_aligner_model: env::var("CERUL_MLX_FORCED_ALIGNER_MODEL")
                .unwrap_or_else(|_| DEFAULT_FORCED_ALIGNER_MODEL.to_string()),
            asr_quantization: env::var("CERUL_MLX_ASR_QUANTIZATION")
                .unwrap_or_else(|_| DEFAULT_ASR_QUANTIZATION.to_string()),
            ocr_det_model: env::var("CERUL_MLX_OCR_DET_MODEL")
                .unwrap_or_else(|_| DEFAULT_OCR_DET_MODEL.to_string()),
            ocr_rec_model: env::var("CERUL_MLX_OCR_REC_MODEL")
                .unwrap_or_else(|_| DEFAULT_OCR_REC_MODEL.to_string()),
            whisper_model: env::var("CERUL_MLX_WHISPER_MODEL")
                .unwrap_or_else(|_| DEFAULT_WHISPER_MODEL.to_string()),
        })
    }
}

pub fn runtime_config(paths: &AppPaths) -> anyhow::Result<MlxSidecarConfig> {
    MlxSidecarConfig::for_paths(paths)
}

pub fn external_runtime_manifest_from_env(
) -> anyhow::Result<Option<(PathBuf, ExternalMlxRuntimeManifest)>> {
    let Some(path) = env::var_os("CERUL_MLX_RUNTIME_MANIFEST").map(PathBuf::from) else {
        return Ok(None);
    };
    if !path.is_file() {
        return Ok(None);
    }
    let manifest: ExternalMlxRuntimeManifest = serde_json::from_slice(&fs::read(&path)?)?;
    Ok(Some((path, manifest)))
}

pub fn prepared_external_runtime_python(paths: &AppPaths) -> Option<PathBuf> {
    let (_, manifest) = external_runtime_manifest_from_env().ok()??;
    prepared_external_runtime_python_for_manifest(paths, &manifest)
}

pub fn prepared_external_runtime_python_for_manifest(
    paths: &AppPaths,
    manifest: &ExternalMlxRuntimeManifest,
) -> Option<PathBuf> {
    let digest = normalize_runtime_sha256(&manifest.sha256)?;
    let runtime_dir = external_runtime_dir(paths, &digest);
    let python = runtime_dir.join("bin").join("python3");
    let marker = runtime_dir.join(EXTERNAL_RUNTIME_READY_MARKER);
    if external_runtime_ready(&marker, &digest, &python) {
        Some(python)
    } else {
        None
    }
}

pub fn external_runtime_dir(paths: &AppPaths, digest: &str) -> PathBuf {
    paths
        .data
        .join("runtimes")
        .join("mlx")
        .join(digest.chars().take(16).collect::<String>())
}

pub fn external_runtime_ready_marker() -> &'static str {
    EXTERNAL_RUNTIME_READY_MARKER
}

pub fn normalize_runtime_sha256(value: &str) -> Option<String> {
    let digest = value.trim().to_ascii_lowercase();
    if digest.len() == 64 && digest.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Some(digest)
    } else {
        None
    }
}

fn external_runtime_ready(marker: &Path, digest: &str, python: &Path) -> bool {
    if !python.is_file() {
        return false;
    }
    let Ok(bytes) = fs::read(marker) else {
        return false;
    };
    let Ok(state) = serde_json::from_slice::<ExternalRuntimeReadyMarker>(&bytes) else {
        return false;
    };
    state.archive_sha256 == digest
}

pub fn runtime_status(paths: &AppPaths) -> anyhow::Result<MlxRuntimeStatus> {
    let config = MlxSidecarConfig::for_paths(paths)?;
    std::fs::create_dir_all(&config.models_cache)?;
    let hf_home = config.models_cache.join("huggingface");
    std::fs::create_dir_all(&hf_home)?;
    let output = Command::new(&config.python)
        .arg("-c")
        .arg(RUNTIME_STATUS_PROBE)
        .env("PYTHONUNBUFFERED", "1")
        // Don't write .pyc back into the (signed, bundled) runtime at runtime —
        // it would break the app's code seal and dirty the read-only bundle.
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("HF_HOME", &hf_home)
        .env("HF_HUB_DISABLE_XET", "1")
        .stdin(Stdio::null())
        .output()
        .with_context(|| {
            format!(
                "failed to run MLX runtime probe with {}",
                config.python.display()
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("MLX runtime probe failed");
        anyhow::bail!("{message}");
    }

    let mut status: MlxRuntimeStatus = serde_json::from_slice(&output.stdout)
        .context("failed to parse MLX runtime probe response")?;
    status.models = json!({
        "embedding": config.embedding_model,
        "asr": config.asr_model,
        "forced_aligner": config.forced_aligner_model,
        "ocr_det": config.ocr_det_model,
        "ocr_rec": config.ocr_rec_model,
    });
    status.cache = json!({ "HF_HOME": hf_home });
    Ok(status)
}

pub fn runtime_available(paths: &AppPaths) -> bool {
    runtime_status(paths).is_ok_and(|status| status.ok)
}

pub struct MlxSidecar {
    config: MlxSidecarConfig,
    process: Mutex<Option<SidecarProcess>>,
    next_id: AtomicU64,
}

impl MlxSidecar {
    pub fn for_paths(paths: &AppPaths) -> anyhow::Result<Self> {
        Ok(Self::new(MlxSidecarConfig::for_paths(paths)?))
    }

    pub fn new(config: MlxSidecarConfig) -> Self {
        Self {
            config,
            process: Mutex::new(None),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn status(&self) -> anyhow::Result<MlxRuntimeStatus> {
        let value = self.request("status", json!({}))?;
        Ok(serde_json::from_value(value)?)
    }

    pub fn release_models(&self, scope: ModelReleaseScope) -> anyhow::Result<()> {
        let _ = self.request_if_running(
            "release_models",
            json!({
                "scope": scope.as_str(),
            }),
        )?;
        Ok(())
    }

    fn request(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        self.request_inner(method, params, true)
    }

    fn request_if_running(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        self.request_inner(method, params, false)
    }

    fn request_inner(
        &self,
        method: &str,
        params: Value,
        start_if_missing: bool,
    ) -> anyhow::Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::AcqRel);
        let request = json!({
            "id": id,
            "method": method,
            "params": params,
        });
        let mut guard = self
            .process
            .lock()
            .map_err(|_| anyhow::anyhow!("MLX sidecar process lock poisoned"))?;
        if start_if_missing {
            ensure_process(&self.config, &mut guard)?;
        } else if !ensure_existing_process(&mut guard)? {
            return Ok(Value::Null);
        }

        {
            let process = guard
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("MLX sidecar process was not started"))?;
            serde_json::to_writer(&mut process.stdin, &request)?;
            process.stdin.write_all(b"\n")?;
            process.stdin.flush()?;
        }

        loop {
            let received = {
                let process = guard
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("MLX sidecar process was not started"))?;
                process.responses.recv_timeout(SIDECAR_IDLE_TIMEOUT)
            };

            let line = match received {
                Ok(Ok(line)) => line,
                Ok(Err(error)) => {
                    *guard = None;
                    anyhow::bail!("MLX sidecar read failed while waiting for {method}: {error}");
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(mut wedged) = guard.take() {
                        let _ = wedged.child.kill();
                        let _ = wedged.child.wait();
                    }
                    anyhow::bail!(
                        "MLX sidecar produced no output for {}s on {method}; \
                         it looks wedged and was restarted",
                        SIDECAR_IDLE_TIMEOUT.as_secs()
                    );
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    *guard = None;
                    anyhow::bail!("MLX sidecar exited before responding to {method}");
                }
            };

            let value: Value = serde_json::from_str(&line)
                .with_context(|| format!("invalid MLX sidecar response: {line}"))?;
            // Heartbeat notifications carry no result; they exist purely to keep
            // the idle timeout from firing during slow work.
            if value.get("event").and_then(Value::as_str) == Some("progress") {
                continue;
            }

            let response: SidecarResponse = serde_json::from_value(value)
                .with_context(|| format!("invalid MLX sidecar response: {line}"))?;
            if response.id != Some(id) {
                // A null-id error means the sidecar could not parse our
                // request line at all — waiting for "our" id would idle out
                // 180s later and needlessly kill the warm sidecar.
                if response.id.is_none() && !response.ok {
                    let error = response
                        .error
                        .map(|err| err.message)
                        .unwrap_or_else(|| "request rejected".to_string());
                    anyhow::bail!("MLX sidecar rejected the {method} request: {error}");
                }
                continue;
            }
            if response.ok {
                return Ok(response.result.unwrap_or(Value::Null));
            }
            let error = response
                .error
                .map(|error| format!("{}: {}", error.error_type, error.message))
                .unwrap_or_else(|| "unknown MLX sidecar error".to_string());
            anyhow::bail!("{method} failed in MLX sidecar: {error}");
        }
    }
}

impl Drop for MlxSidecar {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.process.lock() {
            if let Some(mut process) = guard.take() {
                let _ = process.child.kill();
                let _ = process.child.wait();
            }
        }
    }
}

impl Transcriber for MlxSidecar {
    fn prepare_transcription(&self) -> anyhow::Result<()> {
        let _ = self.request("prepare_transcription", json!({}))?;
        Ok(())
    }

    fn transcribe(
        &self,
        audio_path: &Path,
        progress: Option<TranscriptionProgress>,
    ) -> anyhow::Result<Vec<Segment>> {
        let value = self.request(
            "transcribe",
            json!({
                "audio_path": audio_path,
                "language": "auto",
            }),
        )?;
        let response: TranscribeResponse = serde_json::from_value(value)?;
        if let Some(progress) = progress {
            progress(100);
        }
        Ok(response.into_segments())
    }

    fn inference_provider(&self) -> Option<InferenceProviderInfo> {
        Some(InferenceProviderInfo {
            provider_mode: "local".to_string(),
            provider_id: Some("local".to_string()),
            provider_type: Some("local".to_string()),
            model_id: Some(self.config.asr_model.clone()),
            base_url: None,
        })
    }
}

impl Embedder for MlxSidecar {
    fn embed_texts(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let value = self.request(
            "embed_texts",
            json!({
                "texts": texts,
            }),
        )?;
        Ok(serde_json::from_value::<EmbeddingResponse>(value)?.vectors)
    }

    fn embed_images(&self, paths: &[PathBuf]) -> anyhow::Result<Vec<Vec<f32>>> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let value = self.request(
            "embed_images",
            json!({
                "paths": paths,
            }),
        )?;
        Ok(serde_json::from_value::<EmbeddingResponse>(value)?.vectors)
    }

    fn inference_provider(&self) -> Option<InferenceProviderInfo> {
        Some(InferenceProviderInfo {
            provider_mode: "local".to_string(),
            provider_id: Some("local".to_string()),
            provider_type: Some("local".to_string()),
            model_id: Some(self.config.embedding_model.clone()),
            base_url: None,
        })
    }
}

impl OcrEngine for MlxSidecar {
    fn ocr_images(&self, paths: &[PathBuf]) -> anyhow::Result<Vec<OcrFrame>> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let value = self.request(
            "ocr_images",
            json!({
                "paths": paths,
            }),
        )?;
        let response: OcrResponse = serde_json::from_value(value)?;
        Ok(response
            .results
            .into_iter()
            .map(|result| OcrFrame {
                path: result.path,
                text: result.text,
            })
            .collect())
    }
}

impl ModelRuntimeControl for MlxSidecar {
    fn release_models(&self, scope: ModelReleaseScope) -> anyhow::Result<()> {
        MlxSidecar::release_models(self, scope)
    }
}

struct SidecarProcess {
    child: Child,
    stdin: ChildStdin,
    responses: Receiver<io::Result<String>>,
}

#[derive(Debug, Deserialize)]
struct SidecarResponse {
    // None when the sidecar could not parse the request and echoes id: null.
    id: Option<u64>,
    ok: bool,
    result: Option<Value>,
    error: Option<SidecarError>,
}

#[derive(Debug, Deserialize)]
struct SidecarError {
    #[serde(rename = "type")]
    error_type: String,
    message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MlxRuntimeStatus {
    pub ok: bool,
    pub platform: MlxRuntimePlatform,
    pub apple_silicon: bool,
    pub packages: Value,
    pub missing: Vec<String>,
    pub models: Value,
    pub cache: Value,
    pub loaded: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MlxRuntimePlatform {
    pub system: String,
    pub machine: String,
    pub python: String,
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    vectors: Vec<Vec<f32>>,
}

#[derive(Debug, Deserialize)]
struct TranscribeResponse {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    segments: Vec<SidecarSegment>,
}

impl TranscribeResponse {
    fn into_segments(self) -> Vec<Segment> {
        let segments = self
            .segments
            .into_iter()
            .map(|segment| Segment {
                start: segment.start,
                end: segment.end,
                text: segment.text,
            })
            .collect::<Vec<_>>();
        if !segments.is_empty() {
            return segments;
        }

        let Some(text) = self.text else {
            return Vec::new();
        };
        let text = text.trim();
        if text.is_empty() {
            return Vec::new();
        }
        vec![Segment {
            start: 0.0,
            end: 1.0,
            text: text.to_string(),
        }]
    }
}

#[derive(Debug, Deserialize)]
struct SidecarSegment {
    start: f64,
    end: f64,
    text: String,
}

#[derive(Debug, Deserialize)]
struct OcrResponse {
    results: Vec<OcrItem>,
}

#[derive(Debug, Deserialize)]
struct OcrItem {
    path: PathBuf,
    text: String,
}

fn ensure_process(
    config: &MlxSidecarConfig,
    guard: &mut Option<SidecarProcess>,
) -> anyhow::Result<()> {
    let needs_spawn = match guard.as_mut() {
        Some(process) => match process.child.try_wait()? {
            Some(status) => {
                tracing::warn!(%status, "MLX sidecar exited; restarting");
                true
            }
            None => false,
        },
        None => true,
    };

    if needs_spawn {
        *guard = Some(spawn_process(config)?);
    }

    Ok(())
}

fn ensure_existing_process(guard: &mut Option<SidecarProcess>) -> anyhow::Result<bool> {
    let Some(process) = guard.as_mut() else {
        return Ok(false);
    };
    if let Some(status) = process.child.try_wait()? {
        tracing::warn!(%status, "MLX sidecar exited before release request");
        *guard = None;
        return Ok(false);
    }
    Ok(true)
}

fn spawn_process(config: &MlxSidecarConfig) -> anyhow::Result<SidecarProcess> {
    std::fs::create_dir_all(&config.models_cache)?;
    let hf_home = config.models_cache.join("huggingface");
    std::fs::create_dir_all(&hf_home)?;

    let mut child = Command::new(&config.python)
        .arg("-u")
        .arg(&config.script)
        .arg("--models-cache")
        .arg(&config.models_cache)
        .arg("--embedding-model")
        .arg(&config.embedding_model)
        .arg("--asr-model")
        .arg(&config.asr_model)
        .arg("--forced-aligner-model")
        .arg(&config.forced_aligner_model)
        .arg("--asr-quantization")
        .arg(&config.asr_quantization)
        .arg("--ocr-det-model")
        .arg(&config.ocr_det_model)
        .arg("--ocr-rec-model")
        .arg(&config.ocr_rec_model)
        .arg("--whisper-model")
        .arg(&config.whisper_model)
        .env("PYTHONUNBUFFERED", "1")
        // Don't write .pyc back into the (signed, bundled) runtime at runtime —
        // it would break the app's code seal and dirty the read-only bundle.
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("HF_HOME", &hf_home)
        .env("HF_HUB_DISABLE_XET", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| {
            format!(
                "failed to start MLX sidecar with python={} script={}",
                config.python.display(),
                config.script.display()
            )
        })?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("failed to open MLX sidecar stdin"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("failed to open MLX sidecar stdout"))?;

    let (sender, responses) = mpsc::channel();
    thread::Builder::new()
        .name("mlx-sidecar-reader".to_string())
        .spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if sender.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        break;
                    }
                }
            }
        })
        .context("failed to spawn MLX sidecar reader thread")?;

    Ok(SidecarProcess {
        child,
        stdin,
        responses,
    })
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn default_sidecar_script(repo_root: &Path) -> PathBuf {
    let relative = Path::new("mlx-sidecar").join("cerul_mlx_sidecar.py");
    let repo_script = repo_root.join(&relative);
    if repo_script.is_file() {
        return repo_script;
    }

    for root in sidecar_candidate_roots(repo_root) {
        for candidate in [
            root.join(&relative),
            root.join("resources").join(&relative),
            root.join("Resources").join(&relative),
            root.join("Contents").join("Resources").join(&relative),
        ] {
            if candidate.is_file() {
                return candidate;
            }
        }
    }

    repo_script
}

fn default_python_path(repo_root: &Path) -> PathBuf {
    for candidate in [
        repo_root
            .join(".tmp")
            .join("runtime-matrix-venv")
            .join("bin")
            .join("python"),
        repo_root
            .join(".tmp")
            .join("mlx-p0-venv")
            .join("bin")
            .join("python"),
    ] {
        if candidate.is_file() {
            return candidate;
        }
    }

    PathBuf::from("python3")
}

fn sidecar_candidate_roots(repo_root: &Path) -> Vec<PathBuf> {
    let mut roots = vec![repo_root.to_path_buf()];
    if let Ok(cwd) = env::current_dir() {
        push_ancestors(&mut roots, &cwd);
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            push_ancestors(&mut roots, parent);
        }
    }
    roots
}

fn push_ancestors(roots: &mut Vec<PathBuf>, start: &Path) {
    for path in start.ancestors().take(8) {
        let path = path.to_path_buf();
        if !roots.contains(&path) {
            roots.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_config_finds_repo_sidecar() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let config = MlxSidecarConfig::for_paths(&paths).unwrap();

        assert!(config.script.ends_with("mlx-sidecar/cerul_mlx_sidecar.py"));
        assert_eq!(config.embedding_model, DEFAULT_EMBEDDING_MODEL);
        assert_eq!(config.models_cache, paths.models.join("mlx"));
    }

    #[test]
    fn transcribe_response_preserves_top_level_text_without_segments() {
        let response: TranscribeResponse =
            serde_json::from_value(json!({ "text": "recognized speech", "segments": [] })).unwrap();
        let segments = response.into_segments();

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].start, 0.0);
        assert_eq!(segments[0].end, 1.0);
        assert_eq!(segments[0].text, "recognized speech");
    }

    #[test]
    fn transcribe_response_accepts_empty_text_without_segments_as_no_speech() {
        let response: TranscribeResponse =
            serde_json::from_value(json!({ "text": "  ", "segments": [] })).unwrap();

        assert!(response.into_segments().is_empty());
    }
}
