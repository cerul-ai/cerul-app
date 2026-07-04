use std::{path::PathBuf, sync::Arc};

use crate::run::{OcrEngine, OcrFrame, PipelineProgress};

pub(crate) async fn read_ocr_frames_with_progress(
    item_id: &str,
    frames: Vec<PathBuf>,
    ocr: Arc<dyn OcrEngine>,
    progress: Arc<dyn PipelineProgress>,
    base: f64,
    span: f64,
) -> anyhow::Result<Vec<OcrFrame>> {
    let ocr_item_id = item_id.to_string();
    match tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<OcrFrame>> {
        let total = frames.len();
        let mut collected = Vec::with_capacity(total);
        for (index, frame) in frames.iter().enumerate() {
            collected.extend(ocr.ocr_images(std::slice::from_ref(frame))?);
            let done = index + 1;
            let fraction = done as f64 / total.max(1) as f64;
            progress.update(
                &ocr_item_id,
                "ocr_frames",
                base + fraction * span,
                &format!("Reading text from visual frames · {done}/{total}"),
            );
        }
        Ok(collected)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct FakeOcrEngine;

    impl OcrEngine for FakeOcrEngine {
        fn ocr_images(&self, paths: &[PathBuf]) -> anyhow::Result<Vec<OcrFrame>> {
            Ok(paths
                .iter()
                .map(|path| OcrFrame {
                    path: path.clone(),
                    text: format!("text from {}", path.display()),
                })
                .collect())
        }
    }

    #[derive(Default)]
    struct RecordingProgress {
        events: Mutex<Vec<(String, f64, String)>>,
    }

    impl PipelineProgress for RecordingProgress {
        fn update(&self, _item_id: &str, stage: &'static str, progress: f64, message: &str) {
            self.events
                .lock()
                .unwrap()
                .push((stage.to_string(), progress, message.to_string()));
        }
    }

    #[tokio::test]
    async fn read_ocr_frames_reports_per_frame_progress() {
        let progress = Arc::new(RecordingProgress::default());
        let progress_for_stage: Arc<dyn PipelineProgress> = progress.clone();
        let frames = vec![
            PathBuf::from("/tmp/frame_000001.jpg"),
            PathBuf::from("/tmp/frame_000002.jpg"),
        ];

        let output = read_ocr_frames_with_progress(
            "item-1",
            frames.clone(),
            Arc::new(FakeOcrEngine),
            progress_for_stage,
            0.64,
            0.03,
        )
        .await
        .unwrap();

        assert_eq!(output.len(), frames.len());
        assert_eq!(output[0].path, frames[0]);
        let events = progress.events.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0, "ocr_frames");
        assert!(events[0].1 > 0.64);
        assert_eq!(events[1].1, 0.67);
        assert!(events[1].2.contains("2/2"));
    }
}
