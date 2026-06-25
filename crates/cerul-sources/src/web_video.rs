use anyhow::Context;
use async_trait::async_trait;
use cerul_models::{ContentType, DiscoveredItem};
use reqwest::Url;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::Command,
    task::JoinSet,
};

use crate::{
    url_policy::validate_external_http_url,
    youtube::{
        apply_ytdlp_ffmpeg_location, apply_ytdlp_js_runtime, default_cache_dir, default_ytdlp_path,
        expand_path, is_ytdlp_inaccessible_video_error, merge_browser_cookie_fallback_stderr,
        safe_file_stem, ytdlp_access_candidate_limit, YtdlpAccess, YTDLP_ACCESS_CHECK_CONCURRENCY,
        YTDLP_VIDEO_FORMAT,
    },
    FetchProgress, SourcePlugin,
};

static CONTENT_TYPES: [ContentType; 1] = [ContentType::Video];
const DEFAULT_AUTHOR_MAX_VIDEOS: usize = 20;

#[derive(Debug, Clone)]
pub struct WebVideo {
    source_url: String,
    classified: ClassifiedWebVideo,
    max_videos: Option<usize>,
    ytdlp_path: PathBuf,
    cache_dir: PathBuf,
    command_timeout: Option<Duration>,
    clip_duration_sec: Option<u64>,
    access: YtdlpAccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WebVideoPlatform {
    YouTube,
    Bilibili,
}

impl WebVideoPlatform {
    fn as_str(self) -> &'static str {
        match self {
            WebVideoPlatform::YouTube => "youtube",
            WebVideoPlatform::Bilibili => "bilibili",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WebVideoSourceKind {
    Single,
    Author,
}

impl WebVideoSourceKind {
    fn as_str(self) -> &'static str {
        match self {
            WebVideoSourceKind::Single => "single",
            WebVideoSourceKind::Author => "author",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ClassifiedWebVideo {
    platform: WebVideoPlatform,
    kind: WebVideoSourceKind,
    canonical_url: String,
}

struct YtdlpRunOutput {
    status: ExitStatus,
    stderr: Vec<u8>,
}

impl WebVideo {
    pub fn new(config: Value) -> anyhow::Result<Self> {
        let source_url = config
            .get("url")
            .or_else(|| config.get("source_url"))
            .and_then(|value| value.as_str())
            .context("web_video requires config.url")?
            .to_string();
        let classified = classify_web_video_url(&source_url)?;

        if let Some(source_kind) = config.get("source_kind").and_then(|value| value.as_str()) {
            let requested = match source_kind {
                "single" => WebVideoSourceKind::Single,
                "author" | "channel" => WebVideoSourceKind::Author,
                other => anyhow::bail!("unsupported web_video source_kind: {other}"),
            };
            anyhow::ensure!(
                requested == classified.kind,
                "web_video source_kind does not match URL shape"
            );
        }

        if let Some(platform) = config.get("platform").and_then(|value| value.as_str()) {
            anyhow::ensure!(
                platform == classified.platform.as_str(),
                "web_video platform does not match URL host"
            );
        }

        let max_videos = config
            .get("max_videos")
            .or_else(|| config.get("max"))
            .and_then(|value| value.as_u64())
            .map(|value| value as usize);
        let max_videos = match classified.kind {
            WebVideoSourceKind::Single => Some(1),
            WebVideoSourceKind::Author => match max_videos {
                Some(0) => None,
                Some(value) => Some(value),
                None => Some(DEFAULT_AUTHOR_MAX_VIDEOS),
            },
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
            .unwrap_or_else(|| {
                default_cache_dir()
                    .join("web_video")
                    .join(classified.platform.as_str())
            });
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
            source_url,
            classified,
            max_videos,
            ytdlp_path,
            cache_dir,
            command_timeout,
            clip_duration_sec,
            access,
        })
    }

    pub fn platform(&self) -> &'static str {
        self.classified.platform.as_str()
    }

    pub fn source_kind(&self) -> &'static str {
        self.classified.kind.as_str()
    }

    pub fn canonical_url(&self) -> &str {
        &self.classified.canonical_url
    }

    pub fn max_videos(&self) -> Option<usize> {
        self.max_videos
    }

    fn single_discovery_command(&self, include_browser_cookies: bool) -> Command {
        let mut command = Command::new(&self.ytdlp_path);
        apply_ytdlp_js_runtime(&mut command);
        command.args([
            "--no-update",
            "--dump-single-json",
            "--skip-download",
            "--no-playlist",
        ]);
        self.access
            .apply_to_command_with_browser_cookies(&mut command, include_browser_cookies);
        command
            .arg("--")
            .arg(&self.classified.canonical_url)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn author_discovery_command(&self, include_browser_cookies: bool) -> Command {
        let mut command = Command::new(&self.ytdlp_path);
        apply_ytdlp_js_runtime(&mut command);
        command.args(["--no-update", "--flat-playlist", "--dump-json"]);
        self.access
            .apply_to_command_with_browser_cookies(&mut command, include_browser_cookies);
        if let Some(candidate_limit) = ytdlp_access_candidate_limit(self.max_videos) {
            command
                .arg("--playlist-end")
                .arg(candidate_limit.to_string());
        }
        command
            .arg("--")
            .arg(&self.classified.canonical_url)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn access_check_command(&self, fetch_url: &str, include_browser_cookies: bool) -> Command {
        let mut command = Command::new(&self.ytdlp_path);
        apply_ytdlp_js_runtime(&mut command);
        command.args([
            "--no-update",
            "--dump-single-json",
            "--skip-download",
            "--no-playlist",
        ]);
        self.access
            .apply_to_command_with_browser_cookies(&mut command, include_browser_cookies);
        command
            .arg("--")
            .arg(fetch_url)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn fetch_command(
        &self,
        output_path: &Path,
        fetch_url: &str,
        include_browser_cookies: bool,
    ) -> Command {
        let mut command = Command::new(&self.ytdlp_path);
        apply_ytdlp_js_runtime(&mut command);
        apply_ytdlp_ffmpeg_location(&mut command);
        command.args([
            "--no-update",
            "--no-playlist",
            "-f",
            YTDLP_VIDEO_FORMAT,
            "--merge-output-format",
            "mp4",
            "--newline",
            "--progress-template",
            "download:CERUL_PROGRESS %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s %(progress.eta)s %(progress.speed)s",
        ]);
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
            .arg(fetch_url)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    async fn discover_single(&self) -> anyhow::Result<Vec<DiscoveredItem>> {
        let output = self
            .run_ytdlp_with_browser_cookie_fallback("single discovery", |include_browser_cookies| {
                self.single_discovery_command(include_browser_cookies)
            })
            .await?;
        if !output.status.success() {
            anyhow::bail!(
                "yt-dlp single discovery failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        let metadata = parse_single_ytdlp_json(&output.stdout)?;
        let item = self.item_from_metadata(metadata, WebVideoSourceKind::Single)?;
        Ok(vec![item])
    }

    async fn discover_author(&self) -> anyhow::Result<Vec<DiscoveredItem>> {
        let output = self
            .run_ytdlp_with_browser_cookie_fallback("author discovery", |include_browser_cookies| {
                self.author_discovery_command(include_browser_cookies)
            })
            .await?;
        if !output.status.success() {
            anyhow::bail!(
                "yt-dlp author discovery failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        let mut items = Vec::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if line.trim().is_empty() {
                continue;
            }
            let metadata: Value =
                serde_json::from_str(line).context("yt-dlp emitted invalid JSON")?;
            items.push(self.item_from_metadata(metadata, WebVideoSourceKind::Author)?);
        }
        self.filter_accessible_items(items).await
    }

    async fn is_accessible_video(&self, item: &DiscoveredItem) -> anyhow::Result<bool> {
        let fetch_url = match self.validated_fetch_url(item) {
            Ok(fetch_url) => fetch_url,
            Err(error) if is_non_video_author_candidate(&error) => {
                tracing::info!(
                    platform = self.classified.platform.as_str(),
                    external_id = %item.external_id,
                    title = item.title.as_deref().unwrap_or(""),
                    error = %error,
                    "skipping non-video web item during source discovery"
                );
                return Ok(false);
            }
            Err(error) => return Err(error),
        };
        let output = self
            .run_ytdlp_with_browser_cookie_fallback("access discovery", |include_browser_cookies| {
                self.access_check_command(&fetch_url, include_browser_cookies)
            })
            .await?;

        if output.status.success() {
            return Ok(true);
        }

        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if is_ytdlp_inaccessible_video_error(&stderr) {
            tracing::info!(
                platform = self.classified.platform.as_str(),
                external_id = %item.external_id,
                title = item.title.as_deref().unwrap_or(""),
                error = %stderr,
                "skipping inaccessible web video during source discovery"
            );
            return Ok(false);
        }

        anyhow::bail!("yt-dlp access check failed: {stderr}");
    }

    async fn filter_accessible_items(
        &self,
        candidates: Vec<DiscoveredItem>,
    ) -> anyhow::Result<Vec<DiscoveredItem>> {
        let target = self.max_videos;
        let mut accessible =
            Vec::with_capacity(target.unwrap_or(candidates.len()).min(candidates.len()));
        let mut skipped = 0usize;
        let mut in_flight = JoinSet::new();
        let mut results = std::iter::repeat_with(|| None)
            .take(candidates.len())
            .collect::<Vec<Option<anyhow::Result<bool>>>>();
        let mut next_to_spawn = 0usize;
        let mut next_to_consider = 0usize;
        let mut reached_target = false;

        loop {
            while in_flight.len() < YTDLP_ACCESS_CHECK_CONCURRENCY
                && next_to_spawn < candidates.len()
                && target
                    .map(|target| accessible.len() < target)
                    .unwrap_or(true)
            {
                let index = next_to_spawn;
                let item = candidates[index].clone();
                let source = self.clone();
                in_flight.spawn(async move {
                    let is_accessible = source.is_accessible_video(&item).await;
                    (index, is_accessible)
                });
                next_to_spawn += 1;
            }

            while next_to_consider < results.len() {
                let Some(result) = results[next_to_consider].take() else {
                    break;
                };
                if result? {
                    accessible.push(candidates[next_to_consider].clone());
                } else {
                    skipped += 1;
                }

                next_to_consider += 1;
                if target
                    .map(|target| accessible.len() >= target)
                    .unwrap_or(false)
                {
                    in_flight.abort_all();
                    reached_target = true;
                    break;
                }
            }

            if reached_target || in_flight.is_empty() {
                break;
            }

            let (index, result) = in_flight
                .join_next()
                .await
                .expect("in-flight access check exists")
                .context("failed to join yt-dlp access check task")?;
            results[index] = Some(result);
        }

        if skipped > 0 {
            tracing::info!(
                source_url = %self.classified.canonical_url,
                accessible = accessible.len(),
                skipped,
                "filtered inaccessible web videos during source discovery"
            );
        }

        Ok(accessible)
    }

    fn item_from_metadata(
        &self,
        metadata: Value,
        source_kind: WebVideoSourceKind,
    ) -> anyhow::Result<DiscoveredItem> {
        let external_id = metadata
            .get("id")
            .and_then(|value| value.as_str())
            .or_else(|| metadata.get("display_id").and_then(|value| value.as_str()))
            .context("yt-dlp item is missing id")?
            .to_string();
        let title = metadata
            .get("title")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned);
        let duration_sec = metadata.get("duration").and_then(|value| value.as_f64());
        let metadata = self.enrich_metadata(metadata, &external_id, source_kind);

        Ok(DiscoveredItem {
            external_id,
            title,
            duration_sec,
            metadata,
        })
    }

    fn enrich_metadata(
        &self,
        mut metadata: Value,
        external_id: &str,
        source_kind: WebVideoSourceKind,
    ) -> Value {
        if !metadata.is_object() {
            metadata = json!({});
        }
        let object = metadata.as_object_mut().expect("metadata is object");
        object
            .entry("platform".to_string())
            .or_insert_with(|| Value::String(self.classified.platform.as_str().to_string()));
        object
            .entry("source_kind".to_string())
            .or_insert_with(|| Value::String(source_kind.as_str().to_string()));
        object
            .entry("source_url".to_string())
            .or_insert_with(|| Value::String(self.source_url.clone()));
        object
            .entry("original_url".to_string())
            .or_insert_with(|| Value::String(self.classified.canonical_url.clone()));
        object
            .entry("webpage_url".to_string())
            .or_insert_with(|| Value::String(self.video_url_for_external_id(external_id)));
        metadata
    }

    fn video_url_for_external_id(&self, external_id: &str) -> String {
        match self.classified.platform {
            WebVideoPlatform::YouTube => {
                format!("https://www.youtube.com/watch?v={external_id}")
            }
            WebVideoPlatform::Bilibili => {
                if external_id.starts_with("BV") || external_id.starts_with("av") {
                    format!("https://www.bilibili.com/video/{external_id}")
                } else {
                    self.classified.canonical_url.clone()
                }
            }
        }
    }

    fn fetch_url(&self, item: &DiscoveredItem) -> String {
        metadata_string(&item.metadata, "webpage_url")
            .or_else(|| metadata_string(&item.metadata, "original_url"))
            .or_else(|| metadata_string(&item.metadata, "source_url"))
            .or_else(|| metadata_string(&item.metadata, "url"))
            .unwrap_or_else(|| self.video_url_for_external_id(&item.external_id))
    }

    fn validated_fetch_url(&self, item: &DiscoveredItem) -> anyhow::Result<String> {
        let fetch_url = self.fetch_url(item);
        let classified = classify_web_video_url(&fetch_url)?;
        anyhow::ensure!(
            classified.platform == self.classified.platform,
            "yt-dlp returned a different video platform than the source URL"
        );
        anyhow::ensure!(
            classified.kind == WebVideoSourceKind::Single,
            "yt-dlp returned a non-video download URL"
        );
        Ok(classified.canonical_url)
    }

    fn output_path(&self, item: &DiscoveredItem) -> PathBuf {
        self.cache_dir.join(format!(
            "{}_{}.mp4",
            self.classified.platform.as_str(),
            safe_file_stem(&item.external_id)
        ))
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
            let mut fallback_output = self.run_ytdlp(&mut fallback, phase).await?;
            if !fallback_output.status.success() {
                fallback_output.stderr =
                    merge_browser_cookie_fallback_stderr(&output.stderr, &fallback_output.stderr);
            }
            return Ok(fallback_output);
        }
        Ok(output)
    }

    async fn run_ytdlp_with_progress_and_browser_cookie_fallback<F>(
        &self,
        phase: &str,
        progress: Option<FetchProgress>,
        mut build_command: F,
    ) -> anyhow::Result<YtdlpRunOutput>
    where
        F: FnMut(bool) -> Command,
    {
        let mut command = build_command(true);
        let output = self
            .run_ytdlp_with_progress(&mut command, phase, progress.clone())
            .await?;
        if !output.status.success()
            && self
                .access
                .should_retry_without_browser_cookies(&output.stderr)
        {
            emit_progress(
                &progress,
                0.0,
                "Browser cookies unavailable; retrying without cookies",
            );
            let mut fallback = build_command(false);
            let mut fallback_output = self
                .run_ytdlp_with_progress(&mut fallback, phase, progress)
                .await?;
            if !fallback_output.status.success() {
                fallback_output.stderr =
                    merge_browser_cookie_fallback_stderr(&output.stderr, &fallback_output.stderr);
            }
            return Ok(fallback_output);
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

    async fn run_ytdlp_with_progress(
        &self,
        command: &mut Command,
        phase: &str,
        progress: Option<FetchProgress>,
    ) -> anyhow::Result<YtdlpRunOutput> {
        command.kill_on_drop(true);
        let mut child = command
            .spawn()
            .with_context(|| format!("failed to run {}", self.ytdlp_path.display()))?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let stdout_progress = progress.clone();
        let stderr_progress = progress;
        let stdout_task =
            tokio::spawn(async move { collect_output(stdout, stdout_progress).await });
        let stderr_task =
            tokio::spawn(async move { collect_output(stderr, stderr_progress).await });

        let wait = child.wait();
        let timeout = self
            .command_timeout
            .unwrap_or_else(|| crate::default_ytdlp_timeout(phase));
        let status = tokio::time::timeout(timeout, wait)
            .await
            .with_context(|| format!("yt-dlp {phase} timed out after {}s", timeout.as_secs()))?
            .with_context(|| format!("failed to wait for {}", self.ytdlp_path.display()))?;

        stdout_task
            .await
            .context("failed to join yt-dlp stdout reader")?;
        let stderr = stderr_task
            .await
            .context("failed to join yt-dlp stderr reader")?;

        Ok(YtdlpRunOutput { status, stderr })
    }
}

#[async_trait]
impl SourcePlugin for WebVideo {
    fn name(&self) -> &'static str {
        "web_video"
    }

    fn content_types(&self) -> &[ContentType] {
        &CONTENT_TYPES
    }

    async fn discover(&self) -> anyhow::Result<Vec<DiscoveredItem>> {
        match self.classified.kind {
            WebVideoSourceKind::Single => self.discover_single().await,
            WebVideoSourceKind::Author => self.discover_author().await,
        }
    }

    async fn fetch(&self, item: &DiscoveredItem) -> anyhow::Result<PathBuf> {
        self.fetch_with_progress(item, None).await
    }

    async fn fetch_with_progress(
        &self,
        item: &DiscoveredItem,
        progress: Option<FetchProgress>,
    ) -> anyhow::Result<PathBuf> {
        tokio::fs::create_dir_all(&self.cache_dir).await?;
        let output_path = self.output_path(item);
        if output_path.exists() {
            emit_progress(&progress, 1.0, "Download complete");
            return Ok(output_path);
        }

        emit_progress(&progress, 0.0, "Starting video download");
        let fetch_url = self.validated_fetch_url(item)?;
        let output = self
            .run_ytdlp_with_progress_and_browser_cookie_fallback(
                "fetch",
                progress.clone(),
                |include_browser_cookies| {
                    self.fetch_command(&output_path, &fetch_url, include_browser_cookies)
                },
            )
            .await?;
        if !output.status.success() {
            anyhow::bail!(
                "yt-dlp fetch failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        emit_progress(&progress, 1.0, "Download complete");
        Ok(output_path)
    }
}

fn classify_web_video_url(value: &str) -> anyhow::Result<ClassifiedWebVideo> {
    let parsed = validate_external_http_url(value, "web_video URL")?;
    let host = parsed
        .host_str()
        .map(|host| host.trim_start_matches("www.").to_ascii_lowercase())
        .context("web_video URL is missing a host")?;
    let path = parsed.path().trim_matches('/');
    let path_parts = path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    if host == "youtu.be" {
        anyhow::ensure!(!path_parts.is_empty(), "YouTube URL is missing video id");
        return Ok(ClassifiedWebVideo {
            platform: WebVideoPlatform::YouTube,
            kind: WebVideoSourceKind::Single,
            canonical_url: parsed.to_string(),
        });
    }

    if host == "youtube.com" || host.ends_with(".youtube.com") {
        if path_parts.first() == Some(&"playlist")
            || parsed.query_pairs().any(|(key, _)| key == "list")
                && !parsed.query_pairs().any(|(key, _)| key == "v")
        {
            anyhow::bail!("YouTube playlists are not supported yet");
        }
        let first = path_parts.first().copied().unwrap_or_default();
        if first == "watch"
            && parsed
                .query_pairs()
                .any(|(key, value)| key == "v" && !value.is_empty())
            || matches!(first, "shorts" | "live") && path_parts.len() >= 2
        {
            return Ok(ClassifiedWebVideo {
                platform: WebVideoPlatform::YouTube,
                kind: WebVideoSourceKind::Single,
                canonical_url: parsed.to_string(),
            });
        }
        if first.starts_with('@') || matches!(first, "channel" | "c" | "user") {
            let canonical_url = ensure_path_suffix(parsed, "videos");
            return Ok(ClassifiedWebVideo {
                platform: WebVideoPlatform::YouTube,
                kind: WebVideoSourceKind::Author,
                canonical_url,
            });
        }
        anyhow::bail!("unsupported YouTube URL; use a video URL or author homepage");
    }

    if host == "b23.tv" {
        return Ok(ClassifiedWebVideo {
            platform: WebVideoPlatform::Bilibili,
            kind: WebVideoSourceKind::Single,
            canonical_url: parsed.to_string(),
        });
    }

    if host == "bilibili.com" || host.ends_with(".bilibili.com") {
        if path_parts.first() == Some(&"video") && path_parts.len() >= 2 {
            return Ok(ClassifiedWebVideo {
                platform: WebVideoPlatform::Bilibili,
                kind: WebVideoSourceKind::Single,
                canonical_url: canonical_bilibili_video_url(&path_parts[1], &parsed),
            });
        }
        if host == "space.bilibili.com" && !path_parts.is_empty() {
            return Ok(ClassifiedWebVideo {
                platform: WebVideoPlatform::Bilibili,
                kind: WebVideoSourceKind::Author,
                canonical_url: canonical_bilibili_author_url(path_parts[0]),
            });
        }
        anyhow::bail!("unsupported Bilibili URL; use a video URL or author homepage");
    }

    anyhow::bail!("unsupported video host; supported hosts are YouTube and Bilibili")
}

fn canonical_bilibili_video_url(video_id: &str, source_url: &Url) -> String {
    let mut canonical = format!("https://www.bilibili.com/video/{video_id}");
    if let Some(page) = source_url
        .query_pairs()
        .find_map(|(key, value)| (key == "p").then_some(value))
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|page| *page > 0)
    {
        canonical.push_str(&format!("?p={page}"));
    }
    canonical
}

fn canonical_bilibili_author_url(mid: &str) -> String {
    format!("https://space.bilibili.com/{mid}/video")
}

fn ensure_path_suffix(mut url: Url, suffix: &str) -> String {
    let mut parts = url
        .path()
        .trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.last().copied() != Some(suffix) {
        parts.push(suffix);
    }
    url.set_path(&format!("/{}", parts.join("/")));
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

fn parse_single_ytdlp_json(stdout: &[u8]) -> anyhow::Result<Value> {
    let text = String::from_utf8_lossy(stdout);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        anyhow::bail!("yt-dlp emitted no JSON");
    }
    serde_json::from_str(trimmed)
        .or_else(|_| {
            text.lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .context("yt-dlp emitted no JSON")
                .and_then(|line| serde_json::from_str(line).context("yt-dlp emitted invalid JSON"))
        })
        .context("yt-dlp emitted invalid JSON")
}

fn metadata_string(metadata: &Value, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn is_non_video_author_candidate(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .to_string()
            .contains("yt-dlp returned a non-video download URL")
    })
}

async fn collect_output<R>(reader: Option<R>, progress: Option<FetchProgress>) -> Vec<u8>
where
    R: AsyncRead + Unpin,
{
    let Some(reader) = reader else {
        return Vec::new();
    };
    let mut reader = BufReader::new(reader);
    let mut output = Vec::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = match reader.read_until(b'\n', &mut line).await {
            Ok(read) => read,
            Err(_) => break,
        };
        if read == 0 {
            break;
        }
        output.extend_from_slice(&line);
        if let Some(update) = parse_progress_line(&String::from_utf8_lossy(&line)) {
            emit_progress(&progress, update.0, &update.1);
        }
    }
    output
}

fn emit_progress(progress: &Option<FetchProgress>, fraction: f64, message: &str) {
    if let Some(progress) = progress {
        progress(fraction.clamp(0.0, 1.0), message.to_string());
    }
}

fn parse_progress_line(line: &str) -> Option<(f64, String)> {
    let trimmed = line.trim();
    if let Some(raw) = trimmed.strip_prefix("CERUL_PROGRESS ") {
        let parts = raw.split_whitespace().collect::<Vec<_>>();
        let downloaded = parts.first().and_then(|value| parse_optional_f64(value));
        let total = parts
            .get(1)
            .and_then(|value| parse_optional_f64(value))
            .or_else(|| parts.get(2).and_then(|value| parse_optional_f64(value)));
        let eta = parts.get(3).and_then(|value| parse_optional_f64(value));
        let speed = parts.get(4).and_then(|value| parse_optional_f64(value));
        if let (Some(downloaded), Some(total)) = (downloaded, total) {
            if total > 0.0 {
                let fraction = (downloaded / total).clamp(0.0, 1.0);
                return Some((fraction, download_message(fraction, eta, speed)));
            }
        }
    }

    let percent = parse_bracket_download_percent(trimmed)?;
    let fraction = (percent / 100.0).clamp(0.0, 1.0);
    Some((fraction, download_message(fraction, None, None)))
}

fn parse_optional_f64(value: &str) -> Option<f64> {
    if matches!(value, "NA" | "N/A" | "None" | "none" | "null") {
        return None;
    }
    value.parse::<f64>().ok().filter(|value| value.is_finite())
}

fn parse_bracket_download_percent(line: &str) -> Option<f64> {
    if !line.contains("[download]") {
        return None;
    }
    let percent_index = line.find('%')?;
    let prefix = &line[..percent_index];
    let raw = prefix
        .split_whitespace()
        .last()?
        .trim_matches(|character: char| !character.is_ascii_digit() && character != '.');
    raw.parse::<f64>().ok()
}

// Language-neutral download stats, e.g. "45% · 3.2 MB/s · ETA 1:20". The renderer
// prepends the localized "downloading" stage label, so we deliberately omit any
// leading word here. Speed/ETA are dropped whenever yt-dlp hasn't reported them
// yet (early frames, or the bracket-percent fallback path).
fn download_message(fraction: f64, eta: Option<f64>, speed_bytes_per_sec: Option<f64>) -> String {
    let pct = (fraction * 100.0).round() as u64;
    let mut parts = vec![format!("{pct}%")];
    if let Some(speed) = speed_bytes_per_sec.filter(|value| *value > 0.0) {
        parts.push(format_speed(speed));
    }
    if let Some(eta) = eta.and_then(|value| (value >= 0.0).then_some(value.round() as u64)) {
        parts.push(format!("ETA {}", format_eta(eta)));
    }
    parts.join(" · ")
}

fn format_speed(bytes_per_sec: f64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    if bytes_per_sec >= GIB {
        format!("{:.1} GB/s", bytes_per_sec / GIB)
    } else if bytes_per_sec >= MIB {
        format!("{:.1} MB/s", bytes_per_sec / MIB)
    } else if bytes_per_sec >= KIB {
        format!("{:.0} KB/s", bytes_per_sec / KIB)
    } else {
        format!("{:.0} B/s", bytes_per_sec.max(0.0))
    }
}

fn format_eta(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
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
  case "$*" in
    *"@order"*)
      printf '{"id":"slowFirst","title":"Slow first YouTube video","duration":12}\n'
      printf '{"id":"fastSecond","title":"Fast second YouTube video","duration":34}\n'
      printf '{"id":"fastThird","title":"Fast third YouTube video","duration":56}\n'
      ;;
    *"@unneeded-error"*)
      printf '{"id":"abc123","title":"First YouTube video","duration":12}\n'
      printf '{"id":"forbidden","title":"Forbidden later video","duration":34}\n'
      ;;
    *youtube*)
      printf '{"id":"abc123","title":"First YouTube video","duration":12}\n'
      printf '{"id":"membersOnly","title":"Members-only video","duration":56}\n'
      printf '{"id":"def456","title":"Second YouTube video","duration":34}\n'
      ;;
    *)
      printf '{"id":"BV1aa411c7mD","title":"First Bili video","duration":12}\n'
      printf '{"id":"145149047_2367","title":"Bili season","webpage_url":"https://space.bilibili.com/145149047/lists/2367?type=season","url":"https://space.bilibili.com/145149047/lists/2367?type=season"}\n'
      printf '{"id":"BV1bb411c7mD","title":"Second Bili video","duration":34}\n'
      ;;
  esac
elif printf '%s\n' "$@" | grep -q -- '--dump-single-json'; then
  url=""
  for arg in "$@"; do
    url="$arg"
  done
  case "$url" in
    *slowFirst*)
      sleep 1
      ;;
    *membersOnly*)
      printf 'ERROR: [youtube] membersOnly: This video is available to this channel'"'"'s members\n' >&2
      exit 1
      ;;
    *forbidden*)
      printf 'ERROR: [youtube] forbidden: HTTP Error 403: Forbidden\n' >&2
      exit 1
      ;;
  esac
  printf '{"id":"abc123","title":"Single video","duration":45,"webpage_url":"https://www.youtube.com/watch?v=abc123"}\n'
else
  out=""
  while [ "$#" -gt 0 ]; do
    if [ "$1" = "-o" ]; then
      shift
      out="$1"
    fi
    shift
  done
  printf 'CERUL_PROGRESS 10 100 NA 9 1\n'
  printf 'CERUL_PROGRESS 100 100 NA 0 1\n'
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
if printf '%s\n' "$@" | grep -q -- '--dump-single-json'; then
  printf '{"id":"abc123","title":"Single video","duration":45,"webpage_url":"https://www.youtube.com/watch?v=abc123"}\n'
elif printf '%s\n' "$@" | grep -q -- '--flat-playlist'; then
  printf '{"id":"BV1aa411c7mD","title":"First Bili video","duration":12}\n'
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
    fn fake_ytdlp_with_failed_browser_cookie_fallback(temp: &tempfile::TempDir) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let script = temp.path().join("yt-dlp-cookie-fallback-fails");
        std::fs::write(
            &script,
            r#"#!/bin/sh
if printf '%s\n' "$@" | grep -q -- '--cookies-from-browser'; then
  printf 'ERROR: could not find Chrome cookies database\n' >&2
  exit 1
fi
printf 'ERROR: [BiliBili] BV1xx: HTTP Error 412: Precondition Failed\n' >&2
exit 1
"#,
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();
        script
    }

    #[test]
    fn classifies_supported_urls() {
        let youtube_single =
            classify_web_video_url("https://www.youtube.com/watch?v=abc123").unwrap();
        assert_eq!(youtube_single.platform, WebVideoPlatform::YouTube);
        assert_eq!(youtube_single.kind, WebVideoSourceKind::Single);

        let youtube_author = classify_web_video_url("https://youtube.com/@cerul").unwrap();
        assert_eq!(youtube_author.kind, WebVideoSourceKind::Author);
        assert_eq!(
            youtube_author.canonical_url,
            "https://youtube.com/@cerul/videos"
        );

        let bili_single =
            classify_web_video_url("https://www.bilibili.com/video/BV1aa411c7mD").unwrap();
        assert_eq!(bili_single.platform, WebVideoPlatform::Bilibili);
        assert_eq!(bili_single.kind, WebVideoSourceKind::Single);
        assert_eq!(
            bili_single.canonical_url,
            "https://www.bilibili.com/video/BV1aa411c7mD"
        );

        let bili_single_with_tracking = classify_web_video_url(
            "https://www.bilibili.com/video/BV1LVjd6fEdK/?spm_id_from=333.1007.top_right_bar_window_history.content.click&vd_source=b8130a78bc5596e579d32a2778e31137",
        )
        .unwrap();
        assert_eq!(bili_single_with_tracking.kind, WebVideoSourceKind::Single);
        assert_eq!(
            bili_single_with_tracking.canonical_url,
            "https://www.bilibili.com/video/BV1LVjd6fEdK"
        );

        let bili_single_part = classify_web_video_url(
            "https://www.bilibili.com/video/BV1LVjd6fEdK/?p=2&spm_id_from=333.1007.top_right_bar_window_history.content.click&vd_source=b8130a78bc5596e579d32a2778e31137",
        )
        .unwrap();
        assert_eq!(bili_single_part.kind, WebVideoSourceKind::Single);
        assert_eq!(
            bili_single_part.canonical_url,
            "https://www.bilibili.com/video/BV1LVjd6fEdK?p=2"
        );

        let bili_author = classify_web_video_url("https://space.bilibili.com/12345").unwrap();
        assert_eq!(bili_author.kind, WebVideoSourceKind::Author);
        assert_eq!(
            bili_author.canonical_url,
            "https://space.bilibili.com/12345/video"
        );

        for url in [
            "https://space.bilibili.com/12345/upload/video",
            "https://space.bilibili.com/12345/dynamic",
            "https://space.bilibili.com/12345/?spm_id_from=333.999.0.0",
        ] {
            let classified = classify_web_video_url(url).unwrap();
            assert_eq!(classified.kind, WebVideoSourceKind::Author);
            assert_eq!(
                classified.canonical_url,
                "https://space.bilibili.com/12345/video"
            );
        }
    }

    #[test]
    fn rejects_youtube_playlists() {
        let error = classify_web_video_url("https://youtube.com/playlist?list=abc")
            .unwrap_err()
            .to_string();

        assert!(error.contains("playlists"));
    }

    #[test]
    fn author_defaults_to_twenty_videos() {
        let temp = tempfile::tempdir().unwrap();
        let source = WebVideo::new(json!({
            "url": "https://space.bilibili.com/12345",
            "ytdlp_path": fake_ytdlp(&temp),
            "cache_dir": temp.path().join("cache"),
        }))
        .unwrap();

        assert_eq!(source.platform(), "bilibili");
        assert_eq!(source.source_kind(), "author");
        assert_eq!(source.max_videos(), Some(DEFAULT_AUTHOR_MAX_VIDEOS));
    }

    #[test]
    fn explicit_zero_author_max_videos_remains_unlimited() {
        let temp = tempfile::tempdir().unwrap();
        let source = WebVideo::new(json!({
            "url": "https://space.bilibili.com/12345",
            "max_videos": 0,
            "ytdlp_path": fake_ytdlp(&temp),
            "cache_dir": temp.path().join("cache"),
        }))
        .unwrap();

        assert_eq!(source.max_videos(), None);
    }

    #[test]
    fn explicit_author_max_videos_can_raise_limit() {
        let temp = tempfile::tempdir().unwrap();
        let source = WebVideo::new(json!({
            "url": "https://space.bilibili.com/12345",
            "max_videos": 50,
            "ytdlp_path": fake_ytdlp(&temp),
            "cache_dir": temp.path().join("cache"),
        }))
        .unwrap();

        assert_eq!(source.max_videos(), Some(50));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn discovers_single_video() {
        let temp = tempfile::tempdir().unwrap();
        let source = WebVideo::new(json!({
            "url": "https://www.youtube.com/watch?v=abc123",
            "ytdlp_path": fake_ytdlp(&temp),
            "cache_dir": temp.path().join("cache"),
        }))
        .unwrap();

        let items = source.discover().await.unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].external_id, "abc123");
        assert_eq!(items[0].metadata["platform"].as_str(), Some("youtube"));
        assert_eq!(items[0].metadata["source_kind"].as_str(), Some("single"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn falls_back_when_browser_cookies_are_unavailable() {
        let temp = tempfile::tempdir().unwrap();
        let source = WebVideo::new(json!({
            "url": "https://www.youtube.com/watch?v=abc123",
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
    #[tokio::test]
    async fn keeps_browser_cookie_error_when_fallback_also_fails() {
        let temp = tempfile::tempdir().unwrap();
        let source = WebVideo::new(json!({
            "url": "https://www.bilibili.com/video/BV1aa411c7mD",
            "cookies_from_browser": "chrome",
            "ytdlp_path": fake_ytdlp_with_failed_browser_cookie_fallback(&temp),
            "cache_dir": temp.path().join("cache"),
        }))
        .unwrap();

        let error = source.discover().await.unwrap_err().to_string();

        assert!(error.contains("could not find Chrome cookies database"));
        assert!(error.contains("Retry without browser cookies also failed"));
        assert!(error.contains("HTTP Error 412"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn discovers_author_videos() {
        let temp = tempfile::tempdir().unwrap();
        let source = WebVideo::new(json!({
            "url": "https://space.bilibili.com/12345",
            "max_videos": 2,
            "ytdlp_path": fake_ytdlp(&temp),
            "cache_dir": temp.path().join("cache"),
        }))
        .unwrap();

        let items = source.discover().await.unwrap();

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].external_id, "BV1aa411c7mD");
        assert_eq!(
            items[0].metadata["webpage_url"].as_str(),
            Some("https://www.bilibili.com/video/BV1aa411c7mD")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn author_discovery_skips_inaccessible_videos() {
        let temp = tempfile::tempdir().unwrap();
        let source = WebVideo::new(json!({
            "url": "https://www.youtube.com/@cerul",
            "max_videos": 2,
            "ytdlp_path": fake_ytdlp(&temp),
            "cache_dir": temp.path().join("cache"),
        }))
        .unwrap();

        let items = source.discover().await.unwrap();

        assert_eq!(
            items
                .iter()
                .map(|item| item.external_id.as_str())
                .collect::<Vec<_>>(),
            vec!["abc123", "def456"]
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn author_discovery_preserves_order_with_concurrent_access_checks() {
        let temp = tempfile::tempdir().unwrap();
        let source = WebVideo::new(json!({
            "url": "https://www.youtube.com/@order",
            "max_videos": 2,
            "ytdlp_path": fake_ytdlp(&temp),
            "cache_dir": temp.path().join("cache"),
        }))
        .unwrap();

        let items = source.discover().await.unwrap();

        assert_eq!(
            items
                .iter()
                .map(|item| item.external_id.as_str())
                .collect::<Vec<_>>(),
            vec!["slowFirst", "fastSecond"]
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn author_discovery_ignores_unneeded_later_probe_errors() {
        let temp = tempfile::tempdir().unwrap();
        let source = WebVideo::new(json!({
            "url": "https://www.youtube.com/@unneeded-error",
            "max_videos": 1,
            "ytdlp_path": fake_ytdlp(&temp),
            "cache_dir": temp.path().join("cache"),
        }))
        .unwrap();

        let items = source.discover().await.unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].external_id, "abc123");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fetch_reports_progress_and_downloads_to_cache() {
        let temp = tempfile::tempdir().unwrap();
        let source = WebVideo::new(json!({
            "url": "https://www.youtube.com/watch?v=abc123",
            "ytdlp_path": fake_ytdlp(&temp),
            "cache_dir": temp.path().join("cache"),
        }))
        .unwrap();
        let item = DiscoveredItem {
            external_id: "abc123".to_string(),
            title: Some("Single video".to_string()),
            duration_sec: Some(45.0),
            metadata: json!({
                "webpage_url": "https://www.youtube.com/watch?v=abc123",
                "platform": "youtube",
                "source_kind": "single"
            }),
        };
        let updates = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let updates_for_callback = updates.clone();
        let progress: FetchProgress = std::sync::Arc::new(move |fraction, message| {
            updates_for_callback
                .lock()
                .unwrap()
                .push((fraction, message));
        });

        let fetched = source
            .fetch_with_progress(&item, Some(progress))
            .await
            .unwrap();

        assert_eq!(
            fetched,
            temp.path().join("cache").join("youtube_abc123.mp4")
        );
        assert_eq!(std::fs::read_to_string(fetched).unwrap(), "video");
        let updates = updates.lock().unwrap();
        assert!(updates.iter().any(|(fraction, _)| *fraction > 0.0));
        assert!(updates
            .iter()
            .any(|(fraction, _)| (*fraction - 1.0).abs() < f64::EPSILON));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fetch_rejects_cross_platform_metadata_url() {
        let temp = tempfile::tempdir().unwrap();
        let source = WebVideo::new(json!({
            "url": "https://www.youtube.com/watch?v=abc123",
            "ytdlp_path": fake_ytdlp(&temp),
            "cache_dir": temp.path().join("cache"),
        }))
        .unwrap();
        let item = DiscoveredItem {
            external_id: "abc123".to_string(),
            title: Some("Single video".to_string()),
            duration_sec: Some(45.0),
            metadata: json!({
                "webpage_url": "https://www.bilibili.com/video/BV1aa411c7mD",
                "platform": "youtube",
                "source_kind": "single"
            }),
        };

        let error = source.fetch(&item).await.unwrap_err().to_string();

        assert!(error.contains("different video platform"));
    }

    #[test]
    fn parses_progress_template_and_legacy_download_lines() {
        let structured = parse_progress_line("CERUL_PROGRESS 50 100 NA 5 1").unwrap();
        assert_eq!(structured.0, 0.5);
        assert!(structured.1.contains("ETA 0:05"));
        // The yt-dlp speed field (5th column) now rides along in the message.
        assert!(
            structured.1.contains("B/s"),
            "message missing speed: {}",
            structured.1
        );

        let legacy =
            parse_progress_line("[download]  23.4% of 10.00MiB at 1MiB/s ETA 00:07").unwrap();
        assert!((legacy.0 - 0.234).abs() < 0.001);
    }
}
