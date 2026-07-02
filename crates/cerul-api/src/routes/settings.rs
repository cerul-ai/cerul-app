use std::collections::BTreeMap;

use axum::{extract::State, Json};
use cerul_storage::AppPaths;
use serde_json::Value;

use crate::{
    normalize_inference_mode, parse_api_port, parse_json, setting_string,
    sync_inference_mode_side_effects, ApiError, ApiResult, ApiState, API_PORT_SETTING,
};

const LEGACY_CLOUD_SETTING_KEYS: &[&str] = &[
    "cloud_api_key",
    "cloud_connected",
    "cloud_account_email",
    "cloud_email",
    "cloud_plan",
    "cloud_quota_percent",
];

const INTERNAL_SETTING_KEYS: &[&str] = &[
    crate::DEFERRED_EMBEDDING_REBUILD_MODE_SETTING,
    crate::INDEXING_SCHEMA_VERSION_SETTING,
    crate::VECTOR_INDEX_BACKEND_SETTING,
    // Computed flag returned by list_settings; never persisted.
    "remote_api_key_set",
];

/// Settings that clients may write but must never read back in plaintext.
const SECRET_SETTING_KEYS: &[&str] = &["remote_api_key"];

pub(crate) async fn list_settings(
    State(state): State<ApiState>,
) -> ApiResult<Json<BTreeMap<String, Value>>> {
    let conn = cerul_storage::sqlite::open(&state.paths)?;
    remove_legacy_cloud_settings(&conn)?;
    let mut stmt = conn.prepare("SELECT key, value FROM settings ORDER BY key ASC")?;
    let rows = stmt.query_map([], |row| {
        let key: String = row.get(0)?;
        let value: String = row.get(1)?;
        Ok((key, parse_json(&value)))
    })?;

    let all = rows.collect::<Result<BTreeMap<_, _>, _>>()?;
    let remote_key_set = all
        .get("remote_api_key")
        .and_then(|value| value.as_str())
        .map(|key| !key.trim().is_empty())
        .unwrap_or(false);

    let mut visible: BTreeMap<String, Value> = all
        .into_iter()
        .filter(|(key, _)| !is_hidden_setting(key))
        .map(|(key, value)| {
            let value = normalize_setting_value(&key, value);
            (key, value)
        })
        .collect();
    // The key itself is write-only; expose only whether one is configured.
    visible.insert(
        "remote_api_key_set".to_string(),
        Value::Bool(remote_key_set),
    );

    Ok(Json(visible))
}

pub(crate) async fn update_settings(
    State(state): State<ApiState>,
    Json(settings): Json<BTreeMap<String, Value>>,
) -> ApiResult<Json<BTreeMap<String, Value>>> {
    let previous_inference_mode = configured_inference_mode(&state.paths)?;
    let requested_inference_mode = requested_inference_mode(&settings);
    let mut conn = cerul_storage::sqlite::open(&state.paths)?;
    let tx = conn.transaction()?;
    for (key, value) in &settings {
        if is_legacy_cloud_setting(key) {
            tx.execute("DELETE FROM settings WHERE key = ?1", [key])?;
            continue;
        }
        if is_internal_setting(key) {
            continue;
        }
        let value = validate_write_setting_value(key, normalize_setting_value(key, value.clone()))?;
        tx.execute(
            r#"
            INSERT INTO settings (key, value, updated_at)
            VALUES (?1, ?2, strftime('%s','now'))
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = excluded.updated_at
            "#,
            (key, value.to_string()),
        )?;
    }
    tx.commit()?;

    if let Some(inference_mode) = requested_inference_mode.as_deref() {
        sync_inference_mode_side_effects(&state.paths, &previous_inference_mode, inference_mode)?;
    }

    Ok(Json(
        settings
            .into_iter()
            .filter(|(key, _)| !is_hidden_setting(key))
            .map(|(key, value)| {
                let value = normalize_setting_value(&key, value);
                (key, value)
            })
            .collect(),
    ))
}

fn remove_legacy_cloud_settings(conn: &rusqlite::Connection) -> anyhow::Result<usize> {
    let mut removed = 0;
    for key in LEGACY_CLOUD_SETTING_KEYS {
        removed += conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
    }
    Ok(removed)
}

fn is_legacy_cloud_setting(key: &str) -> bool {
    LEGACY_CLOUD_SETTING_KEYS.contains(&key)
}

fn is_internal_setting(key: &str) -> bool {
    INTERNAL_SETTING_KEYS.contains(&key)
}

fn is_secret_setting(key: &str) -> bool {
    SECRET_SETTING_KEYS.contains(&key)
}

fn is_hidden_setting(key: &str) -> bool {
    is_legacy_cloud_setting(key) || is_internal_setting(key) || is_secret_setting(key)
}

pub(crate) fn normalize_setting_value(key: &str, value: Value) -> Value {
    if key == "inference_mode" {
        return Value::String(
            value
                .as_str()
                .map(normalize_inference_mode)
                .unwrap_or_else(|| "remote".to_string()),
        );
    }
    value
}

fn validate_write_setting_value(key: &str, value: Value) -> ApiResult<Value> {
    if key == API_PORT_SETTING {
        let port = match &value {
            Value::Number(number) => number.as_u64().and_then(|value| u16::try_from(value).ok()),
            Value::String(value) => parse_api_port(value),
            _ => None,
        }
        .filter(|port| (1024..=65535).contains(port))
        .ok_or_else(|| ApiError::bad_request("api_port must be an integer from 1024 to 65535"))?;
        return Ok(Value::from(port));
    }
    Ok(value)
}

fn requested_inference_mode(settings: &BTreeMap<String, Value>) -> Option<String> {
    settings
        .get("inference_mode")
        .and_then(Value::as_str)
        .map(normalize_inference_mode)
}

pub(crate) fn configured_inference_mode(paths: &AppPaths) -> anyhow::Result<String> {
    Ok(setting_string(paths, "inference_mode")?
        .as_deref()
        .map(normalize_inference_mode)
        .unwrap_or_else(|| "auto".to_string()))
}
