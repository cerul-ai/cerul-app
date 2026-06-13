use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use cerul_storage::{
    AppPaths, StorageImageChunk, StorageOcrChunk, StorageTranscriptChunk, StorageTranscriptLine,
    StorageWriteSummary,
};
use serde_json::{json, Map};

use crate::{
    chunking, ffmpeg,
    whisper::{Segment, TranscriptionProgress},
};

pub trait Transcriber: Send + Sync {
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

    fn report_progress(&self, item_id: &str, stage: &'static str, progress: f64, message: &str) {
        self.progress.update(item_id, stage, progress, message);
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

    pub fn release_all_runtime_models(&self, item_id: &str) {
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
        if item.source_type == "web_video" && item.raw_path.as_deref() != video_path.to_str() {
            cerul_storage::set_item_raw_path(&self.paths, item_id, &video_path)?;
        }
        update_item_duration_from_media(&self.paths, item_id, &video_path).await;
        let cache_key = cache_key(item.discovery_id());
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
            let progress_item_id = item_id.to_string();
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
            tracing::info!(item_id, "video has no audio stream; indexing visual frames only");
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
            tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<OcrFrame>> {
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
            .await??
        } else {
            Vec::new()
        };
        if self.ocr_enabled {
            self.release_runtime_models(ModelReleaseScope::Ocr, item_id, "ocr complete");
        }
        let storage_ocr_chunks = ocr_frames
            .into_iter()
            .filter(|frame| !frame.text.trim().is_empty())
            .map(|frame| StorageOcrChunk::frame(frame.path, frame.text))
            .collect::<Vec<_>>();

        self.report_progress(
            item_id,
            "writing_transcript",
            0.68,
            "Saving searchable transcript",
        );
        let sqlite_summary = cerul_storage::write_media_sqlite_chunks_with_ocr_and_lines(
            &self.paths,
            item_id,
            &transcript_storage.chunks,
            &transcript_storage.lines,
            &storage_ocr_chunks,
            &keyframes,
        )?;
        set_embedding_index_status(&self.paths, item_id, "pending", None, 0, 0)?;

        let texts = transcript_storage
            .chunks
            .iter()
            .map(|chunk| chunk.text.clone())
            .chain(storage_ocr_chunks.iter().map(|chunk| chunk.text.clone()))
            .collect::<Vec<_>>();
        let text_input_tokens = estimate_text_tokens(&texts);
        let text_chunk_count = texts.len();
        self.report_progress(
            item_id,
            "embedding_text",
            0.68,
            "Embedding transcript and OCR chunks",
        );
        let text_outcome = self
            .embed_texts_with_progress(
                item_id,
                "embedding_text",
                0.68,
                0.12,
                "Embedding transcript and OCR chunks",
                &texts,
            )
            .await;
        let text_vectors = match text_outcome {
            Ok(vectors) => {
                self.record_embedding_text_usage(
                    item_id,
                    text_input_tokens,
                    text_chunk_count,
                    "succeeded",
                    json!({ "source": "indexing" }),
                );
                vectors
            }
            Err(error) => {
                let message = error.to_string();
                tracing::warn!(%error, item_id, "text embedding failed; transcript remains searchable via FTS");
                self.record_embedding_text_usage(
                    item_id,
                    text_input_tokens,
                    text_chunk_count,
                    "failed",
                    json!({ "source": "indexing", "error": message }),
                );
                set_embedding_index_status(&self.paths, item_id, "failed", Some(&message), 0, 0)?;
                cerul_storage::set_video_multimodal_index_status(
                    &self.paths,
                    item_id,
                    "pending",
                    None,
                    frames.len(),
                    0,
                    if self.ocr_enabled {
                        "indexed"
                    } else {
                        "disabled"
                    },
                    None,
                    storage_ocr_chunks.len(),
                )?;
                self.report_progress(
                    item_id,
                    "partial",
                    1.0,
                    "Transcript searchable; embedding failed",
                );
                cerul_storage::mark_indexed(&self.paths, item_id)?;
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

        let frame_count = frames.len();
        self.report_progress(item_id, "embedding_frames", 0.80, "Embedding visual frames");
        let image_outcome = self
            .embed_images_with_progress(
                item_id,
                "embedding_frames",
                0.80,
                0.12,
                "Embedding visual frames",
                &frames,
            )
            .await;
        // Frame embedding is an enhancement, not a hard requirement: if it fails
        // still keep the transcript searchable instead of failing the job.
        let mut visual_index_error = None;
        let (embedded_keyframes, image_vectors) = match image_outcome {
            Ok(vectors) => {
                self.record_embedding_image_usage(
                    item_id,
                    frame_count,
                    "succeeded",
                    json!({ "source": "indexing" }),
                );
                (keyframes.clone(), vectors)
            }
            Err(error) => {
                let message = error.to_string();
                tracing::warn!(%error, "frame embedding failed; indexing video with transcript only");
                self.record_embedding_image_usage(
                    item_id,
                    frame_count,
                    "failed",
                    json!({ "source": "indexing", "error": message }),
                );
                self.report_progress(
                    item_id,
                    "visual_failed",
                    0.88,
                    "Transcript indexed; visual frames unavailable",
                );
                visual_index_error = Some(message);
                (Vec::new(), Vec::new())
            }
        };

        self.report_progress(item_id, "writing_index", 0.92, "Writing vector index");
        let write_summary = match &self.embedding_profile {
            Some(profile) => {
                cerul_storage::replace_media_embeddings_with_ocr_for_profile(
                    &self.paths,
                    item_id,
                    &transcript_storage.chunks,
                    &storage_ocr_chunks,
                    &embedded_keyframes,
                    &text_vectors,
                    &image_vectors,
                    profile,
                )
                .await?
            }
            None => {
                cerul_storage::replace_media_embeddings_with_ocr(
                    &self.paths,
                    item_id,
                    &transcript_storage.chunks,
                    &storage_ocr_chunks,
                    &embedded_keyframes,
                    &text_vectors,
                    &image_vectors,
                )
                .await?
            }
        };
        let visual_status = if visual_index_error.is_some() {
            "failed"
        } else {
            "indexed"
        };
        cerul_storage::set_video_multimodal_index_status(
            &self.paths,
            item_id,
            visual_status,
            visual_index_error.as_deref(),
            frames.len(),
            write_summary.image_vectors,
            if self.ocr_enabled {
                "indexed"
            } else {
                "disabled"
            },
            None,
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
        update_item_duration_from_media(&self.paths, item_id, &source_audio_path).await;
        let cache_key = cache_key(item.discovery_id());
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
        self.release_runtime_models(
            ModelReleaseScope::Transcription,
            item_id,
            "transcription complete",
        );
        let sqlite_summary = cerul_storage::write_media_sqlite_chunks_with_ocr_and_lines(
            &self.paths,
            item_id,
            &transcript_storage.chunks,
            &transcript_storage.lines,
            &[],
            &[],
        )?;
        set_embedding_index_status(&self.paths, item_id, "pending", None, 0, 0)?;
        let text_vectors = match self.embed_storage_chunks(&transcript_storage.chunks).await {
            Ok(vectors) => {
                let texts = transcript_storage
                    .chunks
                    .iter()
                    .map(|chunk| chunk.text.clone())
                    .collect::<Vec<_>>();
                self.record_embedding_text_usage(
                    item_id,
                    estimate_text_tokens(&texts),
                    texts.len(),
                    "succeeded",
                    json!({ "source": "indexing" }),
                );
                vectors
            }
            Err(error) => {
                let message = error.to_string();
                tracing::warn!(%error, item_id, "audio embedding failed; transcript remains searchable via FTS");
                let texts = transcript_storage
                    .chunks
                    .iter()
                    .map(|chunk| chunk.text.clone())
                    .collect::<Vec<_>>();
                self.record_embedding_text_usage(
                    item_id,
                    estimate_text_tokens(&texts),
                    texts.len(),
                    "failed",
                    json!({ "source": "indexing", "error": message }),
                );
                set_embedding_index_status(&self.paths, item_id, "failed", Some(&message), 0, 0)?;
                cerul_storage::mark_indexed(&self.paths, item_id)?;
                return Ok(ProcessAudioSummary::from_write_summary(
                    item_id,
                    audio_path,
                    sqlite_summary,
                ));
            }
        };
        let write_summary = match &self.embedding_profile {
            Some(profile) => {
                cerul_storage::replace_media_embeddings_for_profile(
                    &self.paths,
                    item_id,
                    &transcript_storage.chunks,
                    &[],
                    &text_vectors,
                    &[],
                    profile,
                )
                .await?
            }
            None => {
                cerul_storage::replace_media_embeddings(
                    &self.paths,
                    item_id,
                    &transcript_storage.chunks,
                    &[],
                    &text_vectors,
                    &[],
                )
                .await?
            }
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

        let image_vectors = match self
            .embed_image_paths(std::slice::from_ref(&image_path))
            .await
        {
            Ok(vectors) => {
                self.record_embedding_image_usage(
                    item_id,
                    1,
                    "succeeded",
                    json!({ "source": "indexing", "image_path": image_path.display().to_string() }),
                );
                vectors
            }
            Err(error) => {
                let message = error.to_string();
                tracing::warn!(%error, item_id, "image embedding failed; image chunk remains searchable");
                self.record_embedding_image_usage(
                    item_id,
                    1,
                    "failed",
                    json!({
                        "source": "indexing",
                        "image_path": image_path.display().to_string(),
                        "error": message
                    }),
                );
                if let Err(clear_error) = self.clear_item_embeddings(item_id).await {
                    tracing::warn!(
                        error = %clear_error,
                        item_id,
                        "failed to clear stale image embeddings after image embedding failure"
                    );
                }
                set_embedding_index_status(&self.paths, item_id, "failed", Some(&message), 0, 0)?;
                cerul_storage::mark_indexed(&self.paths, item_id)?;
                return Ok(ProcessImageSummary::from_write_summary(
                    item_id,
                    image_path,
                    exif_fields,
                    sqlite_summary,
                ));
            }
        };
        let write_summary = match &self.embedding_profile {
            Some(profile) => {
                cerul_storage::replace_media_embeddings_for_profile(
                    &self.paths,
                    item_id,
                    &[],
                    &image_chunks,
                    &[],
                    &image_vectors,
                    profile,
                )
                .await?
            }
            None => {
                cerul_storage::replace_media_embeddings(
                    &self.paths,
                    item_id,
                    &[],
                    &image_chunks,
                    &[],
                    &image_vectors,
                )
                .await?
            }
        };
        let write_summary = StorageWriteSummary {
            transcript_chunks: sqlite_summary.transcript_chunks,
            keyframes: write_summary.keyframes,
            text_vectors: write_summary.text_vectors,
            image_vectors: write_summary.image_vectors,
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

    async fn clear_item_embeddings(&self, item_id: &str) -> anyhow::Result<()> {
        match &self.embedding_profile {
            Some(profile) => {
                cerul_storage::replace_media_embeddings_for_profile(
                    &self.paths,
                    item_id,
                    &[],
                    &[],
                    &[],
                    &[],
                    profile,
                )
                .await?;
            }
            None => {
                cerul_storage::replace_media_embeddings(&self.paths, item_id, &[], &[], &[], &[])
                    .await?;
            }
        }
        Ok(())
    }

    async fn transcribe_to_storage_chunks(
        &self,
        item_id: &str,
        audio_path: &Path,
    ) -> anyhow::Result<TranscriptStorage> {
        let transcriber = Arc::clone(&self.transcriber);
        let audio_for_transcribe = audio_path.to_path_buf();
        let segments = tokio::task::spawn_blocking(move || {
            transcriber.transcribe(&audio_for_transcribe, None)
        })
        .await??;
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

    async fn embed_storage_chunks(
        &self,
        chunks: &[StorageTranscriptChunk],
    ) -> anyhow::Result<Vec<Vec<f32>>> {
        let embedder = Arc::clone(&self.embedder);
        let texts = chunks
            .iter()
            .map(|chunk| chunk.text.clone())
            .collect::<Vec<_>>();

        tokio::task::spawn_blocking(move || embedder.embed_texts(&texts)).await?
    }

    async fn embed_image_paths(&self, paths: &[PathBuf]) -> anyhow::Result<Vec<Vec<f32>>> {
        let embedder = Arc::clone(&self.embedder);
        let paths = paths.to_vec();

        tokio::task::spawn_blocking(move || embedder.embed_images(&paths)).await?
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
            paths
                .source_cache_dir(source_type)
                .to_string_lossy()
                .into_owned(),
        )
    });
    serde_json::Value::Object(object)
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
    }

    struct FailingTextEmbedder;

    impl Embedder for FailingTextEmbedder {
        fn embed_texts(&self, _texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
            anyhow::bail!("Gemini Embedding 2 quota exceeded")
        }

        fn embed_images(&self, _paths: &[PathBuf]) -> anyhow::Result<Vec<Vec<f32>>> {
            unreachable!("image embedding should not run after text embedding failure")
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
        assert_eq!(summary.image_vectors, summary.sampled_frames);
        assert!(summary.audio_path.is_file());

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
        assert_eq!(keyframe_count, summary.image_vectors as i64);
        assert_eq!(ocr_count, 0);
        assert_eq!(
            total_chunk_count,
            transcript_count + transcript_line_count + keyframe_count + ocr_count
        );

        let profile = vectors::ensure_active_embedding_profile(&paths).unwrap();
        let collections = vectors::collection_names(&paths, &profile);
        assert_eq!(
            vectors::collection_point_count(&paths, &collections.text)
                .await
                .unwrap(),
            summary.text_vectors
        );
        assert_eq!(
            vectors::collection_point_count(&paths, &collections.image)
                .await
                .unwrap(),
            summary.image_vectors
        );
        let stages = progress.stages.lock().unwrap();
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

        // No transcript, but frames are still sampled and embedded for visual search.
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
            .query_row("SELECT metadata FROM items WHERE id = 'item-1'", [], |row| {
                row.get(0)
            })
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
        assert_eq!(
            summary.text_vectors,
            summary.transcript_chunks + summary.ocr_chunks
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
    async fn process_video_item_marks_transcript_only_when_image_embedding_fails() {
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
        assert_eq!(metadata["visual_index_status"], "failed");
        assert_eq!(metadata["visual_indexed_frames"], 0);
        assert!(metadata["visual_sampled_frames"].as_u64().unwrap() > 0);
        assert!(metadata["visual_index_error"]
            .as_str()
            .unwrap()
            .contains("Image token span mismatch"));
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
        let (status, metadata): (String, String) = conn
            .query_row(
                "SELECT status, metadata FROM items WHERE id = 'image-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
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
        assert_eq!(metadata["embedding_index_status"], "failed");
        assert!(metadata["embedding_index_error"]
            .as_str()
            .unwrap()
            .contains("Image token span mismatch"));
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
        assert_eq!(keyframe_count as usize, summary.image_vectors);
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
            assert!(audio.audio_path.is_file());

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

        let profile = vectors::ensure_active_embedding_profile(&paths).unwrap();
        let collections = vectors::collection_names(&paths, &profile);
        assert_eq!(
            vectors::collection_point_count(&paths, &collections.text)
                .await
                .unwrap(),
            5
        );
        assert_eq!(
            vectors::collection_point_count(&paths, &collections.image)
                .await
                .unwrap(),
            5
        );
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

    fn fake_vector(seed: usize) -> Vec<f32> {
        let mut vector = vec![0.0; cerul_storage::vectors::VECTOR_DIMENSIONS as usize];
        let index = seed % vector.len();
        vector[index] = 1.0;
        vector
    }
}
