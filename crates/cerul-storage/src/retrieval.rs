use std::collections::HashMap;

use rusqlite::{params, OptionalExtension};
use serde_json::Value;

use crate::{sqlite, AppPaths};

pub const SEARCH_INDEX_VERSION: i32 = 2;

const WINDOW_SEC: f64 = 30.0;
const WINDOW_STEP_SEC: f64 = 25.0;
const TRANSCRIPT_BUDGET: usize = 3_200;
const OCR_BUDGET: usize = 1_200;
const VISUAL_BUDGET: usize = 1_000;
const SUMMARY_BUDGET: usize = 800;

#[derive(Debug, Clone, PartialEq)]
pub struct StorageRetrievalUnit {
    pub id: String,
    pub item_id: String,
    pub unit_index: i64,
    pub unit_kind: String,
    pub start_sec: Option<f64>,
    pub end_sec: Option<f64>,
    pub content_text: String,
    pub transcript_text: Option<String>,
    pub ocr_text: Option<String>,
    pub visual_text: Option<String>,
    pub summary_text: Option<String>,
    pub representative_chunk_id: Option<String>,
    pub representative_frame_path: Option<String>,
    pub embedding_profile_id: String,
    pub index_version: i32,
    pub metadata: Value,
}

impl StorageRetrievalUnit {
    pub fn uses_image_embedding(&self) -> bool {
        self.unit_kind == "image"
            && self.representative_frame_path.is_some()
            && self.transcript_text.as_deref().is_none_or(str::is_empty)
            && self.ocr_text.as_deref().is_none_or(str::is_empty)
            && self.visual_text.as_deref().is_none_or(str::is_empty)
            && self.summary_text.as_deref().is_none_or(str::is_empty)
    }
}

#[derive(Debug, Clone)]
struct ItemInfo {
    id: String,
    title: Option<String>,
    content_type: String,
    raw_path: Option<String>,
    source_type: String,
    source_config: Value,
}

#[derive(Debug, Clone)]
struct ChunkInfo {
    id: String,
    chunk_type: String,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
    text: Option<String>,
    frame_path: Option<String>,
    metadata: Value,
}

#[derive(Debug, Clone)]
struct Window {
    start_sec: Option<f64>,
    end_sec: Option<f64>,
    visual_text: Option<String>,
    summary_text: Option<String>,
}

pub fn rebuild_item_retrieval_units(
    paths: &AppPaths,
    item_id: &str,
    embedding_profile_id: &str,
) -> anyhow::Result<Vec<StorageRetrievalUnit>> {
    let mut conn = sqlite::open(paths)?;
    let units = build_item_retrieval_units_with_conn(&conn, item_id, embedding_profile_id)?;
    let tx = conn.transaction()?;
    replace_item_retrieval_units_with_tx(&tx, item_id, SEARCH_INDEX_VERSION, &units)?;
    tx.commit()?;
    Ok(units)
}

pub fn replace_item_retrieval_units(
    paths: &AppPaths,
    item_id: &str,
    units: &[StorageRetrievalUnit],
) -> anyhow::Result<()> {
    let mut conn = sqlite::open(paths)?;
    let tx = conn.transaction()?;
    replace_item_retrieval_units_with_tx(&tx, item_id, SEARCH_INDEX_VERSION, units)?;
    tx.commit()?;
    Ok(())
}

pub fn set_item_search_index_status(
    paths: &AppPaths,
    item_id: &str,
    status: &str,
    error: Option<&str>,
    unit_count: usize,
    vector_count: usize,
) -> anyhow::Result<()> {
    let conn = sqlite::open(paths)?;
    let updated = conn.execute(
        r#"
        UPDATE items
        SET search_index_version = ?2,
            search_index_status = ?3,
            search_index_error = ?4,
            search_index_unit_count = ?5,
            search_index_vector_count = ?6
        WHERE id = ?1
        "#,
        params![
            item_id,
            SEARCH_INDEX_VERSION,
            status,
            error,
            unit_count as i64,
            vector_count as i64
        ],
    )?;
    anyhow::ensure!(updated == 1, "item not found: {item_id}");
    Ok(())
}

pub fn clear_item_search_index(paths: &AppPaths, item_id: &str) -> anyhow::Result<()> {
    let conn = sqlite::open(paths)?;
    conn.execute(
        "DELETE FROM retrieval_units WHERE item_id = ?1 AND index_version = ?2",
        params![item_id, SEARCH_INDEX_VERSION],
    )?;
    set_item_search_index_status(paths, item_id, "pending", None, 0, 0)
}

fn replace_item_retrieval_units_with_tx(
    tx: &rusqlite::Transaction<'_>,
    item_id: &str,
    index_version: i32,
    units: &[StorageRetrievalUnit],
) -> anyhow::Result<()> {
    tx.execute(
        "DELETE FROM retrieval_units WHERE item_id = ?1 AND index_version = ?2",
        params![item_id, index_version],
    )?;

    let mut stmt = tx.prepare(
        r#"
        INSERT INTO retrieval_units (
            id,
            item_id,
            unit_index,
            unit_kind,
            start_sec,
            end_sec,
            content_text,
            transcript_text,
            ocr_text,
            visual_text,
            summary_text,
            representative_chunk_id,
            representative_frame_path,
            embedding_profile_id,
            index_version,
            metadata,
            updated_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, strftime('%s','now'))
        "#,
    )?;

    for unit in units {
        stmt.execute(params![
            unit.id,
            unit.item_id,
            unit.unit_index,
            unit.unit_kind,
            unit.start_sec,
            unit.end_sec,
            unit.content_text,
            unit.transcript_text,
            unit.ocr_text,
            unit.visual_text,
            unit.summary_text,
            unit.representative_chunk_id,
            unit.representative_frame_path,
            unit.embedding_profile_id,
            unit.index_version,
            unit.metadata.to_string()
        ])?;
    }

    Ok(())
}

fn build_item_retrieval_units_with_conn(
    conn: &rusqlite::Connection,
    item_id: &str,
    embedding_profile_id: &str,
) -> anyhow::Result<Vec<StorageRetrievalUnit>> {
    let item = load_item(conn, item_id)?;
    let chunks = load_chunks(conn, item_id)?;
    let mut units = if item.content_type == "image" {
        build_image_units(&item, &chunks, embedding_profile_id)
    } else {
        build_timed_units(&item, &chunks, embedding_profile_id)
    };

    if units.is_empty() {
        units = build_image_units(&item, &chunks, embedding_profile_id);
    }
    Ok(units)
}

fn load_item(conn: &rusqlite::Connection, item_id: &str) -> anyhow::Result<ItemInfo> {
    conn.query_row(
        r#"
        SELECT i.id, i.title, i.content_type, i.raw_path, s.type, s.config
        FROM items i
        JOIN sources s ON s.id = i.source_id
        WHERE i.id = ?1
        "#,
        [item_id],
        |row| {
            let source_config: String = row.get(5)?;
            Ok(ItemInfo {
                id: row.get(0)?,
                title: row.get(1)?,
                content_type: row.get(2)?,
                raw_path: row.get(3)?,
                source_type: row.get(4)?,
                source_config: serde_json::from_str(&source_config).unwrap_or(Value::Null),
            })
        },
    )
    .map_err(|error| match error {
        rusqlite::Error::QueryReturnedNoRows => anyhow::anyhow!("item not found: {item_id}"),
        other => anyhow::Error::new(other),
    })
}

fn load_chunks(conn: &rusqlite::Connection, item_id: &str) -> anyhow::Result<Vec<ChunkInfo>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, chunk_type, start_sec, end_sec, text, frame_path, metadata
        FROM chunks
        WHERE item_id = ?1
        ORDER BY
          COALESCE(start_sec, 9223372036854775807),
          CASE chunk_type
            WHEN 'transcript_line' THEN 0
            WHEN 'transcript' THEN 1
            WHEN 'audio' THEN 2
            WHEN 'ocr' THEN 3
            WHEN 'understanding' THEN 4
            WHEN 'keyframe' THEN 5
            ELSE 6
          END,
          id
        "#,
    )?;
    let rows = stmt.query_map([item_id], |row| {
        let metadata: Option<String> = row.get(6)?;
        Ok(ChunkInfo {
            id: row.get(0)?,
            chunk_type: row.get(1)?,
            start_sec: row.get(2)?,
            end_sec: row.get(3)?,
            text: row.get(4)?,
            frame_path: row.get(5)?,
            metadata: metadata
                .as_deref()
                .and_then(|value| serde_json::from_str(value).ok())
                .unwrap_or(Value::Null),
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn build_timed_units(
    item: &ItemInfo,
    chunks: &[ChunkInfo],
    embedding_profile_id: &str,
) -> Vec<StorageRetrievalUnit> {
    let transcript_chunks = chunks
        .iter()
        .filter(|chunk| matches!(chunk.chunk_type.as_str(), "transcript" | "audio"))
        .collect::<Vec<_>>();
    let transcript_lines = chunks
        .iter()
        .filter(|chunk| chunk.chunk_type == "transcript_line")
        .collect::<Vec<_>>();
    let ocr_chunks = chunks
        .iter()
        .filter(|chunk| chunk.chunk_type == "ocr")
        .collect::<Vec<_>>();
    let understanding_chunks = chunks
        .iter()
        .filter(|chunk| chunk.chunk_type == "understanding")
        .collect::<Vec<_>>();
    let frame_chunks = chunks
        .iter()
        .filter(|chunk| matches!(chunk.chunk_type.as_str(), "keyframe" | "image"))
        .collect::<Vec<_>>();
    let frame_times = frame_times_by_path(&frame_chunks);
    let windows = windows_for_item(&transcript_chunks, &understanding_chunks);
    let source_label = source_label(item);
    let mut units = Vec::new();

    for (index, window) in windows.into_iter().enumerate() {
        let transcript_text =
            collect_text_in_window(&transcript_chunks, &window, TRANSCRIPT_BUDGET)
                .or_else(|| collect_text_in_window(&transcript_lines, &window, TRANSCRIPT_BUDGET));
        let ocr_text = collect_ocr_text_in_window(&ocr_chunks, &window, &frame_times, OCR_BUDGET);
        let visual_text = window
            .visual_text
            .as_deref()
            .map(|text| limit_text(text, VISUAL_BUDGET))
            .filter(|text| !text.is_empty());
        let summary_text = window
            .summary_text
            .as_deref()
            .map(|text| limit_text(text, SUMMARY_BUDGET))
            .filter(|text| !text.is_empty());

        if transcript_text.is_none()
            && ocr_text.is_none()
            && visual_text.is_none()
            && summary_text.is_none()
        {
            continue;
        }

        let representative_chunk = representative_chunk(
            &transcript_lines,
            &transcript_chunks,
            &ocr_chunks,
            &window,
            &frame_times,
        )
        .or_else(|| nearest_frame(&frame_chunks, window.start_sec).map(|chunk| chunk.id.clone()));
        let representative_frame = nearest_frame(&frame_chunks, window.start_sec)
            .and_then(|chunk| chunk.frame_path.clone());
        let content_text = content_text(
            item,
            source_label.as_deref(),
            window.start_sec,
            window.end_sec,
            transcript_text.as_deref(),
            ocr_text.as_deref(),
            visual_text.as_deref(),
            summary_text.as_deref(),
        );

        units.push(StorageRetrievalUnit {
            id: retrieval_unit_id(&item.id, index),
            item_id: item.id.clone(),
            unit_index: index as i64,
            unit_kind: "moment".to_string(),
            start_sec: window.start_sec,
            end_sec: window.end_sec,
            content_text,
            transcript_text,
            ocr_text,
            visual_text,
            summary_text,
            representative_chunk_id: representative_chunk,
            representative_frame_path: representative_frame,
            embedding_profile_id: embedding_profile_id.to_string(),
            index_version: SEARCH_INDEX_VERSION,
            metadata: serde_json::json!({ "window": "timed" }),
        });
    }

    units
}

fn build_image_units(
    item: &ItemInfo,
    chunks: &[ChunkInfo],
    embedding_profile_id: &str,
) -> Vec<StorageRetrievalUnit> {
    let source_label = source_label(item);
    chunks
        .iter()
        .filter(|chunk| matches!(chunk.chunk_type.as_str(), "image" | "keyframe"))
        .enumerate()
        .map(|(index, chunk)| {
            let summary_text = exif_summary(&chunk.metadata);
            let content_text = content_text(
                item,
                source_label.as_deref(),
                chunk.start_sec,
                chunk.end_sec,
                None,
                None,
                None,
                summary_text.as_deref(),
            );
            StorageRetrievalUnit {
                id: retrieval_unit_id(&item.id, index),
                item_id: item.id.clone(),
                unit_index: index as i64,
                unit_kind: "image".to_string(),
                start_sec: chunk.start_sec,
                end_sec: chunk.end_sec,
                content_text,
                transcript_text: None,
                ocr_text: None,
                visual_text: None,
                summary_text,
                representative_chunk_id: Some(chunk.id.clone()),
                representative_frame_path: chunk.frame_path.clone(),
                embedding_profile_id: embedding_profile_id.to_string(),
                index_version: SEARCH_INDEX_VERSION,
                metadata: serde_json::json!({ "window": "image" }),
            }
        })
        .collect()
}

fn windows_for_item(
    transcript_chunks: &[&ChunkInfo],
    understanding_chunks: &[&ChunkInfo],
) -> Vec<Window> {
    let understanding_windows = understanding_chunks
        .iter()
        .filter_map(|chunk| {
            let text = chunk.text.as_deref()?.trim();
            if text.is_empty() {
                return None;
            }
            Some(Window {
                start_sec: chunk.start_sec,
                end_sec: chunk.end_sec,
                visual_text: Some(text.to_string()),
                summary_text: None,
            })
        })
        .collect::<Vec<_>>();
    if !understanding_windows.is_empty() {
        return understanding_windows;
    }

    let starts = transcript_chunks
        .iter()
        .filter_map(|chunk| chunk.start_sec)
        .collect::<Vec<_>>();
    let ends = transcript_chunks
        .iter()
        .filter_map(|chunk| chunk.end_sec.or(chunk.start_sec))
        .collect::<Vec<_>>();
    let Some(first_start) = starts.iter().copied().reduce(f64::min) else {
        return Vec::new();
    };
    let last_end = ends
        .iter()
        .copied()
        .reduce(f64::max)
        .unwrap_or(first_start + WINDOW_SEC);
    let mut windows = Vec::new();
    let mut start = first_start;
    while start <= last_end {
        let end = (start + WINDOW_SEC).min(last_end.max(start + 1.0));
        windows.push(Window {
            start_sec: Some(start),
            end_sec: Some(end),
            visual_text: None,
            summary_text: None,
        });
        if end >= last_end {
            break;
        }
        start += WINDOW_STEP_SEC;
    }
    windows
}

fn collect_text_in_window(chunks: &[&ChunkInfo], window: &Window, budget: usize) -> Option<String> {
    let mut text = String::new();
    for chunk in chunks {
        if !overlaps(
            chunk.start_sec,
            chunk.end_sec,
            window.start_sec,
            window.end_sec,
        ) {
            continue;
        }
        append_text(&mut text, chunk.text.as_deref(), budget);
        if text.chars().count() >= budget {
            break;
        }
    }
    normalize_optional_text(text)
}

fn collect_ocr_text_in_window(
    chunks: &[&ChunkInfo],
    window: &Window,
    frame_times: &HashMap<String, f64>,
    budget: usize,
) -> Option<String> {
    let mut text = String::new();
    for chunk in chunks {
        let effective_time = chunk.start_sec.or_else(|| {
            chunk
                .frame_path
                .as_ref()
                .and_then(|path| frame_times.get(path))
                .copied()
        });
        if effective_time.is_some()
            && !overlaps(
                effective_time,
                effective_time,
                window.start_sec,
                window.end_sec,
            )
        {
            continue;
        }
        if effective_time.is_none() && window.start_sec.is_some_and(|start| start > 0.0) {
            continue;
        }
        append_text(&mut text, chunk.text.as_deref(), budget);
        if text.chars().count() >= budget {
            break;
        }
    }
    normalize_optional_text(text)
}

fn representative_chunk(
    transcript_lines: &[&ChunkInfo],
    transcript_chunks: &[&ChunkInfo],
    ocr_chunks: &[&ChunkInfo],
    window: &Window,
    frame_times: &HashMap<String, f64>,
) -> Option<String> {
    transcript_lines
        .iter()
        .chain(transcript_chunks.iter())
        .find(|chunk| {
            overlaps(
                chunk.start_sec,
                chunk.end_sec,
                window.start_sec,
                window.end_sec,
            )
        })
        .map(|chunk| chunk.id.clone())
        .or_else(|| {
            ocr_chunks
                .iter()
                .find(|chunk| {
                    let effective_time = chunk.start_sec.or_else(|| {
                        chunk
                            .frame_path
                            .as_ref()
                            .and_then(|path| frame_times.get(path))
                            .copied()
                    });
                    effective_time.is_none()
                        || overlaps(
                            effective_time,
                            effective_time,
                            window.start_sec,
                            window.end_sec,
                        )
                })
                .map(|chunk| chunk.id.clone())
        })
}

fn nearest_frame<'a>(frame_chunks: &'a [&ChunkInfo], target: Option<f64>) -> Option<&'a ChunkInfo> {
    let Some(target) = target else {
        return frame_chunks.first().copied();
    };
    frame_chunks
        .iter()
        .filter(|chunk| chunk.start_sec.is_some())
        .min_by(|left, right| {
            (left.start_sec.unwrap_or(target) - target)
                .abs()
                .partial_cmp(&(right.start_sec.unwrap_or(target) - target).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .copied()
        .or_else(|| frame_chunks.first().copied())
}

fn frame_times_by_path(frame_chunks: &[&ChunkInfo]) -> HashMap<String, f64> {
    frame_chunks
        .iter()
        .filter_map(|chunk| Some((chunk.frame_path.clone()?, chunk.start_sec?)))
        .collect()
}

fn overlaps(
    left_start: Option<f64>,
    left_end: Option<f64>,
    right_start: Option<f64>,
    right_end: Option<f64>,
) -> bool {
    match (left_start, left_end, right_start, right_end) {
        (Some(ls), Some(le), Some(rs), Some(re)) => ls < re && le > rs,
        (Some(ls), None, Some(rs), Some(re)) => ls >= rs && ls <= re,
        (Some(ls), Some(le), Some(rs), None) => le >= rs && ls <= rs,
        (Some(ls), None, Some(rs), None) => (ls - rs).abs() < WINDOW_SEC,
        _ => true,
    }
}

fn content_text(
    item: &ItemInfo,
    source_label: Option<&str>,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
    transcript_text: Option<&str>,
    ocr_text: Option<&str>,
    visual_text: Option<&str>,
    summary_text: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    if let Some(title) = item
        .title
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        parts.push(format!("Title: {}", limit_text(title, 300)));
    }
    if let Some(source) = source_label.map(str::trim).filter(|text| !text.is_empty()) {
        parts.push(format!("Source: {}", limit_text(source, 300)));
    }
    if let Some(raw_path) = item
        .raw_path
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        parts.push(format!("Path: {}", limit_text(raw_path, 240)));
    }
    if start_sec.is_some() || end_sec.is_some() {
        parts.push(format!(
            "Time: {}-{}",
            start_sec
                .map(format_seconds)
                .unwrap_or_else(|| "?".to_string()),
            end_sec
                .map(format_seconds)
                .unwrap_or_else(|| "?".to_string())
        ));
    }
    if let Some(text) = transcript_text {
        parts.push(format!("Transcript: {text}"));
    }
    if let Some(text) = ocr_text {
        parts.push(format!("On-screen text: {text}"));
    }
    if let Some(text) = visual_text {
        parts.push(format!("Visual context: {text}"));
    }
    if let Some(text) = summary_text {
        parts.push(format!("Topics/Summary: {text}"));
    }
    parts.join("\n")
}

fn source_label(item: &ItemInfo) -> Option<String> {
    for key in ["title", "name", "url", "path", "feed_url", "channel_url"] {
        if let Some(value) = item.source_config.get(key).and_then(Value::as_str) {
            if !value.trim().is_empty() {
                return Some(value.trim().to_string());
            }
        }
    }
    Some(item.source_type.clone())
}

fn exif_summary(metadata: &Value) -> Option<String> {
    let exif = metadata.get("exif")?.as_object()?;
    let mut parts = Vec::new();
    for key in [
        "Image Make",
        "Image Model",
        "EXIF DateTimeOriginal",
        "Image ImageDescription",
    ] {
        if let Some(value) = exif.get(key).and_then(Value::as_str) {
            if !value.trim().is_empty() {
                parts.push(format!("{key}: {}", value.trim()));
            }
        }
    }
    normalize_optional_text(limit_text(&parts.join("; "), SUMMARY_BUDGET))
}

fn append_text(target: &mut String, text: Option<&str>, budget: usize) {
    let Some(text) = text.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    if !target.is_empty() {
        target.push(' ');
    }
    target.push_str(text);
    if target.chars().count() > budget {
        *target = limit_text(target, budget);
    }
}

fn normalize_optional_text(text: String) -> Option<String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn limit_text(text: &str, budget: usize) -> String {
    let mut output = String::new();
    for ch in text.chars().take(budget) {
        output.push(ch);
    }
    output
}

fn format_seconds(seconds: f64) -> String {
    format!("{seconds:.1}s")
}

fn retrieval_unit_id(item_id: &str, index: usize) -> String {
    format!("{item_id}:unit:v{SEARCH_INDEX_VERSION}:{index:06}")
}

pub fn retrieval_unit_count(paths: &AppPaths) -> anyhow::Result<usize> {
    let conn = sqlite::open(paths)?;
    count_query(
        &conn,
        "SELECT COUNT(*) FROM retrieval_units WHERE index_version = ?1",
        SEARCH_INDEX_VERSION,
    )
}

pub fn indexed_item_count(paths: &AppPaths) -> anyhow::Result<usize> {
    let conn = sqlite::open(paths)?;
    count_query(
        &conn,
        "SELECT COUNT(*) FROM items WHERE search_index_version = ?1 AND search_index_status = 'indexed'",
        SEARCH_INDEX_VERSION,
    )
}

pub fn items_needing_rebuild_count(paths: &AppPaths) -> anyhow::Result<usize> {
    let conn = sqlite::open(paths)?;
    count_query(
        &conn,
        r#"
        SELECT COUNT(*)
        FROM items
        WHERE status = 'indexed'
          AND (
            search_index_version IS NULL
            OR search_index_version != ?1
            OR search_index_status IS NULL
            OR search_index_status != 'indexed'
          )
        "#,
        SEARCH_INDEX_VERSION,
    )
}

fn count_query(conn: &rusqlite::Connection, sql: &str, version: i32) -> anyhow::Result<usize> {
    conn.query_row(sql, [version], |row| row.get::<_, i64>(0))
        .map(|value| value.max(0) as usize)
        .map_err(Into::into)
}

pub fn best_sub_unit_for_query(
    paths: &AppPaths,
    item_id: &str,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
    query: &str,
) -> anyhow::Result<Option<(String, f64)>> {
    let pattern = literal_pattern_for_terms(query);
    let conn = sqlite::open(paths)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT id, start_sec, text
        FROM chunks
        WHERE item_id = ?1
          AND chunk_type IN ('transcript_line', 'transcript', 'audio')
          AND start_sec IS NOT NULL
          AND (?2 IS NULL OR start_sec >= ?2)
          AND (?3 IS NULL OR start_sec <= ?3)
        ORDER BY
          CASE chunk_type WHEN 'transcript_line' THEN 0 ELSE 1 END,
          start_sec,
          id
        "#,
    )?;
    let rows = stmt.query_map(params![item_id, start_sec, end_sec], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, f64>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;

    let mut fallback = None;
    for row in rows {
        let (id, start, text) = row?;
        if fallback.is_none() {
            fallback = Some((id.clone(), start));
        }
        if let (Some(pattern), Some(text)) = (&pattern, text.as_deref()) {
            if text.to_lowercase().contains(pattern) {
                return Ok(Some((id, start)));
            }
        }
    }
    Ok(fallback)
}

fn literal_pattern_for_terms(query: &str) -> Option<String> {
    let trimmed = query.trim().trim_matches('"').to_lowercase();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

pub fn item_has_retrieval_units(
    paths: &AppPaths,
    item_id: &str,
    embedding_profile_id: &str,
) -> anyhow::Result<bool> {
    let conn = sqlite::open(paths)?;
    let found = conn
        .query_row(
            r#"
            SELECT 1
            FROM retrieval_units
            WHERE item_id = ?1
              AND embedding_profile_id = ?2
              AND index_version = ?3
            LIMIT 1
            "#,
            params![item_id, embedding_profile_id, SEARCH_INDEX_VERSION],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{sqlite, StorageTranscriptChunk};

    fn seed_item(paths: &AppPaths) {
        let conn = sqlite::open(paths).unwrap();
        conn.execute(
            "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'folder_video', '{\"path\":\"/videos\"}', 'active')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO items (id, source_id, content_type, title, status, metadata) VALUES ('item-1', 'source-1', 'video', 'Demo video', 'indexed', '{}')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn build_units_combines_transcript_windows() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        seed_item(&paths);
        crate::write_media_sqlite_chunks_with_ocr_and_lines(
            &paths,
            "item-1",
            &[
                StorageTranscriptChunk {
                    start: 0.0,
                    end: 12.0,
                    text: "first topic about embeddings".to_string(),
                },
                StorageTranscriptChunk {
                    start: 15.0,
                    end: 28.0,
                    text: "second topic mentions search".to_string(),
                },
            ],
            &[],
            &[],
            &[],
        )
        .unwrap();

        let units = rebuild_item_retrieval_units(&paths, "item-1", "profile-1").unwrap();

        assert_eq!(units.len(), 1);
        assert!(units[0].content_text.contains("Demo video"));
        assert!(units[0].content_text.contains("embeddings"));
        assert!(units[0].content_text.contains("search"));
        assert_eq!(retrieval_unit_count(&paths).unwrap(), 1);
    }
}
