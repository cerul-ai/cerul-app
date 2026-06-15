use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Stdio,
};

use std::time::Duration;

use tokio::process::Command;

/// Hung ffmpeg on corrupt media used to pin an indexing job forever; every
/// invocation now runs under a wall-clock ceiling and is killed on timeout.
const FFMPEG_LONG_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const FFMPEG_CLIP_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const FFMPEG_PROBE_TIMEOUT: Duration = Duration::from_secs(60);

async fn run_ffmpeg_with_timeout(
    command: &mut Command,
    label: &str,
    timeout: Duration,
) -> anyhow::Result<std::process::Output> {
    command.kill_on_drop(true);
    tokio::time::timeout(timeout, command.output())
        .await
        .map_err(|_| anyhow::anyhow!("ffmpeg {label} timed out after {}s", timeout.as_secs()))?
        .map_err(anyhow::Error::from)
}

pub async fn extract_audio(video: &Path, out: &Path) -> anyhow::Result<()> {
    if out.exists() {
        return Ok(());
    }

    if let Some(parent) = out.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let mut command = Command::new(bundled_ffmpeg_path());
    command
        .args(["-y", "-i"])
        .arg(video)
        .args([
            "-vn",
            "-ar",
            "16000",
            "-ac",
            "1",
            "-c:a",
            "pcm_s16le",
            "-f",
            "wav",
        ])
        .arg(out)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let output =
        run_ffmpeg_with_timeout(&mut command, "extract_audio", FFMPEG_LONG_TIMEOUT).await?;

    if !output.status.success() {
        anyhow::bail!(
            "ffmpeg extract_audio failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

pub async fn sample_frames(
    video: &Path,
    out_dir: &Path,
    interval_sec: u32,
) -> anyhow::Result<Vec<PathBuf>> {
    anyhow::ensure!(interval_sec > 0, "interval_sec must be greater than zero");

    tokio::fs::create_dir_all(out_dir).await?;
    remove_existing_frames(out_dir).await?;

    let pattern = out_dir.join("frame_%06d.jpg");
    let frame_filter = format!(
        "fps=1/{interval_sec},scale='min(640,iw)':'min(640,ih)':force_original_aspect_ratio=decrease,setsar=1"
    );
    let mut command = Command::new(bundled_ffmpeg_path());
    command
        .args(["-y", "-i"])
        .arg(video)
        .args(["-vf", &frame_filter])
        .args(["-q:v", "3"])
        .arg(pattern)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let output =
        run_ffmpeg_with_timeout(&mut command, "sample_frames", FFMPEG_LONG_TIMEOUT).await?;

    if !output.status.success() {
        anyhow::bail!(
            "ffmpeg sample_frames failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let frames = collect_frames(out_dir).await?;
    dedupe_adjacent_exact_frames(frames).await
}

pub async fn export_clip(
    video: &Path,
    out: &Path,
    start_sec: f64,
    duration_sec: f64,
) -> anyhow::Result<()> {
    anyhow::ensure!(start_sec >= 0.0, "start_sec must be non-negative");
    anyhow::ensure!(duration_sec > 0.0, "duration_sec must be greater than zero");

    if out.exists() && out.metadata()?.len() > 0 {
        return Ok(());
    }

    if let Some(parent) = out.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let copy_result = run_clip_command(video, out, start_sec, duration_sec, true).await;
    if copy_result.is_ok() {
        return Ok(());
    }
    let _ = tokio::fs::remove_file(out).await;

    run_clip_command(video, out, start_sec, duration_sec, false)
        .await
        .map_err(|fallback| {
            anyhow::anyhow!(
                "ffmpeg export_clip failed: copy={} fallback={fallback}",
                copy_result.unwrap_err()
            )
        })
}

async fn remove_existing_frames(out_dir: &Path) -> anyhow::Result<()> {
    let mut entries = tokio::fs::read_dir(out_dir).await?;

    while let Some(entry) = entries.next_entry().await? {
        if is_frame_jpg(&entry.path()) {
            tokio::fs::remove_file(entry.path()).await?;
        }
    }

    Ok(())
}

pub async fn media_duration(path: &Path) -> anyhow::Result<f64> {
    let mut command = Command::new(bundled_ffmpeg_path());
    command
        .args(["-hide_banner", "-i"])
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let output =
        run_ffmpeg_with_timeout(&mut command, "media_duration", FFMPEG_PROBE_TIMEOUT).await?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    parse_duration(&stderr).ok_or_else(|| {
        anyhow::anyhow!(
            "ffmpeg could not read media duration for {}",
            path.display()
        )
    })
}

/// Best-effort check for whether `video` carries at least one audio stream.
///
/// We don't bundle `ffprobe`, so this parses `ffmpeg -i` stderr the same way
/// [`media_duration`] does. Returns `Ok(false)` for a readable container that
/// simply has no audio (e.g. a screen recording); it only errors when ffmpeg
/// can't be launched at all. Callers should treat an error as "assume audio"
/// so a probe hiccup never silently skips transcription on a normal video.
pub async fn probe_has_audio(video: &Path) -> anyhow::Result<bool> {
    let mut command = Command::new(bundled_ffmpeg_path());
    command
        .args(["-hide_banner", "-i"])
        .arg(video)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let output =
        run_ffmpeg_with_timeout(&mut command, "probe_has_audio", FFMPEG_PROBE_TIMEOUT).await?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok(stderr_reports_audio_stream(&stderr))
}

fn stderr_reports_audio_stream(stderr: &str) -> bool {
    // Stream lines look like: "Stream #0:1(eng): Audio: aac (LC), 48000 Hz".
    stderr.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("Stream #") && trimmed.contains("Audio:")
    })
}

async fn collect_frames(out_dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut frames = Vec::new();
    let mut entries = tokio::fs::read_dir(out_dir).await?;

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if is_frame_jpg(&path) {
            frames.push(path);
        }
    }

    frames.sort();
    Ok(frames)
}

async fn dedupe_adjacent_exact_frames(frames: Vec<PathBuf>) -> anyhow::Result<Vec<PathBuf>> {
    let mut kept = Vec::with_capacity(frames.len());
    let mut previous_bytes: Option<Vec<u8>> = None;

    for frame in frames {
        let bytes = tokio::fs::read(&frame).await?;
        if previous_bytes.as_ref() == Some(&bytes) {
            tokio::fs::remove_file(&frame).await?;
            continue;
        }

        previous_bytes = Some(bytes);
        kept.push(frame);
    }

    Ok(kept)
}

async fn run_clip_command(
    video: &Path,
    out: &Path,
    start_sec: f64,
    duration_sec: f64,
    copy_streams: bool,
) -> anyhow::Result<()> {
    let start = format!("{start_sec:.3}");
    let duration = format!("{duration_sec:.3}");
    // Render into a unique temp file and rename into place: concurrent
    // requests for the same uncached clip used to write the same path with
    // `-y` simultaneously and could produce a corrupt mp4.
    let tmp = out.with_file_name(format!(
        "{}.{}.partial.mp4",
        out.file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("clip"),
        std::process::id(),
    ));
    let mut command = Command::new(bundled_ffmpeg_path());
    command
        .args(["-y", "-ss", &start, "-i"])
        .arg(video)
        .args(["-t", &duration, "-map", "0:v:0?", "-map", "0:a:0?"]);

    if copy_streams {
        command.args(["-c", "copy", "-avoid_negative_ts", "make_zero"]);
    } else {
        // Re-encode fallback uses macOS's hardware H.264 encoder (VideoToolbox)
        // instead of libx264: the bundled ffmpeg is an LGPL build with no x264
        // (GPL), and VideoToolbox needs no licence and is faster. `-q:v` is its
        // constant-quality control (resolution-independent), ~equivalent to the
        // old x264 CRF 23.
        command.args(["-c:v", "h264_videotoolbox", "-q:v", "60", "-c:a", "aac"]);
    }

    command
        .args(["-movflags", "+faststart"])
        .arg(&tmp)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let output = run_ffmpeg_with_timeout(&mut command, "export_clip", FFMPEG_CLIP_TIMEOUT).await;
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(error);
        }
    };

    if !output.status.success() {
        let _ = tokio::fs::remove_file(&tmp).await;
        anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr));
    }

    tokio::fs::rename(&tmp, out).await?;
    Ok(())
}

fn is_frame_jpg(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.starts_with("frame_") && name.ends_with(".jpg"))
}

fn parse_duration(output: &str) -> Option<f64> {
    let marker = "Duration:";
    let start = output.find(marker)? + marker.len();
    let raw = output[start..].trim_start();
    let timestamp = raw.split(',').next()?.trim();
    let mut parts = timestamp.split(':');
    let hours = parts.next()?.parse::<f64>().ok()?;
    let minutes = parts.next()?.parse::<f64>().ok()?;
    let seconds = parts.next()?.parse::<f64>().ok()?;
    let total = hours * 3600.0 + minutes * 60.0 + seconds;
    (total.is_finite() && total > 0.0).then_some(total)
}

pub fn bundled_ffmpeg_path() -> PathBuf {
    if let Some(path) = std::env::var_os("CERUL_FFMPEG_PATH") {
        return PathBuf::from(path);
    }

    let executable = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    let bundled = bundled_binary_candidates(executable);
    if let Some(path) = bundled
        .into_iter()
        .find(|path| path.is_file() && command_is_runnable(path))
    {
        return path;
    }

    PathBuf::from(executable)
}

fn command_is_runnable(path: &Path) -> bool {
    std::process::Command::new(path)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn bundled_binary_candidates(executable: &str) -> Vec<PathBuf> {
    let target_dir = bundled_target_dir();
    let legacy_platform_dir = format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS);
    let mut candidates = Vec::new();

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
            candidates.push(
                dir.join("third-party")
                    .join("ffmpeg")
                    .join(&legacy_platform_dir)
                    .join(executable),
            );
            candidates.push(
                dir.join("ffmpeg")
                    .join(&legacy_platform_dir)
                    .join(executable),
            );
        }
    }

    if let Ok(current_dir) = std::env::current_dir() {
        candidates.push(
            current_dir
                .join("third-party")
                .join(&target_dir)
                .join(executable),
        );
        candidates.push(
            current_dir
                .join("third-party")
                .join("ffmpeg")
                .join(legacy_platform_dir)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ffmpeg_duration_line() {
        let output =
            "Input #0, mov,mp4\n  Duration: 00:02:21.01, start: 0.000000, bitrate: 716 kb/s";
        assert_eq!(parse_duration(output), Some(141.01));
    }

    #[test]
    fn detects_audio_stream_from_ffmpeg_streams() {
        let with_audio = "  Stream #0:0(und): Video: h264, 1920x1080\n  Stream #0:1(und): Audio: aac (LC), 48000 Hz, stereo";
        // Matches the user's screen-recording case: a video stream, no audio.
        let video_only =
            "  Stream #0:0(und): Video: h264 (avc1 / 0x31637661), none, 3840x2160, 600 tbr";
        assert!(stderr_reports_audio_stream(with_audio));
        assert!(!stderr_reports_audio_stream(video_only));
    }

    #[tokio::test]
    async fn ffmpeg_extract_audio() {
        let temp = tempfile::tempdir().unwrap();
        let video = temp.path().join("sample.mp4");
        let audio = temp.path().join("audio").join("sample.wav");

        create_sample_video(&video).await.unwrap();
        extract_audio(&video, &audio).await.unwrap();

        assert!(audio.is_file());
        assert!(audio.metadata().unwrap().len() > 0);
    }

    #[tokio::test]
    async fn ffmpeg_probe_has_audio() {
        let temp = tempfile::tempdir().unwrap();
        let with_audio = temp.path().join("with-audio.mp4");
        let silent = temp.path().join("silent.mp4");

        create_sample_video(&with_audio).await.unwrap(); // has a sine audio track
        create_static_video(&silent).await.unwrap(); // video-only, no audio

        assert!(probe_has_audio(&with_audio).await.unwrap());
        assert!(!probe_has_audio(&silent).await.unwrap());
    }

    #[tokio::test]
    async fn ffmpeg_sample_frames() {
        let temp = tempfile::tempdir().unwrap();
        let video = temp.path().join("sample.mp4");
        let frames = temp.path().join("frames");

        create_sample_video(&video).await.unwrap();
        let sampled = sample_frames(&video, &frames, 1).await.unwrap();

        assert!(!sampled.is_empty());
        assert!(sampled.iter().all(|path| path.is_file()));
    }

    #[tokio::test]
    async fn ffmpeg_sample_frames_downsamples_hd_frames() {
        let temp = tempfile::tempdir().unwrap();
        let video = temp.path().join("sample-hd.mp4");
        let frames = temp.path().join("frames");

        create_sample_video_with_size(&video, "1280x720")
            .await
            .unwrap();
        let sampled = sample_frames(&video, &frames, 1).await.unwrap();
        let image = image::open(&sampled[0]).unwrap();

        assert!(image.width() <= 640);
        assert!(image.height() <= 640);
    }

    #[tokio::test]
    async fn ffmpeg_sample_frames_skips_adjacent_static_frames() {
        let temp = tempfile::tempdir().unwrap();
        let video = temp.path().join("static.mp4");
        let frames = temp.path().join("frames");

        create_static_video(&video).await.unwrap();
        let sampled = sample_frames(&video, &frames, 1).await.unwrap();

        assert_eq!(sampled.len(), 1);
        assert!(sampled[0].is_file());
    }

    #[tokio::test]
    async fn ffmpeg_export_clip_writes_short_video() {
        let temp = tempfile::tempdir().unwrap();
        let video = temp.path().join("sample.mp4");
        let clip = temp.path().join("clips").join("clip.mp4");

        create_sample_video(&video).await.unwrap();
        export_clip(&video, &clip, 0.2, 0.8).await.unwrap();

        assert!(clip.is_file());
        assert!(clip.metadata().unwrap().len() > 0);
    }

    #[test]
    fn bundled_candidates_include_target_triple_layout() {
        let candidates = bundled_binary_candidates("ffmpeg");

        assert!(candidates.iter().any(|path| path.ends_with(
            Path::new("third-party")
                .join(bundled_target_dir())
                .join("ffmpeg")
        )));
    }

    async fn create_sample_video(path: &Path) -> anyhow::Result<()> {
        create_sample_video_with_size(path, "64x64").await
    }

    async fn create_sample_video_with_size(path: &Path, size: &str) -> anyhow::Result<()> {
        let output = Command::new(bundled_ffmpeg_path())
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                &format!("testsrc=duration=2:size={size}:rate=10"),
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=1000:duration=2",
                "-shortest",
                "-c:v",
                "mpeg4",
                "-c:a",
                "aac",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !output.status.success() {
            anyhow::bail!(
                "ffmpeg sample video generation failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    async fn create_static_video(path: &Path) -> anyhow::Result<()> {
        let output = Command::new(bundled_ffmpeg_path())
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:duration=3:size=64x64:rate=10",
                "-c:v",
                "mpeg4",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !output.status.success() {
            anyhow::bail!(
                "ffmpeg static video generation failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }
}
