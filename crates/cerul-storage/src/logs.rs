use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Map, Value};

use crate::AppPaths;

static JSONL_LOG_LOCK: Mutex<()> = Mutex::new(());

pub fn log_file_path(paths: &AppPaths, file_name: &str) -> PathBuf {
    paths.logs_dir().join(file_name)
}

pub fn append_jsonl_event(
    paths: &AppPaths,
    file_name: &str,
    mut event: Value,
) -> anyhow::Result<()> {
    if let Value::Object(object) = &mut event {
        object
            .entry("ts".to_string())
            .or_insert_with(|| Value::from(unix_timestamp_secs()));
    } else {
        let mut object = Map::new();
        object.insert("ts".to_string(), Value::from(unix_timestamp_secs()));
        object.insert("event".to_string(), event);
        event = Value::Object(object);
    }

    let mut line = serde_json::to_vec(&event)?;
    line.push(b'\n');

    let _guard = JSONL_LOG_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    fs::create_dir_all(paths.logs_dir())?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file_path(paths, file_name))?;
    file.write_all(&line)?;
    Ok(())
}

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
