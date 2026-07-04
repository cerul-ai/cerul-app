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

#[derive(Debug, Serialize)]
pub(crate) struct V1AgentToolsResponse {
    pub(crate) request_id: String,
    pub(crate) execution: V1Execution,
    pub(crate) runtime: V1AgentRuntime,
    pub(crate) tools: Vec<V1AgentToolContract>,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1AgentRuntime {
    pub(crate) tool_host: &'static str,
    pub(crate) renderer_access: &'static str,
    pub(crate) arbitrary_shell: bool,
    pub(crate) arbitrary_file_write: bool,
    pub(crate) write_actions_require_confirmation: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1AgentToolContract {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) method: &'static str,
    pub(crate) path: &'static str,
    pub(crate) stage: &'static str,
    pub(crate) input_schema: Value,
    pub(crate) output_contract: &'static str,
    pub(crate) safety: V1AgentToolSafety,
    pub(crate) evidence: V1AgentToolEvidence,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1AgentToolSafety {
    pub(crate) read_only: bool,
    pub(crate) billable: bool,
    pub(crate) requires_confirmation: bool,
    pub(crate) arbitrary_shell: bool,
    pub(crate) arbitrary_file_write: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1AgentToolEvidence {
    pub(crate) returns_evidence_locators: bool,
    pub(crate) opens_in_cerul: bool,
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
    #[serde(default, alias = "rankingPreference")]
    pub(crate) ranking_preference: Option<cerul_search::SearchRankingPreference>,
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

#[derive(Debug, Deserialize)]
pub(crate) struct V1MaterialInsightRequest {
    pub(crate) query: Option<String>,
    pub(crate) q: Option<String>,
    pub(crate) max_results: Option<usize>,
    pub(crate) limit: Option<usize>,
    pub(crate) target: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct V1PreEditRequest {
    pub(crate) query: Option<String>,
    pub(crate) q: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) format: Option<String>,
    pub(crate) max_results: Option<usize>,
    pub(crate) limit: Option<usize>,
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
pub(crate) struct V1MaterialInsightResponse {
    pub(crate) request_id: String,
    pub(crate) execution: V1Execution,
    pub(crate) summary: V1MaterialInsightSummary,
    pub(crate) topics: Vec<V1MaterialInsightTopic>,
    pub(crate) usable_shots: Vec<V1MaterialUsableShot>,
    pub(crate) evidence: Vec<V1SearchResult>,
    pub(crate) usage: V1Usage,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1MaterialInsightSummary {
    pub(crate) query: String,
    pub(crate) result_count: usize,
    pub(crate) item_count: usize,
    pub(crate) modalities: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1MaterialInsightTopic {
    pub(crate) title: String,
    pub(crate) modality: String,
    pub(crate) item_count: usize,
    pub(crate) evidence_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1MaterialUsableShot {
    pub(crate) evidence_id: String,
    pub(crate) item_id: String,
    pub(crate) item_title: String,
    pub(crate) modality: String,
    pub(crate) start_sec: Option<f64>,
    pub(crate) end_sec: Option<f64>,
    pub(crate) reason: String,
    pub(crate) open_in_cerul: String,
    pub(crate) clip_url: Option<String>,
    pub(crate) preview_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1StoryboardResponse {
    pub(crate) request_id: String,
    pub(crate) execution: V1Execution,
    pub(crate) storyboard: V1Storyboard,
    pub(crate) shot_list: Vec<V1ShotListEntry>,
    pub(crate) broll_gaps: Vec<V1BrollGap>,
    pub(crate) evidence: Vec<V1SearchResult>,
    pub(crate) usage: V1Usage,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1BrollSearchResponse {
    pub(crate) request_id: String,
    pub(crate) execution: V1Execution,
    pub(crate) query: String,
    pub(crate) candidates: Vec<V1BrollCandidate>,
    pub(crate) evidence: Vec<V1SearchResult>,
    pub(crate) usage: V1Usage,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1TimelineExportResponse {
    pub(crate) request_id: String,
    pub(crate) execution: V1Execution,
    pub(crate) timeline_export: V1TimelineExport,
    pub(crate) storyboard: V1Storyboard,
    pub(crate) shot_list: Vec<V1ShotListEntry>,
    pub(crate) evidence: Vec<V1SearchResult>,
    pub(crate) usage: V1Usage,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1Storyboard {
    pub(crate) title: String,
    pub(crate) intent: String,
    pub(crate) boundary: &'static str,
    pub(crate) beats: Vec<V1StoryboardBeat>,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1StoryboardBeat {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) summary: String,
    pub(crate) evidence_ids: Vec<String>,
    pub(crate) open_in_cerul: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1ShotListEntry {
    pub(crate) id: String,
    pub(crate) beat_id: String,
    pub(crate) evidence_id: String,
    pub(crate) item_id: String,
    pub(crate) item_title: String,
    pub(crate) modality: String,
    pub(crate) role: &'static str,
    pub(crate) start_sec: Option<f64>,
    pub(crate) end_sec: Option<f64>,
    pub(crate) note: String,
    pub(crate) open_in_cerul: String,
    pub(crate) clip_url: Option<String>,
    pub(crate) preview_url: Option<String>,
    #[serde(skip)]
    pub(crate) media_target_url: Option<String>,
    #[serde(skip)]
    pub(crate) item_duration_sec: Option<f64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1BrollGap {
    pub(crate) id: String,
    pub(crate) beat_id: String,
    pub(crate) reason: String,
    pub(crate) search_query: String,
    pub(crate) candidate_evidence_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1BrollCandidate {
    pub(crate) evidence_id: String,
    pub(crate) item_id: String,
    pub(crate) item_title: String,
    pub(crate) modality: String,
    pub(crate) start_sec: Option<f64>,
    pub(crate) end_sec: Option<f64>,
    pub(crate) reason: String,
    pub(crate) open_in_cerul: String,
    pub(crate) clip_url: Option<String>,
    pub(crate) preview_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct V1TimelineExport {
    pub(crate) format: &'static str,
    pub(crate) filename: String,
    pub(crate) mime_type: &'static str,
    pub(crate) content: String,
    pub(crate) compatibility_note: &'static str,
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
    #[serde(skip)]
    pub(crate) source_file_url: Option<String>,
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
    pub(crate) source_file_url: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) page: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) section: Option<String>,
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

    pub(crate) fn material_insight_events(self) -> Vec<V1MeteredEvent> {
        let mut events = vec![V1MeteredEvent {
            capability: "local_material_insight",
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

    pub(crate) fn storyboard_events(self) -> Vec<V1MeteredEvent> {
        let mut events = vec![V1MeteredEvent {
            capability: "local_storyboard",
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

    pub(crate) fn broll_search_events(self) -> Vec<V1MeteredEvent> {
        let mut events = vec![V1MeteredEvent {
            capability: "local_broll_search",
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

    pub(crate) fn timeline_export_events(self) -> Vec<V1MeteredEvent> {
        let mut events = vec![
            V1MeteredEvent {
                capability: "local_storyboard",
                quantity: 1,
                credits: 0,
            },
            V1MeteredEvent {
                capability: "local_timeline_export",
                quantity: 1,
                credits: 0,
            },
        ];
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
