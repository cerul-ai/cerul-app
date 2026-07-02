use std::path::{Path, PathBuf};

use cerul_storage::AppPaths;
use serde_json::{Map, Value};

const WEB_VIDEO_COOKIE_MODE_SETTING: &str = "web_video_cookie_mode";
const WEB_VIDEO_COOKIE_BROWSER_SETTING: &str = "web_video_cookie_browser";
const WEB_VIDEO_COOKIES_PATH_SETTING: &str = "web_video_cookies_path";

pub(crate) fn source_config_with_app_cache(
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
            source_download_dir(paths, source_type)
                .to_string_lossy()
                .into_owned(),
        )
    });
    apply_ytdlp_access_settings(paths, source_type, &mut object);
    serde_json::Value::Object(object)
}

// Resolve where a source's downloaded media is written. Defaults to the app
// cache (`<data>/cache/sources/<type>`), but honors a user-chosen download
// directory so large video files can live on an external disk.
fn source_download_dir(paths: &AppPaths, source_type: &str) -> PathBuf {
    match cerul_storage::read_string_setting(paths, "media_dir") {
        Ok(Some(dir)) => Path::new(&dir).join("sources").join(source_type),
        Ok(None) => paths.source_cache_dir(source_type),
        Err(error) => {
            tracing::warn!(%error, "failed to read media_dir setting; using default cache dir");
            paths.source_cache_dir(source_type)
        }
    }
}

fn apply_ytdlp_access_settings(
    paths: &AppPaths,
    source_type: &str,
    object: &mut Map<String, Value>,
) {
    if !matches!(source_type, "youtube" | "web_video") || has_source_cookie_config(object) {
        return;
    }

    let mode = setting_string(paths, WEB_VIDEO_COOKIE_MODE_SETTING)
        .unwrap_or_else(|| "browser".to_string())
        .trim()
        .to_ascii_lowercase();
    match mode.as_str() {
        "browser" => {
            let browser = setting_string(paths, WEB_VIDEO_COOKIE_BROWSER_SETTING)
                .unwrap_or_else(|| "chrome".to_string());
            let browser = browser.trim();
            if !browser.is_empty() {
                object.insert(
                    "cookies_from_browser".to_string(),
                    Value::String(browser.to_string()),
                );
            }
        }
        "file" => {
            if let Some(path) = setting_string(paths, WEB_VIDEO_COOKIES_PATH_SETTING) {
                let path = path.trim();
                if !path.is_empty() {
                    object.insert("cookies_path".to_string(), Value::String(path.to_string()));
                }
            }
        }
        _ => {}
    }
}

fn has_source_cookie_config(object: &Map<String, Value>) -> bool {
    [
        "cookies_from_browser",
        "cookie_browser",
        "ytdlp_cookies_from_browser",
        "ytdlp_cookie_browser",
        "cookies_path",
        "cookies_file",
        "ytdlp_cookies_path",
        "ytdlp_cookies_file",
    ]
    .iter()
    .any(|key| {
        object
            .get(*key)
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    })
}

fn setting_string(paths: &AppPaths, key: &str) -> Option<String> {
    let conn = cerul_storage::sqlite::open(paths).ok()?;
    let raw: String = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .ok()?;
    match serde_json::from_str::<Value>(&raw).unwrap_or(Value::String(raw)) {
        Value::String(value) => Some(value),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}
