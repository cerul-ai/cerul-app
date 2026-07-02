use std::path::{Path, PathBuf};

use cerul_storage::AppPaths;
use serde_json::{Map, Value};

use super::extract::update_item_duration_from_media;

const WEB_VIDEO_COOKIE_MODE_SETTING: &str = "web_video_cookie_mode";
const WEB_VIDEO_COOKIE_BROWSER_SETTING: &str = "web_video_cookie_browser";
const WEB_VIDEO_COOKIES_PATH_SETTING: &str = "web_video_cookies_path";

pub(crate) async fn fetch_video_media(
    paths: &AppPaths,
    item: &cerul_storage::StoredItem,
    progress: Option<cerul_sources::FetchProgress>,
) -> anyhow::Result<PathBuf> {
    let source = cerul_sources::build(
        &item.source_type,
        source_config_with_app_cache(paths, &item.source_type, item.source_config.clone()),
    )?;
    let video_path = source
        .fetch_with_progress(&item.as_discovered_item(), progress)
        .await?;
    sync_raw_path_for_source(paths, item, &video_path, &["web_video", "youtube"])?;
    update_item_duration_from_media(paths, &item.id, &video_path).await;
    Ok(video_path)
}

pub(crate) async fn fetch_audio_media(
    paths: &AppPaths,
    item: &cerul_storage::StoredItem,
) -> anyhow::Result<PathBuf> {
    let source = cerul_sources::build(
        &item.source_type,
        source_config_with_app_cache(paths, &item.source_type, item.source_config.clone()),
    )?;
    let audio_path = source.fetch(&item.as_discovered_item()).await?;
    sync_raw_path_for_source(paths, item, &audio_path, &["rss_podcast"])?;
    update_item_duration_from_media(paths, &item.id, &audio_path).await;
    Ok(audio_path)
}

pub(crate) async fn fetch_image_media(
    paths: &AppPaths,
    item: &cerul_storage::StoredItem,
) -> anyhow::Result<PathBuf> {
    let source = cerul_sources::build(
        &item.source_type,
        source_config_with_app_cache(paths, &item.source_type, item.source_config.clone()),
    )?;
    source.fetch(&item.as_discovered_item()).await
}

pub(crate) async fn fetch_document_media(
    paths: &AppPaths,
    item: &cerul_storage::StoredItem,
) -> anyhow::Result<PathBuf> {
    let source = cerul_sources::build(
        &item.source_type,
        source_config_with_app_cache(paths, &item.source_type, item.source_config.clone()),
    )?;
    source.fetch(&item.as_discovered_item()).await
}

fn sync_raw_path_for_source(
    paths: &AppPaths,
    item: &cerul_storage::StoredItem,
    fetched_path: &Path,
    source_types: &[&str],
) -> anyhow::Result<()> {
    if source_types.contains(&item.source_type.as_str())
        && item.raw_path.as_deref() != fetched_path.to_str()
    {
        cerul_storage::set_item_raw_path(paths, &item.id, fetched_path)?;
    }
    Ok(())
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_config_with_app_cache_preserves_existing_cache_dir() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path().join("app")).unwrap();

        let config = source_config_with_app_cache(
            &paths,
            "web_video",
            serde_json::json!({
                "url": "https://example.com/video",
                "cache_dir": "/custom/cache",
            }),
        );

        assert_eq!(config["cache_dir"], "/custom/cache");
    }
}
