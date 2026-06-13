use cerul_models::DiscoveredItem;
use rusqlite::params;
use std::path::Path;

use crate::{sqlite, AppPaths};

#[derive(Debug, Clone, PartialEq)]
pub struct StoredItem {
    pub id: String,
    pub source_id: String,
    pub source_type: String,
    pub source_config: serde_json::Value,
    pub content_type: String,
    pub external_id: Option<String>,
    pub title: Option<String>,
    pub duration_sec: Option<f64>,
    pub raw_path: Option<String>,
    pub status: String,
    pub metadata: serde_json::Value,
}

impl StoredItem {
    pub fn discovery_id(&self) -> &str {
        self.external_id.as_deref().unwrap_or(&self.id)
    }

    pub fn as_discovered_item(&self) -> DiscoveredItem {
        let mut metadata = self.metadata.clone();
        if let Some(raw_path) = &self.raw_path {
            if metadata.get("raw_path").is_none() {
                metadata["raw_path"] = serde_json::Value::String(raw_path.clone());
            }
        }

        DiscoveredItem {
            external_id: self.discovery_id().to_string(),
            title: self.title.clone(),
            duration_sec: self.duration_sec,
            metadata,
        }
    }
}

pub fn get_item(paths: &AppPaths, item_id: &str) -> anyhow::Result<StoredItem> {
    let conn = sqlite::open(paths)?;

    conn.query_row(
        r#"
        SELECT
            i.id,
            i.source_id,
            s.type,
            s.config,
            i.content_type,
            i.external_id,
            i.title,
            i.duration_sec,
            i.raw_path,
            i.status,
            i.metadata
        FROM items i
        JOIN sources s ON s.id = i.source_id
        WHERE i.id = ?1
        "#,
        [item_id],
        |row| {
            let source_config: String = row.get(3)?;
            let metadata: Option<String> = row.get(10)?;

            Ok(StoredItem {
                id: row.get(0)?,
                source_id: row.get(1)?,
                source_type: row.get(2)?,
                source_config: serde_json::from_str(&source_config).map_err(json_error)?,
                content_type: row.get(4)?,
                external_id: row.get(5)?,
                title: row.get(6)?,
                duration_sec: row.get(7)?,
                raw_path: row.get(8)?,
                status: row.get(9)?,
                metadata: match metadata {
                    Some(value) => serde_json::from_str(&value).map_err(json_error)?,
                    None => serde_json::Value::Object(Default::default()),
                },
            })
        },
    )
    .map_err(|error| match error {
        rusqlite::Error::QueryReturnedNoRows => anyhow::anyhow!("item not found: {item_id}"),
        other => anyhow::Error::new(other),
    })
}

pub fn mark_indexed(paths: &AppPaths, item_id: &str) -> anyhow::Result<()> {
    let conn = sqlite::open(paths)?;
    let updated = conn.execute(
        r#"
        UPDATE items
        SET status = 'indexed',
            indexed_at = strftime('%s','now'),
            error = NULL
        WHERE id = ?1
        "#,
        [item_id],
    )?;

    anyhow::ensure!(updated == 1, "item not found: {item_id}");
    Ok(())
}

pub fn set_item_duration(paths: &AppPaths, item_id: &str, duration_sec: f64) -> anyhow::Result<()> {
    anyhow::ensure!(
        duration_sec.is_finite() && duration_sec > 0.0,
        "duration must be positive and finite"
    );
    let conn = sqlite::open(paths)?;
    let updated = conn.execute(
        "UPDATE items SET duration_sec = ?2 WHERE id = ?1",
        params![item_id, duration_sec],
    )?;

    anyhow::ensure!(updated == 1, "item not found: {item_id}");
    Ok(())
}

pub fn set_item_raw_path(paths: &AppPaths, item_id: &str, raw_path: &Path) -> anyhow::Result<()> {
    let raw_path = raw_path.to_string_lossy().into_owned();
    let conn = sqlite::open(paths)?;
    let updated = conn.execute(
        "UPDATE items SET raw_path = ?2 WHERE id = ?1",
        params![item_id, raw_path],
    )?;

    anyhow::ensure!(updated == 1, "item not found: {item_id}");
    drop(conn);
    update_item_metadata(paths, item_id, |metadata| {
        metadata.insert(
            "raw_path".to_string(),
            serde_json::Value::String(raw_path.clone()),
        );
    })
}

pub fn item_ids_for_source(paths: &AppPaths, source_id: &str) -> anyhow::Result<Vec<String>> {
    let conn = sqlite::open(paths)?;
    let mut stmt = conn.prepare("SELECT id FROM items WHERE source_id = ?1 ORDER BY id")?;
    let rows = stmt.query_map([source_id], |row| row.get::<_, String>(0))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn set_video_index_status(
    paths: &AppPaths,
    item_id: &str,
    visual_status: &str,
    visual_error: Option<&str>,
    sampled_frames: usize,
    indexed_frames: usize,
) -> anyhow::Result<()> {
    update_item_metadata(paths, item_id, |metadata| {
        metadata.insert(
            "transcript_index_status".to_string(),
            serde_json::Value::String("indexed".to_string()),
        );
        metadata.insert(
            "visual_index_status".to_string(),
            serde_json::Value::String(visual_status.to_string()),
        );
        metadata.insert(
            "visual_sampled_frames".to_string(),
            serde_json::Value::from(sampled_frames as u64),
        );
        metadata.insert(
            "visual_indexed_frames".to_string(),
            serde_json::Value::from(indexed_frames as u64),
        );

        match visual_error {
            Some(error) => {
                metadata.insert(
                    "visual_index_error".to_string(),
                    serde_json::Value::String(error.to_string()),
                );
            }
            None => {
                metadata.remove("visual_index_error");
            }
        }
    })
}

#[allow(clippy::too_many_arguments)]
pub fn set_video_multimodal_index_status(
    paths: &AppPaths,
    item_id: &str,
    visual_status: &str,
    visual_error: Option<&str>,
    sampled_frames: usize,
    indexed_frames: usize,
    ocr_status: &str,
    ocr_error: Option<&str>,
    ocr_chunks: usize,
) -> anyhow::Result<()> {
    update_item_metadata(paths, item_id, |metadata| {
        metadata.insert(
            "transcript_index_status".to_string(),
            serde_json::Value::String("indexed".to_string()),
        );
        metadata.insert(
            "visual_index_status".to_string(),
            serde_json::Value::String(visual_status.to_string()),
        );
        metadata.insert(
            "visual_sampled_frames".to_string(),
            serde_json::Value::from(sampled_frames as u64),
        );
        metadata.insert(
            "visual_indexed_frames".to_string(),
            serde_json::Value::from(indexed_frames as u64),
        );
        metadata.insert(
            "ocr_index_status".to_string(),
            serde_json::Value::String(ocr_status.to_string()),
        );
        metadata.insert(
            "ocr_indexed_chunks".to_string(),
            serde_json::Value::from(ocr_chunks as u64),
        );

        match visual_error {
            Some(error) => {
                metadata.insert(
                    "visual_index_error".to_string(),
                    serde_json::Value::String(error.to_string()),
                );
            }
            None => {
                metadata.remove("visual_index_error");
            }
        }

        match ocr_error {
            Some(error) => {
                metadata.insert(
                    "ocr_index_error".to_string(),
                    serde_json::Value::String(error.to_string()),
                );
            }
            None => {
                metadata.remove("ocr_index_error");
            }
        }
    })
}

pub fn update_item_metadata<F>(paths: &AppPaths, item_id: &str, updater: F) -> anyhow::Result<()>
where
    F: FnOnce(&mut serde_json::Map<String, serde_json::Value>),
{
    let mut conn = sqlite::open(paths)?;
    // IMMEDIATE takes the write lock up front so the read-modify-write cannot
    // interleave with another writer (e.g. playback PATCH vs. index workers)
    // and silently drop one side's fields.
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let current = tx
        .query_row(
            "SELECT metadata FROM items WHERE id = ?1",
            [item_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => anyhow::anyhow!("item not found: {item_id}"),
            other => anyhow::Error::new(other),
        })?;
    let mut metadata = match current {
        Some(value) if !value.trim().is_empty() => serde_json::from_str(&value)?,
        _ => serde_json::Value::Object(Default::default()),
    };
    if !metadata.is_object() {
        metadata = serde_json::Value::Object(Default::default());
    }

    updater(metadata.as_object_mut().expect("metadata is an object"));

    let serialized = serde_json::to_string(&metadata)?;
    let updated = tx.execute(
        "UPDATE items SET metadata = ?2 WHERE id = ?1",
        params![item_id, serialized],
    )?;

    anyhow::ensure!(updated == 1, "item not found: {item_id}");
    tx.commit()?;
    Ok(())
}

fn json_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}
