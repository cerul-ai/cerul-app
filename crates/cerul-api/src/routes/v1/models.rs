use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize)]
pub(crate) struct V1Execution {
    pub(crate) target: &'static str,
    pub(crate) account_id: Option<String>,
    pub(crate) privacy: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1StatusResponse {
    pub(crate) request_id: String,
    pub(crate) status: &'static str,
    pub(crate) version: &'static str,
    pub(crate) execution: V1Execution,
    pub(crate) library: V1StatusLibrary,
    pub(crate) search: V1StatusSearch,
    pub(crate) indexing: V1StatusIndexing,
    pub(crate) account: V1StatusAccount,
    pub(crate) capabilities: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1StatusLibrary {
    pub(crate) total_items: u64,
    pub(crate) indexed_items: u64,
    pub(crate) processing_items: u64,
    pub(crate) failed_items: u64,
    pub(crate) chunk_count: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1StatusSearch {
    pub(crate) ready: bool,
    pub(crate) retrieval_mode: &'static str,
    pub(crate) text_ready: bool,
    pub(crate) vector_ready: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1StatusIndexing {
    pub(crate) paused: bool,
    pub(crate) active_jobs: u64,
    pub(crate) queued_jobs: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1StatusAccount {
    pub(crate) signed_in: bool,
    pub(crate) plan: Option<String>,
    pub(crate) credits_remaining: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct V1ListItemsQuery {
    pub(crate) limit: Option<usize>,
    pub(crate) cursor: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) source_id: Option<String>,
    pub(crate) source_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct V1ItemChunksQuery {
    pub(crate) limit: Option<usize>,
    pub(crate) cursor: Option<String>,
    pub(crate) from_sec: Option<f64>,
    pub(crate) to_sec: Option<f64>,
    #[serde(rename = "type")]
    pub(crate) chunk_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct V1SearchRequest {
    pub(crate) query: Option<String>,
    pub(crate) q: Option<String>,
    pub(crate) max_results: Option<usize>,
    pub(crate) limit: Option<usize>,
    pub(crate) target: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct V1AskRequest {
    pub(crate) question: Option<String>,
    pub(crate) query: Option<String>,
    pub(crate) q: Option<String>,
    pub(crate) max_results: Option<usize>,
    pub(crate) limit: Option<usize>,
    pub(crate) locale: Option<String>,
    pub(crate) mode: Option<String>,
    pub(crate) target: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1SearchResponse {
    pub(crate) request_id: String,
    pub(crate) execution: V1Execution,
    pub(crate) results: Vec<V1SearchResult>,
    pub(crate) diagnostics: V1SearchDiagnostics,
    pub(crate) usage: V1Usage,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1AskResponse {
    pub(crate) request_id: String,
    pub(crate) execution: V1Execution,
    pub(crate) mode: &'static str,
    pub(crate) answer: String,
    pub(crate) citations: Vec<V1SearchResult>,
    pub(crate) warnings: Vec<String>,
    pub(crate) usage: V1Usage,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1ItemsResponse {
    pub(crate) request_id: String,
    pub(crate) execution: V1Execution,
    pub(crate) items: Vec<V1Item>,
    pub(crate) page: V1Page,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1ItemResponse {
    pub(crate) request_id: String,
    pub(crate) execution: V1Execution,
    pub(crate) item: V1Item,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1ItemChunksResponse {
    pub(crate) request_id: String,
    pub(crate) execution: V1Execution,
    pub(crate) item: V1Item,
    pub(crate) chunks: Vec<V1ItemChunk>,
    pub(crate) page: V1Page,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1Page {
    pub(crate) limit: usize,
    pub(crate) next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1Item {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) content_type: String,
    pub(crate) source_type: String,
    pub(crate) source_url: Option<String>,
    pub(crate) status: String,
    pub(crate) duration_sec: Option<f64>,
    pub(crate) indexed_at: Option<i64>,
    pub(crate) chunk_count: usize,
    pub(crate) thumbnail: Option<V1Locator>,
    pub(crate) open_in_cerul: String,
}

#[derive(Debug)]
pub(crate) struct V1ItemRow {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) content_type: String,
    pub(crate) external_id: Option<String>,
    pub(crate) duration_sec: Option<f64>,
    pub(crate) indexed_at: Option<i64>,
    pub(crate) status: String,
    pub(crate) metadata: Value,
    pub(crate) source_type: String,
    pub(crate) source_config: Value,
    pub(crate) thumbnail_chunk_id: Option<String>,
    pub(crate) thumbnail_frame_path: Option<String>,
    pub(crate) chunk_count: usize,
    pub(crate) source_file_exists: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1ItemChunk {
    pub(crate) id: String,
    #[serde(rename = "type")]
    pub(crate) chunk_type: String,
    pub(crate) source: &'static str,
    pub(crate) time: V1SearchTime,
    pub(crate) text: V1ChunkText,
    pub(crate) evidence: V1Evidence,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1ChunkText {
    pub(crate) content: Option<String>,
    pub(crate) snippet: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1SearchResult {
    pub(crate) id: String,
    #[serde(rename = "type")]
    pub(crate) result_type: &'static str,
    pub(crate) source: &'static str,
    pub(crate) item: V1SearchItem,
    pub(crate) time: V1SearchTime,
    pub(crate) text: V1SearchText,
    pub(crate) evidence: V1Evidence,
    pub(crate) score: V1Score,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct V1SearchItem {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) content_type: String,
    pub(crate) source_type: String,
    pub(crate) duration_sec: Option<f64>,
}

#[derive(Debug, Clone)]
pub(crate) struct V1SearchItemMetadata {
    pub(crate) item: V1SearchItem,
    pub(crate) source_file_exists: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1SearchTime {
    pub(crate) start_sec: Option<f64>,
    pub(crate) end_sec: Option<f64>,
    pub(crate) timestamp: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1SearchText {
    pub(crate) snippet: String,
    pub(crate) quote: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1Evidence {
    pub(crate) id: String,
    pub(crate) kind: &'static str,
    pub(crate) clip: Option<V1Locator>,
    pub(crate) preview: Option<V1Locator>,
    pub(crate) open_in_cerul: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1Locator {
    #[serde(rename = "type")]
    pub(crate) locator_type: &'static str,
    pub(crate) url: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1Score {
    #[serde(rename = "match")]
    pub(crate) match_score: f32,
    pub(crate) exact_match: bool,
    pub(crate) similarity: Option<f32>,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1SearchDiagnostics {
    pub(crate) retrieval_mode: String,
    pub(crate) fallback_reason: Option<String>,
    pub(crate) vector_hits: usize,
    pub(crate) text_hits: usize,
    pub(crate) result_count: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1Usage {
    pub(crate) billable: bool,
    pub(crate) metered_events: Vec<V1MeteredEvent>,
    pub(crate) credits_used: i64,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1MeteredEvent {
    pub(crate) capability: &'static str,
    pub(crate) quantity: u64,
    pub(crate) credits: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum V1QueryExecution {
    LocalOnly,
    RemoteEmbedding,
}

impl V1QueryExecution {
    pub(crate) fn execution(self) -> V1Execution {
        V1Execution {
            target: "local",
            account_id: None,
            privacy: match self {
                Self::LocalOnly => "local_only",
                Self::RemoteEmbedding => "local_library_remote_query",
            },
        }
    }

    pub(crate) fn search_events(self) -> Vec<V1MeteredEvent> {
        let mut events = vec![V1MeteredEvent {
            capability: "local_search",
            quantity: 1,
            credits: 0,
        }];
        if self == Self::RemoteEmbedding {
            events.push(V1MeteredEvent {
                capability: "remote_embedding_query",
                quantity: 1,
                credits: 0,
            });
        }
        events
    }

    pub(crate) fn ask_events(self) -> Vec<V1MeteredEvent> {
        let mut events = vec![V1MeteredEvent {
            capability: "local_ask_extractive",
            quantity: 1,
            credits: 0,
        }];
        if self == Self::RemoteEmbedding {
            events.push(V1MeteredEvent {
                capability: "remote_embedding_query",
                quantity: 1,
                credits: 0,
            });
        }
        events
    }
}
