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

#[derive(Debug, Clone, PartialEq)]
pub struct StorageDocumentChunk {
    pub text: String,
    pub page: Option<u32>,
    pub section: Option<String>,
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

impl StorageDocumentChunk {
    pub fn new(
        text: impl Into<String>,
        page: Option<u32>,
        section: Option<String>,
        metadata: serde_json::Value,
    ) -> Self {
        Self {
            text: text.into(),
            page,
            section,
            metadata,
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

#[allow(clippy::too_many_arguments)]
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

pub fn write_document_sqlite_chunks(
    paths: &AppPaths,
    item_id: &str,
    document_chunks: &[StorageDocumentChunk],
) -> anyhow::Result<StorageWriteSummary> {
    let mut conn = sqlite::open(paths)?;
    let tx = conn.transaction()?;

    tx.execute(
        "DELETE FROM chunks WHERE item_id = ?1 AND chunk_type != 'understanding'",
        [item_id],
    )?;

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
            VALUES (?1, ?2, 'document', NULL, NULL, ?3, NULL, ?4)
            "#,
        )?;

        for (index, chunk) in document_chunks.iter().enumerate() {
            let metadata = document_metadata_with_index(chunk, index).to_string();
            stmt.execute((
                document_chunk_id(item_id, index),
                item_id,
                chunk.text.as_str(),
                metadata,
            ))?;
        }
    }

    tx.commit()?;
    Ok(StorageWriteSummary {
        transcript_chunks: document_chunks.len(),
        keyframes: 0,
        text_vectors: 0,
        image_vectors: 0,
    })
}

/// Write only the visual keyframes for an item, without touching transcript,
/// OCR, or vector rows. The indexing pipeline calls this immediately after
/// frame sampling so the library can show real thumbnails while slower ASR and
/// embedding stages continue.
pub fn replace_item_keyframes(
    paths: &AppPaths,
    item_id: &str,
    image_chunks: &[StorageImageChunk],
) -> anyhow::Result<usize> {
    let mut conn = sqlite::open(paths)?;
    let tx = conn.transaction()?;

    tx.execute(
        "DELETE FROM chunks WHERE item_id = ?1 AND chunk_type = 'keyframe'",
        [item_id],
    )?;

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

        for (index, chunk) in image_chunks.iter().enumerate() {
            if chunk.chunk_type != "keyframe" {
                continue;
            }
            let frame_path = path_to_string(&chunk.path);
            let metadata = metadata_with_index(&chunk.metadata, index).to_string();
            stmt.execute((
                keyframe_chunk_id(item_id, index),
                item_id,
                "keyframe",
                chunk.start_sec,
                chunk.end_sec,
                Option::<&str>::None,
                frame_path.as_str(),
                metadata.as_str(),
            ))?;
        }
    }

    tx.commit()?;
    Ok(image_chunks
        .iter()
        .filter(|chunk| chunk.chunk_type == "keyframe")
        .count())
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

#[allow(clippy::too_many_arguments)]
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

#[allow(clippy::too_many_arguments)]
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

#[allow(clippy::too_many_arguments)]
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

    tx.execute(
        "DELETE FROM chunks WHERE item_id = ?1 AND chunk_type != 'understanding'",
        [item_id],
    )?;

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

fn document_chunk_id(item_id: &str, index: usize) -> String {
    format!("{item_id}:document:{index:06}")
}

fn metadata_with_index(metadata: &serde_json::Value, index: usize) -> serde_json::Value {
    let mut metadata = metadata.clone();

    if !metadata.is_object() {
        metadata = serde_json::Value::Object(Default::default());
    }
    metadata["index"] = serde_json::json!(index);

    metadata
}

fn document_metadata_with_index(chunk: &StorageDocumentChunk, index: usize) -> serde_json::Value {
    let mut metadata = metadata_with_index(&chunk.metadata, index);
    if let Some(page) = chunk.page {
        metadata["page"] = serde_json::json!(page);
    }
    if let Some(section) = chunk
        .section
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        metadata["section"] = serde_json::json!(section);
    }
    metadata
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite;

    fn insert_test_item(paths: &AppPaths, item_id: &str) {
        let conn = sqlite::open(paths).unwrap();
        conn.execute(
            "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'local_folder', '{}', 'ready')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO items (id, source_id, content_type, external_id, title, status) VALUES (?1, 'source-1', 'video', 'external-1', 'Test video', 'queued')",
            [item_id],
        )
        .unwrap();
    }

    #[test]
    fn replace_item_keyframes_replaces_existing_keyframes_without_touching_text_chunks() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let item_id = "item-1";
        insert_test_item(&paths, item_id);

        {
            let conn = sqlite::open(&paths).unwrap();
            conn.execute(
                "INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, text, metadata) VALUES ('item-1:transcript:000000', ?1, 'transcript', 0, 1, 'hello', '{}')",
                [item_id],
            )
            .unwrap();
        }

        let first_keyframes = vec![
            StorageImageChunk::keyframe_at(paths.cache.join("frame-0.jpg"), 0.0, 5.0),
            StorageImageChunk::keyframe_at(paths.cache.join("frame-1.jpg"), 5.0, 10.0),
        ];
        assert_eq!(
            replace_item_keyframes(&paths, item_id, &first_keyframes).unwrap(),
            2
        );

        let replacement_keyframes = vec![StorageImageChunk::keyframe_at(
            paths.cache.join("frame-2.jpg"),
            10.0,
            15.0,
        )];
        assert_eq!(
            replace_item_keyframes(&paths, item_id, &replacement_keyframes).unwrap(),
            1
        );

        let conn = sqlite::open(&paths).unwrap();
        let keyframe_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE item_id = ?1 AND chunk_type = 'keyframe'",
                [item_id],
                |row| row.get(0),
            )
            .unwrap();
        let transcript_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE item_id = ?1 AND chunk_type = 'transcript'",
                [item_id],
                |row| row.get(0),
            )
            .unwrap();
        let frame_path: String = conn
            .query_row(
                "SELECT frame_path FROM chunks WHERE item_id = ?1 AND chunk_type = 'keyframe'",
                [item_id],
                |row| row.get(0),
            )
            .unwrap();
        let metadata: String = conn
            .query_row(
                "SELECT metadata FROM chunks WHERE item_id = ?1 AND chunk_type = 'keyframe'",
                [item_id],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(keyframe_count, 1);
        assert_eq!(transcript_count, 1);
        assert!(frame_path.ends_with("frame-2.jpg"));
        assert!(metadata.contains("\"index\":0"));
    }

    #[test]
    fn media_sqlite_rewrite_preserves_understanding_chunks() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let item_id = "item-1";
        insert_test_item(&paths, item_id);
        {
            let conn = sqlite::open(&paths).unwrap();
            conn.execute(
                "INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, text, metadata) VALUES ('item-1:understanding:summary', ?1, 'understanding', NULL, NULL, 'understanding survives rewrite', '{}')",
                [item_id],
            )
            .unwrap();
        }

        write_media_sqlite_chunks_with_ocr_and_lines(
            &paths,
            item_id,
            &[StorageTranscriptChunk {
                start: 1.0,
                end: 2.0,
                text: "fresh transcript".to_string(),
            }],
            &[],
            &[],
            &[],
        )
        .unwrap();

        let conn = sqlite::open(&paths).unwrap();
        let understanding_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE item_id = ?1 AND chunk_type = 'understanding'",
                [item_id],
                |row| row.get(0),
            )
            .unwrap();
        let transcript_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE item_id = ?1 AND chunk_type = 'transcript'",
                [item_id],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(understanding_count, 1);
        assert_eq!(transcript_count, 1);
    }
}
