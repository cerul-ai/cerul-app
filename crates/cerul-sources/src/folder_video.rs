use anyhow::Context;
use async_trait::async_trait;
use cerul_models::{ContentType, DiscoveredItem};
use notify::Watcher;
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use crate::SourcePlugin;

static CONTENT_TYPES: [ContentType; 1] = [ContentType::Video];
const EXTENSIONS: &[&str] = &["mp4", "mkv", "webm", "mov", "m4v"];
const IGNORED_DIR_NAMES: &[&str] = &[
    ".cache",
    ".git",
    ".hg",
    ".svn",
    ".trash",
    "__macosx",
    "cache",
    "caches",
    "com.lveditor.draft",
    "node_modules",
    "templatedraft",
    "videoalg",
];

#[derive(Debug, Clone)]
pub struct FolderVideo {
    path: PathBuf,
}

impl FolderVideo {
    pub fn new(config: serde_json::Value) -> anyhow::Result<Self> {
        let path = config
            .get("path")
            .and_then(|value| value.as_str())
            .context("folder_video requires config.path")?;
        let path = PathBuf::from(shellexpand::tilde(path).into_owned());

        if !path.is_dir() {
            anyhow::bail!("not a directory: {}", path.display());
        }

        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn watch<F>(&self, on_event: F) -> notify::Result<notify::RecommendedWatcher>
    where
        F: FnMut(notify::Result<notify::Event>) + Send + 'static,
    {
        let mut watcher = notify::recommended_watcher(on_event)?;
        watcher.watch(&self.path, notify::RecursiveMode::Recursive)?;
        Ok(watcher)
    }
}

#[async_trait]
impl SourcePlugin for FolderVideo {
    fn name(&self) -> &'static str {
        "folder_video"
    }

    fn content_types(&self) -> &[ContentType] {
        &CONTENT_TYPES
    }

    async fn discover(&self) -> anyhow::Result<Vec<DiscoveredItem>> {
        let mut items = Vec::new();

        for entry in walkdir::WalkDir::new(&self.path)
            .into_iter()
            .filter_entry(|entry| {
                entry.depth() == 0 || should_visit_entry(entry.path(), entry.file_type().is_dir())
            })
        {
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
            .context("folder_video item is missing metadata.raw_path")?;
        let path = PathBuf::from(raw_path);

        if !path.is_file() {
            anyhow::bail!("source file does not exist: {}", path.display());
        }

        Ok(path)
    }
}

fn should_visit_entry(path: &Path, is_dir: bool) -> bool {
    if !is_dir {
        return true;
    }
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return true;
    };
    let lower = name.to_ascii_lowercase();
    !IGNORED_DIR_NAMES.contains(&lower.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn discovers_video_files_recursively() {
        let temp = tempfile::tempdir().unwrap();
        let nested = temp.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(temp.path().join("sample.mp4"), b"video").unwrap();
        std::fs::write(nested.join("clip.MOV"), b"video").unwrap();
        std::fs::write(temp.path().join("notes.txt"), b"not video").unwrap();

        let source = FolderVideo::new(json!({ "path": temp.path() })).unwrap();
        let items = source.discover().await.unwrap();

        assert_eq!(items.len(), 2);
        assert!(items
            .iter()
            .any(|item| item.title.as_deref() == Some("sample")));
        assert!(items
            .iter()
            .any(|item| item.title.as_deref() == Some("clip")));
        assert!(items
            .iter()
            .all(|item| item.metadata["raw_path"].as_str().is_some()));
    }

    #[tokio::test]
    async fn skips_common_cache_directories() {
        let temp = tempfile::tempdir().unwrap();
        let cache = temp.path().join("Cache");
        let nested = temp.path().join("nested");
        std::fs::create_dir(&cache).unwrap();
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(cache.join("temporary.mp4"), b"video").unwrap();
        std::fs::write(nested.join("clip.mp4"), b"video").unwrap();

        let source = FolderVideo::new(json!({ "path": temp.path() })).unwrap();
        let items = source.discover().await.unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title.as_deref(), Some("clip"));
    }

    #[tokio::test]
    async fn skips_editor_working_directories() {
        let temp = tempfile::tempdir().unwrap();
        let draft = temp.path().join("com.lveditor.draft");
        let template = temp.path().join("templateDraft");
        let video_alg = temp.path().join("videoAlg");
        let keep = temp.path().join("exports");
        for dir in [&draft, &template, &video_alg, &keep] {
            std::fs::create_dir(dir).unwrap();
        }
        std::fs::write(draft.join("draft.mp4"), b"video").unwrap();
        std::fs::write(template.join("template.mp4"), b"video").unwrap();
        std::fs::write(video_alg.join("intermediate.mp4"), b"video").unwrap();
        std::fs::write(keep.join("final.mp4"), b"video").unwrap();

        let source = FolderVideo::new(json!({ "path": temp.path() })).unwrap();
        let items = source.discover().await.unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title.as_deref(), Some("final"));
    }

    #[tokio::test]
    async fn still_scans_selected_root_named_cache() {
        let temp = tempfile::tempdir().unwrap();
        let cache = temp.path().join("Cache");
        std::fs::create_dir(&cache).unwrap();
        std::fs::write(cache.join("intentional.mp4"), b"video").unwrap();

        let source = FolderVideo::new(json!({ "path": cache })).unwrap();
        let items = source.discover().await.unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title.as_deref(), Some("intentional"));
    }

    #[tokio::test]
    async fn fetch_returns_discovered_file_path() {
        let temp = tempfile::tempdir().unwrap();
        let video = temp.path().join("sample.webm");
        std::fs::write(&video, b"video").unwrap();

        let source = FolderVideo::new(json!({ "path": temp.path() })).unwrap();
        let item = source.discover().await.unwrap().pop().unwrap();

        assert_eq!(source.fetch(&item).await.unwrap(), video);
    }

    #[test]
    fn rejects_missing_directory() {
        let error = FolderVideo::new(json!({ "path": "/definitely/not/cerul/videos" }))
            .unwrap_err()
            .to_string();

        assert!(error.contains("not a directory"));
    }
}
