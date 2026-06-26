use std::{
    fs::{self, File},
    io::BufReader,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use cerul_storage::{
    AppPaths, StorageImageChunk, StorageOcrChunk, StorageTranscriptChunk, StorageTranscriptLine,
    StorageWriteSummary,
};
use serde_json::{json, Map, Value};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::{
    chunking, ffmpeg,
    whisper::{Segment, TranscriptionProgress},
};

const DEFAULT_PIPELINE_TEMP_CACHE_BUDGET_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const PIPELINE_TEMP_CACHE_BUDGET_MB_ENV: &str = "CERUL_PIPELINE_TEMP_CACHE_BUDGET_MB";
const WEB_VIDEO_COOKIE_MODE_SETTING: &str = "web_video_cookie_mode";
const WEB_VIDEO_COOKIE_BROWSER_SETTING: &str = "web_video_cookie_browser";
const WEB_VIDEO_COOKIES_PATH_SETTING: &str = "web_video_cookies_path";
const PIPELINE_JOB_LOG_FILE: &str = "pipeline-jobs.jsonl";

pub trait Transcriber: Send + Sync {
    fn prepare_transcription(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn transcribe(
        &self,
        audio_path: &Path,
        progress: Option<TranscriptionProgress>,
    ) -> anyhow::Result<Vec<Segment>>;

    fn inference_provider(&self) -> Option<InferenceProviderInfo> {
        None
    }
}

pub trait Embedder: Send + Sync {
    fn embed_texts(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>>;
    fn embed_images(&self, paths: &[PathBuf]) -> anyhow::Result<Vec<Vec<f32>>>;

    fn inference_provider(&self) -> Option<InferenceProviderInfo> {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceProviderInfo {
    pub provider_mode: String,
    pub provider_id: Option<String>,
    pub provider_type: Option<String>,
    pub model_id: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcrFrame {
    pub path: PathBuf,
    pub text: String,
}

pub trait OcrEngine: Send + Sync {
    fn ocr_images(&self, paths: &[PathBuf]) -> anyhow::Result<Vec<OcrFrame>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelReleaseScope {
    Transcription,
    Embedding,
    Ocr,
    All,
}

impl ModelReleaseScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Transcription => "transcription",
            Self::Embedding => "embedding",
            Self::Ocr => "ocr",
            Self::All => "all",
        }
    }
}

pub trait ModelRuntimeControl: Send + Sync {
    fn release_models(&self, scope: ModelReleaseScope) -> anyhow::Result<()>;
}

pub trait PipelineProgress: Send + Sync {
    fn update(&self, item_id: &str, stage: &'static str, progress: f64, message: &str);
}

#[derive(Clone)]
struct NoopPipelineProgress;

impl PipelineProgress for NoopPipelineProgress {
    fn update(&self, _item_id: &str, _stage: &'static str, _progress: f64, _message: &str) {}
}

#[derive(Clone)]
struct NoopOcrEngine;

impl OcrEngine for NoopOcrEngine {
    fn ocr_images(&self, _paths: &[PathBuf]) -> anyhow::Result<Vec<OcrFrame>> {
        Ok(Vec::new())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessVideoSummary {
    pub item_id: String,
    pub audio_path: PathBuf,
    pub frames_dir: PathBuf,
    pub sampled_frames: usize,
    pub transcript_chunks: usize,
    pub ocr_chunks: usize,
    pub text_vectors: usize,
    pub image_vectors: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessAudioSummary {
    pub item_id: String,
    pub audio_path: PathBuf,
    pub transcript_chunks: usize,
    pub text_vectors: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessImageSummary {
    pub item_id: String,
    pub image_path: PathBuf,
    pub image_chunks: usize,
    pub image_vectors: usize,
    pub exif_fields: usize,
}

struct TranscriptStorage {
    chunks: Vec<StorageTranscriptChunk>,
    lines: Vec<StorageTranscriptLine>,
}

#[derive(Clone)]
struct TranscriptFirstIndexSummary {
    write_summary: StorageWriteSummary,
    search_units: usize,
    search_vectors: usize,
}

#[derive(Clone)]
pub struct VideoPipeline {
    paths: AppPaths,
    transcriber: Arc<dyn Transcriber>,
    embedder: Arc<dyn Embedder>,
    ocr: Arc<dyn OcrEngine>,
    ocr_enabled: bool,
    runtime_control: Option<Arc<dyn ModelRuntimeControl>>,
    progress: Arc<dyn PipelineProgress>,
    frame_interval_sec: u32,
    chunk_window_sec: f64,
    chunk_overlap_sec: f64,
    embedding_profile: Option<cerul_storage::vectors::EmbeddingProfile>,
    usage_job_id: Option<String>,
    model_permits: Option<Arc<Semaphore>>,
    transcript_first_indexing: bool,
}

impl VideoPipeline {
    pub fn new(
        paths: AppPaths,
        transcriber: Arc<dyn Transcriber>,
        embedder: Arc<dyn Embedder>,
    ) -> Self {
        Self {
            paths,
            transcriber,
            embedder,
            ocr: Arc::new(NoopOcrEngine),
            ocr_enabled: false,
            runtime_control: None,
            progress: Arc::new(NoopPipelineProgress),
            frame_interval_sec: 10,
            chunk_window_sec: 12.0,
            chunk_overlap_sec: 2.0,
            embedding_profile: None,
            usage_job_id: None,
            model_permits: None,
            transcript_first_indexing: false,
        }
    }

    pub fn with_frame_interval_sec(mut self, frame_interval_sec: u32) -> Self {
        self.frame_interval_sec = frame_interval_sec;
        self
    }

    pub fn with_chunking(mut self, window_sec: f64, overlap_sec: f64) -> Self {
        self.chunk_window_sec = window_sec;
        self.chunk_overlap_sec = overlap_sec;
        self
    }

    pub fn with_progress(mut self, progress: Arc<dyn PipelineProgress>) -> Self {
        self.progress = progress;
        self
    }

    pub fn with_usage_job_id(mut self, job_id: impl Into<String>) -> Self {
        self.usage_job_id = Some(job_id.into());
        self
    }

    pub fn with_embedding_profile(
        mut self,
        profile: cerul_storage::vectors::EmbeddingProfile,
    ) -> Self {
        self.embedding_profile = Some(profile);
        self
    }

    pub fn with_ocr(mut self, ocr: Arc<dyn OcrEngine>) -> Self {
        self.ocr = ocr;
        self.ocr_enabled = true;
        self
    }

    pub fn with_runtime_control(mut self, runtime_control: Arc<dyn ModelRuntimeControl>) -> Self {
        self.runtime_control = Some(runtime_control);
        self
    }

    pub fn with_model_permits(mut self, model_permits: Arc<Semaphore>) -> Self {
        self.model_permits = Some(model_permits);
        self
    }

    pub fn with_transcript_first_indexing(mut self, enabled: bool) -> Self {
        self.transcript_first_indexing = enabled;
        self
    }

    async fn acquire_model_permit(&self) -> anyhow::Result<Option<OwnedSemaphorePermit>> {
        match &self.model_permits {
            Some(model_permits) => Ok(Some(Arc::clone(model_permits).acquire_owned().await?)),
            None => Ok(None),
        }
    }

    async fn acquire_model_permit_with_wait(
        &self,
        item_id: &str,
        progress: f64,
    ) -> anyhow::Result<Option<OwnedSemaphorePermit>> {
        if self.model_permits.is_some() {
            self.report_progress(
                item_id,
                "waiting_model",
                progress,
                "Waiting for local model",
            );
        }
        let started = Instant::now();
        let permit = self.acquire_model_permit().await?;
        if self.model_permits.is_some() {
            self.log_pipeline_event(
                item_id,
                "model_permit_acquired",
                json!({
                    "wait_ms": started.elapsed().as_millis() as u64,
                }),
            );
        }
        Ok(permit)
    }

    fn report_progress(&self, item_id: &str, stage: &'static str, progress: f64, message: &str) {
        self.progress.update(item_id, stage, progress, message);
    }

    fn log_pipeline_event(&self, item_id: &str, event: &str, details: Value) {
        if let Err(error) = cerul_storage::append_jsonl_event(
            &self.paths,
            PIPELINE_JOB_LOG_FILE,
            json!({
                "event": event,
                "item_id": item_id,
                "job_id": self.usage_job_id.as_deref(),
                "details": details,
            }),
        ) {
            tracing::warn!(%error, item_id, event, "failed to append Cerul pipeline event");
        }
    }

    /// Embed `texts` in adaptive batches, advancing the progress bar from `base`
    /// toward `base + span` as each batch lands. A single all-at-once embed call
    /// leaves the bar frozen for the whole pass (which reads as "stuck"); batching
    /// gives the UI live motion and a `done/total` count without changing the
    /// per-item request count for remote providers (they already embed serially).
    async fn embed_texts_with_progress(
        &self,
        item_id: &str,
        stage: &'static str,
        base: f64,
        span: f64,
        message: &str,
        texts: &[String],
    ) -> anyhow::Result<Vec<Vec<f32>>> {
        let total = texts.len();
        if total == 0 {
            return Ok(Vec::new());
        }
        // Aim for ~20 updates across the pass while keeping batches large enough
        // to stay efficient (the local sidecar batches each call on the GPU).
        let batch = (total / 20).max(1);
        let mut vectors = Vec::with_capacity(total);
        let mut done = 0usize;
        let _model_permit = self.acquire_model_permit_with_wait(item_id, base).await?;
        for chunk in texts.chunks(batch) {
            let embedder = Arc::clone(&self.embedder);
            let owned = chunk.to_vec();
            vectors
                .extend(tokio::task::spawn_blocking(move || embedder.embed_texts(&owned)).await??);
            done += chunk.len();
            self.report_progress(
                item_id,
                stage,
                base + span * (done as f64 / total as f64),
                &format!("{message} · {done}/{total}"),
            );
        }
        Ok(vectors)
    }

    /// Image counterpart to [`Self::embed_texts_with_progress`]; embeds keyframes
    /// in batches so frame embedding (often the longest single stage) shows live
    /// progress instead of sitting at a fixed value until every frame is done.
    async fn embed_images_with_progress(
        &self,
        item_id: &str,
        stage: &'static str,
        base: f64,
        span: f64,
        message: &str,
        paths: &[PathBuf],
    ) -> anyhow::Result<Vec<Vec<f32>>> {
        let total = paths.len();
        if total == 0 {
            return Ok(Vec::new());
        }
        let batch = (total / 20).max(1);
        let mut vectors = Vec::with_capacity(total);
        let mut done = 0usize;
        let _model_permit = self.acquire_model_permit_with_wait(item_id, base).await?;
        for chunk in paths.chunks(batch) {
            let embedder = Arc::clone(&self.embedder);
            let owned = chunk.to_vec();
            vectors
                .extend(tokio::task::spawn_blocking(move || embedder.embed_images(&owned)).await??);
            done += chunk.len();
            self.report_progress(
                item_id,
                stage,
                base + span * (done as f64 / total as f64),
                &format!("{message} · {done}/{total}"),
            );
        }
        Ok(vectors)
    }

    pub async fn release_all_runtime_models(&self, item_id: &str) {
        let _model_permit = match self.acquire_model_permit().await {
            Ok(permit) => permit,
            Err(error) => {
                tracing::warn!(%error, item_id, "failed to acquire model runtime release permit");
                None
            }
        };
        self.release_runtime_models(ModelReleaseScope::All, item_id, "job finished");
    }

    fn release_runtime_models(
        &self,
        scope: ModelReleaseScope,
        item_id: &str,
        reason: &'static str,
    ) {
        let Some(runtime_control) = self.runtime_control.as_ref() else {
            return;
        };
        if let Err(error) = runtime_control.release_models(scope) {
            tracing::warn!(
                %error,
                item_id,
                scope = scope.as_str(),
                reason,
                "failed to release local model runtime"
            );
        }
    }

    async fn cleanup_success_temp_artifacts(&self, item_id: &str, audio_path: &Path) {
        if let Err(error) = remove_file_if_exists(audio_path).await {
            tracing::warn!(%error, item_id, path = %audio_path.display(), "failed to remove temporary pipeline audio");
        }
        let budget = pipeline_temp_cache_budget_bytes();
        if let Err(error) = prune_pipeline_temp_cache(&self.paths, budget).await {
            tracing::warn!(%error, item_id, "failed to prune pipeline temp cache");
        }
    }

    pub async fn process_video_item(&self, item_id: &str) -> anyhow::Result<ProcessVideoSummary> {
        anyhow::ensure!(
            self.frame_interval_sec > 0,
            "frame interval must be positive"
        );

        self.report_progress(item_id, "fetching", 0.05, "Fetching source media");
        let item = cerul_storage::get_item(&self.paths, item_id)?;
        let source = cerul_sources::build(
            &item.source_type,
            source_config_with_app_cache(
                &self.paths,
                &item.source_type,
                item.source_config.clone(),
            ),
        )?;
        let fetch_progress = {
            let progress = Arc::clone(&self.progress);
            let item_id = item_id.to_string();
            Arc::new(move |download_fraction: f64, message: String| {
                let progress_fraction = 0.05 + download_fraction.clamp(0.0, 1.0) * 0.06;
                progress.update(&item_id, "downloading", progress_fraction, &message);
            }) as cerul_sources::FetchProgress
        };
        let video_path = source
            .fetch_with_progress(&item.as_discovered_item(), Some(fetch_progress))
            .await?;
        if matches!(item.source_type.as_str(), "web_video" | "youtube")
            && item.raw_path.as_deref() != video_path.to_str()
        {
            cerul_storage::set_item_raw_path(&self.paths, item_id, &video_path)?;
        }
        update_item_duration_from_media(&self.paths, item_id, &video_path).await;
        let cache_key = cache_key_for_item(&item.id, item.discovery_id());
        let audio_path = self
            .paths
            .cache
            .join("pipeline")
            .join("audio")
            .join(format!("{cache_key}.wav"));
        let frames_dir = self
            .paths
            .cache
            .join("pipeline")
            .join("frames")
            .join(&cache_key);

        self.report_progress(item_id, "sampling_frames", 0.18, "Sampling visual frames");
        let frames =
            ffmpeg::sample_frames(&video_path, &frames_dir, self.frame_interval_sec).await?;
        let keyframes = keyframe_chunks(&frames, self.frame_interval_sec);
        match cerul_storage::replace_item_keyframes(&self.paths, item_id, &keyframes) {
            Ok(count) if count > 0 => {
                tracing::info!(item_id, keyframes = count, "stored early video thumbnails");
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(%error, item_id, "failed to store early video thumbnails");
            }
        }

        // Audio is optional. Many screen recordings (and capture tools'
        // intermediate files) are video-only, so probe before extracting: a
        // missing audio track now yields a visual-only index instead of a hard
        // "Output file #0 does not contain any stream" failure. Persist the
        // verdict so the UI can label the item as picture-searchable only.
        let has_audio = ffmpeg::probe_has_audio(&video_path).await.unwrap_or(true);
        cerul_storage::update_item_metadata(&self.paths, item_id, |metadata| {
            metadata.insert("has_audio".to_string(), serde_json::Value::Bool(has_audio));
        })?;

        let segments = if has_audio {
            self.report_progress(item_id, "extracting_audio", 0.12, "Extracting audio");
            ffmpeg::extract_audio(&video_path, &audio_path).await?;

            let transcriber = Arc::clone(&self.transcriber);
            let audio_for_transcribe = audio_path.clone();
            let progress = Arc::clone(&self.progress);
            self.report_progress(
                item_id,
                "preparing_models",
                0.23,
                "Preparing transcription models",
            );
            let transcriber_for_prepare = Arc::clone(&transcriber);
            tokio::task::spawn_blocking(move || transcriber_for_prepare.prepare_transcription())
                .await??;
            let progress_item_id = item_id.to_string();
            let _model_permit = self.acquire_model_permit_with_wait(item_id, 0.24).await?;
            self.report_progress(item_id, "transcribing", 0.25, "Transcribing audio");
            // Whole-file transcription reports no real sub-progress, so ease the
            // bar forward on an elapsed-time estimate to show the job is alive.
            let tick_stop = Arc::new(AtomicBool::new(false));
            let tick_handle = {
                let tick_stop = Arc::clone(&tick_stop);
                let tick_progress = Arc::clone(&self.progress);
                let tick_item_id = item_id.to_string();
                thread::spawn(move || {
                    let started = Instant::now();
                    while !tick_stop.load(Ordering::Relaxed) {
                        thread::sleep(Duration::from_secs(2));
                        if tick_stop.load(Ordering::Relaxed) {
                            break;
                        }
                        let elapsed = started.elapsed().as_secs_f64();
                        let eased = 1.0 - (-elapsed / 150.0).exp();
                        tick_progress.update(
                            &tick_item_id,
                            "transcribing",
                            0.25 + eased * 0.33,
                            "Transcribing audio",
                        );
                    }
                })
            };
            let transcription = tokio::task::spawn_blocking(move || {
                let callback: TranscriptionProgress = Arc::new(move |percent| {
                    let bounded = percent.clamp(0, 100) as f64 / 100.0;
                    progress.update(
                        &progress_item_id,
                        "transcribing",
                        0.25 + (bounded * 0.35),
                        "Transcribing audio",
                    );
                });
                transcriber.transcribe(&audio_for_transcribe, Some(callback))
            })
            .await;
            tick_stop.store(true, Ordering::Relaxed);
            let _ = tick_handle.join();
            let segments = transcription??;
            self.release_runtime_models(
                ModelReleaseScope::Transcription,
                item_id,
                "transcription complete",
            );
            let audio_seconds = audio_seconds_from_segments(&segments);
            self.record_asr_usage(
                item_id,
                audio_seconds,
                "succeeded",
                json!({
                    "segments": segments.len(),
                    "source": "indexing",
                }),
            );
            segments
        } else {
            tracing::info!(
                item_id,
                "video has no audio stream; indexing visual frames only"
            );
            self.report_progress(
                item_id,
                "transcribing",
                0.60,
                "No audio track — indexing visuals only",
            );
            Vec::new()
        };
        self.report_progress(
            item_id,
            "chunking_transcript",
            0.62,
            "Preparing transcript chunks",
        );
        let transcript_storage = transcript_storage_from_segments(
            &segments,
            self.chunk_window_sec,
            self.chunk_overlap_sec,
        );
        let has_transcript_text = transcript_storage
            .chunks
            .iter()
            .any(|chunk| !chunk.text.trim().is_empty());
        let transcript_first_summary = if self.transcript_first_indexing && has_transcript_text {
            self.report_progress(
                item_id,
                "writing_transcript_first",
                0.62,
                "Saving transcript-first index",
            );
            let first_sqlite_summary = cerul_storage::write_media_sqlite_chunks_with_ocr_and_lines(
                &self.paths,
                item_id,
                &transcript_storage.chunks,
                &transcript_storage.lines,
                &[],
                &keyframes,
            )?;
            set_embedding_index_status(&self.paths, item_id, "pending", None, 0, 0)?;
            match self
                .embed_and_write_retrieval_units(item_id, 0.625, 0.01, false, true)
                .await
            {
                Ok(vector_summary) if vector_summary.text_vectors > 0 => {
                    let write_summary = StorageWriteSummary {
                        transcript_chunks: first_sqlite_summary.transcript_chunks,
                        keyframes: first_sqlite_summary.keyframes,
                        text_vectors: vector_summary.text_vectors,
                        image_vectors: 0,
                    };
                    cerul_storage::set_video_multimodal_index_status(
                        &self.paths,
                        item_id,
                        "pending",
                        None,
                        frames.len(),
                        0,
                        "pending",
                        None,
                        0,
                    )?;
                    set_embedding_index_status(
                        &self.paths,
                        item_id,
                        "indexed",
                        None,
                        write_summary.text_vectors,
                        write_summary.image_vectors,
                    )?;
                    self.report_progress(
                        item_id,
                        "transcript_indexed",
                        0.635,
                        "Transcript searchable while indexing visuals",
                    );
                    self.log_pipeline_event(
                        item_id,
                        "transcript_first_indexed",
                        json!({
                            "transcript_chunks": write_summary.transcript_chunks,
                            "text_vectors": write_summary.text_vectors,
                            "sampled_frames": frames.len(),
                        }),
                    );
                    Some(TranscriptFirstIndexSummary {
                        write_summary,
                        search_units: vector_summary.transcript_chunks,
                        search_vectors: vector_summary.text_vectors,
                    })
                }
                Ok(vector_summary) => {
                    cerul_storage::set_item_search_index_status(
                        &self.paths,
                        item_id,
                        "pending",
                        None,
                        0,
                        0,
                    )?;
                    self.log_pipeline_event(
                        item_id,
                        "transcript_first_skipped",
                        json!({
                            "reason": "empty_text_vectors",
                            "transcript_chunks": first_sqlite_summary.transcript_chunks,
                            "text_vectors": vector_summary.text_vectors,
                        }),
                    );
                    None
                }
                Err(error) => {
                    tracing::warn!(
                        %error,
                        item_id,
                        "transcript-first retrieval index failed; continuing full indexing pass"
                    );
                    self.log_pipeline_event(
                        item_id,
                        "transcript_first_failed",
                        json!({ "error": error.to_string() }),
                    );
                    None
                }
            }
        } else {
            if self.transcript_first_indexing {
                self.log_pipeline_event(
                    item_id,
                    "transcript_first_skipped",
                    json!({
                        "reason": "empty_transcript",
                        "transcript_chunks": transcript_storage.chunks.len(),
                    }),
                );
            }
            None
        };

        let mut ocr_error: Option<String> = None;
        let ocr_frames = if self.ocr_enabled {
            let ocr = Arc::clone(&self.ocr);
            let frames_for_ocr = frames.clone();
            let ocr_progress = Arc::clone(&self.progress);
            let ocr_item_id = item_id.to_string();
            self.report_progress(
                item_id,
                "ocr_frames",
                0.64,
                "Reading text from visual frames",
            );
            let _model_permit = self.acquire_model_permit_with_wait(item_id, 0.64).await?;
            let frames_result: anyhow::Result<Vec<OcrFrame>> =
                match tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<OcrFrame>> {
                    let total = frames_for_ocr.len();
                    let mut collected = Vec::with_capacity(total);
                    for (index, frame) in frames_for_ocr.iter().enumerate() {
                        collected.extend(ocr.ocr_images(std::slice::from_ref(frame))?);
                        let done = index + 1;
                        let fraction = done as f64 / total.max(1) as f64;
                        ocr_progress.update(
                            &ocr_item_id,
                            "ocr_frames",
                            0.64 + fraction * 0.03,
                            &format!("Reading text from visual frames · {done}/{total}"),
                        );
                    }
                    Ok(collected)
                })
                .await
                {
                    Ok(result) => result,
                    Err(error) => Err(error.into()),
                };
            match frames_result {
                Ok(frames) => {
                    self.release_runtime_models(ModelReleaseScope::Ocr, item_id, "ocr complete");
                    frames
                }
                Err(error) if transcript_first_summary.is_some() => {
                    let message = error.to_string();
                    self.release_runtime_models(
                        ModelReleaseScope::Ocr,
                        item_id,
                        "ocr failed after transcript-first index",
                    );
                    tracing::warn!(
                        %error,
                        item_id,
                        "ocr failed after transcript-first index; continuing with transcript-only search"
                    );
                    self.log_pipeline_event(
                        item_id,
                        "ocr_failed_after_transcript_first",
                        json!({ "error": message.clone() }),
                    );
                    self.report_progress(
                        item_id,
                        "ocr_frames",
                        0.67,
                        "OCR unavailable; transcript remains searchable",
                    );
                    ocr_error = Some(message);
                    Vec::new()
                }
                Err(error) => {
                    self.release_runtime_models(ModelReleaseScope::Ocr, item_id, "ocr failed");
                    return Err(error);
                }
            }
        } else {
            Vec::new()
        };
        let storage_ocr_chunks = ocr_frames
            .into_iter()
            .filter(|frame| !frame.text.trim().is_empty())
            .map(|frame| StorageOcrChunk::frame(frame.path, frame.text))
            .collect::<Vec<_>>();
        let ocr_status = if self.ocr_enabled {
            if ocr_error.is_some() {
                "failed"
            } else {
                "indexed"
            }
        } else {
            "disabled"
        };
        let ocr_error_message = ocr_error.as_deref();

        self.report_progress(
            item_id,
            "writing_transcript",
            0.68,
            "Saving searchable transcript",
        );
        if transcript_first_summary.is_some() {
            self.log_pipeline_event(
                item_id,
                "visual_enrichment_started",
                json!({
                    "sampled_frames": frames.len(),
                    "ocr_chunks": storage_ocr_chunks.len(),
                }),
            );
        }
        let sqlite_summary = match cerul_storage::write_media_sqlite_chunks_with_ocr_and_lines(
            &self.paths,
            item_id,
            &transcript_storage.chunks,
            &transcript_storage.lines,
            &storage_ocr_chunks,
            &keyframes,
        ) {
            Ok(summary) => summary,
            Err(error) => {
                let message = error.to_string();
                if let Some(transcript_first) = &transcript_first_summary {
                    tracing::warn!(
                        %error,
                        item_id,
                        "canonical SQLite rewrite failed after transcript-first index; preserving transcript-only search"
                    );
                    set_embedding_index_status(
                        &self.paths,
                        item_id,
                        "indexed",
                        None,
                        transcript_first.write_summary.text_vectors,
                        transcript_first.write_summary.image_vectors,
                    )?;
                    cerul_storage::set_item_search_index_status(
                        &self.paths,
                        item_id,
                        "indexed",
                        None,
                        transcript_first.search_units,
                        transcript_first.search_vectors,
                    )?;
                    cerul_storage::set_video_multimodal_index_status(
                        &self.paths,
                        item_id,
                        "display_only",
                        Some(&message),
                        frames.len(),
                        0,
                        if self.ocr_enabled {
                            "failed"
                        } else {
                            "disabled"
                        },
                        if self.ocr_enabled {
                            Some(&message)
                        } else {
                            None
                        },
                        0,
                    )?;
                    cerul_storage::mark_indexed(&self.paths, item_id)?;
                    self.log_pipeline_event(
                        item_id,
                        "transcript_first_preserved_after_sqlite_rewrite_failure",
                        json!({
                            "error": message,
                            "search_units": transcript_first.search_units,
                            "search_vectors": transcript_first.search_vectors,
                        }),
                    );
                    self.report_progress(
                        item_id,
                        "partial",
                        1.0,
                        "Transcript searchable; visual metadata unavailable",
                    );
                    self.cleanup_success_temp_artifacts(item_id, &audio_path)
                        .await;
                    return Ok(ProcessVideoSummary::from_write_summary(
                        item_id,
                        audio_path,
                        frames_dir,
                        frames.len(),
                        0,
                        transcript_first.write_summary.clone(),
                    ));
                }
                return Err(error);
            }
        };
        set_embedding_index_status(&self.paths, item_id, "pending", None, 0, 0)?;

        self.report_progress(
            item_id,
            "writing_index",
            0.80,
            "Writing unified search index",
        );
        let full_index_result = self
            .embed_and_write_retrieval_units(
                item_id,
                0.80,
                0.12,
                true,
                transcript_first_summary.is_none(),
            )
            .await;
        let vector_summary = match full_index_result {
            Ok(write_summary) => write_summary,
            Err(error) => {
                let message = error.to_string();
                tracing::warn!(
                    %error,
                    item_id,
                    "unified retrieval index write failed; keeping canonical transcript and OCR artifacts"
                );
                if let Some(transcript_first) = &transcript_first_summary {
                    set_embedding_index_status(
                        &self.paths,
                        item_id,
                        "indexed",
                        None,
                        transcript_first.write_summary.text_vectors,
                        transcript_first.write_summary.image_vectors,
                    )?;
                    cerul_storage::set_item_search_index_status(
                        &self.paths,
                        item_id,
                        "indexed",
                        None,
                        transcript_first.search_units,
                        transcript_first.search_vectors,
                    )?;
                    self.log_pipeline_event(
                        item_id,
                        "transcript_first_preserved_after_visual_failure",
                        json!({
                            "error": message,
                            "full_index_write_mode": "upsert_preserve_existing",
                            "search_units": transcript_first.search_units,
                            "search_vectors": transcript_first.search_vectors,
                        }),
                    );
                } else {
                    set_embedding_index_status(
                        &self.paths,
                        item_id,
                        "failed",
                        Some(&message),
                        0,
                        0,
                    )?;
                    cerul_storage::set_item_search_index_status(
                        &self.paths,
                        item_id,
                        "failed",
                        Some(&message),
                        0,
                        0,
                    )?;
                }
                cerul_storage::set_video_multimodal_index_status(
                    &self.paths,
                    item_id,
                    "display_only",
                    None,
                    frames.len(),
                    0,
                    ocr_status,
                    ocr_error_message,
                    storage_ocr_chunks.len(),
                )?;
                cerul_storage::mark_indexed(&self.paths, item_id)?;
                self.report_progress(
                    item_id,
                    "partial",
                    1.0,
                    if transcript_first_summary.is_some() {
                        "Transcript searchable; visual index unavailable"
                    } else {
                        "Transcript saved; search index unavailable"
                    },
                );
                self.cleanup_success_temp_artifacts(item_id, &audio_path)
                    .await;
                return Ok(ProcessVideoSummary::from_write_summary(
                    item_id,
                    audio_path,
                    frames_dir,
                    frames.len(),
                    storage_ocr_chunks.len(),
                    sqlite_summary,
                ));
            }
        };
        let write_summary = StorageWriteSummary {
            transcript_chunks: sqlite_summary.transcript_chunks,
            keyframes: sqlite_summary.keyframes,
            text_vectors: vector_summary.text_vectors,
            image_vectors: vector_summary.image_vectors,
        };
        cerul_storage::set_video_multimodal_index_status(
            &self.paths,
            item_id,
            "display_only",
            None,
            frames.len(),
            0,
            ocr_status,
            ocr_error_message,
            storage_ocr_chunks.len(),
        )?;
        set_embedding_index_status(
            &self.paths,
            item_id,
            "indexed",
            None,
            write_summary.text_vectors,
            write_summary.image_vectors,
        )?;
        cerul_storage::mark_indexed(&self.paths, item_id)?;
        self.report_progress(item_id, "completed", 1.0, "Index complete");
        self.log_pipeline_event(
            item_id,
            "video_index_complete",
            json!({
                "sampled_frames": frames.len(),
                "ocr_chunks": storage_ocr_chunks.len(),
                "transcript_chunks": write_summary.transcript_chunks,
                "text_vectors": write_summary.text_vectors,
                "image_vectors": write_summary.image_vectors,
                "transcript_first": transcript_first_summary.is_some(),
            }),
        );
        self.cleanup_success_temp_artifacts(item_id, &audio_path)
            .await;

        Ok(ProcessVideoSummary::from_write_summary(
            item_id,
            audio_path,
            frames_dir,
            frames.len(),
            storage_ocr_chunks.len(),
            write_summary,
        ))
    }

    pub async fn process_audio_item(&self, item_id: &str) -> anyhow::Result<ProcessAudioSummary> {
        let item = cerul_storage::get_item(&self.paths, item_id)?;
        let source = cerul_sources::build(
            &item.source_type,
            source_config_with_app_cache(
                &self.paths,
                &item.source_type,
                item.source_config.clone(),
            ),
        )?;
        let source_audio_path = source.fetch(&item.as_discovered_item()).await?;
        if item.source_type == "rss_podcast"
            && item.raw_path.as_deref() != source_audio_path.to_str()
        {
            cerul_storage::set_item_raw_path(&self.paths, item_id, &source_audio_path)?;
        }
        update_item_duration_from_media(&self.paths, item_id, &source_audio_path).await;
        let cache_key = cache_key_for_item(&item.id, item.discovery_id());
        let audio_path = self
            .paths
            .cache
            .join("pipeline")
            .join("audio")
            .join(format!("{cache_key}.wav"));

        ffmpeg::extract_audio(&source_audio_path, &audio_path).await?;
        let transcript_storage = self
            .transcribe_to_storage_chunks(item_id, &audio_path)
            .await?;
        let sqlite_summary = cerul_storage::write_media_sqlite_chunks_with_ocr_and_lines(
            &self.paths,
            item_id,
            &transcript_storage.chunks,
            &transcript_storage.lines,
            &[],
            &[],
        )?;
        set_embedding_index_status(&self.paths, item_id, "pending", None, 0, 0)?;
        let vector_summary = match self
            .embed_and_write_retrieval_units(item_id, 0.78, 0.16, true, true)
            .await
        {
            Ok(write_summary) => write_summary,
            Err(error) => {
                let message = error.to_string();
                tracing::warn!(
                    %error,
                    item_id,
                    "audio unified retrieval index write failed; keeping canonical transcript artifacts"
                );
                set_embedding_index_status(&self.paths, item_id, "failed", Some(&message), 0, 0)?;
                cerul_storage::set_item_search_index_status(
                    &self.paths,
                    item_id,
                    "failed",
                    Some(&message),
                    0,
                    0,
                )?;
                cerul_storage::mark_indexed(&self.paths, item_id)?;
                self.cleanup_success_temp_artifacts(item_id, &audio_path)
                    .await;
                return Ok(ProcessAudioSummary::from_write_summary(
                    item_id,
                    audio_path,
                    sqlite_summary,
                ));
            }
        };
        let write_summary = StorageWriteSummary {
            transcript_chunks: sqlite_summary.transcript_chunks,
            keyframes: sqlite_summary.keyframes,
            text_vectors: vector_summary.text_vectors,
            image_vectors: vector_summary.image_vectors,
        };
        set_embedding_index_status(
            &self.paths,
            item_id,
            "indexed",
            None,
            write_summary.text_vectors,
            write_summary.image_vectors,
        )?;
        cerul_storage::mark_indexed(&self.paths, item_id)?;
        self.cleanup_success_temp_artifacts(item_id, &audio_path)
            .await;

        Ok(ProcessAudioSummary::from_write_summary(
            item_id,
            audio_path,
            write_summary,
        ))
    }

    pub async fn process_image_item(&self, item_id: &str) -> anyhow::Result<ProcessImageSummary> {
        let item = cerul_storage::get_item(&self.paths, item_id)?;
        let source = cerul_sources::build(
            &item.source_type,
            source_config_with_app_cache(
                &self.paths,
                &item.source_type,
                item.source_config.clone(),
            ),
        )?;
        let image_path = source.fetch(&item.as_discovered_item()).await?;
        let exif = read_exif_metadata(&image_path)?;
        let exif_fields = exif
            .get("exif")
            .and_then(|value| value.as_object())
            .map_or(0, |fields| fields.len());
        let image_chunk = StorageImageChunk::image(image_path.clone(), exif);
        let image_chunks = [image_chunk];
        let sqlite_summary =
            cerul_storage::write_media_sqlite_chunks(&self.paths, item_id, &[], &image_chunks)?;
        set_embedding_index_status(&self.paths, item_id, "pending", None, 0, 0)?;

        let vector_summary = match self
            .embed_and_write_retrieval_units(item_id, 0.78, 0.16, true, true)
            .await
        {
            Ok(write_summary) => write_summary,
            Err(error) => {
                let message = error.to_string();
                tracing::warn!(
                    %error,
                    item_id,
                    "image unified retrieval index write failed; keeping image chunk indexed without vectors"
                );
                if let Err(delete_error) =
                    cerul_storage::vectors::delete_item_embeddings(&self.paths, item_id).await
                {
                    tracing::warn!(
                        error = %delete_error,
                        item_id,
                        "failed to delete stale image vectors after retrieval index write failure"
                    );
                }
                set_embedding_index_status(&self.paths, item_id, "failed", Some(&message), 0, 0)?;
                let searchable_units =
                    cerul_storage::item_retrieval_unit_count(&self.paths, item_id).unwrap_or(0);
                let (search_status, search_error) = if searchable_units > 0 {
                    ("indexed", None)
                } else {
                    ("failed", Some(message.as_str()))
                };
                cerul_storage::set_item_search_index_status(
                    &self.paths,
                    item_id,
                    search_status,
                    search_error,
                    searchable_units,
                    0,
                )?;
                cerul_storage::mark_indexed(&self.paths, item_id)?;
                return Ok(ProcessImageSummary::from_write_summary(
                    item_id,
                    image_path,
                    exif_fields,
                    sqlite_summary,
                ));
            }
        };
        let write_summary = StorageWriteSummary {
            transcript_chunks: sqlite_summary.transcript_chunks,
            keyframes: sqlite_summary.keyframes,
            text_vectors: vector_summary.text_vectors,
            image_vectors: vector_summary.image_vectors,
        };
        set_embedding_index_status(
            &self.paths,
            item_id,
            "indexed",
            None,
            write_summary.text_vectors,
            write_summary.image_vectors,
        )?;
        cerul_storage::mark_indexed(&self.paths, item_id)?;

        Ok(ProcessImageSummary::from_write_summary(
            item_id,
            image_path,
            exif_fields,
            write_summary,
        ))
    }

    async fn transcribe_to_storage_chunks(
        &self,
        item_id: &str,
        audio_path: &Path,
    ) -> anyhow::Result<TranscriptStorage> {
        let transcriber = Arc::clone(&self.transcriber);
        let audio_for_transcribe = audio_path.to_path_buf();
        let transcriber_for_prepare = Arc::clone(&transcriber);
        tokio::task::spawn_blocking(move || transcriber_for_prepare.prepare_transcription())
            .await??;
        let _model_permit = self.acquire_model_permit().await?;
        let segments = tokio::task::spawn_blocking(move || {
            transcriber.transcribe(&audio_for_transcribe, None)
        })
        .await??;
        self.release_runtime_models(
            ModelReleaseScope::Transcription,
            item_id,
            "transcription complete",
        );
        self.record_asr_usage(
            item_id,
            audio_seconds_from_segments(&segments),
            "succeeded",
            json!({ "segments": segments.len(), "source": "indexing" }),
        );

        Ok(transcript_storage_from_segments(
            &segments,
            self.chunk_window_sec,
            self.chunk_overlap_sec,
        ))
    }

    fn active_embedding_profile(&self) -> anyhow::Result<cerul_storage::vectors::EmbeddingProfile> {
        match &self.embedding_profile {
            Some(profile) => Ok(profile.clone()),
            None => cerul_storage::vectors::ensure_active_embedding_profile(&self.paths),
        }
    }

    async fn embed_and_write_retrieval_units(
        &self,
        item_id: &str,
        base: f64,
        span: f64,
        include_image_embeddings: bool,
        replace_existing_vectors: bool,
    ) -> anyhow::Result<StorageWriteSummary> {
        let started = Instant::now();
        let profile = self.active_embedding_profile()?;
        if replace_existing_vectors {
            cerul_storage::set_item_search_index_status(
                &self.paths,
                item_id,
                "pending",
                None,
                0,
                0,
            )?;
        }
        let units = cerul_storage::rebuild_item_retrieval_units(&self.paths, item_id, &profile.id)?;
        anyhow::ensure!(
            !units.is_empty(),
            "no retrieval units generated for item {item_id}"
        );

        let text_units = units
            .iter()
            .filter(|unit| !unit.uses_image_embedding())
            .collect::<Vec<_>>();
        let image_units = if include_image_embeddings {
            units
                .iter()
                .filter(|unit| unit.has_image_embedding_source())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        self.log_pipeline_event(
            item_id,
            "retrieval_units_built",
            json!({
                "units": units.len(),
                "text_units": text_units.len(),
                "image_units": image_units.len(),
                "include_image_embeddings": include_image_embeddings,
                "replace_existing_vectors": replace_existing_vectors,
            }),
        );

        let text_inputs = text_units
            .iter()
            .map(|unit| unit.content_text.clone())
            .collect::<Vec<_>>();
        let text_vectors = match self
            .embed_texts_with_progress(
                item_id,
                "embedding_units",
                base,
                span * 0.75,
                "Embedding searchable moments",
                &text_inputs,
            )
            .await
        {
            Ok(vectors) => {
                if !text_inputs.is_empty() {
                    self.record_embedding_text_usage(
                        item_id,
                        estimate_text_tokens(&text_inputs),
                        text_inputs.len(),
                        "succeeded",
                        json!({ "source": "indexing", "index": "retrieval_units" }),
                    );
                }
                vectors
            }
            Err(error) => {
                if !text_inputs.is_empty() {
                    self.record_embedding_text_usage(
                        item_id,
                        estimate_text_tokens(&text_inputs),
                        text_inputs.len(),
                        "failed",
                        json!({
                            "source": "indexing",
                            "index": "retrieval_units",
                            "error": error.to_string()
                        }),
                    );
                }
                return Err(error);
            }
        };

        let image_paths = image_units
            .iter()
            .filter_map(|unit| unit.representative_frame_path.as_ref().map(PathBuf::from))
            .collect::<Vec<_>>();
        let image_vectors = match self
            .embed_images_with_progress(
                item_id,
                "embedding_unit_images",
                base + span * 0.75,
                span * 0.25,
                "Embedding visual moments",
                &image_paths,
            )
            .await
        {
            Ok(vectors) => {
                if !vectors.is_empty() {
                    self.record_embedding_image_usage(
                        item_id,
                        vectors.len(),
                        "succeeded",
                        json!({ "source": "indexing", "index": "retrieval_units" }),
                    );
                }
                vectors
            }
            Err(error) => {
                if !image_paths.is_empty() {
                    self.record_embedding_image_usage(
                        item_id,
                        image_paths.len(),
                        "failed",
                        json!({
                            "source": "indexing",
                            "index": "retrieval_units",
                            "error": error.to_string()
                        }),
                    );
                }
                if !image_paths.is_empty() && !text_vectors.is_empty() {
                    tracing::warn!(
                        item_id,
                        %error,
                        "visual retrieval embedding failed; keeping text retrieval vectors"
                    );
                    Vec::new()
                } else {
                    return Err(error);
                }
            }
        };

        anyhow::ensure!(
            text_vectors.len() == text_units.len(),
            "retrieval text unit count ({}) does not match vector count ({})",
            text_units.len(),
            text_vectors.len()
        );
        if !image_vectors.is_empty() {
            anyhow::ensure!(
                image_vectors.len() == image_units.len(),
                "retrieval image unit count ({}) does not match vector count ({})",
                image_units.len(),
                image_vectors.len()
            );
        }

        let mut records = Vec::with_capacity(text_vectors.len() + image_vectors.len());
        for (unit, vector) in text_units.into_iter().zip(text_vectors.iter()) {
            records.push(cerul_storage::vectors::VectorRecord::new_for_dimensions(
                unit.id.clone(),
                unit.item_id.clone(),
                vector.clone(),
                profile.output_dimension,
            )?);
        }
        for (unit, vector) in image_units.into_iter().zip(image_vectors.iter()) {
            records.push(
                cerul_storage::vectors::VectorRecord::new_for_dimensions_with_point_key(
                    format!("{}:image", unit.id),
                    unit.id.clone(),
                    unit.item_id.clone(),
                    vector.clone(),
                    profile.output_dimension,
                )?,
            );
        }

        let qdrant_started = Instant::now();
        let stale_vectors_deleted = if replace_existing_vectors {
            cerul_storage::vectors::replace_item_unified_embeddings_for_profile(
                &self.paths,
                item_id,
                &records,
                &profile,
                cerul_storage::SEARCH_INDEX_VERSION,
            )
            .await?;
            0
        } else {
            cerul_storage::vectors::upsert_item_unified_embeddings_for_profile(
                &self.paths,
                &records,
                &profile,
                cerul_storage::SEARCH_INDEX_VERSION,
            )
            .await?;
            cerul_storage::vectors::delete_stale_item_unified_embeddings_for_profile(
                &self.paths,
                item_id,
                &records,
                &profile,
                cerul_storage::SEARCH_INDEX_VERSION,
            )
            .await?
        };
        let qdrant_write_ms = qdrant_started.elapsed().as_millis() as u64;
        cerul_storage::set_item_search_index_status(
            &self.paths,
            item_id,
            "indexed",
            None,
            units.len(),
            records.len(),
        )?;
        self.log_pipeline_event(
            item_id,
            "retrieval_index_written",
            json!({
                "units": units.len(),
                "vectors": records.len(),
                "text_vectors": text_vectors.len(),
                "image_vectors": image_vectors.len(),
                "qdrant_write_ms": qdrant_write_ms,
                "total_ms": started.elapsed().as_millis() as u64,
                "embedding_profile_id": profile.id,
                "include_image_embeddings": include_image_embeddings,
                "replace_existing_vectors": replace_existing_vectors,
                "stale_vectors_deleted": stale_vectors_deleted,
            }),
        );

        Ok(StorageWriteSummary {
            transcript_chunks: units.len(),
            keyframes: image_paths.len(),
            text_vectors: text_vectors.len(),
            image_vectors: image_vectors.len(),
        })
    }

    fn record_asr_usage(
        &self,
        item_id: &str,
        audio_seconds: f64,
        status: &str,
        metadata: serde_json::Value,
    ) {
        let Some(provider) = self.transcriber.inference_provider() else {
            return;
        };
        let estimated_usd = if status == "succeeded" {
            estimate_asr_cost_usd(&provider, audio_seconds)
        } else {
            None
        };
        let price_snapshot_id = asr_price_snapshot(&provider, audio_seconds).map(str::to_string);
        let mut event = cerul_storage::NewUsageEvent::new(provider.provider_mode, "asr");
        event.provider_id = provider.provider_id;
        event.provider_type = provider.provider_type;
        event.model_id = provider.model_id;
        event.item_id = Some(item_id.to_string());
        event.job_id = self.usage_job_id.clone();
        event.status = status.to_string();
        event.audio_seconds = Some(audio_seconds);
        event.estimated_usd = estimated_usd;
        event.price_snapshot_id = price_snapshot_id;
        event.metadata = metadata;
        if let Err(error) = cerul_storage::record_usage_event(&self.paths, event) {
            tracing::warn!(%error, item_id, "failed to record ASR usage");
        }
    }

    fn record_embedding_text_usage(
        &self,
        item_id: &str,
        input_tokens: u64,
        chunk_count: usize,
        status: &str,
        metadata: serde_json::Value,
    ) {
        let Some(provider) = self.embedder.inference_provider() else {
            return;
        };
        let estimated_usd = if status == "succeeded" {
            estimate_embedding_text_cost_usd(&provider, input_tokens)
        } else {
            None
        };
        let price_snapshot_id = embedding_text_price_snapshot(&provider).map(str::to_string);
        let mut event = cerul_storage::NewUsageEvent::new(provider.provider_mode, "embedding_text");
        event.provider_id = provider.provider_id;
        event.provider_type = provider.provider_type;
        event.model_id = provider.model_id;
        event.item_id = Some(item_id.to_string());
        event.job_id = self.usage_job_id.clone();
        event.status = status.to_string();
        event.request_count = chunk_count.max(1) as u64;
        event.input_tokens = Some(input_tokens);
        event.estimated_usd = estimated_usd;
        event.price_snapshot_id = price_snapshot_id;
        event.metadata = metadata;
        if let Err(error) = cerul_storage::record_usage_event(&self.paths, event) {
            tracing::warn!(%error, item_id, "failed to record text embedding usage");
        }
    }

    fn record_embedding_image_usage(
        &self,
        item_id: &str,
        image_count: usize,
        status: &str,
        metadata: serde_json::Value,
    ) {
        let Some(provider) = self.embedder.inference_provider() else {
            return;
        };
        let estimated_usd = if status == "succeeded" {
            estimate_embedding_image_cost_usd(&provider, image_count as u64)
        } else {
            None
        };
        let price_snapshot_id = embedding_image_price_snapshot(&provider).map(str::to_string);
        let mut event =
            cerul_storage::NewUsageEvent::new(provider.provider_mode, "embedding_image");
        event.provider_id = provider.provider_id;
        event.provider_type = provider.provider_type;
        event.model_id = provider.model_id;
        event.item_id = Some(item_id.to_string());
        event.job_id = self.usage_job_id.clone();
        event.status = status.to_string();
        event.request_count = image_count.max(1) as u64;
        event.image_count = Some(image_count as u64);
        event.estimated_usd = estimated_usd;
        event.price_snapshot_id = price_snapshot_id;
        event.metadata = metadata;
        if let Err(error) = cerul_storage::record_usage_event(&self.paths, event) {
            tracing::warn!(%error, item_id, "failed to record image embedding usage");
        }
    }
}

const GROQ_WHISPER_LARGE_V3_TURBO_USD_PER_HOUR: f64 = 0.04;
const OPENAI_WHISPER_1_USD_PER_MINUTE: f64 = 0.006;
const GEMINI_EMBEDDING_2_TEXT_USD_PER_M_TOKENS: f64 = 0.20;
const GEMINI_EMBEDDING_2_IMAGE_USD_EACH: f64 = 0.00012;

fn audio_seconds_from_segments(segments: &[Segment]) -> f64 {
    segments
        .iter()
        .map(|segment| segment.end.max(segment.start))
        .fold(0.0, f64::max)
}

fn estimate_text_tokens(texts: &[String]) -> u64 {
    let chars = texts.iter().map(|text| text.chars().count()).sum::<usize>();
    ((chars as f64 / 4.0).ceil() as u64).max(texts.len() as u64)
}

fn estimate_asr_cost_usd(info: &InferenceProviderInfo, audio_seconds: f64) -> Option<f64> {
    let model = info.model_id.as_deref().unwrap_or_default();
    let base_url = info.base_url.as_deref().unwrap_or_default();
    if model == "whisper-large-v3-turbo" && base_url.contains("api.groq.com") {
        return Some(audio_seconds / 3600.0 * GROQ_WHISPER_LARGE_V3_TURBO_USD_PER_HOUR);
    }
    if model == "whisper-1" && info.provider_type.as_deref() == Some("openai") {
        return Some(audio_seconds / 60.0 * OPENAI_WHISPER_1_USD_PER_MINUTE);
    }
    None
}

fn asr_price_snapshot(info: &InferenceProviderInfo, _audio_seconds: f64) -> Option<&'static str> {
    let model = info.model_id.as_deref().unwrap_or_default();
    let base_url = info.base_url.as_deref().unwrap_or_default();
    if model == "whisper-large-v3-turbo" && base_url.contains("api.groq.com") {
        return Some("groq-whisper-large-v3-turbo-2026-05");
    }
    if model == "whisper-1" && info.provider_type.as_deref() == Some("openai") {
        return Some("openai-whisper-1-2026-05");
    }
    None
}

fn estimate_embedding_text_cost_usd(
    info: &InferenceProviderInfo,
    input_tokens: u64,
) -> Option<f64> {
    if info.model_id.as_deref() == Some("gemini-embedding-2") {
        return Some(input_tokens as f64 / 1_000_000.0 * GEMINI_EMBEDDING_2_TEXT_USD_PER_M_TOKENS);
    }
    None
}

fn embedding_text_price_snapshot(info: &InferenceProviderInfo) -> Option<&'static str> {
    if info.model_id.as_deref() == Some("gemini-embedding-2") {
        return Some("gemini-embedding-2-text-standard-2026-05");
    }
    None
}

fn estimate_embedding_image_cost_usd(
    info: &InferenceProviderInfo,
    image_count: u64,
) -> Option<f64> {
    if info.model_id.as_deref() == Some("gemini-embedding-2") {
        return Some(image_count as f64 * GEMINI_EMBEDDING_2_IMAGE_USD_EACH);
    }
    None
}

fn embedding_image_price_snapshot(info: &InferenceProviderInfo) -> Option<&'static str> {
    if info.model_id.as_deref() == Some("gemini-embedding-2") {
        return Some("gemini-embedding-2-image-standard-2026-05");
    }
    None
}

impl ProcessVideoSummary {
    fn from_write_summary(
        item_id: &str,
        audio_path: PathBuf,
        frames_dir: PathBuf,
        sampled_frames: usize,
        ocr_chunks: usize,
        write_summary: StorageWriteSummary,
    ) -> Self {
        Self {
            item_id: item_id.to_string(),
            audio_path,
            frames_dir,
            sampled_frames,
            transcript_chunks: write_summary.transcript_chunks.saturating_sub(ocr_chunks),
            ocr_chunks,
            text_vectors: write_summary.text_vectors,
            image_vectors: write_summary.image_vectors,
        }
    }
}

impl ProcessAudioSummary {
    fn from_write_summary(
        item_id: &str,
        audio_path: PathBuf,
        write_summary: StorageWriteSummary,
    ) -> Self {
        Self {
            item_id: item_id.to_string(),
            audio_path,
            transcript_chunks: write_summary.transcript_chunks,
            text_vectors: write_summary.text_vectors,
        }
    }
}

impl ProcessImageSummary {
    fn from_write_summary(
        item_id: &str,
        image_path: PathBuf,
        exif_fields: usize,
        write_summary: StorageWriteSummary,
    ) -> Self {
        Self {
            item_id: item_id.to_string(),
            image_path,
            image_chunks: write_summary.keyframes,
            image_vectors: write_summary.image_vectors,
            exif_fields,
        }
    }
}

pub struct GlobalEmbedder;

impl Embedder for GlobalEmbedder {
    fn embed_texts(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let text_refs = texts.iter().map(String::as_str).collect::<Vec<_>>();
        cerul_embed::embed_texts(&text_refs)
    }

    fn embed_images(&self, paths: &[PathBuf]) -> anyhow::Result<Vec<Vec<f32>>> {
        cerul_embed::embed_images(paths)
    }
}

impl Transcriber for crate::whisper::WhisperEngine {
    fn transcribe(
        &self,
        audio_path: &Path,
        progress: Option<TranscriptionProgress>,
    ) -> anyhow::Result<Vec<Segment>> {
        crate::whisper::WhisperEngine::transcribe_with_progress(self, audio_path, progress)
    }
}

pub fn cache_key_for_discovery_id(input: &str) -> String {
    cache_key(input)
}

pub fn cache_key_for_item(item_id: &str, discovery_id: &str) -> String {
    format!("{}-{}", cache_key(item_id), cache_key(discovery_id))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineTempCachePrune {
    pub removed_entries: usize,
    pub removed_bytes: u64,
    pub remaining_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PipelineTempCacheEntryKind {
    File,
    Dir,
}

#[derive(Debug, Clone)]
struct PipelineTempCacheEntry {
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
    kind: PipelineTempCacheEntryKind,
}

pub async fn prune_pipeline_temp_cache(
    paths: &AppPaths,
    budget_bytes: u64,
) -> anyhow::Result<PipelineTempCachePrune> {
    let mut entries = collect_pipeline_temp_cache_entries(paths)?;
    let mut total_bytes = entries.iter().map(|entry| entry.bytes).sum::<u64>();
    if total_bytes <= budget_bytes {
        return Ok(PipelineTempCachePrune {
            removed_entries: 0,
            removed_bytes: 0,
            remaining_bytes: total_bytes,
        });
    }

    entries.sort_by_key(|entry| entry.modified);
    let mut removed_entries = 0usize;
    let mut removed_bytes = 0u64;
    for entry in entries {
        if total_bytes <= budget_bytes {
            break;
        }
        match entry.kind {
            PipelineTempCacheEntryKind::File => remove_file_if_exists(&entry.path).await?,
            PipelineTempCacheEntryKind::Dir => remove_dir_if_exists(&entry.path).await?,
        }
        removed_entries += 1;
        removed_bytes = removed_bytes.saturating_add(entry.bytes);
        total_bytes = total_bytes.saturating_sub(entry.bytes);
    }

    Ok(PipelineTempCachePrune {
        removed_entries,
        removed_bytes,
        remaining_bytes: total_bytes,
    })
}

fn pipeline_temp_cache_budget_bytes() -> u64 {
    std::env::var(PIPELINE_TEMP_CACHE_BUDGET_MB_ENV)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(|mb| mb.saturating_mul(1024 * 1024))
        .unwrap_or(DEFAULT_PIPELINE_TEMP_CACHE_BUDGET_BYTES)
}

fn collect_pipeline_temp_cache_entries(
    paths: &AppPaths,
) -> anyhow::Result<Vec<PipelineTempCacheEntry>> {
    let mut entries = Vec::new();
    collect_audio_temp_entries(paths, &mut entries)?;
    collect_orphan_frame_dir_entries(paths, &mut entries)?;
    Ok(entries)
}

fn collect_audio_temp_entries(
    paths: &AppPaths,
    entries: &mut Vec<PipelineTempCacheEntry>,
) -> anyhow::Result<()> {
    let audio_dir = paths.cache.join("pipeline").join("audio");
    if !audio_dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(audio_dir)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = entry.metadata()?;
        if !metadata.is_file() {
            continue;
        }
        entries.push(PipelineTempCacheEntry {
            path,
            bytes: metadata.len(),
            modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            kind: PipelineTempCacheEntryKind::File,
        });
    }
    Ok(())
}

fn collect_orphan_frame_dir_entries(
    paths: &AppPaths,
    entries: &mut Vec<PipelineTempCacheEntry>,
) -> anyhow::Result<()> {
    let frames_root = paths.cache.join("pipeline").join("frames");
    if !frames_root.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(frames_root)? {
        let entry = entry?;
        let path = entry.path();
        if !entry.metadata()?.is_dir() || frame_dir_has_referenced_chunks(paths, &path)? {
            continue;
        }
        entries.push(PipelineTempCacheEntry {
            bytes: path_size(&path)?,
            modified: entry
                .metadata()?
                .modified()
                .unwrap_or(SystemTime::UNIX_EPOCH),
            kind: PipelineTempCacheEntryKind::Dir,
            path,
        });
    }
    Ok(())
}

fn frame_dir_has_referenced_chunks(paths: &AppPaths, dir: &Path) -> anyhow::Result<bool> {
    let mut prefix = dir.to_string_lossy().to_string();
    if !prefix.ends_with(std::path::MAIN_SEPARATOR) {
        prefix.push(std::path::MAIN_SEPARATOR);
    }
    let like = format!("{}%", escape_sql_like(&prefix));
    let conn = cerul_storage::sqlite::open(paths)?;
    let count: i64 = conn.query_row(
        r#"
        SELECT COUNT(*)
        FROM chunks
        WHERE frame_path LIKE ?1 ESCAPE '\'
        LIMIT 1
        "#,
        [like],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn escape_sql_like(value: &str) -> String {
    value
        .replace('\\', r"\\")
        .replace('%', r"\%")
        .replace('_', r"\_")
}

fn path_size(path: &Path) -> anyhow::Result<u64> {
    let metadata = fs::metadata(path)?;
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    if !metadata.is_dir() {
        return Ok(0);
    }
    let mut bytes = 0u64;
    for entry in fs::read_dir(path)? {
        bytes = bytes.saturating_add(path_size(&entry?.path())?);
    }
    Ok(bytes)
}

async fn remove_file_if_exists(path: &Path) -> anyhow::Result<()> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

async fn remove_dir_if_exists(path: &Path) -> anyhow::Result<()> {
    match tokio::fs::remove_dir_all(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn cache_key(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

async fn update_item_duration_from_media(paths: &AppPaths, item_id: &str, media_path: &Path) {
    match ffmpeg::media_duration(media_path).await {
        Ok(duration_sec) => {
            if let Err(error) = cerul_storage::set_item_duration(paths, item_id, duration_sec) {
                tracing::warn!(%error, item_id, "failed to store media duration");
            }
        }
        Err(error) => {
            tracing::warn!(%error, item_id, "failed to read media duration");
        }
    }
}

fn keyframe_chunks(frames: &[PathBuf], interval_sec: u32) -> Vec<StorageImageChunk> {
    let interval = f64::from(interval_sec.max(1));
    frames
        .iter()
        .enumerate()
        .map(|(index, frame)| {
            let start = frame_index(frame).unwrap_or(index) as f64 * interval;
            StorageImageChunk::keyframe_at(frame.clone(), start, start + interval)
        })
        .collect()
}

fn transcript_storage_from_segments(
    segments: &[Segment],
    window_sec: f64,
    overlap_sec: f64,
) -> TranscriptStorage {
    let lines = segments
        .iter()
        .filter_map(|segment| {
            let text = segment.text.trim();
            if text.is_empty() {
                return None;
            }
            Some(StorageTranscriptLine {
                start: segment.start,
                end: segment.end,
                text: text.to_string(),
            })
        })
        .collect::<Vec<_>>();
    let chunks = chunking::chunk_segments(segments, window_sec, overlap_sec)
        .into_iter()
        .map(|chunk| StorageTranscriptChunk {
            start: chunk.start,
            end: chunk.end,
            text: chunk.text,
        })
        .collect();

    TranscriptStorage { chunks, lines }
}

fn frame_index(path: &Path) -> Option<usize> {
    let stem = path.file_stem()?.to_str()?;
    let raw = stem.strip_prefix("frame_")?;
    raw.parse::<usize>().ok()?.checked_sub(1)
}

fn source_config_with_app_cache(
    paths: &AppPaths,
    source_type: &str,
    config: serde_json::Value,
) -> serde_json::Value {
    if !matches!(source_type, "youtube" | "web_video" | "rss_podcast") {
        return config;
    }

    let mut object = match config {
        serde_json::Value::Object(object) => object,
        other => return other,
    };
    object.entry("cache_dir").or_insert_with(|| {
        serde_json::Value::String(
            source_download_dir(paths, source_type)
                .to_string_lossy()
                .into_owned(),
        )
    });
    apply_ytdlp_access_settings(paths, source_type, &mut object);
    serde_json::Value::Object(object)
}

// Resolve where a source's downloaded media is written. Defaults to the app
// cache (`<data>/cache/sources/<type>`), but honors a user-chosen download
// directory (Settings → Storage, persisted as the `media_dir` setting) so large
// video files can live on an external disk. The setting is read per fetch, so a
// change takes effect for the next download without a restart; the lookup is a
// cached single-row query. Models, the database and the vector index are never
// relocated by this.
fn source_download_dir(paths: &AppPaths, source_type: &str) -> PathBuf {
    match cerul_storage::read_string_setting(paths, "media_dir") {
        Ok(Some(dir)) => Path::new(&dir).join("sources").join(source_type),
        Ok(None) => paths.source_cache_dir(source_type),
        Err(error) => {
            tracing::warn!(%error, "failed to read media_dir setting; using default cache dir");
            paths.source_cache_dir(source_type)
        }
    }
}

fn apply_ytdlp_access_settings(
    paths: &AppPaths,
    source_type: &str,
    object: &mut Map<String, Value>,
) {
    if !matches!(source_type, "youtube" | "web_video") || has_source_cookie_config(object) {
        return;
    }

    let mode = setting_string(paths, WEB_VIDEO_COOKIE_MODE_SETTING)
        .unwrap_or_else(|| "browser".to_string())
        .trim()
        .to_ascii_lowercase();
    match mode.as_str() {
        "browser" => {
            let browser = setting_string(paths, WEB_VIDEO_COOKIE_BROWSER_SETTING)
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
            if let Some(path) = setting_string(paths, WEB_VIDEO_COOKIES_PATH_SETTING) {
                let path = path.trim();
                if !path.is_empty() {
                    object.insert("cookies_path".to_string(), Value::String(path.to_string()));
                }
            }
        }
        _ => {}
    }
}

fn has_source_cookie_config(object: &Map<String, Value>) -> bool {
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

fn setting_string(paths: &AppPaths, key: &str) -> Option<String> {
    let conn = cerul_storage::sqlite::open(paths).ok()?;
    let raw: String = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .ok()?;
    match serde_json::from_str::<Value>(&raw).unwrap_or(Value::String(raw)) {
        Value::String(value) => Some(value),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn read_exif_metadata(path: &Path) -> anyhow::Result<serde_json::Value> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let exif = match exif::Reader::new().read_from_container(&mut reader) {
        Ok(exif) => exif,
        Err(error) => {
            return Ok(json!({
                "exif": {},
                "exif_error": error.to_string(),
            }));
        }
    };
    let mut fields = Map::new();

    for field in exif.fields() {
        fields.insert(
            format!("{:?}.{:?}", field.ifd_num, field.tag),
            json!(field.display_value().with_unit(&exif).to_string()),
        );
    }

    Ok(json!({ "exif": fields }))
}

fn set_embedding_index_status(
    paths: &AppPaths,
    item_id: &str,
    status: &str,
    error: Option<&str>,
    text_vectors: usize,
    image_vectors: usize,
) -> anyhow::Result<()> {
    cerul_storage::update_item_metadata(paths, item_id, |metadata| {
        metadata.insert(
            "embedding_index_status".to_string(),
            serde_json::Value::String(status.to_string()),
        );
        metadata.insert(
            "embedding_text_vectors".to_string(),
            serde_json::Value::from(text_vectors as u64),
        );
        metadata.insert(
            "embedding_image_vectors".to_string(),
            serde_json::Value::from(image_vectors as u64),
        );
        match error {
            Some(error) => {
                metadata.insert(
                    "embedding_index_error".to_string(),
                    serde_json::Value::String(error.to_string()),
                );
            }
            None => {
                metadata.remove("embedding_index_error");
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;
    use cerul_storage::{sqlite, vectors};
    use image::{Rgb, RgbImage};
    use std::sync::Mutex;
    use tokio::process::Command;

    #[test]
    fn keyframe_chunks_preserve_sampled_frame_timestamps() {
        let frames = vec![
            PathBuf::from("/tmp/frame_000001.jpg"),
            PathBuf::from("/tmp/frame_000004.jpg"),
        ];

        let chunks = keyframe_chunks(&frames, 5);

        assert_eq!(chunks[0].start_sec, Some(0.0));
        assert_eq!(chunks[0].end_sec, Some(5.0));
        assert_eq!(chunks[1].start_sec, Some(15.0));
        assert_eq!(chunks[1].end_sec, Some(20.0));
    }

    struct FakeTranscriber;

    impl Transcriber for FakeTranscriber {
        fn transcribe(
            &self,
            _audio_path: &Path,
            progress: Option<TranscriptionProgress>,
        ) -> anyhow::Result<Vec<Segment>> {
            if let Some(progress) = progress {
                progress(100);
            }
            Ok(vec![
                Segment {
                    start: 0.0,
                    end: 4.0,
                    text: "red square introduction".to_string(),
                },
                Segment {
                    start: 4.0,
                    end: 8.0,
                    text: "green square middle".to_string(),
                },
                Segment {
                    start: 8.0,
                    end: 10.0,
                    text: "blue square ending".to_string(),
                },
            ])
        }
    }

    struct UnexpectedTranscriber;

    impl Transcriber for UnexpectedTranscriber {
        fn transcribe(
            &self,
            _audio_path: &Path,
            _progress: Option<TranscriptionProgress>,
        ) -> anyhow::Result<Vec<Segment>> {
            unreachable!("transcription must be skipped for a video with no audio track")
        }
    }

    struct FakeEmbedder;

    impl Embedder for FakeEmbedder {
        fn embed_texts(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
            Ok(texts
                .iter()
                .enumerate()
                .map(|(index, _)| fake_vector(index))
                .collect())
        }

        fn embed_images(&self, paths: &[PathBuf]) -> anyhow::Result<Vec<Vec<f32>>> {
            Ok(paths
                .iter()
                .enumerate()
                .map(|(index, _)| fake_vector(index + 100))
                .collect())
        }
    }

    struct FailingImageEmbedder;

    impl Embedder for FailingImageEmbedder {
        fn embed_texts(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
            Ok(texts
                .iter()
                .enumerate()
                .map(|(index, _)| fake_vector(index))
                .collect())
        }

        fn embed_images(&self, _paths: &[PathBuf]) -> anyhow::Result<Vec<Vec<f32>>> {
            anyhow::bail!("Image token span mismatch: prompt has 496, preprocessor expects 880")
        }

        fn inference_provider(&self) -> Option<InferenceProviderInfo> {
            Some(fake_embedding_provider_info())
        }
    }

    struct FailingTextEmbedder;

    impl Embedder for FailingTextEmbedder {
        fn embed_texts(&self, _texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
            anyhow::bail!("Gemini Embedding 2 quota exceeded")
        }

        fn embed_images(&self, _paths: &[PathBuf]) -> anyhow::Result<Vec<Vec<f32>>> {
            unreachable!("image embedding should not run after text embedding failure")
        }

        fn inference_provider(&self) -> Option<InferenceProviderInfo> {
            Some(fake_embedding_provider_info())
        }
    }

    struct FakeOcrEngine;

    impl OcrEngine for FakeOcrEngine {
        fn ocr_images(&self, paths: &[PathBuf]) -> anyhow::Result<Vec<OcrFrame>> {
            Ok(paths
                .iter()
                .enumerate()
                .map(|(index, path)| OcrFrame {
                    path: path.clone(),
                    text: format!("visible frame text {index}"),
                })
                .collect())
        }
    }

    struct FailingOcrEngine;

    impl OcrEngine for FailingOcrEngine {
        fn ocr_images(&self, _paths: &[PathBuf]) -> anyhow::Result<Vec<OcrFrame>> {
            anyhow::bail!("OCR sidecar unavailable")
        }
    }

    struct FailingSqliteRewriteOcrEngine {
        paths: AppPaths,
    }

    impl OcrEngine for FailingSqliteRewriteOcrEngine {
        fn ocr_images(&self, paths: &[PathBuf]) -> anyhow::Result<Vec<OcrFrame>> {
            let conn = sqlite::open(&self.paths)?;
            conn.execute_batch(
                r#"
                CREATE TRIGGER IF NOT EXISTS fail_final_ocr_rewrite
                BEFORE INSERT ON chunks
                WHEN NEW.chunk_type = 'ocr'
                BEGIN
                    SELECT RAISE(FAIL, 'injected sqlite rewrite failure');
                END;
                "#,
            )?;
            Ok(paths
                .iter()
                .enumerate()
                .map(|(index, path)| OcrFrame {
                    path: path.clone(),
                    text: format!("rewrite failure frame text {index}"),
                })
                .collect())
        }
    }

    #[derive(Default)]
    struct RecordingProgress {
        stages: Mutex<Vec<(String, f64)>>,
    }

    impl PipelineProgress for RecordingProgress {
        fn update(&self, _item_id: &str, stage: &'static str, progress: f64, _message: &str) {
            self.stages
                .lock()
                .unwrap()
                .push((stage.to_string(), progress));
        }
    }

    #[test]
    fn remote_sources_default_to_app_cache() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path().join("app")).unwrap();

        for source_type in ["youtube", "web_video"] {
            let config = source_config_with_app_cache(
                &paths,
                source_type,
                serde_json::json!({ "url": "u" }),
            );
            let expected = paths
                .cache
                .join("sources")
                .join(source_type)
                .to_string_lossy()
                .into_owned();

            assert_eq!(config["cache_dir"].as_str(), Some(expected.as_str()));
        }
    }

    #[test]
    fn remote_sources_honor_media_dir_setting() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path().join("app")).unwrap();
        let media = temp.path().join("external-media");
        let conn = sqlite::open(&paths).unwrap();
        conn.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES ('media_dir', ?1, strftime('%s','now'))",
            [serde_json::Value::String(media.to_string_lossy().into_owned()).to_string()],
        )
        .unwrap();

        let config =
            source_config_with_app_cache(&paths, "web_video", serde_json::json!({ "url": "u" }));
        let expected = media
            .join("sources")
            .join("web_video")
            .to_string_lossy()
            .into_owned();

        assert_eq!(config["cache_dir"].as_str(), Some(expected.as_str()));
    }

    #[test]
    fn web_video_source_config_uses_browser_cookies_setting() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path().join("app")).unwrap();
        let conn = sqlite::open(&paths).unwrap();
        conn.execute(
            r#"
            INSERT INTO settings (key, value, updated_at)
            VALUES
              ('web_video_cookie_mode', '"browser"', strftime('%s','now')),
              ('web_video_cookie_browser', '"chrome:Default"', strftime('%s','now'))
            "#,
            [],
        )
        .unwrap();

        let config = source_config_with_app_cache(
            &paths,
            "web_video",
            serde_json::json!({ "url": "https://www.youtube.com/watch?v=abc123" }),
        );

        assert_eq!(
            config["cookies_from_browser"].as_str(),
            Some("chrome:Default")
        );
    }

    #[test]
    fn web_video_source_config_defaults_to_browser_cookies() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path().join("app")).unwrap();

        let config = source_config_with_app_cache(
            &paths,
            "web_video",
            serde_json::json!({ "url": "https://space.bilibili.com/123456" }),
        );

        assert_eq!(config["cookies_from_browser"].as_str(), Some("chrome"));
    }

    #[test]
    fn remote_source_custom_cache_is_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path().join("app")).unwrap();
        let custom = temp.path().join("custom-cache");
        let expected = custom.to_string_lossy().into_owned();

        let config = source_config_with_app_cache(
            &paths,
            "rss_podcast",
            serde_json::json!({ "url": "u", "cache_dir": custom }),
        );

        assert_eq!(config["cache_dir"].as_str(), Some(expected.as_str()));
    }

    #[tokio::test]
    async fn prune_pipeline_temp_cache_removes_audio_and_orphan_frames_only() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path().join("app")).unwrap();
        let audio_dir = paths.cache.join("pipeline").join("audio");
        let frames_root = paths.cache.join("pipeline").join("frames");
        let referenced_dir = frames_root.join("referenced");
        let orphan_dir = frames_root.join("orphan");
        std::fs::create_dir_all(&audio_dir).unwrap();
        std::fs::create_dir_all(&referenced_dir).unwrap();
        std::fs::create_dir_all(&orphan_dir).unwrap();
        let audio = audio_dir.join("old.wav");
        let referenced_frame = referenced_dir.join("frame.jpg");
        let orphan_frame = orphan_dir.join("frame.jpg");
        std::fs::write(&audio, b"audio-temp").unwrap();
        std::fs::write(&referenced_frame, b"keep-frame").unwrap();
        std::fs::write(&orphan_frame, b"drop-frame").unwrap();

        let conn = sqlite::open(&paths).unwrap();
        conn.execute(
            "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'folder_video', '{}', 'active')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO items (id, source_id, content_type, status, metadata) VALUES ('item-1', 'source-1', 'video', 'indexed', '{}')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chunks (id, item_id, chunk_type, frame_path, metadata) VALUES ('frame-1', 'item-1', 'keyframe', ?1, '{}')",
            [referenced_frame.to_string_lossy().as_ref()],
        )
        .unwrap();
        drop(conn);

        let pruned = prune_pipeline_temp_cache(&paths, 0).await.unwrap();

        assert!(pruned.removed_entries >= 2);
        assert!(!audio.exists());
        assert!(!orphan_dir.exists());
        assert!(referenced_dir.exists());
        assert!(referenced_frame.exists());
    }

    #[tokio::test]
    async fn process_video_item_writes_sqlite_and_qdrant() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path().join("app")).unwrap();
        let videos = temp.path().join("videos");
        std::fs::create_dir(&videos).unwrap();
        let video = videos.join("sample.mp4");
        create_sample_video(&video).await.unwrap();
        insert_source_and_item(&paths, &videos, &video);

        let progress = Arc::new(RecordingProgress::default());
        let pipeline = VideoPipeline::new(
            paths.clone(),
            Arc::new(FakeTranscriber),
            Arc::new(FakeEmbedder),
        )
        .with_progress(progress.clone())
        .with_frame_interval_sec(2);
        let summary = pipeline.process_video_item("item-1").await.unwrap();

        assert_eq!(summary.transcript_chunks, 1);
        assert_eq!(summary.text_vectors, 1);
        assert!(summary.sampled_frames > 0);
        assert_eq!(summary.image_vectors, 1);
        assert!(!summary.audio_path.exists());

        let conn = sqlite::open(&paths).unwrap();
        let status: String = conn
            .query_row("SELECT status FROM items WHERE id = 'item-1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(status, "indexed");

        let total_chunk_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE item_id = 'item-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let chunk_count = |chunk_type: &str| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM chunks WHERE item_id = 'item-1' AND chunk_type = ?1",
                [chunk_type],
                |row| row.get(0),
            )
            .unwrap()
        };
        let transcript_count = chunk_count("transcript");
        let transcript_line_count = chunk_count("transcript_line");
        let keyframe_count = chunk_count("keyframe");
        let ocr_count = chunk_count("ocr");
        assert_eq!(transcript_count, summary.transcript_chunks as i64);
        assert!(transcript_line_count > 0);
        assert_eq!(keyframe_count, summary.sampled_frames as i64);
        assert_eq!(ocr_count, 0);
        assert_eq!(
            total_chunk_count,
            transcript_count + transcript_line_count + keyframe_count + ocr_count
        );

        assert_eq!(
            retrieval_unit_count_for_item(&paths, "item-1"),
            summary.text_vectors as i64
        );
        assert_eq!(
            unified_point_count(&paths).await,
            summary.text_vectors + summary.image_vectors
        );
        let stages = progress.stages.lock().unwrap();
        let preparing_index = stages
            .iter()
            .position(|(stage, progress)| {
                stage == "preparing_models" && (*progress - 0.23).abs() < f64::EPSILON
            })
            .expect("video indexing should report model preparation before transcription");
        let transcribing_index = stages
            .iter()
            .position(|(stage, progress)| {
                stage == "transcribing" && *progress >= 0.25 && *progress <= 0.60
            })
            .expect("video indexing should report transcription progress");
        assert!(preparing_index < transcribing_index);
        assert!(stages.iter().any(|(stage, progress)| {
            stage == "transcribing" && *progress >= 0.25 && *progress <= 0.60
        }));
        assert!(stages.iter().any(
            |(stage, progress)| stage == "completed" && (*progress - 1.0).abs() < f64::EPSILON
        ));
    }

    #[tokio::test]
    async fn process_video_item_indexes_video_without_audio() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path().join("app")).unwrap();
        let videos = temp.path().join("videos");
        std::fs::create_dir(&videos).unwrap();
        let video = videos.join("silent.mp4");
        create_silent_video(&video).await.unwrap();
        insert_source_and_item(&paths, &videos, &video);

        // UnexpectedTranscriber panics if invoked — proving transcription is
        // skipped entirely when there's no audio track to transcribe.
        let pipeline = VideoPipeline::new(
            paths.clone(),
            Arc::new(UnexpectedTranscriber),
            Arc::new(FakeEmbedder),
        )
        .with_frame_interval_sec(2);
        let summary = pipeline.process_video_item("item-1").await.unwrap();

        // No transcript, but frames are still sampled and embedded as image-only retrieval units.
        assert_eq!(summary.transcript_chunks, 0);
        assert_eq!(summary.text_vectors, 0);
        assert!(summary.sampled_frames > 0);
        assert_eq!(summary.image_vectors, summary.sampled_frames);

        let conn = sqlite::open(&paths).unwrap();
        let status: String = conn
            .query_row("SELECT status FROM items WHERE id = 'item-1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(status, "indexed");

        let metadata: String = conn
            .query_row(
                "SELECT metadata FROM items WHERE id = 'item-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let metadata: serde_json::Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(metadata["has_audio"].as_bool(), Some(false));

        let keyframe_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE item_id = 'item-1' AND chunk_type = 'keyframe'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(keyframe_count, summary.image_vectors as i64);
    }

    #[tokio::test]
    async fn process_video_item_writes_ocr_chunks_when_ocr_is_configured() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path().join("app")).unwrap();
        let videos = temp.path().join("videos");
        std::fs::create_dir(&videos).unwrap();
        let video = videos.join("sample.mp4");
        create_sample_video(&video).await.unwrap();
        insert_source_and_item(&paths, &videos, &video);

        let pipeline = VideoPipeline::new(
            paths.clone(),
            Arc::new(FakeTranscriber),
            Arc::new(FakeEmbedder),
        )
        .with_ocr(Arc::new(FakeOcrEngine))
        .with_frame_interval_sec(2);
        let summary = pipeline.process_video_item("item-1").await.unwrap();

        assert!(summary.sampled_frames > 0);
        assert_eq!(summary.ocr_chunks, summary.sampled_frames);
        assert!(summary.text_vectors >= 1);
        assert!(summary.image_vectors >= 1);
        assert_eq!(
            retrieval_unit_count_for_item(&paths, "item-1"),
            summary.text_vectors as i64
        );

        let conn = sqlite::open(&paths).unwrap();
        let ocr_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE item_id = 'item-1' AND chunk_type = 'ocr'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(ocr_count, summary.ocr_chunks as i64);

        let ocr_text: String = conn
            .query_row(
                "SELECT text FROM chunks WHERE chunk_type = 'ocr' ORDER BY id LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(ocr_text.contains("visible frame text"));

        let metadata: String = conn
            .query_row(
                "SELECT metadata FROM items WHERE id = 'item-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let metadata: serde_json::Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(metadata["ocr_index_status"], "indexed");
        assert_eq!(metadata["ocr_indexed_chunks"], summary.ocr_chunks as u64);
    }

    #[tokio::test]
    async fn process_video_item_keeps_transcript_searchable_when_ocr_fails_after_transcript_first()
    {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path().join("app")).unwrap();
        let videos = temp.path().join("videos");
        std::fs::create_dir(&videos).unwrap();
        let video = videos.join("sample.mp4");
        create_sample_video(&video).await.unwrap();
        insert_source_and_item(&paths, &videos, &video);

        let pipeline = VideoPipeline::new(
            paths.clone(),
            Arc::new(FakeTranscriber),
            Arc::new(FakeEmbedder),
        )
        .with_ocr(Arc::new(FailingOcrEngine))
        .with_frame_interval_sec(2)
        .with_transcript_first_indexing(true);
        let summary = pipeline.process_video_item("item-1").await.unwrap();

        assert_eq!(summary.ocr_chunks, 0);
        assert!(summary.text_vectors >= 1);

        let conn = sqlite::open(&paths).unwrap();
        let (
            status,
            metadata,
            search_index_status,
            search_index_unit_count,
            search_index_vector_count,
        ): (String, String, String, i64, i64) = conn
            .query_row(
                r#"
                SELECT status, metadata, search_index_status, search_index_unit_count, search_index_vector_count
                FROM items
                WHERE id = 'item-1'
                "#,
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        let metadata: serde_json::Value = serde_json::from_str(&metadata).unwrap();

        assert_eq!(status, "indexed");
        assert_eq!(search_index_status, "indexed");
        assert!(search_index_unit_count > 0);
        assert!(search_index_vector_count > 0);
        assert_eq!(metadata["ocr_index_status"], "failed");
        assert_eq!(metadata["ocr_indexed_chunks"], 0);
        assert!(metadata["ocr_index_error"]
            .as_str()
            .unwrap()
            .contains("OCR sidecar unavailable"));

        let hits = cerul_search::search_fts_only(
            &paths,
            cerul_search::SearchRequest {
                q: "red square introduction".to_string(),
                limit: 3,
            },
        )
        .await
        .unwrap();
        assert_eq!(hits.first().map(|hit| hit.item_id.as_str()), Some("item-1"));
    }

    #[tokio::test]
    async fn process_video_item_keeps_transcript_searchable_when_sqlite_rewrite_fails_after_transcript_first(
    ) {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path().join("app")).unwrap();
        let videos = temp.path().join("videos");
        std::fs::create_dir(&videos).unwrap();
        let video = videos.join("sample.mp4");
        create_sample_video(&video).await.unwrap();
        insert_source_and_item(&paths, &videos, &video);

        let pipeline = VideoPipeline::new(
            paths.clone(),
            Arc::new(FakeTranscriber),
            Arc::new(FakeEmbedder),
        )
        .with_ocr(Arc::new(FailingSqliteRewriteOcrEngine {
            paths: paths.clone(),
        }))
        .with_frame_interval_sec(2)
        .with_transcript_first_indexing(true);
        let summary = pipeline.process_video_item("item-1").await.unwrap();

        assert_eq!(summary.ocr_chunks, 0);
        assert_eq!(summary.image_vectors, 0);
        assert!(summary.text_vectors >= 1);

        let conn = sqlite::open(&paths).unwrap();
        let (
            status,
            metadata,
            search_index_status,
            search_index_unit_count,
            search_index_vector_count,
        ): (String, String, String, i64, i64) = conn
            .query_row(
                r#"
                SELECT status, metadata, search_index_status, search_index_unit_count, search_index_vector_count
                FROM items
                WHERE id = 'item-1'
                "#,
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        let metadata: serde_json::Value = serde_json::from_str(&metadata).unwrap();
        let ocr_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE item_id = 'item-1' AND chunk_type = 'ocr'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(status, "indexed");
        assert_eq!(search_index_status, "indexed");
        assert!(search_index_unit_count > 0);
        assert!(search_index_vector_count > 0);
        assert_eq!(ocr_count, 0);
        assert_eq!(metadata["visual_index_status"], "display_only");
        assert!(metadata["visual_index_error"]
            .as_str()
            .unwrap()
            .contains("injected sqlite rewrite failure"));
        assert_eq!(metadata["ocr_index_status"], "failed");
        assert!(metadata["ocr_index_error"]
            .as_str()
            .unwrap()
            .contains("injected sqlite rewrite failure"));

        let hits = cerul_search::search_fts_only(
            &paths,
            cerul_search::SearchRequest {
                q: "red square introduction".to_string(),
                limit: 3,
            },
        )
        .await
        .unwrap();
        assert_eq!(hits.first().map(|hit| hit.item_id.as_str()), Some("item-1"));
    }

    #[tokio::test]
    async fn process_video_item_marks_visual_frames_display_only() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path().join("app")).unwrap();
        let videos = temp.path().join("videos");
        std::fs::create_dir(&videos).unwrap();
        let video = videos.join("sample.mp4");
        create_sample_video(&video).await.unwrap();
        insert_source_and_item(&paths, &videos, &video);

        let pipeline = VideoPipeline::new(
            paths.clone(),
            Arc::new(FakeTranscriber),
            Arc::new(FailingImageEmbedder),
        )
        .with_frame_interval_sec(2);
        let summary = pipeline.process_video_item("item-1").await.unwrap();

        assert_eq!(summary.transcript_chunks, 1);
        assert_eq!(summary.text_vectors, 1);
        assert!(summary.sampled_frames > 0);
        assert_eq!(summary.image_vectors, 0);

        let conn = sqlite::open(&paths).unwrap();
        let (status, metadata): (String, String) = conn
            .query_row(
                "SELECT status, metadata FROM items WHERE id = 'item-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let metadata: serde_json::Value = serde_json::from_str(&metadata).unwrap();

        assert_eq!(status, "indexed");
        assert_eq!(metadata["transcript_index_status"], "indexed");
        assert_eq!(metadata["visual_index_status"], "display_only");
        assert_eq!(metadata["visual_indexed_frames"], 0);
        assert!(metadata["visual_sampled_frames"].as_u64().unwrap() > 0);
        assert!(metadata["visual_index_error"].is_null());
        let failed_image_usage: i64 = conn
            .query_row(
                r#"
                SELECT COUNT(*)
                FROM inference_usage_events
                WHERE item_id = 'item-1'
                  AND capability = 'embedding_image'
                  AND status = 'failed'
                "#,
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(failed_image_usage, 1);
    }

    #[tokio::test]
    async fn process_image_item_keeps_chunk_when_embedding_fails() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path().join("app")).unwrap();
        let images = temp.path().join("images");
        std::fs::create_dir(&images).unwrap();
        let image = images.join("photo.jpg");
        write_color_image(&image, [40, 120, 200]);
        insert_source(&paths, "image-source", "folder_image", &images);
        insert_item(&paths, "image-1", "image-source", "image", "photo", &image);

        let pipeline = VideoPipeline::new(
            paths.clone(),
            Arc::new(FakeTranscriber),
            Arc::new(FailingImageEmbedder),
        );
        let summary = pipeline.process_image_item("image-1").await.unwrap();

        assert_eq!(summary.image_chunks, 1);
        assert_eq!(summary.image_vectors, 0);
        let conn = sqlite::open(&paths).unwrap();
        let (status, metadata, search_index_status, search_index_unit_count, search_index_vector_count): (
            String,
            String,
            String,
            i64,
            i64,
        ) = conn
            .query_row(
                r#"
                SELECT status, metadata, search_index_status, search_index_unit_count, search_index_vector_count
                FROM items
                WHERE id = 'image-1'
                "#,
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        let image_chunks: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE item_id = 'image-1' AND chunk_type = 'image'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let metadata: serde_json::Value = serde_json::from_str(&metadata).unwrap();

        assert_eq!(status, "indexed");
        assert_eq!(image_chunks, 1);
        assert_eq!(search_index_status, "indexed");
        assert_eq!(search_index_unit_count, 1);
        assert_eq!(search_index_vector_count, 0);
        assert_eq!(metadata["embedding_index_status"], "failed");
        assert!(metadata["embedding_index_error"]
            .as_str()
            .unwrap()
            .contains("Image token span mismatch"));
        let failed_image_usage: i64 = conn
            .query_row(
                r#"
                SELECT COUNT(*)
                FROM inference_usage_events
                WHERE item_id = 'image-1'
                  AND capability = 'embedding_image'
                  AND status = 'failed'
                "#,
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(failed_image_usage, 1);

        let hits = cerul_search::search_fts_only(
            &paths,
            cerul_search::SearchRequest {
                q: "photo".to_string(),
                limit: 3,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            hits.first().map(|hit| hit.item_id.as_str()),
            Some("image-1")
        );
    }

    #[tokio::test]
    async fn process_video_item_keeps_transcript_searchable_when_vector_write_fails() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path().join("app")).unwrap();
        let videos = temp.path().join("videos");
        std::fs::create_dir(&videos).unwrap();
        let video = videos.join("sample.mp4");
        create_sample_video(&video).await.unwrap();
        insert_source_and_item(&paths, &videos, &video);

        let pipeline = VideoPipeline::new(
            paths.clone(),
            Arc::new(FakeTranscriber),
            Arc::new(FakeEmbedder),
        )
        .with_embedding_profile(bad_dimension_profile(&paths))
        .with_frame_interval_sec(2);
        let summary = pipeline.process_video_item("item-1").await.unwrap();

        assert_eq!(summary.transcript_chunks, 1);
        assert_eq!(summary.text_vectors, 0);
        assert_eq!(summary.image_vectors, 0);

        let conn = sqlite::open(&paths).unwrap();
        let (status, metadata): (String, String) = conn
            .query_row(
                "SELECT status, metadata FROM items WHERE id = 'item-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let metadata: serde_json::Value = serde_json::from_str(&metadata).unwrap();

        assert_eq!(status, "indexed");
        assert_eq!(metadata["embedding_index_status"], "failed");
        assert_eq!(metadata["visual_index_status"], "display_only");
        assert!(metadata["embedding_index_error"]
            .as_str()
            .unwrap()
            .contains("expected"));
        assert!(retrieval_unit_count_for_item(&paths, "item-1") > 0);
    }

    #[tokio::test]
    async fn process_image_item_keeps_chunk_when_vector_write_fails() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path().join("app")).unwrap();
        let images = temp.path().join("images");
        std::fs::create_dir(&images).unwrap();
        let image = images.join("photo.jpg");
        write_color_image(&image, [40, 120, 200]);
        insert_source(&paths, "image-source", "folder_image", &images);
        insert_item(&paths, "image-1", "image-source", "image", "photo", &image);

        let pipeline = VideoPipeline::new(
            paths.clone(),
            Arc::new(FakeTranscriber),
            Arc::new(FakeEmbedder),
        )
        .with_embedding_profile(bad_dimension_profile(&paths));
        let summary = pipeline.process_image_item("image-1").await.unwrap();

        assert_eq!(summary.image_chunks, 1);
        assert_eq!(summary.image_vectors, 0);

        let conn = sqlite::open(&paths).unwrap();
        let (status, metadata, search_index_status, search_index_unit_count, search_index_vector_count): (
            String,
            String,
            String,
            i64,
            i64,
        ) = conn
            .query_row(
                r#"
                SELECT status, metadata, search_index_status, search_index_unit_count, search_index_vector_count
                FROM items
                WHERE id = 'image-1'
                "#,
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        let image_chunks: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE item_id = 'image-1' AND chunk_type = 'image'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let metadata: serde_json::Value = serde_json::from_str(&metadata).unwrap();

        assert_eq!(status, "indexed");
        assert_eq!(image_chunks, 1);
        assert_eq!(search_index_status, "indexed");
        assert_eq!(search_index_unit_count, 1);
        assert_eq!(search_index_vector_count, 0);
        assert_eq!(metadata["embedding_index_status"], "failed");
        assert!(metadata["embedding_index_error"]
            .as_str()
            .unwrap()
            .contains("expected"));

        let hits = cerul_search::search_fts_only(
            &paths,
            cerul_search::SearchRequest {
                q: "photo".to_string(),
                limit: 3,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            hits.first().map(|hit| hit.item_id.as_str()),
            Some("image-1")
        );
    }

    #[tokio::test]
    async fn retrieval_unit_rebuild_marks_search_index_pending_before_vector_write() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path().join("app")).unwrap();
        let images = temp.path().join("images");
        std::fs::create_dir(&images).unwrap();
        let image = images.join("photo.jpg");
        write_color_image(&image, [40, 120, 200]);
        insert_source(&paths, "image-source", "folder_image", &images);
        insert_item(&paths, "image-1", "image-source", "image", "photo", &image);
        cerul_storage::write_media_sqlite_chunks(
            &paths,
            "image-1",
            &[],
            &[StorageImageChunk::image(image.clone(), json!({}))],
        )
        .unwrap();
        cerul_storage::set_item_search_index_status(&paths, "image-1", "indexed", None, 1, 1)
            .unwrap();

        let pipeline = VideoPipeline::new(
            paths.clone(),
            Arc::new(FakeTranscriber),
            Arc::new(FakeEmbedder),
        )
        .with_embedding_profile(bad_dimension_profile(&paths));
        let error = pipeline
            .embed_and_write_retrieval_units("image-1", 0.0, 0.1, true, true)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("expected"));
        let conn = sqlite::open(&paths).unwrap();
        let state: (String, i64, i64) = conn
            .query_row(
                r#"
                SELECT search_index_status, search_index_unit_count, search_index_vector_count
                FROM items
                WHERE id = 'image-1'
                "#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(state, ("pending".to_string(), 0, 0));
    }

    #[tokio::test]
    async fn process_video_item_keeps_transcript_searchable_when_text_embedding_fails() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path().join("app")).unwrap();
        let videos = temp.path().join("videos");
        std::fs::create_dir(&videos).unwrap();
        let video = videos.join("sample.mp4");
        create_sample_video(&video).await.unwrap();
        insert_source_and_item(&paths, &videos, &video);

        let pipeline = VideoPipeline::new(
            paths.clone(),
            Arc::new(FakeTranscriber),
            Arc::new(FailingTextEmbedder),
        )
        .with_frame_interval_sec(2);
        let summary = pipeline.process_video_item("item-1").await.unwrap();

        assert_eq!(summary.transcript_chunks, 1);
        assert_eq!(summary.text_vectors, 0);
        assert_eq!(summary.image_vectors, 0);

        let conn = sqlite::open(&paths).unwrap();
        let (status, metadata): (String, String) = conn
            .query_row(
                "SELECT status, metadata FROM items WHERE id = 'item-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let metadata: serde_json::Value = serde_json::from_str(&metadata).unwrap();

        assert_eq!(status, "indexed");
        assert_eq!(metadata["embedding_index_status"], "failed");
        assert!(metadata["embedding_index_error"]
            .as_str()
            .unwrap()
            .contains("Gemini Embedding 2 quota exceeded"));
        let failed_text_usage: i64 = conn
            .query_row(
                r#"
                SELECT COUNT(*)
                FROM inference_usage_events
                WHERE item_id = 'item-1'
                  AND capability = 'embedding_text'
                  AND status = 'failed'
                "#,
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(failed_text_usage, 1);

        let hits = cerul_search::search_fts_only(
            &paths,
            cerul_search::SearchRequest {
                q: "red square introduction".to_string(),
                limit: 3,
            },
        )
        .await
        .unwrap();
        assert_eq!(hits.first().map(|hit| hit.item_id.as_str()), Some("item-1"));
    }

    #[tokio::test]
    #[ignore = "release smoke; run scripts/smoke-folder-happy-path.sh"]
    async fn folder_happy_path_smoke_indexes_video_and_finds_timestamp() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path().join("app")).unwrap();
        let videos = temp.path().join("videos");
        std::fs::create_dir(&videos).unwrap();
        let video = videos.join("sample.mp4");
        create_sample_video(&video).await.unwrap();
        insert_source_and_item(&paths, &videos, &video);

        let pipeline = VideoPipeline::new(
            paths.clone(),
            Arc::new(FakeTranscriber),
            Arc::new(FakeEmbedder),
        )
        .with_chunking(4.0, 0.0)
        .with_frame_interval_sec(2);
        let summary = pipeline.process_video_item("item-1").await.unwrap();

        assert_eq!(summary.item_id, "item-1");
        assert_eq!(summary.transcript_chunks, 3);
        assert_eq!(summary.text_vectors, 3);
        assert!(summary.sampled_frames > 0);

        let results = cerul_search::search_with_vector(
            &paths,
            cerul_search::SearchRequest {
                q: "blue square ending".to_string(),
                limit: 3,
            },
            fake_vector(2),
        )
        .await
        .unwrap();
        let top = results.first().expect("folder happy path search result");

        assert_eq!(top.item_id, "item-1");
        assert_eq!(top.start_sec, Some(8.0));
        assert!(
            top.snippet.contains("blue square ending"),
            "expected known phrase in top result, got {top:?}"
        );

        println!(
            "folder_happy_path_smoke item={} query=\"blue square ending\" timestamp={}s hits={}",
            top.item_id,
            top.start_sec.unwrap_or_default(),
            results.len()
        );
    }

    #[tokio::test]
    #[ignore = "runs real MLX sidecar models; run scripts/smoke-mlx-sidecar-pipeline.sh"]
    async fn mlx_sidecar_video_pipeline_smoke_indexes_video() {
        let sample_wav = std::env::var("CERUL_MLX_SMOKE_WAV")
            .context("CERUL_MLX_SMOKE_WAV is required")
            .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path().join("app")).unwrap();
        let videos = temp.path().join("videos");
        std::fs::create_dir(&videos).unwrap();
        let video = videos.join("mlx-sidecar-sample.mp4");
        create_video_with_audio(Path::new(&sample_wav), &video)
            .await
            .unwrap();
        insert_source_and_item(&paths, &videos, &video);

        let sidecar = Arc::new(crate::mlx_sidecar::MlxSidecar::for_paths(&paths).unwrap());
        let transcriber: Arc<dyn Transcriber> = sidecar.clone();
        let embedder: Arc<dyn Embedder> = sidecar.clone();
        let ocr: Arc<dyn OcrEngine> = sidecar;
        let pipeline = VideoPipeline::new(paths.clone(), transcriber, embedder)
            .with_ocr(ocr)
            .with_frame_interval_sec(4);

        let summary = pipeline.process_video_item("item-1").await.unwrap();

        assert!(summary.transcript_chunks > 0);
        assert!(summary.sampled_frames > 0);
        assert!(summary.text_vectors >= summary.transcript_chunks);
        assert!(summary.image_vectors > 0);

        let conn = sqlite::open(&paths).unwrap();
        let transcript_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE item_id = 'item-1' AND chunk_type = 'transcript'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let keyframe_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE item_id = 'item-1' AND chunk_type = 'keyframe'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let ocr_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE item_id = 'item-1' AND chunk_type = 'ocr'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(transcript_count as usize, summary.transcript_chunks);
        assert!(summary.image_vectors > 0);
        assert!(summary.image_vectors <= keyframe_count as usize);
        assert_eq!(ocr_count as usize, summary.ocr_chunks);

        println!(
            "mlx_sidecar_video_pipeline_smoke transcripts={} ocr={} image_vectors={}",
            summary.transcript_chunks, summary.ocr_chunks, summary.image_vectors
        );
    }

    #[tokio::test]
    async fn audio_image_smoke() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path().join("app")).unwrap();
        let audios = temp.path().join("audios");
        let images = temp.path().join("images");
        std::fs::create_dir(&audios).unwrap();
        std::fs::create_dir(&images).unwrap();
        insert_source(&paths, "audio-source", "folder_audio", &audios);
        insert_source(&paths, "image-source", "folder_image", &images);

        for index in 0..5 {
            let audio = audios.join(format!("episode-{index}.mp3"));
            let image = images.join(format!("photo-{index}.jpg"));
            create_sample_audio(&audio).await.unwrap();
            write_color_image(&image, [index as u8 * 30, 64, 200]);
            insert_item(
                &paths,
                &format!("audio-{index}"),
                "audio-source",
                "audio",
                &format!("episode-{index}"),
                &audio,
            );
            insert_item(
                &paths,
                &format!("image-{index}"),
                "image-source",
                "image",
                &format!("photo-{index}"),
                &image,
            );
        }

        let pipeline = VideoPipeline::new(
            paths.clone(),
            Arc::new(FakeTranscriber),
            Arc::new(FakeEmbedder),
        );

        for index in 0..5 {
            let audio = pipeline
                .process_audio_item(&format!("audio-{index}"))
                .await
                .unwrap();
            assert_eq!(audio.transcript_chunks, 1);
            assert_eq!(audio.text_vectors, 1);
            assert!(!audio.audio_path.exists());

            let image = pipeline
                .process_image_item(&format!("image-{index}"))
                .await
                .unwrap();
            assert_eq!(image.image_chunks, 1);
            assert_eq!(image.image_vectors, 1);
            assert!(image.image_path.is_file());
        }

        assert_eq!(chunk_count(&paths, "transcript"), 5);
        assert_eq!(chunk_count(&paths, "image"), 5);

        let conn = sqlite::open(&paths).unwrap();
        let indexed_items: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM items WHERE status = 'indexed'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(indexed_items, 10);

        assert_eq!(retrieval_unit_count_for_item_prefix(&paths, "audio-"), 5);
        assert_eq!(retrieval_unit_count_for_item_prefix(&paths, "image-"), 5);
        assert_eq!(unified_point_count(&paths).await, 10);
    }

    fn insert_source_and_item(paths: &AppPaths, videos: &Path, video: &Path) {
        insert_source(paths, "source-1", "folder_video", videos);
        insert_item(paths, "item-1", "source-1", "video", "sample-video", video);
    }

    fn insert_source(paths: &AppPaths, source_id: &str, source_type: &str, source_path: &Path) {
        let conn = sqlite::open(paths).unwrap();
        let config = serde_json::json!({ "path": source_path }).to_string();

        conn.execute(
            "INSERT INTO sources (id, type, config, status) VALUES (?1, ?2, ?3, ?4)",
            (source_id, source_type, config, "active"),
        )
        .unwrap();
    }

    fn insert_item(
        paths: &AppPaths,
        item_id: &str,
        source_id: &str,
        content_type: &str,
        external_id: &str,
        raw_path: &Path,
    ) {
        let conn = sqlite::open(paths).unwrap();
        let raw_path = raw_path.to_string_lossy().into_owned();
        let metadata = serde_json::json!({ "raw_path": raw_path.clone() }).to_string();
        let title = Path::new(&raw_path)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(item_id)
            .to_string();

        conn.execute(
            r#"
            INSERT INTO items (
                id,
                source_id,
                content_type,
                external_id,
                title,
                raw_path,
                status,
                metadata
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            (
                item_id,
                source_id,
                content_type,
                external_id,
                title,
                raw_path.as_str(),
                "ready",
                metadata,
            ),
        )
        .unwrap();
    }

    async fn create_sample_video(path: &Path) -> anyhow::Result<()> {
        let output = Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=10:size=64x64:rate=10",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=1000:duration=10",
                "-shortest",
                "-c:v",
                "mpeg4",
                "-c:a",
                "aac",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(path)
            .output()
            .await?;

        if !output.status.success() {
            anyhow::bail!(
                "ffmpeg sample video generation failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    async fn create_silent_video(path: &Path) -> anyhow::Result<()> {
        let output = Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=10:size=64x64:rate=10",
                "-c:v",
                "mpeg4",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(path)
            .output()
            .await?;

        if !output.status.success() {
            anyhow::bail!(
                "ffmpeg silent video generation failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    async fn create_sample_audio(path: &Path) -> anyhow::Result<()> {
        let output = Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-c:a",
                "libmp3lame",
                "-q:a",
                "4",
            ])
            .arg(path)
            .output()
            .await?;

        if !output.status.success() {
            anyhow::bail!(
                "ffmpeg sample audio generation failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    async fn create_video_with_audio(audio: &Path, out: &Path) -> anyhow::Result<()> {
        let output = Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=640x360:d=24,drawtext=fontfile=/System/Library/Fonts/Supplemental/Arial.ttf:text=CERUL:fontcolor=white:fontsize=96:x=(w-text_w)/2:y=(h-text_h)/2",
                "-i",
            ])
            .arg(audio)
            .args([
                "-c:v",
                "mpeg4",
                "-c:a",
                "aac",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(out)
            .output()
            .await?;

        if !output.status.success() {
            anyhow::bail!(
                "ffmpeg sample MLX smoke video generation failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    fn write_color_image(path: &Path, rgb: [u8; 3]) {
        let image = RgbImage::from_pixel(32, 32, Rgb(rgb));
        image.save(path).unwrap();
    }

    fn chunk_count(paths: &AppPaths, chunk_type: &str) -> i64 {
        let conn = sqlite::open(paths).unwrap();

        conn.query_row(
            "SELECT COUNT(*) FROM chunks WHERE chunk_type = ?1",
            [chunk_type],
            |row| row.get(0),
        )
        .unwrap()
    }

    fn retrieval_unit_count_for_item(paths: &AppPaths, item_id: &str) -> i64 {
        let conn = sqlite::open(paths).unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM retrieval_units WHERE item_id = ?1 AND index_version = ?2",
            (item_id, cerul_storage::SEARCH_INDEX_VERSION),
            |row| row.get(0),
        )
        .unwrap()
    }

    fn retrieval_unit_count_for_item_prefix(paths: &AppPaths, item_prefix: &str) -> i64 {
        let conn = sqlite::open(paths).unwrap();
        let pattern = format!("{item_prefix}%");
        conn.query_row(
            "SELECT COUNT(*) FROM retrieval_units WHERE item_id LIKE ?1 AND index_version = ?2",
            (pattern, cerul_storage::SEARCH_INDEX_VERSION),
            |row| row.get(0),
        )
        .unwrap()
    }

    async fn unified_point_count(paths: &AppPaths) -> usize {
        let profile = vectors::ensure_active_embedding_profile(paths).unwrap();
        let collection =
            vectors::unified_collection_name(paths, &profile, cerul_storage::SEARCH_INDEX_VERSION);
        vectors::collection_point_count(paths, &collection)
            .await
            .unwrap()
    }

    fn fake_vector(seed: usize) -> Vec<f32> {
        let mut vector = vec![0.0; cerul_storage::vectors::VECTOR_DIMENSIONS as usize];
        let index = seed % vector.len();
        vector[index] = 1.0;
        vector
    }

    fn fake_embedding_provider_info() -> InferenceProviderInfo {
        InferenceProviderInfo {
            provider_mode: "remote".to_string(),
            provider_id: Some("test-embedding-provider".to_string()),
            provider_type: Some("gemini".to_string()),
            model_id: Some("gemini-embedding-2".to_string()),
            base_url: None,
        }
    }

    fn bad_dimension_profile(paths: &AppPaths) -> cerul_storage::vectors::EmbeddingProfile {
        let mut profile = cerul_storage::vectors::ensure_active_embedding_profile(paths).unwrap();
        profile.id = "bad-dimension-profile".to_string();
        profile.output_dimension = cerul_storage::vectors::VECTOR_DIMENSIONS + 1;
        profile
    }
}
