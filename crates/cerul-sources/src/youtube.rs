use anyhow::Context;
use async_trait::async_trait;
use cerul_models::{ContentType, DiscoveredItem};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::process::Command;

use crate::{url_policy::validate_external_http_url, SourcePlugin};

static CONTENT_TYPES: [ContentType; 1] = [ContentType::Video];

#[derive(Debug, Clone)]
pub struct YouTube {
    channel_url: String,
    max_videos: Option<usize>,
    ytdlp_path: PathBuf,
    cache_dir: PathBuf,
    command_timeout: Option<Duration>,
    clip_duration_sec: Option<u64>,
    access: YtdlpAccess,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct YtdlpAccess {
    cookies_from_browser: Option<String>,
    cookies_path: Option<PathBuf>,
}

impl YtdlpAccess {
    pub(crate) fn from_config(config: &serde_json::Value) -> Self {
        let cookies_from_browser = string_setting(
            config,
            &[
                "cookies_from_browser",
                "cookie_browser",
                "ytdlp_cookies_from_browser",
                "ytdlp_cookie_browser",
            ],
        );
        let cookies_path = string_setting(
            config,
            &[
                "cookies_path",
                "cookies_file",
                "ytdlp_cookies_path",
                "ytdlp_cookies_file",
            ],
        )
        .map(|path| expand_path(&path));

        Self {
            cookies_from_browser,
            cookies_path,
        }
    }

    pub(crate) fn apply_to_command_with_browser_cookies(
        &self,
        command: &mut Command,
        include_browser_cookies: bool,
    ) {
        if include_browser_cookies {
            if let Some(browser) = self.cookies_from_browser.as_deref() {
                command.args(["--cookies-from-browser", browser]);
                return;
            }
        }
        if let Some(path) = self.cookies_path.as_deref() {
            command.arg("--cookies").arg(path);
        }
    }

    pub(crate) fn should_retry_without_browser_cookies(&self, stderr: &[u8]) -> bool {
        self.cookies_from_browser.is_some() && is_browser_cookie_load_error(stderr)
    }
}

fn is_browser_cookie_load_error(stderr: &[u8]) -> bool {
    let normalized = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    normalized.contains("cookie database")
        || normalized.contains("cookies database")
        || normalized.contains("failed to decrypt")
        || normalized.contains("unsupported browser")
        || normalized.contains("keyring")
        || (normalized.contains("browser cookies")
            && (normalized.contains("could not")
                || normalized.contains("cannot")
                || normalized.contains("can't")
                || normalized.contains("failed")
                || normalized.contains("permission denied")
                || normalized.contains("no such file")
                || normalized.contains("unable")))
        || (normalized.contains("cookies from browser")
            && (normalized.contains("could not")
                || normalized.contains("cannot")
                || normalized.contains("can't")
                || normalized.contains("failed")
                || normalized.contains("permission denied")
                || normalized.contains("no such file")
                || normalized.contains("unable")))
}

impl YouTube {
    pub fn new(config: serde_json::Value) -> anyhow::Result<Self> {
        let channel_url = config
            .get("url")
            .or_else(|| config.get("channel_url"))
            .and_then(|value| value.as_str())
            .context("youtube requires config.url")
            .and_then(validate_youtube_source_url)?;
        let max_videos = config
            .get("max_videos")
            .or_else(|| config.get("max"))
            .and_then(|value| value.as_u64())
            .map(|value| value as usize)
            .unwrap_or(50);
        let max_videos = if config
            .get("max_videos_unlimited")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
            || max_videos == 0
        {
            None
        } else {
            Some(max_videos)
        };
        let ytdlp_path = config
            .get("ytdlp_path")
            .and_then(|value| value.as_str())
            .map(expand_path)
            .unwrap_or_else(default_ytdlp_path);
        let cache_dir = config
            .get("cache_dir")
            .and_then(|value| value.as_str())
            .map(expand_path)
            .unwrap_or_else(|| default_cache_dir().join("youtube"));
        let command_timeout = config
            .get("timeout_sec")
            .or_else(|| config.get("command_timeout_sec"))
            .and_then(|value| value.as_u64())
            .filter(|value| *value > 0)
            .map(Duration::from_secs);
        let clip_duration_sec = config
            .get("clip_duration_sec")
            .and_then(|value| value.as_u64())
            .filter(|value| *value > 0);
        let access = YtdlpAccess::from_config(&config);

        Ok(Self {
            channel_url,
            max_videos,
            ytdlp_path,
            cache_dir,
            command_timeout,
            clip_duration_sec,
            access,
        })
    }

    pub fn channel_url(&self) -> &str {
        &self.channel_url
    }

    pub fn max_videos(&self) -> Option<usize> {
        self.max_videos
    }

    pub fn command_timeout_sec(&self) -> Option<u64> {
        self.command_timeout.map(|timeout| timeout.as_secs())
    }

    pub fn clip_duration_sec(&self) -> Option<u64> {
        self.clip_duration_sec
    }

    fn discovery_command(&self, include_browser_cookies: bool) -> Command {
        let mut command = Command::new(&self.ytdlp_path);
        command.args(["--flat-playlist", "--dump-json"]);
        self.access
            .apply_to_command_with_browser_cookies(&mut command, include_browser_cookies);
        if let Some(max_videos) = self.max_videos {
            command.arg("--playlist-end").arg(max_videos.to_string());
        }
        command
            .arg("--")
            .arg(&self.channel_url)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn fetch_command(
        &self,
        output_path: &Path,
        external_id: &str,
        include_browser_cookies: bool,
    ) -> Command {
        let mut command = Command::new(&self.ytdlp_path);
        command.args(["--no-playlist", "-f", "best[height<=720]/best"]);
        self.access
            .apply_to_command_with_browser_cookies(&mut command, include_browser_cookies);
        if let Some(duration_sec) = self.clip_duration_sec {
            command
                .arg("--download-sections")
                .arg(format!("*0-{duration_sec}"))
                .arg("--force-keyframes-at-cuts");
        }
        command
            .arg("-o")
            .arg(output_path)
            .arg("--")
            .arg(format!("https://www.youtube.com/watch?v={external_id}"))
            .stderr(Stdio::piped());
        command
    }

    async fn run_ytdlp_with_browser_cookie_fallback<F>(
        &self,
        phase: &str,
        mut build_command: F,
    ) -> anyhow::Result<std::process::Output>
    where
        F: FnMut(bool) -> Command,
    {
        let mut command = build_command(true);
        let output = self.run_ytdlp(&mut command, phase).await?;
        if !output.status.success()
            && self
                .access
                .should_retry_without_browser_cookies(&output.stderr)
        {
            let mut fallback = build_command(false);
            return self.run_ytdlp(&mut fallback, phase).await;
        }
        Ok(output)
    }

    async fn run_ytdlp(
        &self,
        command: &mut Command,
        phase: &str,
    ) -> anyhow::Result<std::process::Output> {
        command.kill_on_drop(true);
        let timeout = self
            .command_timeout
            .unwrap_or_else(|| crate::default_ytdlp_timeout(phase));
        let output = tokio::time::timeout(timeout, command.output())
            .await
            .with_context(|| format!("yt-dlp {phase} timed out after {}s", timeout.as_secs()))?;

        output.with_context(|| format!("failed to run {}", self.ytdlp_path.display()))
    }
}

#[async_trait]
impl SourcePlugin for YouTube {
    fn name(&self) -> &'static str {
        "youtube"
    }

    fn content_types(&self) -> &[ContentType] {
        &CONTENT_TYPES
    }

    async fn discover(&self) -> anyhow::Result<Vec<DiscoveredItem>> {
        let output = self
            .run_ytdlp_with_browser_cookie_fallback("discovery", |include_browser_cookies| {
                self.discovery_command(include_browser_cookies)
            })
            .await?;

        if !output.status.success() {
            anyhow::bail!(
                "yt-dlp discovery failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        let mut items = Vec::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if line.trim().is_empty() {
                continue;
            }

            let metadata: serde_json::Value =
                serde_json::from_str(line).context("yt-dlp emitted invalid JSON")?;
            let external_id = metadata
                .get("id")
                .and_then(|value| value.as_str())
                .context("yt-dlp item is missing id")?
                .to_string();
            let title = metadata
                .get("title")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned);
            let duration_sec = metadata.get("duration").and_then(|value| value.as_f64());

            items.push(DiscoveredItem {
                external_id,
                title,
                duration_sec,
                metadata,
            });
        }

        Ok(items)
    }

    async fn fetch(&self, item: &DiscoveredItem) -> anyhow::Result<PathBuf> {
        tokio::fs::create_dir_all(&self.cache_dir).await?;
        let output_path = self
            .cache_dir
            .join(format!("{}.mp4", safe_file_stem(&item.external_id)));

        if output_path.exists() {
            return Ok(output_path);
        }

        let status = self
            .run_ytdlp_with_browser_cookie_fallback("fetch", |include_browser_cookies| {
                self.fetch_command(&output_path, &item.external_id, include_browser_cookies)
            })
            .await?;

        if !status.status.success() {
            anyhow::bail!(
                "yt-dlp fetch failed: {}",
                String::from_utf8_lossy(&status.stderr).trim()
            );
        }

        Ok(output_path)
    }
}

fn validate_youtube_source_url(value: &str) -> anyhow::Result<String> {
    let url = validate_external_http_url(value, "YouTube source URL")?;
    let host = url
        .host_str()
        .map(|host| host.trim_start_matches("www.").to_ascii_lowercase())
        .context("YouTube source URL is missing a host")?;
    anyhow::ensure!(
        host == "youtube.com" || host.ends_with(".youtube.com") || host == "youtu.be",
        "YouTube source URL must use youtube.com or youtu.be"
    );
    Ok(url.to_string())
}

pub(crate) fn default_ytdlp_path() -> PathBuf {
    if let Ok(path) = std::env::var("CERUL_YTDLP_PATH") {
        return PathBuf::from(path);
    }

    let executable = if cfg!(windows) {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    };
    for candidate in bundled_ytdlp_candidates(executable) {
        if candidate.is_file() {
            return candidate;
        }
    }

    PathBuf::from(executable)
}

fn bundled_ytdlp_candidates(executable: &str) -> Vec<PathBuf> {
    let target_dir = bundled_target_dir();
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut candidates = vec![
        repo_root
            .join("third-party")
            .join(&target_dir)
            .join(executable),
        repo_root
            .join("third-party")
            .join("yt-dlp")
            .join(executable),
    ];

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(dir) = current_exe.parent() {
            candidates.push(dir.join("third-party").join(&target_dir).join(executable));
            if let Some(contents_dir) = dir.parent() {
                candidates.push(
                    contents_dir
                        .join("Resources")
                        .join("third-party")
                        .join(&target_dir)
                        .join(executable),
                );
            }
        }
    }

    if let Ok(current_dir) = std::env::current_dir() {
        candidates.push(
            current_dir
                .join("third-party")
                .join(target_dir)
                .join(executable),
        );
    }

    candidates
}

fn bundled_target_dir() -> String {
    let arch = std::env::consts::ARCH;
    match std::env::consts::OS {
        "macos" => format!("{arch}-apple-darwin"),
        "linux" => format!("{arch}-unknown-linux-gnu"),
        "windows" => format!("{arch}-pc-windows-msvc"),
        other => format!("{arch}-{other}"),
    }
}

pub(crate) fn default_cache_dir() -> PathBuf {
    if let Ok(path) = std::env::var("CERUL_CACHE_DIR") {
        PathBuf::from(path)
    } else {
        std::env::temp_dir().join("cerul-cache")
    }
}

pub(crate) fn expand_path(path: &str) -> PathBuf {
    PathBuf::from(shellexpand::tilde(path).into_owned())
}

fn string_setting(config: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        config
            .get(*key)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

pub(crate) fn safe_file_stem(value: &str) -> String {
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

    #[cfg(unix)]
    fn fake_ytdlp(temp: &tempfile::TempDir) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let script = temp.path().join("yt-dlp");
        std::fs::write(
            &script,
            r#"#!/bin/sh
if printf '%s\n' "$@" | grep -q -- '--flat-playlist'; then
  printf '{"id":"abc123","title":"First video","duration":12}\n'
  printf '{"id":"def456","title":"Second video","duration":34}\n'
else
  out=""
  while [ "$#" -gt 0 ]; do
    if [ "$1" = "-o" ]; then
      shift
      out="$1"
    fi
    shift
  done
  mkdir -p "$(dirname "$out")"
  printf 'video' > "$out"
fi
"#,
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();
        script
    }

    #[cfg(unix)]
    fn fake_ytdlp_with_missing_browser_cookies(temp: &tempfile::TempDir) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let script = temp.path().join("yt-dlp-cookie-fallback");
        std::fs::write(
            &script,
            r#"#!/bin/sh
if printf '%s\n' "$@" | grep -q -- '--cookies-from-browser'; then
  printf 'ERROR: could not find Chrome cookies database\n' >&2
  exit 1
fi
if printf '%s\n' "$@" | grep -q -- '--flat-playlist'; then
  printf '{"id":"abc123","title":"First video","duration":12}\n'
else
  out=""
  while [ "$#" -gt 0 ]; do
    if [ "$1" = "-o" ]; then
      shift
      out="$1"
    fi
    shift
  done
  mkdir -p "$(dirname "$out")"
  printf 'video' > "$out"
fi
"#,
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();
        script
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn discovers_videos_from_ytdlp_json_lines() {
        let temp = tempfile::tempdir().unwrap();
        let source = YouTube::new(json!({
            "url": "https://www.youtube.com/@cerul",
            "max_videos": 2,
            "ytdlp_path": fake_ytdlp(&temp),
            "cache_dir": temp.path().join("cache"),
        }))
        .unwrap();

        let items = source.discover().await.unwrap();

        assert_eq!(source.max_videos(), Some(2));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].external_id, "abc123");
        assert_eq!(items[0].title.as_deref(), Some("First video"));
        assert_eq!(items[0].duration_sec, Some(12.0));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn falls_back_when_browser_cookies_are_unavailable() {
        let temp = tempfile::tempdir().unwrap();
        let source = YouTube::new(json!({
            "url": "https://www.youtube.com/@cerul",
            "max_videos": 1,
            "cookies_from_browser": "chrome",
            "ytdlp_path": fake_ytdlp_with_missing_browser_cookies(&temp),
            "cache_dir": temp.path().join("cache"),
        }))
        .unwrap();

        let items = source.discover().await.unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].external_id, "abc123");
    }

    #[cfg(unix)]
    #[test]
    fn zero_max_videos_means_unlimited() {
        let temp = tempfile::tempdir().unwrap();
        let source = YouTube::new(json!({
            "url": "https://www.youtube.com/@cerul",
            "max_videos": 0,
            "ytdlp_path": fake_ytdlp(&temp),
            "cache_dir": temp.path().join("cache"),
        }))
        .unwrap();

        assert_eq!(source.max_videos(), None);
    }

    #[cfg(unix)]
    #[test]
    fn parses_command_timeout_seconds() {
        let temp = tempfile::tempdir().unwrap();
        let source = YouTube::new(json!({
            "url": "https://www.youtube.com/@cerul",
            "timeout_sec": 7,
            "ytdlp_path": fake_ytdlp(&temp),
            "cache_dir": temp.path().join("cache"),
        }))
        .unwrap();

        assert_eq!(source.command_timeout_sec(), Some(7));
    }

    #[cfg(unix)]
    #[test]
    fn parses_clip_duration_seconds() {
        let temp = tempfile::tempdir().unwrap();
        let source = YouTube::new(json!({
            "url": "https://www.youtube.com/@cerul",
            "clip_duration_sec": 12,
            "ytdlp_path": fake_ytdlp(&temp),
            "cache_dir": temp.path().join("cache"),
        }))
        .unwrap();

        assert_eq!(source.clip_duration_sec(), Some(12));
    }

    #[test]
    fn ytdlp_access_reads_browser_cookie_config() {
        let access = YtdlpAccess::from_config(&json!({
            "cookies_from_browser": "chrome:Default"
        }));

        assert_eq!(
            access.cookies_from_browser.as_deref(),
            Some("chrome:Default")
        );
        assert!(access.cookies_path.is_none());
    }

    #[test]
    fn ytdlp_access_reads_cookie_file_config() {
        let access = YtdlpAccess::from_config(&json!({
            "cookies_path": "~/Downloads/youtube-cookies.txt"
        }));

        assert!(access.cookies_from_browser.is_none());
        assert!(access
            .cookies_path
            .as_deref()
            .is_some_and(|path| path.ends_with("Downloads/youtube-cookies.txt")));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fetch_downloads_video_to_cache() {
        let temp = tempfile::tempdir().unwrap();
        let source = YouTube::new(json!({
            "url": "https://www.youtube.com/@cerul",
            "ytdlp_path": fake_ytdlp(&temp),
            "cache_dir": temp.path().join("cache"),
        }))
        .unwrap();
        let item = DiscoveredItem {
            external_id: "abc123".to_string(),
            title: Some("First video".to_string()),
            duration_sec: Some(12.0),
            metadata: json!({}),
        };

        let fetched = source.fetch(&item).await.unwrap();

        assert_eq!(fetched, temp.path().join("cache").join("abc123.mp4"));
        assert_eq!(std::fs::read_to_string(fetched).unwrap(), "video");
    }

    #[test]
    fn requires_channel_url() {
        let error = YouTube::new(json!({})).unwrap_err().to_string();

        assert!(error.contains("config.url"));
    }

    #[test]
    fn rejects_non_youtube_source_url() {
        let error = YouTube::new(json!({
            "url": "https://example.com/@cerul"
        }))
        .unwrap_err()
        .to_string();

        assert!(error.contains("youtube.com"));
    }

    #[test]
    fn bundled_candidates_include_target_triple_layout() {
        let candidates = bundled_ytdlp_candidates("yt-dlp");

        assert!(candidates.iter().any(|path| {
            path.ends_with(
                Path::new("third-party")
                    .join(bundled_target_dir())
                    .join("yt-dlp"),
            )
        }));
    }
}
