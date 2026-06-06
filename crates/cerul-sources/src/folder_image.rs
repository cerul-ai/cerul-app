use anyhow::Context;
use async_trait::async_trait;
use cerul_models::{ContentType, DiscoveredItem};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use crate::SourcePlugin;

static CONTENT_TYPES: [ContentType; 1] = [ContentType::Image];
const EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "heic", "webp"];

#[derive(Debug, Clone)]
pub struct FolderImage {
    path: PathBuf,
}

impl FolderImage {
    pub fn new(config: serde_json::Value) -> anyhow::Result<Self> {
        let path = config
            .get("path")
            .and_then(|value| value.as_str())
            .context("folder_image requires config.path")?;
        let path = PathBuf::from(shellexpand::tilde(path).into_owned());

        if !path.is_dir() {
            anyhow::bail!("not a directory: {}", path.display());
        }

        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[async_trait]
impl SourcePlugin for FolderImage {
    fn name(&self) -> &'static str {
        "folder_image"
    }

    fn content_types(&self) -> &[ContentType] {
        &CONTENT_TYPES
    }

    async fn discover(&self) -> anyhow::Result<Vec<DiscoveredItem>> {
        let mut items = Vec::new();

        for entry in walkdir::WalkDir::new(&self.path) {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            let Some(extension) = path
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| extension.to_ascii_lowercase())
            else {
                continue;
            };

            if !EXTENSIONS.contains(&extension.as_str()) {
                continue;
            }

            let metadata = entry.metadata()?;
            let modified_at = metadata
                .modified()?
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let raw_path = path.to_string_lossy().into_owned();
            let id_input = format!("{}:{}:{}", raw_path, metadata.len(), modified_at);
            let id = blake3::hash(id_input.as_bytes()).to_hex()[..16].to_string();

            items.push(DiscoveredItem {
                external_id: id,
                title: path
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
            });
        }

        items.sort_by(|left, right| {
            left.metadata["raw_path"]
                .as_str()
                .cmp(&right.metadata["raw_path"].as_str())
        });

        Ok(items)
    }

    async fn fetch(&self, item: &DiscoveredItem) -> anyhow::Result<PathBuf> {
        let raw_path = item
            .metadata
            .get("raw_path")
            .and_then(|value| value.as_str())
            .context("folder_image item is missing metadata.raw_path")?;
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
    async fn discovers_image_files_recursively() {
        let temp = tempfile::tempdir().unwrap();
        let nested = temp.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(temp.path().join("photo.jpg"), b"image").unwrap();
        std::fs::write(nested.join("diagram.PNG"), b"image").unwrap();
        std::fs::write(temp.path().join("notes.txt"), b"not image").unwrap();

        let source = FolderImage::new(json!({ "path": temp.path() })).unwrap();
        let items = source.discover().await.unwrap();

        assert_eq!(items.len(), 2);
        assert!(items
            .iter()
            .any(|item| item.title.as_deref() == Some("photo")));
        assert!(items
            .iter()
            .any(|item| item.title.as_deref() == Some("diagram")));
        assert!(items
            .iter()
            .all(|item| item.metadata["raw_path"].as_str().is_some()));
    }

    #[tokio::test]
    async fn fetch_returns_discovered_file_path() {
        let temp = tempfile::tempdir().unwrap();
        let image = temp.path().join("photo.webp");
        std::fs::write(&image, b"image").unwrap();

        let source = FolderImage::new(json!({ "path": temp.path() })).unwrap();
        let item = source.discover().await.unwrap().pop().unwrap();

        assert_eq!(source.fetch(&item).await.unwrap(), image);
    }

    #[test]
    fn rejects_missing_directory() {
        let error = FolderImage::new(json!({ "path": "/definitely/not/cerul/images" }))
            .unwrap_err()
            .to_string();

        assert!(error.contains("not a directory"));
    }
}
