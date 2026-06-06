use anyhow::Context;
use async_trait::async_trait;
use cerul_models::{ContentType, DiscoveredItem};
use feed_rs::{model::Entry, parser};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

use crate::SourcePlugin;

static CONTENT_TYPES: [ContentType; 1] = [ContentType::Audio];

#[derive(Debug, Clone)]
pub struct RssPodcast {
    feed_url: String,
    max_episodes: usize,
    cache_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RssPodcastPreview {
    pub feed_url: String,
    pub title: String,
    pub image_url: Option<String>,
    pub episode_count: usize,
}

impl RssPodcast {
    pub fn new(config: serde_json::Value) -> anyhow::Result<Self> {
        let feed_url = config
            .get("url")
            .or_else(|| config.get("feed_url"))
            .and_then(|value| value.as_str())
            .context("rss_podcast requires config.url")?
            .to_string();
        let max_episodes = config
            .get("max_episodes")
            .or_else(|| config.get("max"))
            .and_then(|value| value.as_u64())
            .unwrap_or(25) as usize;
        let cache_dir = config
            .get("cache_dir")
            .and_then(|value| value.as_str())
            .map(expand_path)
            .unwrap_or_else(|| default_cache_dir().join("rss_podcast"));

        Ok(Self {
            feed_url,
            max_episodes,
            cache_dir,
        })
    }

    pub fn feed_url(&self) -> &str {
        &self.feed_url
    }

    pub fn max_episodes(&self) -> usize {
        self.max_episodes
    }
}

pub async fn preview_feed(feed_url: &str) -> anyhow::Result<RssPodcastPreview> {
    let body = read_url_or_file(feed_url).await?;
    let feed = parser::parse(&body[..]).context("failed to parse RSS/Atom feed")?;
    let title = feed
        .title
        .as_ref()
        .map(|title| title.content.trim())
        .filter(|title| !title.is_empty())
        .unwrap_or("Podcast feed")
        .to_string();
    let image_url = feed
        .logo
        .as_ref()
        .or(feed.icon.as_ref())
        .map(|image| image.uri.clone());

    Ok(RssPodcastPreview {
        feed_url: feed_url.to_string(),
        title,
        image_url,
        episode_count: feed.entries.len(),
    })
}

#[async_trait]
impl SourcePlugin for RssPodcast {
    fn name(&self) -> &'static str {
        "rss_podcast"
    }

    fn content_types(&self) -> &[ContentType] {
        &CONTENT_TYPES
    }

    async fn discover(&self) -> anyhow::Result<Vec<DiscoveredItem>> {
        let body = read_url_or_file(&self.feed_url).await?;
        let feed = parser::parse(&body[..]).context("failed to parse RSS/Atom feed")?;
        let mut items = Vec::new();

        for entry in feed.entries.into_iter().take(self.max_episodes) {
            let enclosure_url = enclosure_url_for(&entry);
            let external_id = if entry.id.is_empty() {
                enclosure_url
                    .as_ref()
                    .map(|url| blake3::hash(url.as_bytes()).to_hex()[..16].to_string())
                    .context("feed entry has no id or enclosure URL")?
            } else {
                entry.id.clone()
            };
            let title = entry.title.as_ref().map(|title| title.content.clone());
            let entry_metadata =
                serde_json::to_value(&entry).context("failed to serialize feed entry metadata")?;

            items.push(DiscoveredItem {
                external_id,
                title,
                duration_sec: None,
                metadata: json!({
                    "feed_url": self.feed_url,
                    "enclosure_url": enclosure_url,
                    "published": entry.published.map(|date| date.to_rfc3339()),
                    "updated": entry.updated.map(|date| date.to_rfc3339()),
                    "entry": entry_metadata,
                }),
            });
        }

        Ok(items)
    }

    async fn fetch(&self, item: &DiscoveredItem) -> anyhow::Result<PathBuf> {
        let enclosure_url = item
            .metadata
            .get("enclosure_url")
            .and_then(|value| value.as_str())
            .context("rss_podcast item is missing metadata.enclosure_url")?;
        tokio::fs::create_dir_all(&self.cache_dir).await?;
        let extension = extension_from_url(enclosure_url).unwrap_or("mp3");
        let out = self
            .cache_dir
            .join(format!("{}.{extension}", safe_file_stem(&item.external_id)));

        if out.exists() {
            return Ok(out);
        }

        download_url_or_file(enclosure_url, &out).await?;
        Ok(out)
    }
}

async fn read_url_or_file(location: &str) -> anyhow::Result<Vec<u8>> {
    if let Some(path) = file_url_path(location) {
        return Ok(tokio::fs::read(path).await?);
    }

    if Path::new(location).is_file() {
        return Ok(tokio::fs::read(location).await?);
    }

    let bytes = reqwest::get(location)
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    Ok(bytes.to_vec())
}

async fn download_url_or_file(location: &str, out: &Path) -> anyhow::Result<()> {
    if let Some(path) = file_url_path(location) {
        tokio::fs::copy(path, out).await?;
        return Ok(());
    }

    if Path::new(location).is_file() {
        tokio::fs::copy(location, out).await?;
        return Ok(());
    }

    let bytes = reqwest::get(location)
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    tokio::fs::write(out, bytes).await?;
    Ok(())
}

fn file_url_path(location: &str) -> Option<&str> {
    location.strip_prefix("file://")
}

fn enclosure_url_for(entry: &Entry) -> Option<String> {
    entry
        .links
        .iter()
        .find(|link| link.rel.as_deref() == Some("enclosure"))
        .map(|link| link.href.clone())
        .or_else(|| {
            entry
                .content
                .as_ref()
                .and_then(|content| content.src.as_ref())
                .map(|link| link.href.clone())
        })
        .or_else(|| {
            entry.media.iter().find_map(|media| {
                media
                    .content
                    .iter()
                    .find_map(|content| content.url.as_ref().map(ToString::to_string))
            })
        })
        .or_else(|| entry.links.first().map(|link| link.href.clone()))
}

fn extension_from_url(url: &str) -> Option<&str> {
    let path = url.split(['?', '#']).next()?;
    path.rsplit_once('.')
        .map(|(_, extension)| extension)
        .filter(|extension| !extension.is_empty() && extension.len() <= 8)
}

fn default_cache_dir() -> PathBuf {
    if let Ok(path) = std::env::var("CERUL_CACHE_DIR") {
        PathBuf::from(path)
    } else {
        std::env::temp_dir().join("cerul-cache")
    }
}

fn expand_path(path: &str) -> PathBuf {
    PathBuf::from(shellexpand::tilde(path).into_owned())
}

fn safe_file_stem(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn discovers_feed_entries_with_enclosures() {
        let temp = tempfile::tempdir().unwrap();
        let audio = temp.path().join("episode.mp3");
        std::fs::write(&audio, b"audio").unwrap();
        let feed = temp.path().join("feed.xml");
        std::fs::write(
            &feed,
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Cerul Podcast</title>
    <item>
      <guid>episode-1</guid>
      <title>Episode One</title>
      <enclosure url="file://{}" type="audio/mpeg" length="5" />
    </item>
  </channel>
</rss>"#,
                audio.display()
            ),
        )
        .unwrap();

        let source = RssPodcast::new(json!({
            "url": feed,
            "cache_dir": temp.path().join("cache"),
        }))
        .unwrap();
        let items = source.discover().await.unwrap();

        assert_eq!(source.feed_url(), feed.to_string_lossy());
        assert_eq!(source.max_episodes(), 25);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].external_id, "episode-1");
        assert_eq!(items[0].title.as_deref(), Some("Episode One"));
        assert!(items[0].metadata["enclosure_url"]
            .as_str()
            .unwrap()
            .starts_with("file://"));
    }

    #[tokio::test]
    async fn discovery_respects_max_episodes() {
        let temp = tempfile::tempdir().unwrap();
        let audio = temp.path().join("episode.mp3");
        std::fs::write(&audio, b"audio").unwrap();
        let feed = temp.path().join("feed.xml");
        std::fs::write(
            &feed,
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Cerul Podcast</title>
    <item>
      <guid>episode-1</guid>
      <title>Episode One</title>
      <enclosure url="file://{}" type="audio/mpeg" length="5" />
    </item>
    <item>
      <guid>episode-2</guid>
      <title>Episode Two</title>
      <enclosure url="file://{}" type="audio/mpeg" length="5" />
    </item>
  </channel>
</rss>"#,
                audio.display(),
                audio.display()
            ),
        )
        .unwrap();

        let source = RssPodcast::new(json!({
            "url": feed,
            "max_episodes": 1,
        }))
        .unwrap();
        let items = source.discover().await.unwrap();

        assert_eq!(source.max_episodes(), 1);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].external_id, "episode-1");
    }

    #[tokio::test]
    async fn previews_feed_title_image_and_episode_count() {
        let temp = tempfile::tempdir().unwrap();
        let feed = temp.path().join("feed.xml");
        std::fs::write(
            &feed,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Cerul Podcast</title>
    <image>
      <url>https://example.com/art.jpg</url>
      <title>Cerul Podcast</title>
      <link>https://example.com</link>
    </image>
    <item>
      <guid>episode-1</guid>
      <title>Episode One</title>
    </item>
    <item>
      <guid>episode-2</guid>
      <title>Episode Two</title>
    </item>
  </channel>
</rss>"#,
        )
        .unwrap();

        let preview = preview_feed(&feed.to_string_lossy()).await.unwrap();

        assert_eq!(preview.feed_url, feed.to_string_lossy().as_ref());
        assert_eq!(preview.title, "Cerul Podcast");
        assert_eq!(
            preview.image_url.as_deref(),
            Some("https://example.com/art.jpg")
        );
        assert_eq!(preview.episode_count, 2);
    }

    #[tokio::test]
    async fn fetch_downloads_enclosure_to_cache() {
        let temp = tempfile::tempdir().unwrap();
        let audio = temp.path().join("episode.mp3");
        std::fs::write(&audio, b"audio").unwrap();
        let source = RssPodcast::new(json!({
            "url": "https://example.com/feed.xml",
            "cache_dir": temp.path().join("cache"),
        }))
        .unwrap();
        let item = DiscoveredItem {
            external_id: "episode-1".to_string(),
            title: Some("Episode One".to_string()),
            duration_sec: None,
            metadata: json!({ "enclosure_url": format!("file://{}", audio.display()) }),
        };

        let fetched = source.fetch(&item).await.unwrap();

        assert_eq!(fetched, temp.path().join("cache").join("episode-1.mp3"));
        assert_eq!(std::fs::read_to_string(fetched).unwrap(), "audio");
    }

    #[test]
    fn requires_feed_url() {
        let error = RssPodcast::new(json!({})).unwrap_err().to_string();

        assert!(error.contains("config.url"));
    }
}
