use std::{
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use candle_core::{DType, Device};
pub use downloads::{ModelDownloadConfig, ModelDownloadSource};
use fastembed::Qwen3VLEmbedding;

mod downloads;

pub const QWEN3_VL_REPO_ID: &str = "Qwen/Qwen3-VL-Embedding-2B";
pub const DEFAULT_MAX_TEXT_TOKENS: usize = 512;
pub const VECTOR_DIMENSIONS: usize = 2048;

static QWEN3_EMBEDDER: OnceLock<Mutex<Qwen3VLEmbedding>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingConfig {
    pub models_dir: PathBuf,
    pub max_text_tokens: usize,
    pub download: ModelDownloadConfig,
}

impl EmbeddingConfig {
    pub fn new(models_dir: impl Into<PathBuf>) -> Self {
        Self {
            models_dir: models_dir.into(),
            max_text_tokens: DEFAULT_MAX_TEXT_TOKENS,
            download: ModelDownloadConfig::default(),
        }
    }

    pub fn with_download_config(mut self, download: ModelDownloadConfig) -> Self {
        self.download = download;
        self
    }
}

pub fn crate_ready() -> bool {
    true
}

pub fn init(models_dir: &Path) -> anyhow::Result<()> {
    init_with_config(EmbeddingConfig::new(models_dir))
}

pub fn init_with_config(config: EmbeddingConfig) -> anyhow::Result<()> {
    configure_download_env(&config.models_dir, &config.download)?;
    std::fs::create_dir_all(&config.models_dir)?;
    QWEN3_EMBEDDER
        .set(Mutex::new(build_qwen3_embedder(&config)?))
        .map_err(|_| anyhow::anyhow!("embedder already initialized"))?;
    Ok(())
}

pub fn configure_cache_env(models_dir: &Path) -> anyhow::Result<()> {
    configure_download_env(models_dir, &ModelDownloadConfig::default())
}

pub fn configure_download_env(
    models_dir: &Path,
    download: &ModelDownloadConfig,
) -> anyhow::Result<()> {
    downloads::apply_download_env(models_dir, download)
}

pub fn cache_size_bytes(models_dir: &Path) -> u64 {
    downloads::qwen_cache_size_bytes(models_dir)
}

/// True once the Qwen3-VL embedding model has been loaded into memory.
pub fn is_ready() -> bool {
    QWEN3_EMBEDDER.get().is_some()
}

pub fn embed_texts(texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
    let model = QWEN3_EMBEDDER
        .get()
        .ok_or_else(|| anyhow::anyhow!("embedder not initialized"))?
        .lock()
        .map_err(|_| anyhow::anyhow!("embedder lock poisoned"))?;
    require_storage_dimensions(model.embed_texts(texts)?)
}

pub fn embed_images<P: AsRef<Path>>(paths: &[P]) -> anyhow::Result<Vec<Vec<f32>>> {
    let model = QWEN3_EMBEDDER
        .get()
        .ok_or_else(|| anyhow::anyhow!("embedder not initialized"))?
        .lock()
        .map_err(|_| anyhow::anyhow!("embedder lock poisoned"))?;
    require_storage_dimensions(model.embed_images(paths)?)
}

fn build_qwen3_embedder(config: &EmbeddingConfig) -> anyhow::Result<Qwen3VLEmbedding> {
    anyhow::ensure!(
        legacy_cpu_embedding_allowed(),
        "Rust fastembed Qwen3-VL CPU embedding is disabled; use the MLX embedding sidecar"
    );
    downloads::ensure_qwen3_vl_prefetched(&config.models_dir, &config.download)?;
    Ok(Qwen3VLEmbedding::from_hf(
        QWEN3_VL_REPO_ID,
        &Device::Cpu,
        DType::F32,
        config.max_text_tokens,
    )?)
}

fn legacy_cpu_embedding_allowed() -> bool {
    std::env::var("CERUL_ALLOW_LEGACY_CPU_EMBEDDING").is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "on" | "yes"
        )
    })
}

fn require_storage_dimensions(vectors: Vec<Vec<f32>>) -> anyhow::Result<Vec<Vec<f32>>> {
    vectors
        .into_iter()
        .map(|vector| {
            anyhow::ensure!(
                vector.len() == VECTOR_DIMENSIONS,
                "embedding returned {} dimensions, active profile expects {VECTOR_DIMENSIONS}",
                vector.len()
            );
            Ok(vector)
        })
        .collect()
}

pub fn cosine_similarity(left: &[f32], right: &[f32]) -> anyhow::Result<f32> {
    anyhow::ensure!(
        left.len() == right.len(),
        "vector length mismatch: {} != {}",
        left.len(),
        right.len()
    );

    let mut dot = 0.0f32;
    let mut left_norm = 0.0f32;
    let mut right_norm = 0.0f32;

    for (left, right) in left.iter().zip(right) {
        dot += left * right;
        left_norm += left * left;
        right_norm += right * right;
    }

    anyhow::ensure!(left_norm > 0.0, "left vector has zero norm");
    anyhow::ensure!(right_norm > 0.0, "right vector has zero norm");

    Ok(dot / (left_norm.sqrt() * right_norm.sqrt()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    #[test]
    fn embed_texts_requires_initialization() {
        let error = embed_texts(&["hello"]).unwrap_err().to_string();

        assert!(error.contains("embedder not initialized"));
    }

    #[test]
    fn cosine_similarity_scores_identical_vectors_highest() {
        let same = cosine_similarity(&[1.0, 0.0, 0.0], &[1.0, 0.0, 0.0]).unwrap();
        let different = cosine_similarity(&[1.0, 0.0, 0.0], &[0.0, 1.0, 0.0]).unwrap();

        assert!(same > different);
    }

    #[test]
    fn init_rejects_legacy_cpu_embedding_by_default() {
        std::env::remove_var("CERUL_ALLOW_LEGACY_CPU_EMBEDDING");
        let temp = tempfile::tempdir().unwrap();

        let error = init(temp.path()).unwrap_err().to_string();

        assert!(error.contains("CPU embedding is disabled"));
    }

    #[test]
    #[ignore = "downloads Qwen3-VL-Embedding-2B from Hugging Face"]
    fn qwen3_smoke() {
        std::env::set_var("CERUL_ALLOW_LEGACY_CPU_EMBEDDING", "1");
        let temp = tempfile::tempdir().unwrap();
        let red = temp.path().join("red.png");
        let green = temp.path().join("green.png");
        let blue = temp.path().join("blue.png");
        write_color_image(&red, [255, 0, 0]);
        write_color_image(&green, [0, 255, 0]);
        write_color_image(&blue, [0, 0, 255]);

        init(temp.path()).unwrap();
        let texts = embed_texts(&["a red square", "a green square", "a blue square"]).unwrap();
        let images = embed_images(&[red, green, blue]).unwrap();

        assert_eq!(texts.len(), 3);
        assert_eq!(images.len(), 3);
        assert_eq!(texts[0].len(), VECTOR_DIMENSIONS);
        assert_eq!(images[0].len(), VECTOR_DIMENSIONS);

        let matching = cosine_similarity(&texts[0], &images[0]).unwrap();
        let non_matching = cosine_similarity(&texts[0], &images[1]).unwrap();
        assert!(matching > non_matching);
    }

    fn write_color_image(path: &Path, rgb: [u8; 3]) {
        let image = RgbImage::from_pixel(64, 64, Rgb(rgb));
        image.save(path).unwrap();
    }
}
