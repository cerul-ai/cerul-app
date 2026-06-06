use crate::whisper::Segment;

#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptChunk {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

pub fn chunk_segments(
    segments: &[Segment],
    window_sec: f64,
    overlap_sec: f64,
) -> Vec<TranscriptChunk> {
    assert!(window_sec > 0.0, "window_sec must be greater than zero");
    assert!(overlap_sec >= 0.0, "overlap_sec must not be negative");
    assert!(
        overlap_sec < window_sec,
        "overlap_sec must be smaller than window_sec"
    );

    let Some(first) = segments.first() else {
        return Vec::new();
    };
    let Some(last) = segments.last() else {
        return Vec::new();
    };

    let step_sec = window_sec - overlap_sec;
    let mut chunks = Vec::new();
    let mut window_start = first.start;

    while window_start < last.end {
        let window_end = window_start + window_sec;
        let mut chunk_text = Vec::new();
        let mut actual_start: Option<f64> = None;
        let mut actual_end: Option<f64> = None;

        for segment in segments {
            if segment.end <= window_start {
                continue;
            }
            if segment.start >= window_end {
                break;
            }

            let text = segment.text.trim();
            if text.is_empty() {
                continue;
            }

            actual_start.get_or_insert(segment.start.max(window_start));
            actual_end = Some(segment.end.min(window_end));
            chunk_text.push(text);
        }

        if let (Some(start), Some(end)) = (actual_start, actual_end) {
            chunks.push(TranscriptChunk {
                start,
                end,
                text: chunk_text.join(" "),
            });
        }

        window_start += step_sec;
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_segments_returns_empty_for_empty_input() {
        assert!(chunk_segments(&[], 30.0, 5.0).is_empty());
    }

    #[test]
    fn chunk_segments_uses_overlapping_windows() {
        let segments = (0..12)
            .map(|index| Segment {
                start: index as f64 * 5.0,
                end: index as f64 * 5.0 + 5.0,
                text: format!("segment-{index}"),
            })
            .collect::<Vec<_>>();

        let chunks = chunk_segments(&segments, 30.0, 5.0);

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].start, 0.0);
        assert_eq!(chunks[0].end, 30.0);
        assert!(chunks[0].text.contains("segment-0"));
        assert!(chunks[0].text.contains("segment-5"));

        assert_eq!(chunks[1].start, 25.0);
        assert_eq!(chunks[1].end, 55.0);
        assert!(chunks[1].text.contains("segment-5"));
        assert!(chunks[1].text.contains("segment-10"));
    }

    #[test]
    fn ten_minute_transcript_produces_roughly_twenty_chunks() {
        let segments = (0..120)
            .map(|index| Segment {
                start: index as f64 * 5.0,
                end: index as f64 * 5.0 + 5.0,
                text: format!("segment-{index}"),
            })
            .collect::<Vec<_>>();

        let chunks = chunk_segments(&segments, 30.0, 5.0);

        assert!((20..=25).contains(&chunks.len()));
        assert_eq!(chunks.len(), 24);
        assert!(chunks.iter().all(|chunk| chunk.end - chunk.start <= 30.0));
        assert_eq!(chunks.first().unwrap().start, 0.0);
        assert_eq!(chunks.last().unwrap().end, 600.0);
    }
}
