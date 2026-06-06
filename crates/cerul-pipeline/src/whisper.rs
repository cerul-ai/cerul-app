use std::{path::Path, sync::Arc};

use anyhow::Context;
use hound::{SampleFormat, WavReader};
use whisper_rs::{
    convert_integer_to_float_audio, convert_stereo_to_mono_audio, FullParams, SamplingStrategy,
    WhisperContext, WhisperContextParameters,
};

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

pub type TranscriptionProgress = Arc<dyn Fn(i32) + Send + Sync + 'static>;

pub struct WhisperEngine {
    ctx: WhisperContext,
}

impl WhisperEngine {
    pub fn load(model_path: &Path) -> anyhow::Result<Self> {
        anyhow::ensure!(
            model_path.is_file(),
            "Whisper model file does not exist: {}",
            model_path.display()
        );

        let ctx = WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
            .with_context(|| format!("failed to load Whisper model: {}", model_path.display()))?;
        Ok(Self { ctx })
    }

    pub fn transcribe(&self, audio_path: &Path) -> anyhow::Result<Vec<Segment>> {
        self.transcribe_with_progress(audio_path, None)
    }

    pub fn transcribe_with_progress(
        &self,
        audio_path: &Path,
        progress: Option<TranscriptionProgress>,
    ) -> anyhow::Result<Vec<Segment>> {
        let samples = wav_to_samples(audio_path)?;
        let mut state = self.ctx.create_state()?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

        params.set_n_threads(num_cpus::get() as i32);
        params.set_translate(false);
        params.set_language(Some("auto"));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        if let Some(progress) = progress {
            let callback: Box<dyn FnMut(i32)> = Box::new(move |percent: i32| {
                progress(percent);
            });
            params.set_progress_callback_safe::<Option<Box<dyn FnMut(i32)>>, Box<dyn FnMut(i32)>>(
                Some(callback),
            );
        }

        state.full(params, &samples)?;

        state
            .as_iter()
            .map(|segment| {
                Ok(Segment {
                    start: segment.start_timestamp() as f64 / 100.0,
                    end: segment.end_timestamp() as f64 / 100.0,
                    text: segment.to_str_lossy()?.trim().to_string(),
                })
            })
            .collect()
    }
}

pub fn wav_to_samples(audio_path: &Path) -> anyhow::Result<Vec<f32>> {
    let reader = WavReader::open(audio_path)
        .with_context(|| format!("failed to open WAV file: {}", audio_path.display()))?;
    let spec = reader.spec();

    anyhow::ensure!(
        spec.sample_rate == 16_000,
        "Whisper input must be 16 kHz WAV, got {} Hz",
        spec.sample_rate
    );

    let samples = match (spec.sample_format, spec.bits_per_sample) {
        (SampleFormat::Int, 16) => {
            let pcm = reader
                .into_samples::<i16>()
                .collect::<Result<Vec<_>, _>>()?;
            let mut samples = vec![0.0f32; pcm.len()];
            convert_integer_to_float_audio(&pcm, &mut samples)?;
            samples
        }
        (SampleFormat::Float, 32) => reader
            .into_samples::<f32>()
            .collect::<Result<Vec<_>, _>>()?,
        (format, bits) => anyhow::bail!(
            "unsupported WAV format for Whisper input: {format:?} with {bits} bits per sample"
        ),
    };

    mono_samples(samples, spec.channels)
}

fn mono_samples(samples: Vec<f32>, channels: u16) -> anyhow::Result<Vec<f32>> {
    match channels {
        1 => Ok(samples),
        2 => {
            anyhow::ensure!(
                samples.len().is_multiple_of(2),
                "stereo WAV input has an odd sample count"
            );
            let mut mono = vec![0.0; samples.len() / 2];
            convert_stereo_to_mono_audio(&samples, &mut mono)?;
            Ok(mono)
        }
        other => anyhow::bail!("unsupported WAV channel count for Whisper input: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::{WavSpec, WavWriter};

    #[test]
    fn wav_to_samples_reads_16khz_mono_i16() {
        let temp = tempfile::tempdir().unwrap();
        let wav = temp.path().join("sample.wav");
        write_test_wav(&wav, 16_000, 1).unwrap();

        let samples = wav_to_samples(&wav).unwrap();

        assert_eq!(samples.len(), 16_000);
        assert!(samples.iter().any(|sample| sample.abs() > 0.0));
    }

    #[test]
    fn wav_to_samples_rejects_non_16khz_input() {
        let temp = tempfile::tempdir().unwrap();
        let wav = temp.path().join("sample.wav");
        write_test_wav(&wav, 44_100, 1).unwrap();

        let error = wav_to_samples(&wav).unwrap_err().to_string();

        assert!(error.contains("16 kHz"));
    }

    #[test]
    fn wav_to_samples_downmixes_stereo() {
        let temp = tempfile::tempdir().unwrap();
        let wav = temp.path().join("sample.wav");
        write_test_wav(&wav, 16_000, 2).unwrap();

        let samples = wav_to_samples(&wav).unwrap();

        assert_eq!(samples.len(), 16_000);
    }

    #[test]
    fn whisper_load_rejects_missing_model() {
        match WhisperEngine::load(Path::new("/definitely/missing/ggml-model.bin")) {
            Ok(_) => panic!("missing Whisper model should be rejected"),
            Err(error) => assert!(error
                .to_string()
                .contains("Whisper model file does not exist")),
        }
    }

    #[test]
    #[ignore = "requires CERUL_WHISPER_MODEL_PATH and CERUL_WHISPER_SAMPLE_WAV"]
    fn whisper_transcribe_sample() {
        let model = std::env::var("CERUL_WHISPER_MODEL_PATH")
            .context("CERUL_WHISPER_MODEL_PATH is required")
            .unwrap();
        let wav = std::env::var("CERUL_WHISPER_SAMPLE_WAV")
            .context("CERUL_WHISPER_SAMPLE_WAV is required")
            .unwrap();

        let engine = WhisperEngine::load(Path::new(&model)).unwrap();
        let segments = engine.transcribe(Path::new(&wav)).unwrap();

        assert!(!segments.is_empty());
        assert!(segments.iter().any(|segment| !segment.text.is_empty()));
    }

    fn write_test_wav(path: &Path, sample_rate: u32, channels: u16) -> anyhow::Result<()> {
        let spec = WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::create(path, spec)?;
        let frequency = 440.0f32;

        for index in 0..sample_rate {
            let amplitude =
                (2.0 * std::f32::consts::PI * frequency * index as f32 / sample_rate as f32).sin()
                    * i16::MAX as f32
                    * 0.25;
            let sample = amplitude as i16;

            for _ in 0..channels {
                writer.write_sample(sample)?;
            }
        }

        writer.finalize()?;
        Ok(())
    }
}
