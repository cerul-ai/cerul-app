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
        self.representative_frame_path.is_some()
            && (self.unit_kind == "image"
                || (self.transcript_text.is_none()
                    && self.ocr_text.is_none()
                    && self.visual_text.is_none()
                    && self.summary_text.is_none()))
    }

    pub fn has_image_embedding_source(&self) -> bool {
        self.representative_frame_path.is_some()
    }
}

#[derive(Debug, Clone)]
struct ItemInfo {
    id: String,
    title: Option<String>,
    content_type: String,
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

struct ContentTextParts<'a> {
    item: &'a ItemInfo,
    source_label: Option<&'a str>,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
    transcript_text: Option<&'a str>,
    ocr_text: Option<&'a str>,
    visual_text: Option<&'a str>,
    summary_text: Option<&'a str>,
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
        SELECT i.id, i.title, i.content_type, s.type, s.config
        FROM items i
        JOIN sources s ON s.id = i.source_id
        WHERE i.id = ?1
        "#,
        [item_id],
        |row| {
            let source_config: String = row.get(4)?;
            Ok(ItemInfo {
                id: row.get(0)?,
                title: row.get(1)?,
                content_type: row.get(2)?,
                source_type: row.get(3)?,
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
    let windows = windows_for_item(
        &transcript_chunks,
        &understanding_chunks,
        &ocr_chunks,
        &frame_chunks,
        &frame_times,
    );
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
            &understanding_chunks,
            &ocr_chunks,
            &window,
            &frame_times,
        )
        .or_else(|| nearest_frame(&frame_chunks, window.start_sec).map(|chunk| chunk.id.clone()));
        let representative_frame = nearest_frame(&frame_chunks, window.start_sec)
            .and_then(|chunk| chunk.frame_path.clone());
        let content_text = content_text(ContentTextParts {
            item,
            source_label: source_label.as_deref(),
            start_sec: window.start_sec,
            end_sec: window.end_sec,
            transcript_text: transcript_text.as_deref(),
            ocr_text: ocr_text.as_deref(),
            visual_text: visual_text.as_deref(),
            summary_text: summary_text.as_deref(),
        });

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

    let mut next_index = units
        .iter()
        .map(|unit| unit.unit_index as usize)
        .max()
        .map_or(0, |index| index + 1);
    for chunk in frame_chunks {
        let covered_by_timed_unit = units
            .iter()
            .filter(|unit| {
                unit.transcript_text.is_some()
                    || unit.ocr_text.is_some()
                    || unit.visual_text.is_some()
                    || unit.summary_text.is_some()
            })
            .any(|unit| {
                overlaps(
                    chunk_effective_start(chunk, &frame_times),
                    chunk_effective_end(chunk, &frame_times),
                    unit.start_sec,
                    unit.end_sec,
                )
            });
        if covered_by_timed_unit {
            continue;
        }
        units.push(image_unit(
            item,
            source_label.as_deref(),
            chunk,
            next_index,
            embedding_profile_id,
        ));
        next_index += 1;
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
            image_unit(
                item,
                source_label.as_deref(),
                chunk,
                index,
                embedding_profile_id,
            )
        })
        .collect()
}

fn image_unit(
    item: &ItemInfo,
    source_label: Option<&str>,
    chunk: &ChunkInfo,
    index: usize,
    embedding_profile_id: &str,
) -> StorageRetrievalUnit {
    let summary_text = exif_summary(&chunk.metadata);
    let content_text = content_text(ContentTextParts {
        item,
        source_label,
        start_sec: chunk.start_sec,
        end_sec: chunk.end_sec,
        transcript_text: None,
        ocr_text: None,
        visual_text: None,
        summary_text: summary_text.as_deref(),
    });
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
}

fn windows_for_item(
    transcript_chunks: &[&ChunkInfo],
    understanding_chunks: &[&ChunkInfo],
    ocr_chunks: &[&ChunkInfo],
    frame_chunks: &[&ChunkInfo],
    frame_times: &HashMap<String, f64>,
) -> Vec<Window> {
    let mut windows = understanding_chunks
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

    let mut starts = transcript_chunks
        .iter()
        .filter_map(|chunk| chunk.start_sec)
        .collect::<Vec<_>>();
    starts.extend(
        ocr_chunks
            .iter()
            .filter_map(|chunk| chunk_effective_start(chunk, frame_times)),
    );
    starts.extend(
        frame_chunks
            .iter()
            .filter_map(|chunk| chunk_effective_start(chunk, frame_times)),
    );

    let mut ends = transcript_chunks
        .iter()
        .filter_map(|chunk| chunk.end_sec.or(chunk.start_sec))
        .collect::<Vec<_>>();
    ends.extend(
        ocr_chunks
            .iter()
            .filter_map(|chunk| chunk_effective_end(chunk, frame_times)),
    );
    ends.extend(
        frame_chunks
            .iter()
            .filter_map(|chunk| chunk_effective_end(chunk, frame_times)),
    );
    let Some(first_start) = starts.iter().copied().reduce(f64::min) else {
        if windows.is_empty() {
            return visual_only_windows(ocr_chunks, frame_chunks, frame_times);
        }
        return windows;
    };
    let last_end = ends
        .iter()
        .copied()
        .reduce(f64::max)
        .unwrap_or(first_start + WINDOW_SEC);
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

fn visual_only_windows(
    ocr_chunks: &[&ChunkInfo],
    frame_chunks: &[&ChunkInfo],
    frame_times: &HashMap<String, f64>,
) -> Vec<Window> {
    let mut windows = frame_chunks
        .iter()
        .map(|chunk| {
            let start = chunk_effective_start(chunk, frame_times);
            let end = chunk
                .end_sec
                .or_else(|| start.map(|value| value + WINDOW_SEC));
            Window {
                start_sec: start,
                end_sec: end,
                visual_text: None,
                summary_text: None,
            }
        })
        .collect::<Vec<_>>();

    if windows.is_empty() {
        windows = ocr_chunks
            .iter()
            .map(|chunk| Window {
                start_sec: chunk_effective_start(chunk, frame_times),
                end_sec: chunk_effective_end(chunk, frame_times).or_else(|| {
                    chunk_effective_start(chunk, frame_times).map(|value| value + WINDOW_SEC)
                }),
                visual_text: None,
                summary_text: None,
            })
            .collect();
    }

    if windows.is_empty() && !ocr_chunks.is_empty() {
        windows.push(Window {
            start_sec: None,
            end_sec: None,
            visual_text: None,
            summary_text: None,
        });
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
        let effective_start = chunk_effective_start(chunk, frame_times);
        let effective_end = chunk_effective_end(chunk, frame_times);
        if effective_start.is_some()
            && !overlaps(
                effective_start,
                effective_end,
                window.start_sec,
                window.end_sec,
            )
        {
            continue;
        }
        if effective_start.is_none() && window.start_sec.is_some_and(|start| start > 0.0) {
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
    understanding_chunks: &[&ChunkInfo],
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
                    let effective_start = chunk_effective_start(chunk, frame_times);
                    let effective_end = chunk_effective_end(chunk, frame_times);
                    effective_start.is_none()
                        || overlaps(
                            effective_start,
                            effective_end,
                            window.start_sec,
                            window.end_sec,
                        )
                })
                .map(|chunk| chunk.id.clone())
        })
        .or_else(|| {
            understanding_chunks
                .iter()
                .find(|chunk| {
                    overlaps(
                        chunk.start_sec,
                        chunk.end_sec,
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

fn chunk_effective_start(chunk: &ChunkInfo, frame_times: &HashMap<String, f64>) -> Option<f64> {
    chunk.start_sec.or_else(|| {
        chunk
            .frame_path
            .as_ref()
            .and_then(|path| frame_times.get(path))
            .copied()
    })
}

fn chunk_effective_end(chunk: &ChunkInfo, frame_times: &HashMap<String, f64>) -> Option<f64> {
    chunk
        .end_sec
        .or_else(|| chunk_effective_start(chunk, frame_times))
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
        (Some(ls), Some(le), Some(rs), Some(re)) if (le - ls).abs() < f64::EPSILON => {
            ls >= rs && ls <= re
        }
        (Some(ls), Some(le), Some(rs), Some(re)) => ls < re && le > rs,
        (Some(ls), None, Some(rs), Some(re)) => ls >= rs && ls <= re,
        (Some(ls), Some(le), Some(rs), None) => le >= rs && ls <= rs,
        (Some(ls), None, Some(rs), None) => (ls - rs).abs() < WINDOW_SEC,
        _ => true,
    }
}

fn content_text(content: ContentTextParts<'_>) -> String {
    let mut parts = Vec::new();
    if let Some(title) = content
        .item
        .title
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        parts.push(format!("Title: {}", limit_text(title, 300)));
    }
    if let Some(source) = content
        .source_label
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        parts.push(format!("Source: {}", limit_text(source, 300)));
    }
    if content.start_sec.is_some() || content.end_sec.is_some() {
        parts.push(format!(
            "Time: {}-{}",
            content
                .start_sec
                .map(format_seconds)
                .unwrap_or_else(|| "?".to_string()),
            content
                .end_sec
                .map(format_seconds)
                .unwrap_or_else(|| "?".to_string())
        ));
    }
    if let Some(text) = content.transcript_text {
        parts.push(format!("Transcript: {text}"));
    }
    if let Some(text) = content.ocr_text {
        parts.push(format!("On-screen text: {text}"));
    }
    if let Some(text) = content.visual_text {
        parts.push(format!("Visual context: {text}"));
    }
    if let Some(text) = content.summary_text {
        parts.push(format!("Topics/Summary: {text}"));
    }
    parts.join("\n")
}

fn source_label(item: &ItemInfo) -> Option<String> {
    for key in ["title", "name", "url", "feed_url", "channel_url"] {
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

pub fn item_retrieval_unit_count(paths: &AppPaths, item_id: &str) -> anyhow::Result<usize> {
    let conn = sqlite::open(paths)?;
    conn.query_row(
        r#"
        SELECT COUNT(*)
        FROM retrieval_units
        WHERE item_id = ?1
          AND index_version = ?2
        "#,
        params![item_id, SEARCH_INDEX_VERSION],
        |row| row.get::<_, i64>(0),
    )
    .map(|value| value.max(0) as usize)
    .map_err(Into::into)
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
    let terms = literal_terms_for_query(query);
    let conn = sqlite::open(paths)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT id, start_sec, text
        FROM chunks
        WHERE item_id = ?1
          AND chunk_type IN ('transcript_line', 'transcript', 'audio')
          AND start_sec IS NOT NULL
          AND (?2 IS NULL OR COALESCE(end_sec, start_sec) >= ?2)
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
    let mut best_match = None::<(String, f64, usize)>;
    for row in rows {
        let (id, start, text) = row?;
        if fallback.is_none() {
            fallback = Some((id.clone(), start));
        }
        if let Some(text) = text.as_deref() {
            let score = query_text_score(text, pattern.as_deref(), &terms);
            if score > 0
                && best_match
                    .as_ref()
                    .is_none_or(|(_, _, best_score)| score > *best_score)
            {
                best_match = Some((id, start, score));
            }
        }
    }
    if let Some((id, start, _)) = best_match {
        return Ok(Some((id, start)));
    }
    Ok(fallback)
}

pub fn best_visual_sub_unit_for_query(
    paths: &AppPaths,
    item_id: &str,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
    query: &str,
) -> anyhow::Result<Option<(String, Option<f64>)>> {
    let pattern = literal_pattern_for_terms(query);
    let terms = literal_terms_for_query(query);
    if pattern.is_none() && terms.is_empty() {
        return Ok(None);
    };
    let conn = sqlite::open(paths)?;
    let frame_times = load_frame_times_for_item(&conn, item_id)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT id, start_sec, end_sec, text, frame_path
        FROM chunks
        WHERE item_id = ?1
          AND chunk_type IN ('ocr', 'understanding')
          AND text IS NOT NULL
          AND TRIM(text) <> ''
        ORDER BY
          CASE chunk_type WHEN 'ocr' THEN 0 ELSE 1 END,
          COALESCE(start_sec, 9223372036854775807),
          id
        "#,
    )?;
    let rows = stmt.query_map(params![item_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<f64>>(1)?,
            row.get::<_, Option<f64>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
        ))
    })?;

    let mut best_match = None::<(String, Option<f64>, usize)>;
    for row in rows {
        let (id, chunk_start, chunk_end, text, frame_path) = row?;
        let effective_start = chunk_start.or_else(|| {
            frame_path
                .as_deref()
                .and_then(|path| frame_times.get(path).copied())
        });
        let effective_end = chunk_end.or(effective_start);
        if effective_start.is_some()
            && !overlaps(effective_start, effective_end, start_sec, end_sec)
        {
            continue;
        }
        let Some(text) = text.as_deref() else {
            continue;
        };
        let score = query_text_score(text, pattern.as_deref(), &terms);
        if score > 0
            && best_match
                .as_ref()
                .is_none_or(|(_, _, best_score)| score > *best_score)
        {
            best_match = Some((id, effective_start, score));
        }
    }
    if let Some((id, start, _)) = best_match {
        return Ok(Some((id, start)));
    }
    Ok(None)
}

fn load_frame_times_for_item(
    conn: &rusqlite::Connection,
    item_id: &str,
) -> anyhow::Result<HashMap<String, f64>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT frame_path, start_sec
        FROM chunks
        WHERE item_id = ?1
          AND chunk_type IN ('keyframe', 'image')
          AND frame_path IS NOT NULL
          AND start_sec IS NOT NULL
        ORDER BY start_sec, id
        "#,
    )?;
    let rows = stmt.query_map([item_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
    })?;

    let mut frame_times = HashMap::new();
    for row in rows {
        let (frame_path, start_sec) = row?;
        frame_times.entry(frame_path).or_insert(start_sec);
    }
    Ok(frame_times)
}

fn literal_pattern_for_terms(query: &str) -> Option<String> {
    let trimmed = query.trim().trim_matches('"').to_lowercase();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn literal_terms_for_query(query: &str) -> Vec<String> {
    query
        .trim()
        .trim_matches('"')
        .to_lowercase()
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn query_text_score(text: &str, pattern: Option<&str>, terms: &[String]) -> usize {
    let normalized = text.to_lowercase();
    let term_weight_sum = terms
        .iter()
        .map(|term| query_term_weight(term))
        .sum::<usize>();
    let exact_score = pattern
        .filter(|pattern| normalized.contains(*pattern))
        .map_or(0, |_| term_weight_sum.max(1) + 1);
    let term_score = terms
        .iter()
        .filter(|term| normalized.contains(term.as_str()))
        .map(|term| query_term_weight(term))
        .sum();
    exact_score.max(term_score)
}

fn query_term_weight(term: &str) -> usize {
    if term.chars().any(|ch| ch.is_ascii_digit()) {
        4
    } else if term.chars().any(|ch| !ch.is_alphanumeric()) {
        3
    } else {
        1
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
    use crate::{sqlite, StorageImageChunk, StorageOcrChunk, StorageTranscriptChunk};

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

    #[test]
    fn build_units_keeps_transcript_windows_with_understanding() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        seed_item(&paths);
        crate::write_media_sqlite_chunks_with_ocr_and_lines(
            &paths,
            "item-1",
            &[
                StorageTranscriptChunk {
                    start: 0.0,
                    end: 10.0,
                    text: "early spoken phrase".to_string(),
                },
                StorageTranscriptChunk {
                    start: 120.0,
                    end: 130.0,
                    text: "late spoken phrase survives understanding".to_string(),
                },
            ],
            &[],
            &[],
            &[],
        )
        .unwrap();
        let conn = sqlite::open(&paths).unwrap();
        conn.execute(
            r#"
            INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, text, metadata)
            VALUES ('item-1:understanding:event:0000', 'item-1', 'understanding', 4, 8, 'visual understanding event', '{}')
            "#,
            [],
        )
        .unwrap();

        let units = rebuild_item_retrieval_units(&paths, "item-1", "profile-1").unwrap();

        assert!(units
            .iter()
            .any(|unit| unit.visual_text.as_deref() == Some("visual understanding event")));
        assert!(units.iter().any(|unit| {
            unit.start_sec.is_some()
                && unit
                    .transcript_text
                    .as_deref()
                    .is_some_and(|text| text.contains("late spoken phrase"))
        }));
    }

    #[test]
    fn build_units_uses_understanding_chunk_as_representative() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        seed_item(&paths);
        let conn = sqlite::open(&paths).unwrap();
        conn.execute(
            r#"
            INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, text, metadata)
            VALUES ('item-1:understanding:event:0000', 'item-1', 'understanding', 40, 45, 'only visual understanding evidence', '{}')
            "#,
            [],
        )
        .unwrap();

        let units = rebuild_item_retrieval_units(&paths, "item-1", "profile-1").unwrap();

        assert_eq!(units.len(), 1);
        assert_eq!(
            units[0].representative_chunk_id.as_deref(),
            Some("item-1:understanding:event:0000")
        );
    }

    #[test]
    fn build_units_preserves_frame_only_windows_in_mixed_videos() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        seed_item(&paths);
        let silent_frame = temp.path().join("silent-frame.jpg");
        crate::write_media_sqlite_chunks_with_ocr_and_lines(
            &paths,
            "item-1",
            &[StorageTranscriptChunk {
                start: 0.0,
                end: 10.0,
                text: "spoken introduction".to_string(),
            }],
            &[],
            &[],
            &[StorageImageChunk::keyframe_at(
                silent_frame.clone(),
                100.0,
                105.0,
            )],
        )
        .unwrap();

        let units = rebuild_item_retrieval_units(&paths, "item-1", "profile-1").unwrap();

        assert!(units.iter().any(|unit| unit.transcript_text.is_some()));
        assert!(units.iter().any(|unit| {
            unit.transcript_text.is_none()
                && unit.ocr_text.is_none()
                && unit.visual_text.is_none()
                && unit.representative_frame_path.as_deref()
                    == Some(silent_frame.to_string_lossy().as_ref())
        }));
    }

    #[test]
    fn build_units_does_not_reuse_skipped_window_indexes_for_frame_units() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        seed_item(&paths);
        let intro_frame = temp.path().join("intro-frame.jpg");
        crate::write_media_sqlite_chunks_with_ocr_and_lines(
            &paths,
            "item-1",
            &[StorageTranscriptChunk {
                start: 35.0,
                end: 45.0,
                text: "spoken section after silent intro".to_string(),
            }],
            &[],
            &[],
            &[StorageImageChunk::keyframe_at(intro_frame, 0.0, 5.0)],
        )
        .unwrap();

        let units = rebuild_item_retrieval_units(&paths, "item-1", "profile-1").unwrap();
        let ids = units
            .iter()
            .map(|unit| unit.id.as_str())
            .collect::<std::collections::HashSet<_>>();

        assert_eq!(ids.len(), units.len());
        assert!(units.iter().any(|unit| unit.transcript_text.is_some()));
        assert!(units.iter().any(|unit| unit.unit_kind == "image"));
    }

    #[test]
    fn best_sub_unit_scores_query_terms_before_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        seed_item(&paths);
        crate::write_media_sqlite_chunks_with_ocr_and_lines(
            &paths,
            "item-1",
            &[],
            &[
                StorageTranscriptChunk {
                    start: 1.0,
                    end: 2.0,
                    text: "database overview".to_string(),
                },
                StorageTranscriptChunk {
                    start: 20.0,
                    end: 21.0,
                    text: "database settings open the config panel".to_string(),
                },
            ],
            &[],
            &[],
        )
        .unwrap();

        let sub_unit =
            best_sub_unit_for_query(&paths, "item-1", Some(0.0), Some(30.0), "database config")
                .unwrap()
                .unwrap();

        assert_eq!(sub_unit.0, "item-1:transcript-line:000001");
        assert_eq!(sub_unit.1, 20.0);
    }

    #[test]
    fn best_sub_unit_includes_chunks_overlapping_window_start() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        seed_item(&paths);
        crate::write_media_sqlite_chunks_with_ocr_and_lines(
            &paths,
            "item-1",
            &[],
            &[
                StorageTranscriptChunk {
                    start: 20.0,
                    end: 32.0,
                    text: "checkout phrase starts before the window".to_string(),
                },
                StorageTranscriptChunk {
                    start: 35.0,
                    end: 40.0,
                    text: "later fallback line".to_string(),
                },
            ],
            &[],
            &[],
        )
        .unwrap();

        let sub_unit =
            best_sub_unit_for_query(&paths, "item-1", Some(25.0), Some(55.0), "checkout")
                .unwrap()
                .unwrap();

        assert_eq!(sub_unit.0, "item-1:transcript-line:000000");
        assert_eq!(sub_unit.1, 20.0);
    }

    #[test]
    fn build_units_preserves_ocr_for_visual_only_video() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        seed_item(&paths);
        let frame_path = temp.path().join("checkout-frame.jpg");
        crate::write_media_sqlite_chunks_with_ocr_and_lines(
            &paths,
            "item-1",
            &[],
            &[],
            &[StorageOcrChunk::frame(
                frame_path.clone(),
                "visible checkout code XR-42",
            )],
            &[StorageImageChunk::keyframe_at(frame_path, 5.0, 10.0)],
        )
        .unwrap();

        let units = rebuild_item_retrieval_units(&paths, "item-1", "profile-1").unwrap();

        assert_eq!(units.len(), 1);
        assert_eq!(
            units[0].ocr_text.as_deref(),
            Some("visible checkout code XR-42")
        );
        assert!(units[0]
            .content_text
            .contains("On-screen text: visible checkout code XR-42"));
    }

    #[test]
    fn build_units_includes_ocr_outside_transcript_span() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        seed_item(&paths);
        let frame_path = temp.path().join("early-checkout-frame.jpg");
        crate::write_media_sqlite_chunks_with_ocr_and_lines(
            &paths,
            "item-1",
            &[StorageTranscriptChunk {
                start: 60.0,
                end: 90.0,
                text: "late spoken checkout narration".to_string(),
            }],
            &[],
            &[StorageOcrChunk::frame(
                frame_path.clone(),
                "early silent code XR-42",
            )],
            &[StorageImageChunk::keyframe_at(frame_path, 5.0, 10.0)],
        )
        .unwrap();

        let units = rebuild_item_retrieval_units(&paths, "item-1", "profile-1").unwrap();

        let early_ocr_unit = units
            .iter()
            .find(|unit| {
                unit.ocr_text
                    .as_deref()
                    .is_some_and(|text| text.contains("XR-42"))
            })
            .expect("early OCR should be copied into a retrieval unit");
        assert!(early_ocr_unit.start_sec.is_some_and(|start| start <= 5.0));
        assert!(units.iter().any(|unit| {
            unit.transcript_text
                .as_deref()
                .is_some_and(|text| text.contains("late spoken checkout"))
        }));
    }

    #[test]
    fn content_text_omits_local_file_paths() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let conn = sqlite::open(&paths).unwrap();
        conn.execute(
            r#"
            INSERT INTO sources (id, type, config, status)
            VALUES ('source-1', 'folder_video', '{"path":"/Users/alice/Videos"}', 'active')
            "#,
            [],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO items (id, source_id, content_type, title, raw_path, status, metadata)
            VALUES ('item-1', 'source-1', 'video', 'Private clip', '/Users/alice/Videos/clip.mp4', 'indexed', '{}')
            "#,
            [],
        )
        .unwrap();
        crate::write_media_sqlite_chunks_with_ocr_and_lines(
            &paths,
            "item-1",
            &[StorageTranscriptChunk {
                start: 0.0,
                end: 5.0,
                text: "searchable spoken text".to_string(),
            }],
            &[],
            &[],
            &[],
        )
        .unwrap();

        let units = rebuild_item_retrieval_units(&paths, "item-1", "profile-1").unwrap();

        assert_eq!(units.len(), 1);
        assert!(!units[0].content_text.contains("/Users/alice"));
        assert!(!units[0].content_text.contains("clip.mp4"));
        assert!(!units[0].content_text.contains("Path:"));
    }

    #[test]
    fn image_units_with_metadata_still_use_image_embedding() {
        let unit = StorageRetrievalUnit {
            id: "item-1:unit:v2:000000".to_string(),
            item_id: "item-1".to_string(),
            unit_index: 0,
            unit_kind: "image".to_string(),
            start_sec: None,
            end_sec: None,
            content_text: "Title: Photo\nTopics/Summary: EXIF DateTimeOriginal: 2026:06:22"
                .to_string(),
            transcript_text: None,
            ocr_text: None,
            visual_text: None,
            summary_text: Some("EXIF DateTimeOriginal: 2026:06:22".to_string()),
            representative_chunk_id: Some("item-1:image:000000".to_string()),
            representative_frame_path: Some("/tmp/photo.jpg".to_string()),
            embedding_profile_id: "profile-1".to_string(),
            index_version: SEARCH_INDEX_VERSION,
            metadata: Value::Null,
        };

        assert!(unit.uses_image_embedding());
    }
}
