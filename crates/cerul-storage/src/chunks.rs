use std::path::{Path, PathBuf};

use crate::{sqlite, vectors, AppPaths};

#[derive(Debug, Clone, PartialEq)]
pub struct StorageTranscriptChunk {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

pub type StorageTranscriptLine = StorageTranscriptChunk;

#[derive(Debug, Clone, PartialEq)]
pub struct StorageImageChunk {
    pub path: PathBuf,
    pub chunk_type: String,
    pub start_sec: Option<f64>,
    pub end_sec: Option<f64>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StorageOcrChunk {
    pub path: PathBuf,
    pub text: String,
    pub metadata: serde_json::Value,
}

impl StorageImageChunk {
    pub fn keyframe(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            chunk_type: "keyframe".to_string(),
            start_sec: None,
            end_sec: None,
            metadata: serde_json::Value::Object(Default::default()),
        }
    }

    pub fn keyframe_at(path: impl Into<PathBuf>, start_sec: f64, end_sec: f64) -> Self {
        Self {
            path: path.into(),
            chunk_type: "keyframe".to_string(),
            start_sec: Some(start_sec),
            end_sec: Some(end_sec),
            metadata: serde_json::json!({
                "timestamp_sec": start_sec,
            }),
        }
    }

    pub fn image(path: impl Into<PathBuf>, metadata: serde_json::Value) -> Self {
        Self {
            path: path.into(),
            chunk_type: "image".to_string(),
            start_sec: None,
            end_sec: None,
            metadata,
        }
    }
}

impl StorageOcrChunk {
    pub fn frame(path: impl Into<PathBuf>, text: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            text: text.into(),
            metadata: serde_json::Value::Object(Default::default()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageWriteSummary {
    pub transcript_chunks: usize,
    pub keyframes: usize,
    pub text_vectors: usize,
    pub image_vectors: usize,
}

pub async fn write_video_chunks(
    paths: &AppPaths,
    item_id: &str,
    transcript_chunks: &[StorageTranscriptChunk],
    frames: &[PathBuf],
    text_vectors: &[Vec<f32>],
    image_vectors: &[Vec<f32>],
) -> anyhow::Result<StorageWriteSummary> {
    write_video_chunks_with_ocr(
        paths,
        item_id,
        transcript_chunks,
        &[],
        frames,
        text_vectors,
        image_vectors,
    )
    .await
}

pub async fn write_video_chunks_with_ocr(
    paths: &AppPaths,
    item_id: &str,
    transcript_chunks: &[StorageTranscriptChunk],
    ocr_chunks: &[StorageOcrChunk],
    frames: &[PathBuf],
    text_vectors: &[Vec<f32>],
    image_vectors: &[Vec<f32>],
) -> anyhow::Result<StorageWriteSummary> {
    let image_chunks = frames
        .iter()
        .cloned()
        .map(StorageImageChunk::keyframe)
        .collect::<Vec<_>>();

    write_media_chunks_with_ocr(
        paths,
        item_id,
        transcript_chunks,
        ocr_chunks,
        &image_chunks,
        text_vectors,
        image_vectors,
    )
    .await
}

pub fn write_video_sqlite_chunks_with_ocr(
    paths: &AppPaths,
    item_id: &str,
    transcript_chunks: &[StorageTranscriptChunk],
    ocr_chunks: &[StorageOcrChunk],
    frames: &[PathBuf],
) -> anyhow::Result<StorageWriteSummary> {
    let image_chunks = frames
        .iter()
        .cloned()
        .map(StorageImageChunk::keyframe)
        .collect::<Vec<_>>();
    write_sqlite_chunks(
        paths,
        item_id,
        transcript_chunks,
        &[],
        ocr_chunks,
        &image_chunks,
    )?;
    Ok(StorageWriteSummary {
        transcript_chunks: transcript_chunks.len() + ocr_chunks.len(),
        keyframes: image_chunks.len(),
        text_vectors: 0,
        image_vectors: 0,
    })
}

pub async fn replace_video_embeddings_with_ocr(
    paths: &AppPaths,
    item_id: &str,
    transcript_chunks: &[StorageTranscriptChunk],
    ocr_chunks: &[StorageOcrChunk],
    frames: &[PathBuf],
    text_vectors: &[Vec<f32>],
    image_vectors: &[Vec<f32>],
) -> anyhow::Result<StorageWriteSummary> {
    let image_chunks = frames
        .iter()
        .cloned()
        .map(StorageImageChunk::keyframe)
        .collect::<Vec<_>>();
    replace_chunk_vectors(
        paths,
        item_id,
        transcript_chunks.len(),
        ocr_chunks.len(),
        &image_chunks,
        text_vectors,
        image_vectors,
    )
    .await
}

pub async fn replace_video_embeddings_with_ocr_for_profile(
    paths: &AppPaths,
    item_id: &str,
    transcript_chunks: &[StorageTranscriptChunk],
    ocr_chunks: &[StorageOcrChunk],
    frames: &[PathBuf],
    text_vectors: &[Vec<f32>],
    image_vectors: &[Vec<f32>],
    profile: &vectors::EmbeddingProfile,
) -> anyhow::Result<StorageWriteSummary> {
    let image_chunks = frames
        .iter()
        .cloned()
        .map(StorageImageChunk::keyframe)
        .collect::<Vec<_>>();
    replace_chunk_vectors_for_profile(
        paths,
        item_id,
        transcript_chunks.len(),
        ocr_chunks.len(),
        &image_chunks,
        text_vectors,
        image_vectors,
        profile,
    )
    .await
}

pub async fn write_media_chunks(
    paths: &AppPaths,
    item_id: &str,
    transcript_chunks: &[StorageTranscriptChunk],
    image_chunks: &[StorageImageChunk],
    text_vectors: &[Vec<f32>],
    image_vectors: &[Vec<f32>],
) -> anyhow::Result<StorageWriteSummary> {
    write_media_chunks_with_ocr(
        paths,
        item_id,
        transcript_chunks,
        &[],
        image_chunks,
        text_vectors,
        image_vectors,
    )
    .await
}

pub fn write_media_sqlite_chunks(
    paths: &AppPaths,
    item_id: &str,
    transcript_chunks: &[StorageTranscriptChunk],
    image_chunks: &[StorageImageChunk],
) -> anyhow::Result<StorageWriteSummary> {
    write_media_sqlite_chunks_with_ocr(paths, item_id, transcript_chunks, &[], image_chunks)
}

pub fn write_media_sqlite_chunks_with_ocr(
    paths: &AppPaths,
    item_id: &str,
    transcript_chunks: &[StorageTranscriptChunk],
    ocr_chunks: &[StorageOcrChunk],
    image_chunks: &[StorageImageChunk],
) -> anyhow::Result<StorageWriteSummary> {
    write_media_sqlite_chunks_with_ocr_and_lines(
        paths,
        item_id,
        transcript_chunks,
        &[],
        ocr_chunks,
        image_chunks,
    )
}

pub fn write_media_sqlite_chunks_with_ocr_and_lines(
    paths: &AppPaths,
    item_id: &str,
    transcript_chunks: &[StorageTranscriptChunk],
    transcript_lines: &[StorageTranscriptLine],
    ocr_chunks: &[StorageOcrChunk],
    image_chunks: &[StorageImageChunk],
) -> anyhow::Result<StorageWriteSummary> {
    write_sqlite_chunks(
        paths,
        item_id,
        transcript_chunks,
        transcript_lines,
        ocr_chunks,
        image_chunks,
    )?;
    Ok(StorageWriteSummary {
        transcript_chunks: transcript_chunks.len() + ocr_chunks.len(),
        keyframes: image_chunks.len(),
        text_vectors: 0,
        image_vectors: 0,
    })
}

pub async fn replace_media_embeddings(
    paths: &AppPaths,
    item_id: &str,
    transcript_chunks: &[StorageTranscriptChunk],
    image_chunks: &[StorageImageChunk],
    text_vectors: &[Vec<f32>],
    image_vectors: &[Vec<f32>],
) -> anyhow::Result<StorageWriteSummary> {
    replace_chunk_vectors(
        paths,
        item_id,
        transcript_chunks.len(),
        0,
        image_chunks,
        text_vectors,
        image_vectors,
    )
    .await
}

pub async fn replace_media_embeddings_for_profile(
    paths: &AppPaths,
    item_id: &str,
    transcript_chunks: &[StorageTranscriptChunk],
    image_chunks: &[StorageImageChunk],
    text_vectors: &[Vec<f32>],
    image_vectors: &[Vec<f32>],
    profile: &vectors::EmbeddingProfile,
) -> anyhow::Result<StorageWriteSummary> {
    replace_chunk_vectors_for_profile(
        paths,
        item_id,
        transcript_chunks.len(),
        0,
        image_chunks,
        text_vectors,
        image_vectors,
        profile,
    )
    .await
}

pub async fn replace_media_embeddings_with_ocr(
    paths: &AppPaths,
    item_id: &str,
    transcript_chunks: &[StorageTranscriptChunk],
    ocr_chunks: &[StorageOcrChunk],
    image_chunks: &[StorageImageChunk],
    text_vectors: &[Vec<f32>],
    image_vectors: &[Vec<f32>],
) -> anyhow::Result<StorageWriteSummary> {
    replace_chunk_vectors(
        paths,
        item_id,
        transcript_chunks.len(),
        ocr_chunks.len(),
        image_chunks,
        text_vectors,
        image_vectors,
    )
    .await
}

pub async fn replace_media_embeddings_with_ocr_for_profile(
    paths: &AppPaths,
    item_id: &str,
    transcript_chunks: &[StorageTranscriptChunk],
    ocr_chunks: &[StorageOcrChunk],
    image_chunks: &[StorageImageChunk],
    text_vectors: &[Vec<f32>],
    image_vectors: &[Vec<f32>],
    profile: &vectors::EmbeddingProfile,
) -> anyhow::Result<StorageWriteSummary> {
    replace_chunk_vectors_for_profile(
        paths,
        item_id,
        transcript_chunks.len(),
        ocr_chunks.len(),
        image_chunks,
        text_vectors,
        image_vectors,
        profile,
    )
    .await
}

pub async fn write_media_chunks_with_ocr_and_lines(
    paths: &AppPaths,
    item_id: &str,
    transcript_chunks: &[StorageTranscriptChunk],
    transcript_lines: &[StorageTranscriptLine],
    ocr_chunks: &[StorageOcrChunk],
    image_chunks: &[StorageImageChunk],
    text_vectors: &[Vec<f32>],
    image_vectors: &[Vec<f32>],
) -> anyhow::Result<StorageWriteSummary> {
    write_sqlite_chunks(
        paths,
        item_id,
        transcript_chunks,
        transcript_lines,
        ocr_chunks,
        image_chunks,
    )?;
    replace_chunk_vectors(
        paths,
        item_id,
        transcript_chunks.len(),
        ocr_chunks.len(),
        image_chunks,
        text_vectors,
        image_vectors,
    )
    .await
}

async fn write_media_chunks_with_ocr(
    paths: &AppPaths,
    item_id: &str,
    transcript_chunks: &[StorageTranscriptChunk],
    ocr_chunks: &[StorageOcrChunk],
    image_chunks: &[StorageImageChunk],
    text_vectors: &[Vec<f32>],
    image_vectors: &[Vec<f32>],
) -> anyhow::Result<StorageWriteSummary> {
    write_sqlite_chunks(
        paths,
        item_id,
        transcript_chunks,
        &[],
        ocr_chunks,
        image_chunks,
    )?;
    replace_chunk_vectors(
        paths,
        item_id,
        transcript_chunks.len(),
        ocr_chunks.len(),
        image_chunks,
        text_vectors,
        image_vectors,
    )
    .await
}

async fn replace_chunk_vectors(
    paths: &AppPaths,
    item_id: &str,
    transcript_count: usize,
    ocr_count: usize,
    image_chunks: &[StorageImageChunk],
    text_vectors: &[Vec<f32>],
    image_vectors: &[Vec<f32>],
) -> anyhow::Result<StorageWriteSummary> {
    let profile = vectors::ensure_active_embedding_profile(paths)?;
    replace_chunk_vectors_for_profile(
        paths,
        item_id,
        transcript_count,
        ocr_count,
        image_chunks,
        text_vectors,
        image_vectors,
        &profile,
    )
    .await
}

async fn replace_chunk_vectors_for_profile(
    paths: &AppPaths,
    item_id: &str,
    transcript_count: usize,
    ocr_count: usize,
    image_chunks: &[StorageImageChunk],
    text_vectors: &[Vec<f32>],
    image_vectors: &[Vec<f32>],
    profile: &vectors::EmbeddingProfile,
) -> anyhow::Result<StorageWriteSummary> {
    let text_chunk_count = transcript_count + ocr_count;
    anyhow::ensure!(
        text_chunk_count == text_vectors.len(),
        "text chunk count ({text_chunk_count}) does not match text vector count ({})",
        text_vectors.len()
    );
    anyhow::ensure!(
        image_chunks.len() == image_vectors.len(),
        "image chunk count ({}) does not match image vector count ({})",
        image_chunks.len(),
        image_vectors.len()
    );

    let text_records = (0..transcript_count)
        .map(|index| {
            let chunk_id = transcript_chunk_id(item_id, index);
            vectors::VectorRecord::new_for_dimensions(
                chunk_id,
                item_id.to_string(),
                text_vectors[index].clone(),
                profile.output_dimension,
            )
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let ocr_records = (0..ocr_count)
        .map(|index| {
            let chunk_id = ocr_chunk_id(item_id, index);
            vectors::VectorRecord::new_for_dimensions(
                chunk_id,
                item_id.to_string(),
                text_vectors[transcript_count + index].clone(),
                profile.output_dimension,
            )
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let text_records = text_records
        .into_iter()
        .chain(ocr_records)
        .collect::<Vec<_>>();
    let image_records = image_chunks
        .iter()
        .zip(image_vectors)
        .enumerate()
        .map(|(index, (chunk, vector))| {
            let chunk_id = image_chunk_id(item_id, &chunk.chunk_type, index);
            vectors::VectorRecord::new_for_dimensions(
                chunk_id,
                item_id.to_string(),
                vector.clone(),
                profile.output_dimension,
            )
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    vectors::replace_item_embeddings_for_profile(
        paths,
        item_id,
        &text_records,
        &image_records,
        profile,
    )
    .await?;

    Ok(StorageWriteSummary {
        transcript_chunks: text_chunk_count,
        keyframes: image_chunks.len(),
        text_vectors: text_records.len(),
        image_vectors: image_records.len(),
    })
}

fn write_sqlite_chunks(
    paths: &AppPaths,
    item_id: &str,
    transcript_chunks: &[StorageTranscriptChunk],
    transcript_lines: &[StorageTranscriptLine],
    ocr_chunks: &[StorageOcrChunk],
    image_chunks: &[StorageImageChunk],
) -> anyhow::Result<()> {
    let mut conn = sqlite::open(paths)?;
    let tx = conn.transaction()?;

    tx.execute("DELETE FROM chunks WHERE item_id = ?1", [item_id])?;

    {
        let mut stmt = tx.prepare(
            r#"
            INSERT INTO chunks (
                id,
                item_id,
                chunk_type,
                start_sec,
                end_sec,
                text,
                frame_path,
                metadata
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
        )?;

        for (index, chunk) in transcript_chunks.iter().enumerate() {
            let metadata = serde_json::json!({ "index": index }).to_string();
            stmt.execute((
                transcript_chunk_id(item_id, index),
                item_id,
                "transcript",
                chunk.start,
                chunk.end,
                chunk.text.as_str(),
                Option::<&str>::None,
                metadata,
            ))?;
        }

        for (index, line) in transcript_lines.iter().enumerate() {
            let metadata = serde_json::json!({ "index": index }).to_string();
            stmt.execute((
                transcript_line_chunk_id(item_id, index),
                item_id,
                "transcript_line",
                line.start,
                line.end,
                line.text.as_str(),
                Option::<&str>::None,
                metadata,
            ))?;
        }

        for (index, chunk) in ocr_chunks.iter().enumerate() {
            let frame_path = path_to_string(&chunk.path);
            let metadata = metadata_with_index(&chunk.metadata, index).to_string();
            stmt.execute((
                ocr_chunk_id(item_id, index),
                item_id,
                "ocr",
                Option::<f64>::None,
                Option::<f64>::None,
                chunk.text.as_str(),
                frame_path.as_str(),
                metadata,
            ))?;
        }

        for (index, chunk) in image_chunks.iter().enumerate() {
            let frame_path = path_to_string(&chunk.path);
            let metadata = metadata_with_index(&chunk.metadata, index).to_string();
            stmt.execute((
                image_chunk_id(item_id, &chunk.chunk_type, index),
                item_id,
                chunk.chunk_type.as_str(),
                chunk.start_sec,
                chunk.end_sec,
                Option::<&str>::None,
                frame_path.as_str(),
                metadata,
            ))?;
        }
    }

    tx.commit()?;
    Ok(())
}

fn transcript_chunk_id(item_id: &str, index: usize) -> String {
    format!("{item_id}:transcript:{index:06}")
}

fn transcript_line_chunk_id(item_id: &str, index: usize) -> String {
    format!("{item_id}:transcript-line:{index:06}")
}

fn keyframe_chunk_id(item_id: &str, index: usize) -> String {
    format!("{item_id}:keyframe:{index:06}")
}

fn image_chunk_id(item_id: &str, chunk_type: &str, index: usize) -> String {
    if chunk_type == "keyframe" {
        keyframe_chunk_id(item_id, index)
    } else {
        format!("{item_id}:{chunk_type}:{index:06}")
    }
}

fn ocr_chunk_id(item_id: &str, index: usize) -> String {
    format!("{item_id}:ocr:{index:06}")
}

fn metadata_with_index(metadata: &serde_json::Value, index: usize) -> serde_json::Value {
    let mut metadata = metadata.clone();

    if !metadata.is_object() {
        metadata = serde_json::Value::Object(Default::default());
    }
    metadata["index"] = serde_json::json!(index);

    metadata
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
