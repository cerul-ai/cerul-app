use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};
use cerul_pipeline::{
    run::{Embedder, InferenceProviderInfo, Transcriber},
    whisper::{Segment, TranscriptionProgress},
};
use cerul_storage::AppPaths;
use reqwest::{
    blocking::{multipart, Client, RequestBuilder, Response},
    StatusCode,
};
use serde_json::{json, Value};

const API_TIMEOUT: Duration = Duration::from_secs(120);
const RETRY_SLEEP: Duration = Duration::from_secs(2);
const MAX_RETRIES: usize = 3;
const OPENAI_AUDIO_LIMIT_BYTES: u64 = 25 * 1024 * 1024;
const OPENAI_UPLOAD_AUDIO_BITRATE: &str = "32k";
const GEMINI_INLINE_LIMIT_BYTES: u64 = 20 * 1024 * 1024;
const GEMINI_EMBEDDING_2_MODEL: &str = "gemini-embedding-2";
const MAX_ESTIMATED_SEGMENT_SEC: f64 = 8.0;
const MAX_PASSTHROUGH_SEGMENT_SEC: f64 = 18.0;
const TARGET_ESTIMATED_SEGMENT_CHARS: usize = 140;
static LOCAL_QUERY_SIDECAR: Mutex<Option<CachedLocalSidecar>> = Mutex::new(None);

#[derive(Debug, Clone)]
pub struct ApiTranscriber {
    provider: cerul_storage::providers::Provider,
    api_key: String,
    model: String,
}

#[derive(Debug, Clone)]
pub struct GeminiMultimodalEmbedder {
    provider: cerul_storage::providers::Provider,
    api_key: String,
    model: String,
    output_dimension: i32,
}

#[derive(Debug, Clone)]
pub(crate) struct ProfiledApiEmbedder {
    paths: AppPaths,
    profile: cerul_storage::vectors::EmbeddingProfile,
}

#[derive(Debug, Clone)]
pub struct RoutedApiTranscriber {
    paths: AppPaths,
}

pub(crate) struct QueryEmbedding {
    pub vector: Vec<f32>,
    pub profile: cerul_storage::vectors::EmbeddingProfile,
}

struct CachedLocalSidecar {
    config: cerul_pipeline::mlx_sidecar::MlxSidecarConfig,
    sidecar: Arc<cerul_pipeline::mlx_sidecar::MlxSidecar>,
}

struct OpenAiAudioUpload {
    bytes: Vec<u8>,
    file_name: String,
    mime_type: &'static str,
}

pub(crate) fn routed_transcriber(paths: AppPaths) -> RoutedApiTranscriber {
    RoutedApiTranscriber { paths }
}

pub(crate) fn selected_transcriber(paths: &AppPaths) -> anyhow::Result<ApiTranscriber> {
    let configured_model = crate::models::selected_asr_model_id(paths)
        .unwrap_or_else(|| crate::models::DEFAULT_ASR_MODEL_ID.to_string());
    let model = if crate::models::is_local_asr_model_id(&configured_model) {
        tracing::warn!(
            model = %configured_model,
            "local ASR model is selected while Remote API mode is active; using default remote ASR"
        );
        crate::models::DEFAULT_ASR_MODEL_ID.to_string()
    } else {
        configured_model
    };
    let provider = if let Some(provider_id) =
        crate::setting_string(paths, "asr_provider_id")?.filter(|id| !id.is_empty())
    {
        let provider = provider_by_id_for_type(
            paths,
            &provider_id,
            &["openai", "openai-compatible", "gemini"],
            "ASR",
        )?;
        ensure_asr_model_matches_provider(&provider, &model)?;
        provider
    } else if is_gemini_audio_model(&model) {
        provider_for_type(paths, "asr_provider_id", &["gemini"], "Gemini Audio ASR")?
    } else {
        provider_for_type(
            paths,
            "asr_provider_id",
            &["openai", "openai-compatible"],
            "OpenAI ASR",
        )?
    };
    let api_key = crate::providers::get_provider_key_for_provider(paths, &provider)?
        .ok_or_else(|| missing_key_error(&provider.label, "ASR"))?;

    Ok(ApiTranscriber {
        provider,
        api_key,
        model,
    })
}

pub(crate) fn selected_embedder(paths: &AppPaths) -> anyhow::Result<GeminiMultimodalEmbedder> {
    let profile = cerul_storage::vectors::ensure_active_embedding_profile(paths)?;
    embedder_for_profile(paths, profile)
}

pub(crate) fn embedder_for_profile(
    paths: &AppPaths,
    profile: cerul_storage::vectors::EmbeddingProfile,
) -> anyhow::Result<GeminiMultimodalEmbedder> {
    anyhow::ensure!(
        profile.provider_id == "gemini",
        "embedding profile {} uses provider {}; Cerul v1 expects Gemini Embedding 2",
        profile.id,
        profile.provider_id
    );
    let provider = provider_for_type(
        paths,
        "embedding_provider_id",
        &["gemini"],
        "Gemini Embedding 2",
    )?;
    let api_key = crate::providers::get_provider_key_for_provider(paths, &provider)?
        .ok_or_else(|| missing_key_error(&provider.label, "multimodal embedding"))?;

    Ok(GeminiMultimodalEmbedder {
        provider,
        api_key,
        model: env_setting("CERUL_EMBEDDING_MODEL").unwrap_or(profile.model_id),
        output_dimension: profile.output_dimension,
    })
}

pub(crate) fn profiled_embedder(
    paths: AppPaths,
    profile: cerul_storage::vectors::EmbeddingProfile,
) -> ProfiledApiEmbedder {
    ProfiledApiEmbedder { paths, profile }
}

pub(crate) fn embed_query(paths: &AppPaths, query: &str) -> anyhow::Result<QueryEmbedding> {
    if effective_query_inference_mode(paths)? == "local" {
        let profile =
            cerul_storage::vectors::ensure_embedding_profile_for_inference_mode(paths, "local")?;
        anyhow::ensure!(
            local_embedding_model_cached(paths)?,
            "Local embedding model is not prepared yet; using text search fallback"
        );
        let embedder = local_query_sidecar(paths)?;
        let mut vectors = embedder.embed_texts(&[query.to_string()])?;
        let vector = vectors
            .pop()
            .ok_or_else(|| anyhow::anyhow!("local embedder returned no query vector"))?;
        if let Some(info) = cerul_pipeline::run::Embedder::inference_provider(embedder.as_ref()) {
            record_search_query_usage(paths, info, query);
        }
        return Ok(QueryEmbedding { vector, profile });
    }

    let profile = cerul_storage::vectors::embedding_profile_for_inference_mode(paths, "remote")?;
    let embedder = embedder_for_profile(paths, profile.clone())?;
    let vector = embedder.embed_query(query)?;
    record_search_query_usage(
        paths,
        provider_info(&embedder.provider, &embedder.model),
        query,
    );
    Ok(QueryEmbedding { vector, profile })
}

fn local_embedding_model_cached(paths: &AppPaths) -> anyhow::Result<bool> {
    let config = cerul_pipeline::mlx_sidecar::runtime_config(paths)?;
    let model_path = Path::new(&config.embedding_model);
    if model_path.exists() {
        return Ok(true);
    }
    Ok(crate::local_models::local_model_weights_ready(
        paths,
        &config.embedding_model,
    ))
}

pub(crate) fn effective_query_inference_mode(paths: &AppPaths) -> anyhow::Result<String> {
    let runtime = crate::models::model_runtime_status(paths);
    query_inference_mode(paths, &runtime)
}

pub(crate) fn read_only_effective_query_inference_mode(paths: &AppPaths) -> String {
    let runtime = crate::models::model_runtime_status(paths);
    read_only_query_inference_mode(paths, &runtime)
}

fn read_only_query_inference_mode(
    paths: &AppPaths,
    runtime: &crate::models::ModelRuntimeStatus,
) -> String {
    let selected = selected_inference_mode(paths);
    crate::effective_inference_mode_for_runtime(&selected, runtime)
}

fn query_inference_mode(
    paths: &AppPaths,
    runtime: &crate::models::ModelRuntimeStatus,
) -> anyhow::Result<String> {
    let selected = selected_inference_mode(paths);
    if selected == "remote" {
        return Ok("remote".to_string());
    }

    crate::sync_deferred_embedding_rebuild_if_ready(paths, runtime)?;
    match selected.as_str() {
        "auto" if runtime.local_runtime_ready => Ok("local".to_string()),
        "auto" => Ok("remote".to_string()),
        "local" if runtime.local_runtime_ready => Ok("local".to_string()),
        "local" => anyhow::bail!(
            "Local-only smart processing is selected, but the local runtime is not ready: {}",
            runtime
                .local_runtime_error
                .clone()
                .unwrap_or_else(|| "local runtime unavailable".to_string())
        ),
        _ => Ok("remote".to_string()),
    }
}

fn local_query_sidecar(
    paths: &AppPaths,
) -> anyhow::Result<Arc<cerul_pipeline::mlx_sidecar::MlxSidecar>> {
    let mut config = cerul_pipeline::mlx_sidecar::runtime_config(paths)?;
    crate::local_runtime::ensure_external_mlx_runtime(paths, &mut config)?;
    let mut cached = LOCAL_QUERY_SIDECAR
        .lock()
        .map_err(|_| anyhow::anyhow!("local query sidecar cache lock poisoned"))?;

    if let Some(cached) = cached.as_ref() {
        if cached.config == config {
            return Ok(cached.sidecar.clone());
        }
    }

    let sidecar = Arc::new(cerul_pipeline::mlx_sidecar::MlxSidecar::new(config.clone()));
    *cached = Some(CachedLocalSidecar {
        config,
        sidecar: sidecar.clone(),
    });
    Ok(sidecar)
}

pub(crate) fn shutdown_local_query_sidecar() {
    let cached = LOCAL_QUERY_SIDECAR
        .lock()
        .ok()
        .and_then(|mut guard| guard.take());
    if let Some(cached) = cached {
        if let Err(error) = cached
            .sidecar
            .release_models(cerul_pipeline::run::ModelReleaseScope::All)
        {
            tracing::warn!(%error, "failed to release cached local query sidecar models");
        }
    }
}

impl Transcriber for ApiTranscriber {
    fn transcribe(
        &self,
        audio_path: &Path,
        progress: Option<TranscriptionProgress>,
    ) -> anyhow::Result<Vec<Segment>> {
        if let Some(progress) = progress.as_ref() {
            progress(5);
        }
        let segments = match self.provider.provider_type.as_str() {
            "openai" | "openai-compatible" => self.transcribe_openai(audio_path)?,
            "gemini" => self.transcribe_gemini(audio_path)?,
            other => anyhow::bail!("provider type {other} cannot run ASR"),
        };
        if let Some(progress) = progress.as_ref() {
            progress(100);
        }
        Ok(segments)
    }

    fn inference_provider(&self) -> Option<InferenceProviderInfo> {
        Some(provider_info(&self.provider, &self.model))
    }
}

impl Transcriber for RoutedApiTranscriber {
    fn prepare_transcription(&self) -> anyhow::Result<()> {
        selected_transcriber(&self.paths)?.prepare_transcription()
    }

    fn transcribe(
        &self,
        audio_path: &Path,
        progress: Option<TranscriptionProgress>,
    ) -> anyhow::Result<Vec<Segment>> {
        selected_transcriber(&self.paths)?.transcribe(audio_path, progress)
    }

    fn inference_provider(&self) -> Option<InferenceProviderInfo> {
        selected_transcriber(&self.paths)
            .ok()
            .and_then(|transcriber| transcriber.inference_provider())
    }
}

impl Embedder for GeminiMultimodalEmbedder {
    fn embed_texts(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        // batchEmbedContents: one HTTP round-trip per 100 chunks instead of
        // one per chunk (long videos produce hundreds of serial requests).
        const BATCH_SIZE: usize = 100;
        let mut vectors = Vec::with_capacity(texts.len());
        for batch in texts.chunks(BATCH_SIZE) {
            vectors.extend(self.embed_text_batch(batch, Some("RETRIEVAL_DOCUMENT"))?);
        }
        Ok(vectors)
    }

    fn embed_images(&self, paths: &[PathBuf]) -> anyhow::Result<Vec<Vec<f32>>> {
        paths
            .iter()
            .map(|path| {
                let bytes = std::fs::read(path)?;
                anyhow::ensure!(
                    bytes.len() as u64 <= GEMINI_INLINE_LIMIT_BYTES,
                    "image {} is larger than Gemini inline request limit of 20 MB",
                    path.display()
                );
                let part = json!({
                    "inlineData": {
                        "mimeType": image_mime_type(path),
                        "data": BASE64_STANDARD.encode(bytes),
                    }
                });
                self.embed_parts(vec![part], Some("RETRIEVAL_DOCUMENT"))
            })
            .collect()
    }

    fn inference_provider(&self) -> Option<InferenceProviderInfo> {
        Some(provider_info(&self.provider, &self.model))
    }
}

impl Embedder for ProfiledApiEmbedder {
    fn embed_texts(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        embedder_for_profile(&self.paths, self.profile.clone())?.embed_texts(texts)
    }

    fn embed_images(&self, paths: &[PathBuf]) -> anyhow::Result<Vec<Vec<f32>>> {
        embedder_for_profile(&self.paths, self.profile.clone())?.embed_images(paths)
    }

    fn inference_provider(&self) -> Option<InferenceProviderInfo> {
        embedder_for_profile(&self.paths, self.profile.clone())
            .ok()
            .and_then(|embedder| embedder.inference_provider())
    }
}

impl ApiTranscriber {
    fn transcribe_openai(&self, audio_path: &Path) -> anyhow::Result<Vec<Segment>> {
        let upload = prepare_openai_audio_upload(audio_path)?;
        anyhow::ensure!(
            upload.bytes.len() as u64 <= OPENAI_AUDIO_LIMIT_BYTES,
            "audio file is {:.1} MB after compression; OpenAI transcription uploads are limited to 25 MB. Split the source or choose Gemini Audio for larger files.",
            upload.bytes.len() as f64 / 1_000_000.0
        );

        let client = http_client()?;
        let url = format!(
            "{}/audio/transcriptions",
            provider_base_url(&self.provider)?
        );
        let response_format = if supports_openai_segment_timestamps(&self.model) {
            "verbose_json"
        } else {
            "json"
        };
        let request = || {
            let file_part = multipart::Part::bytes(upload.bytes.clone())
                .file_name(upload.file_name.clone())
                .mime_str(upload.mime_type)?;
            let mut form = multipart::Form::new()
                .part("file", file_part)
                .text("model", self.model.clone())
                .text("response_format", response_format.to_string());
            if supports_openai_segment_timestamps(&self.model) {
                form = form.text("timestamp_granularities[]", "segment");
            }
            Ok(client
                .post(&url)
                .bearer_auth(self.api_key.trim())
                .multipart(form))
        };
        let json = send_json_with_retry(request)?;
        openai_segments(json, audio_duration_sec(audio_path)?)
    }

    fn transcribe_gemini(&self, audio_path: &Path) -> anyhow::Result<Vec<Segment>> {
        let bytes = std::fs::read(audio_path)?;
        anyhow::ensure!(
            bytes.len() as u64 <= GEMINI_INLINE_LIMIT_BYTES,
            "audio file is {:.1} MB after extraction; Gemini inline audio requests are limited to 20 MB in v1.",
            bytes.len() as f64 / 1_000_000.0
        );

        let url = format!(
            "{}/models/{}:generateContent",
            provider_base_url(&self.provider)?,
            self.model.trim_start_matches("models/")
        );
        let body = json!({
            "contents": [{
                "role": "user",
                "parts": [
                    {
                        "text": "Transcribe this audio. Return strict JSON only: {\"segments\":[{\"start\":0.0,\"end\":1.0,\"text\":\"...\"}],\"text\":\"full transcript\"}. Use seconds for start/end. If exact timestamps are not available, create one segment covering the full audio."
                    },
                    {
                        "inlineData": {
                            "mimeType": audio_mime_type(audio_path),
                            "data": BASE64_STANDARD.encode(bytes),
                        }
                    }
                ]
            }],
            "generationConfig": {
                "responseMimeType": "application/json"
            }
        });
        let client = http_client()?;
        let response = send_json_with_retry(|| {
            Ok(client
                .post(&url)
                .header("x-goog-api-key", self.api_key.trim())
                .json(&body))
        })?;
        let text = gemini_candidate_text(&response)?;
        let duration = audio_duration_sec(audio_path)?;
        Ok(gemini_transcript_segments(&text, duration))
    }
}

fn prepare_openai_audio_upload(audio_path: &Path) -> anyhow::Result<OpenAiAudioUpload> {
    let upload_path = temp_openai_audio_path(audio_path);
    transcode_openai_audio(audio_path, &upload_path)?;
    let bytes = std::fs::read(&upload_path)
        .with_context(|| format!("failed to read compressed audio {}", upload_path.display()))?;
    let file_name = upload_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("cerul-openai-audio.m4a")
        .to_string();
    let mime_type = audio_mime_type(&upload_path);
    let _ = std::fs::remove_file(&upload_path);

    Ok(OpenAiAudioUpload {
        bytes,
        file_name,
        mime_type,
    })
}

fn transcode_openai_audio(input: &Path, output: &Path) -> anyhow::Result<()> {
    let result = Command::new(cerul_pipeline::ffmpeg::bundled_ffmpeg_path())
        .args(["-y", "-i"])
        .arg(input)
        .args(["-vn", "-ar", "16000", "-ac", "1"])
        .args(["-c:a", "aac", "-b:a", OPENAI_UPLOAD_AUDIO_BITRATE])
        .arg(output)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()?;

    if !result.status.success() {
        let _ = std::fs::remove_file(output);
        anyhow::bail!(
            "ffmpeg OpenAI upload transcode failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    Ok(())
}

fn temp_openai_audio_path(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("audio");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "cerul-openai-asr-{}-{nonce}-{stem}.m4a",
        std::process::id()
    ))
}

impl GeminiMultimodalEmbedder {
    fn embed_query(&self, query: &str) -> anyhow::Result<Vec<f32>> {
        let text = if is_gemini_embedding_2_model(&self.model) {
            gemini_embedding_2_query_text(query)
        } else {
            query.trim().to_string()
        };
        self.embed_parts(vec![json!({ "text": text })], Some("RETRIEVAL_QUERY"))
    }

    fn document_text_parts(&self, text: &str) -> Vec<Value> {
        if is_gemini_embedding_2_model(&self.model) {
            vec![json!({ "text": gemini_embedding_2_document_text(text) })]
        } else {
            vec![json!({ "text": text })]
        }
    }

    fn embed_text_batch(
        &self,
        texts: &[String],
        task_type: Option<&str>,
    ) -> anyhow::Result<Vec<Vec<f32>>> {
        let url = format!(
            "{}/models/{}:batchEmbedContents",
            provider_base_url(&self.provider)?,
            self.model.trim_start_matches("models/")
        );
        let requests: Vec<Value> = texts
            .iter()
            .map(|text| {
                gemini_embedding_request_body(
                    &self.model,
                    self.document_text_parts(text),
                    self.output_dimension,
                    task_type,
                )
            })
            .collect();
        let body = json!({ "requests": requests });
        let client = http_client()?;
        let response = send_json_with_retry(|| {
            Ok(client
                .post(&url)
                .header("x-goog-api-key", self.api_key.trim())
                .json(&body))
        })?;
        let embeddings = response
            .get("embeddings")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("Gemini batch embedding response missing embeddings"))?;
        anyhow::ensure!(
            embeddings.len() == texts.len(),
            "Gemini batch embedding returned {} vectors for {} inputs",
            embeddings.len(),
            texts.len()
        );
        embeddings
            .iter()
            .map(|entry| {
                let values = entry
                    .get("values")
                    .and_then(Value::as_array)
                    .ok_or_else(|| anyhow::anyhow!("Gemini embedding entry missing values"))?;
                let vector = values
                    .iter()
                    .map(|value| value.as_f64().unwrap_or_default() as f32)
                    .collect::<Vec<f32>>();
                anyhow::ensure!(
                    vector.len() == self.output_dimension as usize,
                    "Gemini Embedding 2 returned {} dimensions, expected {}",
                    vector.len(),
                    self.output_dimension
                );
                Ok(vector)
            })
            .collect()
    }

    fn embed_parts(&self, parts: Vec<Value>, task_type: Option<&str>) -> anyhow::Result<Vec<f32>> {
        let url = format!(
            "{}/models/{}:embedContent",
            provider_base_url(&self.provider)?,
            self.model.trim_start_matches("models/")
        );
        let body =
            gemini_embedding_request_body(&self.model, parts, self.output_dimension, task_type);
        let client = http_client()?;
        let response = send_json_with_retry(|| {
            Ok(client
                .post(&url)
                .header("x-goog-api-key", self.api_key.trim())
                .json(&body))
        })?;
        let vector = embedding_values(&response)?;
        anyhow::ensure!(
            vector.len() == self.output_dimension as usize,
            "Gemini Embedding 2 returned {} dimensions, expected {}",
            vector.len(),
            self.output_dimension
        );
        Ok(vector)
    }
}

fn is_gemini_embedding_2_model(model: &str) -> bool {
    model.trim_start_matches("models/") == GEMINI_EMBEDDING_2_MODEL
}

fn gemini_embedding_2_query_text(query: &str) -> String {
    format!("task: search result | query: {}", query.trim())
}

fn gemini_embedding_2_document_text(text: &str) -> String {
    format!("title: none | text: {}", text.trim())
}

fn gemini_embedding_request_body(
    model: &str,
    parts: Vec<Value>,
    output_dimension: i32,
    task_type: Option<&str>,
) -> Value {
    let mut body = json!({
        "model": format!("models/{}", model.trim_start_matches("models/")),
        "content": {
            "role": "user",
            "parts": parts,
        },
        "outputDimensionality": output_dimension,
    });
    if !is_gemini_embedding_2_model(model) {
        if let Some(task_type) = task_type {
            body["taskType"] = Value::String(task_type.to_string());
        }
    }
    body
}

fn provider_for_type(
    paths: &AppPaths,
    setting_key: &str,
    allowed_types: &[&str],
    capability: &str,
) -> anyhow::Result<cerul_storage::providers::Provider> {
    let providers = cerul_storage::providers::list_providers(paths)?;
    if let Some(provider_id) =
        crate::setting_string(paths, setting_key)?.filter(|id| !id.is_empty())
    {
        let provider = providers
            .iter()
            .find(|provider| provider.id == provider_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("{capability} provider {provider_id} was not found"))?;
        anyhow::ensure!(
            allowed_types.contains(&provider.provider_type.as_str()),
            "{capability} provider {} has unsupported type {}; expected one of {}",
            provider.label,
            provider.provider_type,
            allowed_types.join(", ")
        );
        return Ok(provider);
    }

    providers
        .into_iter()
        .find(|provider| {
            provider.id != cerul_storage::providers::LOCAL_PROVIDER_ID
                && allowed_types.contains(&provider.provider_type.as_str())
                && crate::providers::has_provider_key_for_provider(paths, provider)
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Connect a {} provider before indexing. Expected provider type: {}.",
                capability,
                allowed_types.join(" or ")
            )
        })
}

fn provider_by_id_for_type(
    paths: &AppPaths,
    provider_id: &str,
    allowed_types: &[&str],
    capability: &str,
) -> anyhow::Result<cerul_storage::providers::Provider> {
    let provider = cerul_storage::providers::get_provider(paths, provider_id)?
        .ok_or_else(|| anyhow::anyhow!("{capability} provider {provider_id} was not found"))?;
    anyhow::ensure!(
        allowed_types.contains(&provider.provider_type.as_str()),
        "{capability} provider {} has unsupported type {}; expected one of {}",
        provider.label,
        provider.provider_type,
        allowed_types.join(", ")
    );
    Ok(provider)
}

fn ensure_asr_model_matches_provider(
    provider: &cerul_storage::providers::Provider,
    model: &str,
) -> anyhow::Result<()> {
    let gemini_model = is_gemini_audio_model(model);
    match provider.provider_type.as_str() {
        "gemini" => anyhow::ensure!(
            gemini_model,
            "ASR provider {} is Gemini but selected model {} is not a Gemini audio model",
            provider.label,
            model
        ),
        "openai" => anyhow::ensure!(
            !gemini_model,
            "ASR provider {} is OpenAI but selected model {} requires a Gemini provider",
            provider.label,
            model
        ),
        // OpenAI-compatible gateways may intentionally expose provider-prefixed
        // model ids behind the OpenAI transcription protocol.
        "openai-compatible" => {}
        _ => {}
    }
    Ok(())
}

fn missing_key_error(label: &str, capability: &str) -> anyhow::Error {
    anyhow::anyhow!("{capability} provider {label} has no API key configured")
}

fn provider_base_url(provider: &cerul_storage::providers::Provider) -> anyhow::Result<String> {
    provider
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(|url| url.trim_end_matches('/').to_string())
        .ok_or_else(|| anyhow::anyhow!("provider {} has no base_url configured", provider.label))
}

fn provider_info(
    provider: &cerul_storage::providers::Provider,
    model: &str,
) -> InferenceProviderInfo {
    InferenceProviderInfo {
        provider_mode: if provider.provider_type == "local" {
            "local".to_string()
        } else {
            "remote".to_string()
        },
        provider_id: Some(provider.id.clone()),
        provider_type: Some(provider.provider_type.clone()),
        model_id: Some(model.to_string()),
        base_url: provider.base_url.clone(),
    }
}

fn record_search_query_usage(paths: &AppPaths, info: InferenceProviderInfo, query: &str) {
    let input_tokens = estimate_text_tokens(query);
    let estimated_usd = if info.model_id.as_deref() == Some("gemini-embedding-2") {
        Some(input_tokens as f64 / 1_000_000.0 * 0.20)
    } else {
        None
    };
    let mut event = cerul_storage::NewUsageEvent::new(info.provider_mode, "search_query");
    event.provider_id = info.provider_id;
    event.provider_type = info.provider_type;
    event.model_id = info.model_id;
    event.input_tokens = Some(input_tokens);
    event.estimated_usd = estimated_usd;
    event.price_snapshot_id = if estimated_usd.is_some() {
        Some("gemini-embedding-2-text-standard-2026-05".to_string())
    } else {
        None
    };
    event.metadata = json!({ "query_chars": query.chars().count() });
    if let Err(error) = cerul_storage::record_usage_event(paths, event) {
        tracing::warn!(%error, "failed to record query embedding usage");
    }
}

fn selected_inference_mode(paths: &AppPaths) -> String {
    crate::setting_string(paths, "inference_mode")
        .ok()
        .flatten()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| value == "remote" || value == "local" || value == "auto")
        .unwrap_or_else(|| "auto".to_string())
}

fn estimate_text_tokens(text: &str) -> u64 {
    ((text.chars().count() as f64 / 4.0).ceil() as u64).max(1)
}

fn http_client() -> anyhow::Result<Client> {
    Ok(Client::builder().timeout(API_TIMEOUT).build()?)
}

fn send_json_with_retry<F>(mut build: F) -> anyhow::Result<Value>
where
    F: FnMut() -> anyhow::Result<RequestBuilder>,
{
    let mut last_error = None;
    for attempt in 1..=MAX_RETRIES {
        match build()?.send() {
            Ok(response) if response.status().is_success() => return response_json(response),
            Ok(response) => {
                let status = response.status();
                let body = response.text().unwrap_or_default();
                let message = format!("provider returned HTTP {status}: {body}");
                if !retryable_status(status) || attempt == MAX_RETRIES {
                    anyhow::bail!(message);
                }
                last_error = Some(message);
            }
            Err(error) => {
                let message = error.to_string();
                if attempt == MAX_RETRIES {
                    anyhow::bail!(
                        "provider request failed after {MAX_RETRIES} attempts: {message}"
                    );
                }
                last_error = Some(message);
            }
        }
        thread::sleep(RETRY_SLEEP);
    }

    anyhow::bail!(
        "provider request failed: {}",
        last_error.unwrap_or_else(|| "unknown error".to_string())
    )
}

fn retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn response_json(response: Response) -> anyhow::Result<Value> {
    let status = response.status();
    let value = response.json::<Value>()?;
    anyhow::ensure!(
        status.is_success(),
        "provider returned HTTP {status}: {value}"
    );
    Ok(value)
}

fn openai_segments(response: Value, duration: f64) -> anyhow::Result<Vec<Segment>> {
    if let Some(segments) = response.get("segments").and_then(Value::as_array) {
        let parsed = segments
            .iter()
            .filter_map(|segment| {
                let text = segment.get("text")?.as_str()?.trim().to_string();
                if text.is_empty() {
                    return None;
                }
                Some(Segment {
                    start: segment.get("start").and_then(Value::as_f64).unwrap_or(0.0),
                    end: segment
                        .get("end")
                        .and_then(Value::as_f64)
                        .unwrap_or(duration),
                    text,
                })
            })
            .collect::<Vec<_>>();
        if !parsed.is_empty() {
            return Ok(normalize_transcript_segments(parsed, duration));
        }
    }

    let text = response
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| anyhow::anyhow!("transcription response did not include text"))?;
    Ok(estimated_segments_from_text(text, duration))
}

fn gemini_candidate_text(response: &Value) -> anyhow::Result<String> {
    response
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|candidates| candidates.first())
        .and_then(|candidate| candidate.get("content"))
        .and_then(|content| content.get("parts"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
        .pipe(|text| {
            if text.is_empty() {
                anyhow::bail!("Gemini Audio response did not include transcript text")
            } else {
                Ok(text)
            }
        })
}

fn gemini_transcript_segments(text: &str, duration: f64) -> Vec<Segment> {
    let parsed = serde_json::from_str::<Value>(strip_json_fence(text)).ok();
    if let Some(segments) = parsed
        .as_ref()
        .and_then(|value| value.get("segments"))
        .and_then(Value::as_array)
    {
        let segments = segments
            .iter()
            .filter_map(|segment| {
                let text = segment.get("text")?.as_str()?.trim().to_string();
                if text.is_empty() {
                    return None;
                }
                Some(Segment {
                    start: segment.get("start").and_then(Value::as_f64).unwrap_or(0.0),
                    end: segment
                        .get("end")
                        .and_then(Value::as_f64)
                        .unwrap_or(duration),
                    text,
                })
            })
            .collect::<Vec<_>>();
        if !segments.is_empty() {
            return normalize_transcript_segments(segments, duration);
        }
    }

    let transcript = parsed
        .as_ref()
        .and_then(|value| value.get("text"))
        .and_then(Value::as_str)
        .unwrap_or(text)
        .trim()
        .to_string();
    if transcript.is_empty() {
        Vec::new()
    } else {
        estimated_segments_from_text(&transcript, duration)
    }
}

fn normalize_transcript_segments(segments: Vec<Segment>, duration: f64) -> Vec<Segment> {
    let duration = duration.max(0.0);
    segments
        .into_iter()
        .flat_map(|segment| {
            let text = segment.text.trim().to_string();
            if text.is_empty() {
                return Vec::new();
            }
            let start = segment.start.max(0.0).min(duration);
            let mut end = segment.end.max(start).min(duration);
            if end <= start {
                if duration <= start {
                    return Vec::new();
                }
                end = (start + 1.0).min(duration);
            }
            let span = end - start;
            if (span > MAX_PASSTHROUGH_SEGMENT_SEC
                || text.chars().count() > TARGET_ESTIMATED_SEGMENT_CHARS * 2)
                && text.chars().count() > TARGET_ESTIMATED_SEGMENT_CHARS
            {
                estimated_segments_for_span(&text, start, end)
            } else {
                vec![Segment { start, end, text }]
            }
        })
        .collect()
}

fn estimated_segments_from_text(text: &str, duration: f64) -> Vec<Segment> {
    estimated_segments_for_span(text, 0.0, duration.max(1.0))
}

fn estimated_segments_for_span(text: &str, start: f64, end: f64) -> Vec<Segment> {
    let (units, separator) = estimated_text_units(text);
    if units.is_empty() {
        return Vec::new();
    }

    let total_chars = units
        .iter()
        .map(|unit| unit.chars().count())
        .sum::<usize>()
        .max(1);
    let span = (end - start).max(1.0);
    let by_time = (span / MAX_ESTIMATED_SEGMENT_SEC).ceil() as usize;
    let by_chars = total_chars.div_ceil(TARGET_ESTIMATED_SEGMENT_CHARS);
    let target_count = by_time.max(by_chars).max(1).min(units.len());
    let target_chars = (total_chars as f64 / target_count as f64).ceil() as usize;

    let mut groups = Vec::with_capacity(target_count);
    let mut current = Vec::new();
    let mut current_chars = 0usize;
    let units_len = units.len();
    for (unit_index, unit) in units.into_iter().enumerate() {
        current_chars += unit.chars().count();
        current.push(unit);
        let remaining_units = units_len.saturating_sub(unit_index + 1);
        let remaining_groups = target_count.saturating_sub(groups.len() + 1);
        let must_flush_to_hit_target = remaining_units <= remaining_groups;
        if groups.len() + 1 < target_count
            && (current_chars >= target_chars || must_flush_to_hit_target)
        {
            groups.push(current.join(separator));
            current = Vec::new();
            current_chars = 0;
        }
    }
    if !current.is_empty() {
        groups.push(current.join(separator));
    }

    let group_count = groups.len().max(1);
    let step = span / group_count as f64;
    groups
        .into_iter()
        .enumerate()
        .map(|(index, text)| {
            let segment_start = start + index as f64 * step;
            let segment_end = if index + 1 == group_count {
                end.max(segment_start)
            } else {
                (segment_start + step).min(end)
            };
            Segment {
                start: segment_start,
                end: segment_end,
                text,
            }
        })
        .collect()
}

fn estimated_text_units(text: &str) -> (Vec<String>, &'static str) {
    let words = text
        .split_whitespace()
        .map(str::trim)
        .filter(|word| !word.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if words.len() > 1 {
        return (words, " ");
    }

    let mut units = Vec::new();
    let mut current = String::new();
    for ch in text.trim().chars() {
        current.push(ch);
        let current_chars = current.chars().count();
        if is_sentence_boundary(ch) || current_chars >= 24 {
            let unit = current.trim().to_string();
            if !unit.is_empty() {
                units.push(unit);
            }
            current.clear();
        }
    }
    let tail = current.trim();
    if !tail.is_empty() {
        units.push(tail.to_string());
    }
    (units, "")
}

fn is_sentence_boundary(ch: char) -> bool {
    matches!(
        ch,
        '.' | '!' | '?' | ';' | ':' | ',' | '。' | '！' | '？' | '；' | '：' | '，' | '、'
    )
}

fn supports_openai_segment_timestamps(model: &str) -> bool {
    let model = model.trim_start_matches("models/").to_ascii_lowercase();
    model == "whisper-1" || model.contains("whisper")
}

fn embedding_values(response: &Value) -> anyhow::Result<Vec<f32>> {
    let values = response
        .get("embedding")
        .and_then(|embedding| embedding.get("values"))
        .or_else(|| {
            response
                .get("embeddings")
                .and_then(Value::as_array)
                .and_then(|embeddings| embeddings.first())
                .and_then(|embedding| embedding.get("values"))
        })
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("embedding response did not include values"))?;

    values
        .iter()
        .map(|value| {
            value
                .as_f64()
                .map(|value| value as f32)
                .ok_or_else(|| anyhow::anyhow!("embedding value was not numeric"))
        })
        .collect()
}

fn audio_duration_sec(path: &Path) -> anyhow::Result<f64> {
    let reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let sample_rate = spec.sample_rate.max(1) as f64;
    Ok(reader.duration() as f64 / sample_rate)
}

fn audio_mime_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("mp3") => "audio/mp3",
        Some("mp4") => "audio/mp4",
        Some("mpeg") | Some("mpga") => "audio/mpeg",
        Some("m4a") => "audio/mp4",
        Some("webm") => "audio/webm",
        Some("wav") => "audio/wav",
        _ => "application/octet-stream",
    }
}

fn image_mime_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    }
}

fn is_gemini_audio_model(model: &str) -> bool {
    model.trim().to_ascii_lowercase().starts_with("gemini-")
}

fn env_setting(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn strip_json_fence(text: &str) -> &str {
    let trimmed = text.trim();
    let Some(without_prefix) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    let without_lang = without_prefix
        .strip_prefix("json")
        .unwrap_or(without_prefix)
        .trim_start();
    without_lang
        .strip_suffix("```")
        .unwrap_or(without_lang)
        .trim()
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}

impl<T> Pipe for T {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_verbose_json_segments_keep_timestamps() {
        let value = json!({
            "segments": [
                { "start": 1.5, "end": 3.0, "text": " hello " }
            ]
        });

        let segments = openai_segments(value, 10.0).unwrap();

        assert_eq!(
            segments,
            vec![Segment {
                start: 1.5,
                end: 3.0,
                text: "hello".to_string()
            }]
        );
    }

    #[test]
    fn whisper_compatible_models_request_segment_timestamps() {
        assert!(supports_openai_segment_timestamps("whisper-1"));
        assert!(supports_openai_segment_timestamps("whisper-large-v3-turbo"));
        assert!(!supports_openai_segment_timestamps("gpt-4o-transcribe"));
    }

    #[test]
    fn asr_provider_model_validation_matches_provider_protocol() {
        let openai = provider("openai");
        let gemini = provider("gemini");
        let gateway = provider("openai-compatible");

        assert!(ensure_asr_model_matches_provider(&openai, "whisper-1").is_ok());
        assert!(ensure_asr_model_matches_provider(&openai, "gemini-2.5-flash").is_err());
        assert!(ensure_asr_model_matches_provider(&gemini, "gemini-2.5-flash").is_ok());
        assert!(ensure_asr_model_matches_provider(&gemini, "whisper-1").is_err());
        assert!(ensure_asr_model_matches_provider(&gateway, "gemini-2.5-flash").is_ok());
    }

    #[test]
    fn gemini_embedding_2_uses_prefixes_without_task_type() {
        let query = gemini_embedding_2_query_text(" harness engineering ");
        let document = gemini_embedding_2_document_text(" context management ");
        assert_eq!(query, "task: search result | query: harness engineering");
        assert_eq!(document, "title: none | text: context management");

        let body = gemini_embedding_request_body(
            "gemini-embedding-2",
            vec![json!({ "text": query })],
            3072,
            Some("RETRIEVAL_QUERY"),
        );
        assert!(body.get("taskType").is_none());
        assert_eq!(
            body["content"]["parts"][0]["text"],
            "task: search result | query: harness engineering"
        );
    }

    #[test]
    fn legacy_gemini_embedding_keeps_task_type() {
        let body = gemini_embedding_request_body(
            "gemini-embedding-001",
            vec![json!({ "text": "hello" })],
            768,
            Some("RETRIEVAL_DOCUMENT"),
        );

        assert_eq!(body["taskType"], "RETRIEVAL_DOCUMENT");
    }

    #[test]
    fn gemini_json_transcript_segments_parse() {
        let segments = gemini_transcript_segments(
            r#"```json
            {"segments":[{"start":0,"end":2.5,"text":"hi"}],"text":"hi"}
            ```"#,
            4.0,
        );

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].end, 2.5);
        assert_eq!(segments[0].text, "hi");
    }

    #[test]
    fn transcript_text_without_timestamps_is_estimated_into_short_segments() {
        let text = (0..80)
            .map(|index| format!("word{index}"))
            .collect::<Vec<_>>()
            .join(" ");
        let segments = openai_segments(json!({ "text": text }), 64.0).unwrap();

        assert!(segments.len() > 1);
        assert!(segments
            .iter()
            .all(|segment| segment.end - segment.start <= 8.1));
        assert_ne!(segments[0].text, segments[1].text);
    }

    #[test]
    fn cjk_transcript_text_without_spaces_is_estimated_into_short_segments() {
        let text = "今天我们来聊一个最近在AI圈特别火但很多人还没真正弄懂的词。这个问题会影响视频搜索跳转的准确性，所以需要更细的时间戳。".repeat(8);
        let segments = openai_segments(json!({ "text": text }), 96.0).unwrap();

        assert!(segments.len() > 1);
        assert!(segments
            .iter()
            .all(|segment| segment.end - segment.start <= 8.1));
        assert_ne!(segments[0].text, segments[1].text);
    }

    #[test]
    fn oversized_provider_segments_are_split() {
        let text = (0..70)
            .map(|index| format!("token{index}"))
            .collect::<Vec<_>>()
            .join(" ");
        let segments = gemini_transcript_segments(
            &json!({
                "segments": [{ "start": 0, "end": 40, "text": text }],
                "text": text,
            })
            .to_string(),
            40.0,
        );

        assert!(segments.len() > 1);
        assert!(segments
            .iter()
            .all(|segment| segment.end - segment.start <= 8.1));
    }

    #[test]
    fn zero_length_segments_at_duration_are_dropped() {
        let segments = normalize_transcript_segments(
            vec![Segment {
                start: 10.0,
                end: 10.0,
                text: "trailing".to_string(),
            }],
            10.0,
        );

        assert!(segments.is_empty());
    }

    #[test]
    fn adjusted_segment_ends_are_clamped_to_duration() {
        let segments = normalize_transcript_segments(
            vec![Segment {
                start: 9.7,
                end: 9.7,
                text: "tail".to_string(),
            }],
            10.0,
        );

        assert_eq!(segments[0].start, 9.7);
        assert_eq!(segments[0].end, 10.0);
    }

    #[test]
    fn query_embedding_auto_uses_remote_until_local_runtime_ready() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        set_setting(&paths, "inference_mode", serde_json::json!("auto"));
        set_setting(
            &paths,
            "embedding_profile_rebuild_deferred_mode",
            serde_json::json!("auto"),
        );

        let mode = query_inference_mode(&paths, &local_runtime_status(false)).unwrap();

        assert_eq!(mode, "remote");
        assert_eq!(
            crate::setting_string(&paths, "embedding_profile_rebuild_deferred_mode").unwrap(),
            Some("auto".to_string())
        );
    }

    #[test]
    fn read_only_query_mode_does_not_consume_deferred_rebuild() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        set_setting(&paths, "inference_mode", serde_json::json!("local"));
        set_setting(
            &paths,
            "embedding_profile_rebuild_deferred_mode",
            serde_json::json!("local"),
        );

        let mode = read_only_query_inference_mode(&paths, &local_runtime_status(true));

        assert_eq!(mode, "local");
        assert_eq!(
            crate::setting_string(&paths, "embedding_profile_rebuild_deferred_mode").unwrap(),
            Some("local".to_string())
        );
    }

    fn provider(provider_type: &str) -> cerul_storage::providers::Provider {
        cerul_storage::providers::Provider {
            id: format!("provider-{provider_type}"),
            provider_type: provider_type.to_string(),
            label: provider_type.to_string(),
            base_url: None,
            status: cerul_storage::providers::PROVIDER_STATUS_READY.to_string(),
            last_error: None,
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn query_embedding_local_only_errors_until_local_runtime_ready() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        set_setting(&paths, "inference_mode", serde_json::json!("local"));

        let error = query_inference_mode(&paths, &local_runtime_status(false)).unwrap_err();

        assert!(error
            .to_string()
            .contains("Local-only smart processing is selected"));
    }

    #[test]
    fn query_embedding_uses_local_after_runtime_ready() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        set_setting(&paths, "inference_mode", serde_json::json!("local"));

        let mode = query_inference_mode(&paths, &local_runtime_status(true)).unwrap();

        assert_eq!(mode, "local");
    }

    #[test]
    fn local_embedding_model_cache_uses_shared_model_weight_readiness() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();

        assert!(!local_embedding_model_cached(&paths).unwrap());

        let repo_cache = paths
            .models
            .join("mlx")
            .join("modelscope")
            .join("models--mlx-community--Qwen3-VL-Embedding-2B-6bit");
        let snapshot = repo_cache
            .join("snapshots")
            .join("27b74bcc0d0019a4d270abc5936c93f3f58c34fa");
        std::fs::create_dir_all(&snapshot).unwrap();
        std::fs::write(snapshot.join("config.json"), "{}").unwrap();
        std::fs::write(snapshot.join("model.safetensors"), vec![0u8; 1024]).unwrap();

        assert!(!local_embedding_model_cached(&paths).unwrap());

        std::fs::File::create(snapshot.join("model.safetensors"))
            .unwrap()
            .set_len(2_200_000_000)
            .unwrap();

        assert!(local_embedding_model_cached(&paths).unwrap());

        std::fs::remove_file(snapshot.join("model.safetensors")).unwrap();
        std::fs::write(snapshot.join("model.safetensors.incomplete"), "").unwrap();

        assert!(!local_embedding_model_cached(&paths).unwrap());
    }

    #[test]
    fn embedding_response_values_parse() {
        let vector = embedding_values(&json!({
            "embedding": { "values": [0.1, 0.2] }
        }))
        .unwrap();

        assert_eq!(vector, vec![0.1, 0.2]);
    }

    fn set_setting(paths: &AppPaths, key: &str, value: serde_json::Value) {
        let conn = cerul_storage::sqlite::open(paths).unwrap();
        conn.execute(
            r#"
            INSERT INTO settings (key, value, updated_at)
            VALUES (?1, ?2, strftime('%s','now'))
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = excluded.updated_at
            "#,
            (key, value.to_string()),
        )
        .unwrap();
    }

    fn local_runtime_status(local_runtime_ready: bool) -> crate::models::ModelRuntimeStatus {
        crate::models::ModelRuntimeStatus {
            platform: "test".to_string(),
            api_runtime_ready: false,
            local_runtime_ready,
            openai_ready: false,
            gemini_ready: false,
            last_error: Some(
                "Connect OpenAI ASR provider and Gemini Embedding 2 provider before indexing."
                    .to_string(),
            ),
            local_runtime_error: if local_runtime_ready {
                None
            } else {
                Some("missing mlx".to_string())
            },
        }
    }
}
