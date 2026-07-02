use std::path::{Path, PathBuf};

use cerul_storage::StorageImageChunk;

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
