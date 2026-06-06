use anyhow::Context;
use async_trait::async_trait;
use cerul_models::{ContentType, DiscoveredItem};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use crate::SourcePlugin;

static CONTENT_TYPES: [ContentType; 1] = [ContentType::Video];
const EXTENSIONS: &[&str] = &["mp4", "mkv", "webm", "mov", "m4v"];

/// Source plugin for a single video file (as opposed to a folder).
///
/// Configured with `{ "path": "/path/to/clip.mp4" }`. `discover()` yields
/// exactly one item if the file is a supported video format, otherwise an
/// empty list. `fetch()` returns the file path unchanged (no copy).
#[derive(Debug, Clone)]
pub struct FileVideo {
    path: PathBuf,
}

impl FileVideo {
    pub fn new(config: serde_json::Value) -> anyhow::Result<Self> {
        let path = config
            .get("path")
            .and_then(|value| value.as_str())
            .context("file_video requires config.path")?;
        let path = PathBuf::from(shellexpand::tilde(path).into_owned());

        if !path.is_file() {
            anyhow::bail!("not a file: {}", path.display());
        }

        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase());

        match extension {
            Some(ext) if EXTENSIONS.contains(&ext.as_str()) => {}
            Some(ext) => anyhow::bail!(
                "unsupported video extension '{ext}'; expected one of: {EXTENSIONS:?}"
            ),
            None => anyhow::bail!("file has no extension: {}", path.display()),
        }

        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[async_trait]
impl SourcePlugin for FileVideo {
    fn name(&self) -> &'static str {
        "file_video"
    }

    fn content_types(&self) -> &[ContentType] {
        &CONTENT_TYPES
    }

    async fn discover(&self) -> anyhow::Result<Vec<DiscoveredItem>> {
        let metadata = std::fs::metadata(&self.path)
            .with_context(|| format!("failed to read metadata for {}", self.path.display()))?;
        let modified_at = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let raw_path = self.path.to_string_lossy().into_owned();
        let extension = self
            .path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .unwrap_or_default();
        let id_input = format!("{}:{}:{}", raw_path, metadata.len(), modified_at);
        let id = blake3::hash(id_input.as_bytes()).to_hex()[..16].to_string();

        Ok(vec![DiscoveredItem {
            external_id: id,
            title: self
                .path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(ToOwned::to_owned),
            duration_sec: None,
            metadata: json!({
                "raw_path": raw_path,
                "size_bytes": metadata.len(),
                "modified_at": modified_at,
                "extension": extension,
            }),
        }])
    }

    async fn fetch(&self, item: &DiscoveredItem) -> anyhow::Result<PathBuf> {
        let raw_path = item
            .metadata
            .get("raw_path")
            .and_then(|value| value.as_str())
            .context("file_video item is missing metadata.raw_path")?;
        let path = PathBuf::from(raw_path);

        if !path.is_file() {
            anyhow::bail!("source file does not exist: {}", path.display());
        }

        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn discovers_single_video_file() {
        let temp = tempfile::tempdir().unwrap();
        let video = temp.path().join("sample.mp4");
        std::fs::write(&video, b"video bytes").unwrap();

        let source = FileVideo::new(json!({ "path": video })).unwrap();
        let items = source.discover().await.unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title.as_deref(), Some("sample"));
        assert_eq!(items[0].metadata["raw_path"].as_str(), video.to_str());
        assert_eq!(items[0].metadata["extension"].as_str(), Some("mp4"));
    }

    #[tokio::test]
    async fn fetch_returns_file_path() {
        let temp = tempfile::tempdir().unwrap();
        let video = temp.path().join("clip.MKV");
        std::fs::write(&video, b"video").unwrap();

        let source = FileVideo::new(json!({ "path": video.clone() })).unwrap();
        let item = source.discover().await.unwrap().pop().unwrap();
        assert_eq!(source.fetch(&item).await.unwrap(), video);
    }

    #[test]
    fn rejects_missing_file() {
        let error = FileVideo::new(json!({ "path": "/definitely/not/cerul/sample.mp4" }))
            .unwrap_err()
            .to_string();
        assert!(error.contains("not a file"));
    }

    #[test]
    fn rejects_directory_path() {
        let temp = tempfile::tempdir().unwrap();
        let error = FileVideo::new(json!({ "path": temp.path() }))
            .unwrap_err()
            .to_string();
        assert!(error.contains("not a file"));
    }

    #[test]
    fn rejects_unsupported_extension() {
        let temp = tempfile::tempdir().unwrap();
        let txt = temp.path().join("notes.txt");
        std::fs::write(&txt, b"hello").unwrap();
        let error = FileVideo::new(json!({ "path": txt }))
            .unwrap_err()
            .to_string();
        assert!(error.contains("unsupported video extension"));
    }
}
