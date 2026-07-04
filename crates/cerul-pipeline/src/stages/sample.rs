use std::path::{Path, PathBuf};

use cerul_storage::{AppPaths, StorageImageChunk};

use crate::ffmpeg;

pub(crate) struct SampledVideoKeyframes {
    pub(crate) frames: Vec<PathBuf>,
    pub(crate) keyframes: Vec<StorageImageChunk>,
}

pub(crate) async fn sample_video_keyframes(
    paths: &AppPaths,
    item_id: &str,
    video_path: &Path,
    frames_dir: &Path,
    interval_sec: u32,
) -> anyhow::Result<SampledVideoKeyframes> {
    let frames = ffmpeg::sample_frames(video_path, frames_dir, interval_sec).await?;
    let keyframes = keyframe_chunks(&frames, interval_sec);
    match cerul_storage::replace_item_keyframes(paths, item_id, &keyframes) {
        Ok(count) if count > 0 => {
            tracing::info!(item_id, keyframes = count, "stored early video thumbnails");
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(%error, item_id, "failed to store early video thumbnails");
        }
    }
    Ok(SampledVideoKeyframes { frames, keyframes })
}

pub(crate) fn keyframe_chunks(frames: &[PathBuf], interval_sec: u32) -> Vec<StorageImageChunk> {
    let interval = f64::from(interval_sec.max(1));
    frames
        .iter()
        .enumerate()
        .map(|(index, frame)| {
            let start = frame_index(frame).unwrap_or(index) as f64 * interval;
            StorageImageChunk::keyframe_at(frame.clone(), start, start + interval)
        })
        .collect()
}

fn frame_index(path: &Path) -> Option<usize> {
    let stem = path.file_stem()?.to_str()?;
    let raw = stem.strip_prefix("frame_")?;
    raw.parse::<usize>().ok()?.checked_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyframe_chunks_preserve_sampled_frame_timestamps() {
        let frames = vec![
            PathBuf::from("/tmp/frame_000001.jpg"),
            PathBuf::from("/tmp/frame_000004.jpg"),
        ];

        let chunks = keyframe_chunks(&frames, 5);

        assert_eq!(chunks[0].start_sec, Some(0.0));
        assert_eq!(chunks[0].end_sec, Some(5.0));
        assert_eq!(chunks[1].start_sec, Some(15.0));
        assert_eq!(chunks[1].end_sec, Some(20.0));
    }
}
