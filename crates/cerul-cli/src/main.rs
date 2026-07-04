use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use cerul_pipeline::{
    run::{Embedder, Transcriber, VideoPipeline},
    whisper::Segment,
};
use clap::{Parser, Subcommand};
use serde_json::{json, Map, Value};

#[derive(Debug, Parser)]
#[command(name = "cerul-cli")]
#[command(about = "Cerul source discovery and indexing utilities")]
struct Cli {
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Discover items from a source plugin without writing to the index.
    ListSource {
        plugin_type: String,
        #[command(flatten)]
        source: SourceOptions,
    },
    /// Add a source to the local index and enqueue discovered items.
    AddSource {
        plugin_type: String,
        #[command(flatten)]
        source: SourceOptions,
    },
    /// Discover a source and fetch the first item without writing index rows.
    FetchFirst {
        plugin_type: String,
        #[command(flatten)]
        source: SourceOptions,
    },
    /// Add a source, index its first video with smoke model adapters, and search it.
    IndexFirst {
        plugin_type: String,
        #[command(flatten)]
        source: SourceOptions,
        #[arg(long, default_value = "cerul youtube indexing smoke phrase")]
        query: String,
        #[arg(long, default_value_t = 1)]
        count: usize,
    },
}

#[derive(Debug, Clone, clap::Args)]
struct SourceOptions {
    #[arg(long)]
    path: Option<PathBuf>,

    #[arg(long)]
    url: Option<String>,

    #[arg(long)]
    max: Option<u64>,

    #[arg(long)]
    cache_dir: Option<PathBuf>,

    #[arg(long)]
    ytdlp_path: Option<PathBuf>,

    #[arg(long)]
    timeout: Option<u64>,

    #[arg(long)]
    clip_duration_sec: Option<u64>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::ListSource {
            plugin_type,
            source,
        } => {
            let config = source_config(&plugin_type, source);
            let plugin = cerul_sources::build(&plugin_type, config)?;
            let items = plugin.discover().await?;
            for item in items {
                println!(
                    "{}\t{}",
                    item.external_id,
                    item.title.as_deref().unwrap_or("")
                );
            }
        }
        Command::AddSource {
            plugin_type,
            source,
        } => {
            let config = source_config(&plugin_type, source);
            let paths = match cli.data_dir {
                Some(data_dir) => cerul_storage::AppPaths::from_data_dir(data_dir)?,
                None => cerul_storage::AppPaths::resolve()?,
            };
            let summary = cerul_api::add_source_to_paths(
                &paths,
                cerul_api::AddSourceRequest {
                    source_type: plugin_type,
                    config,
                },
            )
            .await?;

            println!(
                "source\t{}\t{}\t{}",
                summary.source.id, summary.source.source_type, summary.source.status
            );
            for item in summary.items {
                println!(
                    "item\t{}\t{}\t{}",
                    item.id,
                    item.external_id.as_deref().unwrap_or(""),
                    item.title.as_deref().unwrap_or("")
                );
            }
            println!("jobs\t{}", summary.queued_jobs);
        }
        Command::FetchFirst {
            plugin_type,
            source,
        } => {
            let config = source_config(&plugin_type, source);
            let plugin = cerul_sources::build(&plugin_type, config)?;
            let items = plugin.discover().await?;
            let item = items
                .first()
                .ok_or_else(|| anyhow::anyhow!("source discovered no items"))?;
            let fetched = plugin.fetch(item).await?;
            let bytes = std::fs::metadata(&fetched)
                .map(|metadata| metadata.len())
                .unwrap_or(0);

            println!(
                "fetched\t{}\t{}\t{}",
                item.external_id,
                fetched.display(),
                bytes
            );
        }
        Command::IndexFirst {
            plugin_type,
            source,
            query,
            count,
        } => {
            anyhow::ensure!(count > 0, "--count must be greater than zero");
            let config = source_config(&plugin_type, source);
            let paths = match cli.data_dir {
                Some(data_dir) => cerul_storage::AppPaths::from_data_dir(data_dir)?,
                None => cerul_storage::AppPaths::resolve()?,
            };
            let summary = cerul_api::add_source_to_paths(
                &paths,
                cerul_api::AddSourceRequest {
                    source_type: plugin_type,
                    config,
                },
            )
            .await?;
            let items = summary.items.into_iter().take(count).collect::<Vec<_>>();
            anyhow::ensure!(
                items.len() == count,
                "source discovered {} items, cannot index requested count {}",
                items.len(),
                count
            );

            let mut indexed_items = 0usize;
            let mut transcript_chunks = 0usize;
            let mut image_vectors = 0usize;
            for (index, item) in items.into_iter().enumerate() {
                let item_query = format!("{} video {}", query, index + 1);
                let pipeline = VideoPipeline::new(
                    paths.clone(),
                    Arc::new(SmokeTranscriber {
                        phrase: item_query.clone(),
                    }),
                    Arc::new(SmokeEmbedder),
                )
                .with_chunking(4.0, 0.0)
                .with_frame_interval_sec(2);
                let process_summary = pipeline.process_video_item(&item.id).await?;
                let results = cerul_search::search_with_vector(
                    &paths,
                    cerul_search::SearchRequest {
                        q: item_query.clone(),
                        limit: count.max(3),
                        ranking_preference: cerul_search::SearchRankingPreference::Smart,
                    },
                    smoke_vector(0),
                )
                .await?;
                let top = results.first().ok_or_else(|| {
                    anyhow::anyhow!("indexed video {} did not return search results", item.id)
                })?;

                anyhow::ensure!(
                    top.item_id == item.id,
                    "top search result item mismatch for {}: expected {}, got {}",
                    item_query,
                    item.id,
                    top.item_id
                );
                anyhow::ensure!(
                    top.snippet.contains(&item_query),
                    "top search result did not contain query phrase: {}",
                    top.snippet
                );

                indexed_items += 1;
                transcript_chunks += process_summary.transcript_chunks;
                image_vectors += process_summary.image_vectors;
                println!(
                    "indexed\t{}\t{}\t{}\t{}\t{}",
                    item.id,
                    item.external_id.as_deref().unwrap_or(""),
                    process_summary.transcript_chunks,
                    process_summary.image_vectors,
                    top.start_sec.unwrap_or_default()
                );
            }
            println!(
                "indexed_summary\t{}\t{}\t{}",
                indexed_items, transcript_chunks, image_vectors
            );
        }
    }

    Ok(())
}

fn source_config(plugin_type: &str, options: SourceOptions) -> Value {
    let mut config = Map::new();

    if let Some(path) = options.path {
        config.insert("path".to_string(), json!(path));
    }
    if let Some(url) = options.url {
        config.insert("url".to_string(), Value::String(url));
    }
    if let Some(max) = options.max {
        let key = match plugin_type {
            "youtube" => "max_videos",
            "rss_podcast" => "max_episodes",
            _ => "max",
        };
        config.insert(key.to_string(), Value::Number(max.into()));
    }
    if let Some(cache_dir) = options.cache_dir {
        config.insert("cache_dir".to_string(), json!(cache_dir));
    }
    if let Some(ytdlp_path) = options.ytdlp_path {
        config.insert("ytdlp_path".to_string(), json!(ytdlp_path));
    }
    if let Some(timeout) = options.timeout {
        config.insert("timeout_sec".to_string(), Value::Number(timeout.into()));
    }
    if let Some(clip_duration_sec) = options.clip_duration_sec {
        config.insert(
            "clip_duration_sec".to_string(),
            Value::Number(clip_duration_sec.into()),
        );
    }

    Value::Object(config)
}

struct SmokeTranscriber {
    phrase: String,
}

impl Transcriber for SmokeTranscriber {
    fn transcribe(
        &self,
        _audio_path: &Path,
        progress: Option<cerul_pipeline::whisper::TranscriptionProgress>,
    ) -> anyhow::Result<Vec<Segment>> {
        if let Some(progress) = progress {
            progress(100);
        }
        Ok(vec![Segment {
            start: 0.0,
            end: 4.0,
            text: self.phrase.clone(),
        }])
    }
}

struct SmokeEmbedder;

impl Embedder for SmokeEmbedder {
    fn embed_texts(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .enumerate()
            .map(|(index, _)| smoke_vector(index))
            .collect())
    }

    fn embed_images(&self, paths: &[PathBuf]) -> anyhow::Result<Vec<Vec<f32>>> {
        Ok(paths
            .iter()
            .enumerate()
            .map(|(index, _)| smoke_vector(index + 100))
            .collect())
    }
}

fn smoke_vector(seed: usize) -> Vec<f32> {
    let mut vector = vec![0.0; cerul_storage::vectors::VECTOR_DIMENSIONS as usize];
    let index = seed % vector.len();
    vector[index] = 1.0;
    vector
}
