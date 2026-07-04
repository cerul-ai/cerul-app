use cerul_storage::{StorageTranscriptChunk, StorageTranscriptLine};

use crate::{chunking, whisper::Segment};

pub(crate) struct TranscriptStorage {
    pub(crate) chunks: Vec<StorageTranscriptChunk>,
    pub(crate) lines: Vec<StorageTranscriptLine>,
}

pub(crate) fn audio_seconds_from_segments(segments: &[Segment]) -> f64 {
    segments
        .iter()
        .map(|segment| segment.end.max(segment.start))
        .fold(0.0, f64::max)
}

pub(crate) fn transcript_storage_from_segments(
    segments: &[Segment],
    window_sec: f64,
    overlap_sec: f64,
) -> TranscriptStorage {
    let lines = segments
        .iter()
        .filter_map(|segment| {
            let text = segment.text.trim();
            if text.is_empty() {
                return None;
            }
            Some(StorageTranscriptLine {
                start: segment.start,
                end: segment.end,
                text: text.to_string(),
            })
        })
        .collect::<Vec<_>>();
    let chunks = chunking::chunk_segments(segments, window_sec, overlap_sec)
        .into_iter()
        .map(|chunk| StorageTranscriptChunk {
            start: chunk.start,
            end: chunk.end,
            text: chunk.text,
        })
        .collect();

    TranscriptStorage { chunks, lines }
}
