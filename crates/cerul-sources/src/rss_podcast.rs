use anyhow::Context;
use async_trait::async_trait;
use cerul_models::{ContentType, DiscoveredItem};
use quick_xml::{
    escape::resolve_predefined_entity,
    events::{BytesRef, BytesStart, Event},
    Reader, XmlVersion,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

use crate::{
    url_policy::{safe_http_client, validate_external_http_url},
    SourcePlugin,
};

static CONTENT_TYPES: [ContentType; 1] = [ContentType::Audio];
const MAX_FEED_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ENCLOSURE_BYTES: u64 = 10 * 1024 * 1024 * 1024;

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

#[derive(Debug, Default)]
struct ParsedFeed {
    title: Option<String>,
    image_url: Option<String>,
    entries: Vec<ParsedEntry>,
}

#[derive(Debug, Default)]
struct ParsedEntry {
    id: Option<String>,
    title: Option<String>,
    enclosure_url: Option<String>,
    first_link: Option<String>,
    published: Option<String>,
    updated: Option<String>,
}

impl ParsedEntry {
    fn effective_enclosure_url(&self) -> Option<String> {
        self.enclosure_url
            .clone()
            .or_else(|| self.first_link.clone())
    }

    fn metadata(&self) -> serde_json::Value {
        json!({
            "id": self.id,
            "title": self.title,
            "enclosure_url": self.enclosure_url,
            "link": self.first_link,
            "published": self.published,
            "updated": self.updated,
        })
    }
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
    let feed = parse_feed(&body).context("failed to parse RSS/Atom feed")?;
    let title = feed
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or("Podcast feed")
        .to_string();

    Ok(RssPodcastPreview {
        feed_url: feed_url.to_string(),
        title,
        image_url: feed.image_url,
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
        let feed = parse_feed(&body).context("failed to parse RSS/Atom feed")?;
        let mut items = Vec::new();

        for entry in feed.entries.into_iter().take(self.max_episodes) {
            let enclosure_url = entry.effective_enclosure_url();
            let external_id = if let Some(id) = entry.id.as_deref().filter(|id| !id.is_empty()) {
                id.to_string()
            } else {
                enclosure_url
                    .as_ref()
                    .map(|url| blake3::hash(url.as_bytes()).to_hex()[..16].to_string())
                    .context("feed entry has no id or enclosure URL")?
            };

            items.push(DiscoveredItem {
                external_id,
                title: entry.title.clone(),
                duration_sec: None,
                metadata: json!({
                    "feed_url": self.feed_url,
                    "enclosure_url": enclosure_url,
                    "published": entry.published,
                    "updated": entry.updated,
                    "entry": entry.metadata(),
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

/// Local feeds (plain paths / file:// URLs) are a test and power-user
/// affordance; the URL otherwise comes from API input, so remote locations
/// are restricted to http(s) on non-internal hosts (SSRF + local file read).
fn allow_local_feeds() -> bool {
    std::env::var("CERUL_ALLOW_LOCAL_FEEDS").map_or(cfg!(test), |v| v == "1")
}

async fn read_url_or_file(location: &str) -> anyhow::Result<Vec<u8>> {
    if allow_local_feeds() {
        if let Some(path) = file_url_path(location) {
            return Ok(tokio::fs::read(path).await?);
        }
        if Path::new(location).is_file() {
            return Ok(tokio::fs::read(location).await?);
        }
    }

    let url = validate_external_http_url(location, "feed URL")?;
    let client = safe_http_client("feed URL")?;
    let response = client.get(url).send().await?.error_for_status()?;
    if let Some(length) = response.content_length() {
        anyhow::ensure!(
            length <= MAX_FEED_BYTES,
            "feed response is too large: {length} bytes"
        );
    }
    let bytes = response.bytes().await?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_FEED_BYTES,
        "feed response is too large: {} bytes",
        bytes.len()
    );
    Ok(bytes.to_vec())
}

async fn download_url_or_file(location: &str, out: &Path) -> anyhow::Result<()> {
    if allow_local_feeds() {
        if let Some(path) = file_url_path(location) {
            tokio::fs::copy(path, out).await?;
            return Ok(());
        }
        if Path::new(location).is_file() {
            tokio::fs::copy(location, out).await?;
            return Ok(());
        }
    }

    let url = validate_external_http_url(location, "podcast enclosure URL")?;
    // Stream to disk: episodes can be hundreds of MB and used to be
    // buffered fully in memory before writing.
    use tokio::io::AsyncWriteExt;
    let client = safe_http_client("podcast enclosure URL")?;
    let mut response = client.get(url).send().await?.error_for_status()?;
    if let Some(length) = response.content_length() {
        anyhow::ensure!(
            length <= MAX_ENCLOSURE_BYTES,
            "podcast enclosure is too large: {length} bytes"
        );
    }
    let tmp = out.with_extension("partial");
    let mut file = tokio::fs::File::create(&tmp).await?;
    let mut written = 0_u64;
    while let Some(chunk) = response.chunk().await? {
        written += chunk.len() as u64;
        anyhow::ensure!(
            written <= MAX_ENCLOSURE_BYTES,
            "podcast enclosure is too large: {written} bytes"
        );
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);
    tokio::fs::rename(&tmp, out).await?;
    Ok(())
}

fn file_url_path(location: &str) -> Option<&str> {
    location.strip_prefix("file://")
}

fn parse_feed(body: &[u8]) -> anyhow::Result<ParsedFeed> {
    let xml = decode_supported_xml(body)?;
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut path = Vec::<String>::new();
    let mut feed = ParsedFeed::default();
    let mut current_entry: Option<ParsedEntry> = None;
    let mut current_text: Option<TextAccumulator> = None;
    let mut saw_feed_root = false;

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(event) => {
                let tag = local_name(event.name().as_ref());
                if path.is_empty() && is_feed_root(&tag) {
                    saw_feed_root = true;
                }
                if is_entry_element(&tag) {
                    current_entry = Some(ParsedEntry::default());
                }
                if let Some(entry) = current_entry.as_mut() {
                    collect_entry_attributes(entry, &tag, &event)?;
                } else {
                    collect_feed_attributes(&mut feed, &tag, &event)?;
                }
                path.push(tag);
                current_text = text_target_for_path(current_entry.is_some(), &path)
                    .map(|(target, element)| TextAccumulator::new(target, element));
            }
            Event::Empty(event) => {
                let tag = local_name(event.name().as_ref());
                if path.is_empty() && is_feed_root(&tag) {
                    saw_feed_root = true;
                }
                if let Some(entry) = current_entry.as_mut() {
                    collect_entry_attributes(entry, &tag, &event)?;
                } else {
                    collect_feed_attributes(&mut feed, &tag, &event)?;
                }
            }
            Event::Text(event) => {
                if let Some(accumulator) = current_text.as_mut() {
                    accumulator.push(&event.decode()?);
                }
            }
            Event::GeneralRef(event) => {
                if let Some(accumulator) = current_text.as_mut() {
                    accumulator.push(&decode_xml_reference(&event)?);
                }
            }
            Event::CData(event) => {
                if let Some(accumulator) = current_text.as_mut() {
                    accumulator.push(&String::from_utf8_lossy(&event));
                }
            }
            Event::End(event) => {
                let tag = local_name(event.name().as_ref());
                if current_text
                    .as_ref()
                    .is_some_and(|accumulator| accumulator.ends_at(&tag, &path))
                {
                    if let Some(accumulator) = current_text.take() {
                        accumulator.apply(&mut feed, current_entry.as_mut());
                    }
                }
                if is_entry_element(&tag) {
                    if let Some(entry) = current_entry.take() {
                        feed.entries.push(entry);
                    }
                }
                path.pop();
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    anyhow::ensure!(saw_feed_root, "XML document is not an RSS or Atom feed");
    Ok(feed)
}

fn decode_supported_xml(body: &[u8]) -> anyhow::Result<&str> {
    if let Some(encoding) = declared_xml_encoding(body) {
        let normalized = encoding.to_ascii_lowercase().replace('_', "-");
        anyhow::ensure!(
            matches!(normalized.as_str(), "utf-8" | "utf8" | "us-ascii" | "ascii"),
            "unsupported XML encoding: {encoding}"
        );
    }
    std::str::from_utf8(body).context("feed XML is not valid UTF-8")
}

fn declared_xml_encoding(body: &[u8]) -> Option<String> {
    let prefix = String::from_utf8_lossy(body.get(..body.len().min(512))?);
    let declaration_end = prefix.find("?>")?;
    let declaration = prefix[..declaration_end].trim_start();
    if !declaration.starts_with("<?xml") {
        return None;
    }
    let encoding_index = declaration.to_ascii_lowercase().find("encoding")?;
    let after_encoding = &declaration[encoding_index + "encoding".len()..];
    let (_, value) = after_encoding.split_once('=')?;
    let value = value.trim_start();
    let quote = value.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let value = &value[quote.len_utf8()..];
    let end = value.find(quote)?;
    Some(value[..end].to_string())
}

fn decode_xml_reference(reference: &BytesRef<'_>) -> anyhow::Result<String> {
    if let Some(ch) = reference.resolve_char_ref()? {
        return Ok(ch.to_string());
    }
    let name = reference.decode()?;
    Ok(resolve_predefined_entity(&name)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("&{name};")))
}

fn collect_entry_attributes(
    entry: &mut ParsedEntry,
    tag: &str,
    event: &BytesStart<'_>,
) -> anyhow::Result<()> {
    match tag {
        "enclosure" => {
            if entry.enclosure_url.is_none() {
                entry.enclosure_url = attr_value(event, "url")?;
            }
        }
        "link" => {
            let href = attr_value(event, "href")?;
            let rel = attr_value(event, "rel")?;
            if rel.as_deref() == Some("enclosure") && entry.enclosure_url.is_none() {
                entry.enclosure_url = href.clone();
            }
            if entry.first_link.is_none() {
                entry.first_link = href;
            }
        }
        "content" => {
            if entry.enclosure_url.is_none() {
                entry.enclosure_url = attr_value(event, "url")?.or(attr_value(event, "src")?);
            }
        }
        _ => {}
    }
    Ok(())
}

fn collect_feed_attributes(
    feed: &mut ParsedFeed,
    tag: &str,
    event: &BytesStart<'_>,
) -> anyhow::Result<()> {
    if tag == "image" && feed.image_url.is_none() {
        feed.image_url = attr_value(event, "href")?.or(attr_value(event, "url")?);
    }
    Ok(())
}

#[derive(Debug)]
struct TextAccumulator {
    target: TextTarget,
    element: String,
    value: String,
}

impl TextAccumulator {
    fn new(target: TextTarget, element: String) -> Self {
        Self {
            target,
            element,
            value: String::new(),
        }
    }

    fn push(&mut self, text: &str) {
        self.value.push_str(text);
    }

    fn ends_at(&self, tag: &str, path: &[String]) -> bool {
        path.last().is_some_and(|current| current == tag) && self.element == tag
    }

    fn apply(self, feed: &mut ParsedFeed, current_entry: Option<&mut ParsedEntry>) {
        let text = normalize_text(&self.value);
        if text.is_empty() {
            return;
        }
        match (self.target, current_entry) {
            (TextTarget::EntryId, Some(entry)) => set_if_empty(&mut entry.id, &text),
            (TextTarget::EntryTitle, Some(entry)) => set_if_empty(&mut entry.title, &text),
            (TextTarget::EntryPublished, Some(entry)) => set_if_empty(&mut entry.published, &text),
            (TextTarget::EntryUpdated, Some(entry)) => set_if_empty(&mut entry.updated, &text),
            (TextTarget::EntryLink, Some(entry)) => set_if_empty(&mut entry.first_link, &text),
            (TextTarget::FeedTitle, _) => set_if_empty(&mut feed.title, &text),
            (TextTarget::FeedImageUrl, _) => set_if_empty(&mut feed.image_url, &text),
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum TextTarget {
    EntryId,
    EntryTitle,
    EntryPublished,
    EntryUpdated,
    EntryLink,
    FeedTitle,
    FeedImageUrl,
}

fn text_target_for_path(in_entry: bool, path: &[String]) -> Option<(TextTarget, String)> {
    let tag = path.last()?.as_str();
    let target = if in_entry {
        match tag {
            "guid" | "id" => TextTarget::EntryId,
            "title" => TextTarget::EntryTitle,
            "pubdate" | "published" => TextTarget::EntryPublished,
            "updated" => TextTarget::EntryUpdated,
            "link" => TextTarget::EntryLink,
            _ => return None,
        }
    } else {
        match tag {
            "title" => TextTarget::FeedTitle,
            "logo" | "icon" => TextTarget::FeedImageUrl,
            "url" if path.iter().any(|part| part == "image") => TextTarget::FeedImageUrl,
            _ => return None,
        }
    };
    Some((target, tag.to_string()))
}

fn attr_value(event: &BytesStart<'_>, key: &str) -> anyhow::Result<Option<String>> {
    for attr in event.attributes().with_checks(false) {
        let attr = attr?;
        if local_name(attr.key.as_ref()) == key {
            return Ok(Some(
                attr.normalized_value(XmlVersion::Implicit1_0)?.to_string(),
            ));
        }
    }
    Ok(None)
}

fn set_if_empty(target: &mut Option<String>, value: &str) {
    if target
        .as_deref()
        .is_some_and(|existing| !existing.is_empty())
    {
        return;
    }
    let value = value.trim();
    if !value.is_empty() {
        *target = Some(value.to_string());
    }
}

fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_entry_element(tag: &str) -> bool {
    matches!(tag, "item" | "entry")
}

fn is_feed_root(tag: &str) -> bool {
    matches!(tag, "rss" | "feed")
}

fn local_name(name: &[u8]) -> String {
    let name = std::str::from_utf8(name).unwrap_or_default();
    name.rsplit_once(':')
        .map(|(_, local)| local)
        .unwrap_or(name)
        .to_ascii_lowercase()
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
    async fn discovers_atom_content_src_and_accumulates_split_text() {
        let temp = tempfile::tempdir().unwrap();
        let audio = temp.path().join("episode.mp3");
        std::fs::write(&audio, b"audio").unwrap();
        let feed = temp.path().join("atom.xml");
        std::fs::write(
            &feed,
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Cerul &amp; Friends</title>
  <entry>
    <id>episode-&amp;42</id>
    <title>R&amp;D &amp; AI</title>
    <updated>2026-07-03T00:00:00Z</updated>
    <content type="audio/mpeg" src="file://{}" />
  </entry>
</feed>"#,
                audio.display()
            ),
        )
        .unwrap();

        let source = RssPodcast::new(json!({ "url": feed })).unwrap();
        let items = source.discover().await.unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].external_id, "episode-&42");
        assert_eq!(items[0].title.as_deref(), Some("R&D & AI"));
        assert!(items[0].metadata["enclosure_url"]
            .as_str()
            .unwrap()
            .starts_with("file://"));
        assert_eq!(
            items[0].metadata["entry"]["updated"],
            "2026-07-03T00:00:00Z"
        );
    }

    #[tokio::test]
    async fn previews_itunes_image_href() {
        let temp = tempfile::tempdir().unwrap();
        let feed = temp.path().join("feed.xml");
        std::fs::write(
            &feed,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
  <channel>
    <title>Cerul Podcast</title>
    <itunes:image href="https://example.com/itunes.jpg" />
  </channel>
</rss>"#,
        )
        .unwrap();

        let preview = preview_feed(&feed.to_string_lossy()).await.unwrap();

        assert_eq!(
            preview.image_url.as_deref(),
            Some("https://example.com/itunes.jpg")
        );
    }

    #[test]
    fn parse_feed_rejects_non_feed_xml() {
        let error = parse_feed(br#"<?xml version="1.0"?><html><title>Nope</title></html>"#)
            .unwrap_err()
            .to_string();

        assert!(error.contains("not an RSS or Atom feed"));
    }

    #[test]
    fn parse_feed_rejects_unsupported_declared_encoding() {
        let error = parse_feed(
            br#"<?xml version="1.0" encoding="ISO-8859-1"?><rss version="2.0"><channel /></rss>"#,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("unsupported XML encoding"));
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
