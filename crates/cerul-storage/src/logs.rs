use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Map, Value};

use crate::AppPaths;

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

    fs::create_dir_all(paths.logs_dir())?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file_path(paths, file_name))?;
    serde_json::to_writer(&mut file, &event)?;
    file.write_all(b"\n")?;
    Ok(())
}

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
