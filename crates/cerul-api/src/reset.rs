use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path as FsPath, PathBuf},
};

use axum::{extract::State, Json};
use cerul_storage::AppPaths;
use rusqlite::OptionalExtension;
use serde_json::{json, Value};

use crate::{ApiResult, ApiState};

pub(crate) async fn reset_local_library(State(state): State<ApiState>) -> ApiResult<Json<Value>> {
    let reset = reset_local_library_database(&state.paths)?;
    Ok(Json(json!({
        "status": "ok",
        "cleared": reset.cleared,
        "compacted": reset.compacted,
        "compaction_error": reset.compaction_error,
        "download_targets": reset.download_targets,
    })))
}

#[derive(Debug)]
struct LibraryResetResult {
    cleared: BTreeMap<String, usize>,
    compacted: bool,
    compaction_error: Option<String>,
    download_targets: Vec<String>,
}

fn reset_local_library_database(paths: &AppPaths) -> anyhow::Result<LibraryResetResult> {
    let mut conn = cerul_storage::sqlite::open(paths)?;
    let download_targets = local_library_download_targets(paths, &conn)?;
    let tx = conn.transaction()?;
    let mut cleared = BTreeMap::new();

    for (label, sql) in [
        (
            "usage_events",
            "DELETE FROM inference_usage_events WHERE item_id IS NOT NULL OR job_id IS NOT NULL",
        ),
        ("moments", "DELETE FROM moments"),
        ("retrieval_units", "DELETE FROM retrieval_units"),
        ("chunks", "DELETE FROM chunks"),
        ("item_understandings", "DELETE FROM item_understandings"),
        ("ignored_items", "DELETE FROM ignored_items"),
        ("jobs", "DELETE FROM jobs"),
        ("items", "DELETE FROM items"),
        ("sources", "DELETE FROM sources"),
    ] {
        let rows = tx.execute(sql, [])?;
        cleared.insert(label.to_string(), rows);
    }

    tx.commit()?;
    let compaction_error = compact_library_database(&conn).err().map(|error| {
        let message = error.to_string();
        tracing::warn!(%message, "failed to compact SQLite database after local library reset");
        message
    });
    Ok(LibraryResetResult {
        cleared,
        compacted: compaction_error.is_none(),
        compaction_error,
        download_targets,
    })
}

fn local_library_download_targets(
    paths: &AppPaths,
    conn: &rusqlite::Connection,
) -> anyhow::Result<Vec<String>> {
    let mut targets = BTreeSet::new();
    if let Some(media_dir) = read_setting_string(conn, "media_dir")? {
        targets.insert(PathBuf::from(media_dir).join("sources"));
    }

    let mut stmt = conn.prepare(
        r#"
        SELECT s.type, s.config, i.raw_path, i.metadata
        FROM items i
        JOIN sources s ON s.id = i.source_id
        WHERE s.type IN ('youtube', 'web_video', 'rss_podcast')
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;
    for row in rows {
        let (source_type, config, raw_path, metadata) = row?;
        if let Some(target) = download_target_from_source_config(&config, &source_type) {
            targets.insert(target);
        }
        let mut candidates = Vec::new();
        if let Some(raw_path) = raw_path {
            candidates.push(raw_path);
        }
        if let Some(raw_path) = metadata.as_deref().and_then(metadata_raw_path) {
            candidates.push(raw_path);
        }
        for candidate in candidates {
            if let Some(target) = download_target_from_raw_path(&candidate, &source_type) {
                targets.insert(target);
            }
        }
    }

    Ok(targets
        .into_iter()
        .filter(|target| target != &paths.cache)
        .filter(|target| !reset_target_conflicts_with_preserved_path(target, &paths.models))
        .map(|target| target.to_string_lossy().to_string())
        .collect())
}

fn read_setting_string(conn: &rusqlite::Connection, key: &str) -> anyhow::Result<Option<String>> {
    let raw: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(raw
        .and_then(|value| serde_json::from_str::<Value>(&value).ok())
        .and_then(|value| value.as_str().map(str::to_string))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty()))
}

fn metadata_raw_path(metadata: &str) -> Option<String> {
    serde_json::from_str::<Value>(metadata)
        .ok()
        .and_then(|value| {
            value
                .get("raw_path")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

fn download_target_from_source_config(config: &str, source_type: &str) -> Option<PathBuf> {
    let cache_dir = serde_json::from_str::<Value>(config)
        .ok()
        .and_then(|value| {
            value
                .get("cache_dir")
                .and_then(Value::as_str)
                .map(str::to_string)
        })?;
    download_target_from_cache_dir(&cache_dir, source_type)
}

fn download_target_from_cache_dir(cache_dir: &str, source_type: &str) -> Option<PathBuf> {
    let cache_dir = FsPath::new(cache_dir.trim());
    if cache_dir.as_os_str().is_empty() {
        return None;
    }
    if file_name_eq(cache_dir, "sources") {
        return Some(cache_dir.to_path_buf());
    }
    if file_name_eq(cache_dir, source_type) {
        let parent = cache_dir.parent()?;
        if file_name_eq(parent, "sources") {
            return Some(parent.to_path_buf());
        }
    }
    None
}

fn download_target_from_raw_path(raw_path: &str, source_type: &str) -> Option<PathBuf> {
    let mut current = FsPath::new(raw_path.trim()).parent();
    while let Some(dir) = current {
        if file_name_eq(dir, source_type) {
            let parent = dir.parent()?;
            if file_name_eq(parent, "sources") {
                return Some(parent.to_path_buf());
            }
        }
        current = dir.parent();
    }
    None
}

fn file_name_eq(path: &FsPath, expected: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == expected)
}

fn reset_target_conflicts_with_preserved_path(target: &FsPath, preserved: &FsPath) -> bool {
    path_contains(target, preserved) || path_contains(preserved, target)
}

fn path_contains(parent: &FsPath, candidate: &FsPath) -> bool {
    candidate == parent || candidate.starts_with(parent)
}

fn compact_library_database(conn: &rusqlite::Connection) -> anyhow::Result<()> {
    checkpoint_wal(conn)?;
    conn.execute_batch("VACUUM")?;
    checkpoint_wal(conn)?;
    Ok(())
}

fn checkpoint_wal(conn: &rusqlite::Connection) -> anyhow::Result<()> {
    let busy: i64 = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))?;
    anyhow::ensure!(busy == 0, "SQLite WAL checkpoint was busy");
    Ok(())
}
