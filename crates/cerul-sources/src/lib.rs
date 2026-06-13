use async_trait::async_trait;
use cerul_models::{ContentType, DiscoveredItem};
use std::{path::PathBuf, sync::Arc};

pub mod file_video;
pub mod folder_audio;
pub mod folder_image;
pub mod folder_video;
pub mod rss_podcast;
pub mod web_video;
pub mod youtube;

/// Default wall-clock limits for yt-dlp subprocesses. Without a ceiling a
/// hung yt-dlp (bot checks, stalled network) pins the calling HTTP request
/// or indexing job forever. Explicit `timeout_sec` config still wins.
pub(crate) fn default_ytdlp_timeout(phase: &str) -> std::time::Duration {
    if phase.contains("discovery") {
        std::time::Duration::from_secs(120)
    } else {
        std::time::Duration::from_secs(3600)
    }
}

pub const REGISTERED_PLUGIN_TYPES: &[&str] = &[
    "folder_video",
    "folder_audio",
    "folder_image",
    "file_video",
    "youtube",
    "web_video",
    "rss_podcast",
];

pub type FetchProgress = Arc<dyn Fn(f64, String) + Send + Sync + 'static>;

#[async_trait]
pub trait SourcePlugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn content_types(&self) -> &[ContentType];

    async fn discover(&self) -> anyhow::Result<Vec<DiscoveredItem>>;
    async fn fetch(&self, item: &DiscoveredItem) -> anyhow::Result<PathBuf>;

    async fn fetch_with_progress(
        &self,
        item: &DiscoveredItem,
        _progress: Option<FetchProgress>,
    ) -> anyhow::Result<PathBuf> {
        self.fetch(item).await
    }

    async fn cleanup(&self, _item: &DiscoveredItem) -> anyhow::Result<()> {
        Ok(())
    }
}

pub fn build(
    plugin_type: &str,
    config: serde_json::Value,
) -> anyhow::Result<Box<dyn SourcePlugin>> {
    match plugin_type {
        "folder_video" => Ok(Box::new(folder_video::FolderVideo::new(config)?)),
        "folder_audio" => Ok(Box::new(folder_audio::FolderAudio::new(config)?)),
        "folder_image" => Ok(Box::new(folder_image::FolderImage::new(config)?)),
        "file_video" => Ok(Box::new(file_video::FileVideo::new(config)?)),
        "youtube" => Ok(Box::new(youtube::YouTube::new(config)?)),
        "web_video" => Ok(Box::new(web_video::WebVideo::new(config)?)),
        "rss_podcast" => Ok(Box::new(rss_podcast::RssPodcast::new(config)?)),
        _ => anyhow::bail!("unknown source plugin: {plugin_type}"),
    }
}

pub fn crate_ready() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn registry_resolves_all_known_plugins() {
        let temp = tempfile::tempdir().unwrap();
        let video_file = temp.path().join("sample.mp4");
        std::fs::write(&video_file, b"video").unwrap();
        let cases = [
            (
                "folder_video",
                json!({ "path": temp.path() }),
                "folder_video",
                &[ContentType::Video][..],
            ),
            (
                "file_video",
                json!({ "path": video_file }),
                "file_video",
                &[ContentType::Video][..],
            ),
            (
                "folder_audio",
                json!({ "path": temp.path() }),
                "folder_audio",
                &[ContentType::Audio][..],
            ),
            (
                "folder_image",
                json!({ "path": temp.path() }),
                "folder_image",
                &[ContentType::Image][..],
            ),
            (
                "youtube",
                json!({ "url": "https://www.youtube.com/@cerul" }),
                "youtube",
                &[ContentType::Video][..],
            ),
            (
                "web_video",
                json!({ "url": "https://www.youtube.com/watch?v=abc123" }),
                "web_video",
                &[ContentType::Video][..],
            ),
            (
                "rss_podcast",
                json!({ "url": "https://example.com/feed.xml" }),
                "rss_podcast",
                &[ContentType::Audio][..],
            ),
        ];

        for (plugin_type, config, name, content_types) in cases {
            let plugin = build(plugin_type, config).unwrap();
            assert_eq!(plugin.name(), name);
            assert_eq!(plugin.content_types(), content_types);
        }
    }

    #[test]
    fn registry_rejects_unknown_plugin_type() {
        match build("unknown", json!({})) {
            Ok(_) => panic!("unknown plugin type should be rejected"),
            Err(error) => assert!(error.to_string().contains("unknown source plugin")),
        }
    }
}
