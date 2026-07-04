use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::Path as FsPath,
};

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap},
    routing::{get, post},
    Json, Router,
};
use cerul_storage::AppPaths;
use rusqlite::{types::Value as SqlValue, OptionalExtension};
use serde_json::{json, Value};

use crate::{
    api_models, chunk_from_row, configured_addr, encode_path_segment, format_playback_timestamp,
    format_seconds_param, has_timed_video_clip_start, jobs, new_id, openapi_document, parse_json,
    search_health_diagnostics, search_records, split_filter_values, trim_for_answer, ApiError,
    ApiResult, ApiState, ChunkRecord, DEFAULT_API_PORT,
};

mod models;

#[cfg(test)]
mod tests;

pub(crate) use models::*;

pub(crate) const API_PATHS: &[(&str, &[&str])] = &[
    ("/v1/status", &["get"]),
    ("/v1/openapi.json", &["get"]),
    ("/v1/agent/tools", &["get"]),
    ("/v1/agent/material-insight", &["post"]),
    ("/v1/agent/storyboard", &["post"]),
    ("/v1/agent/broll-search", &["post"]),
    ("/v1/agent/timeline-export", &["post"]),
    ("/v1/search", &["post"]),
    ("/v1/ask", &["post"]),
    ("/v1/items", &["get"]),
    ("/v1/items/{id}", &["get"]),
    ("/v1/items/{id}/chunks", &["get"]),
    ("/v1/chunks/{id}/frame", &["get"]),
    ("/v1/chunks/{id}/video-segment", &["get"]),
    ("/v1/chunks/{id}/video-clip", &["get"]),
];

pub(crate) fn router() -> Router<ApiState> {
    Router::new()
        .route("/status", get(v1_status))
        .route("/openapi.json", get(v1_openapi_json))
        .route("/agent/tools", get(v1_agent_tools))
        .route("/agent/material-insight", post(v1_material_insight))
        .route("/agent/storyboard", post(v1_storyboard))
        .route("/agent/broll-search", post(v1_broll_search))
        .route("/agent/timeline-export", post(v1_timeline_export))
        .route("/search", post(v1_search))
        .route("/ask", post(v1_ask))
        .route("/items", get(v1_list_items))
        .route("/items/:id", get(v1_get_item))
        .route("/items/:id/chunks", get(v1_list_item_chunks))
        .route(
            "/chunks/:id/frame",
            get(crate::routes::library::get_chunk_frame),
        )
        .route(
            "/chunks/:id/video-segment",
            get(crate::routes::library::get_chunk_video_segment),
        )
        .route(
            "/chunks/:id/video-clip",
            get(crate::routes::library::get_chunk_video_clip),
        )
}

async fn v1_openapi_json() -> Json<Value> {
    Json(openapi_document("Cerul Agent API", API_PATHS))
}

async fn v1_status(State(state): State<ApiState>) -> ApiResult<Json<V1StatusResponse>> {
    let indexing = jobs::indexing_diagnostics(&state.paths)?;
    let search = search_health_diagnostics(&state.paths).await?;
    let text_ready = search.retrieval_unit_fts_row_count > 0;
    let vector_ready =
        search.vector_index_error.is_none() && search.vector_index_point_count.unwrap_or(0) > 0;
    let retrieval_mode = match (text_ready, vector_ready) {
        (true, true) => "hybrid",
        (true, false) => "text",
        (false, true) => "vector",
        (false, false) => "empty",
    };
    let counts = &indexing.counts;

    Ok(Json(V1StatusResponse {
        request_id: new_id("req"),
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        execution: V1Execution {
            target: "local",
            account_id: None,
            privacy: "local_only",
        },
        library: V1StatusLibrary {
            total_items: counts.total_items,
            indexed_items: counts.indexed_items,
            processing_items: counts.processing_items,
            failed_items: counts.failed_items,
            chunk_count: search.chunk_count,
        },
        search: V1StatusSearch {
            ready: text_ready || vector_ready,
            retrieval_mode,
            text_ready,
            vector_ready,
        },
        indexing: V1StatusIndexing {
            paused: indexing.paused,
            active_jobs: counts.running_jobs,
            queued_jobs: counts.queued_jobs,
        },
        account: V1StatusAccount {
            signed_in: false,
            plan: None,
            credits_remaining: None,
        },
        capabilities: vec![
            "status",
            "openapi",
            "agent_tools",
            "material_insight",
            "storyboard",
            "broll_search",
            "timeline_export",
            "search",
            "ask",
            "items",
            "chunks",
        ],
    }))
}

async fn v1_agent_tools(State(state): State<ApiState>) -> Json<V1AgentToolsResponse> {
    let query_execution = v1_read_only_query_execution(&state.paths);

    Json(V1AgentToolsResponse {
        request_id: new_id("req"),
        execution: query_execution.execution(),
        runtime: V1AgentRuntime {
            tool_host: "cerul_core_v1",
            renderer_access: "ui_only",
            arbitrary_shell: false,
            arbitrary_file_write: false,
            write_actions_require_confirmation: true,
        },
        tools: v1_agent_tool_contracts(),
    })
}

fn v1_agent_tool_contracts() -> Vec<V1AgentToolContract> {
    vec![
        V1AgentToolContract {
            name: "search_library",
            description: "Search the local Cerul library and return evidence-backed results.",
            method: "POST",
            path: "/v1/search",
            stage: "a1",
            input_schema: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["query"],
                "properties": {
                    "query": {"type": "string", "minLength": 1},
                    "max_results": {"type": "integer", "minimum": 1, "maximum": 50},
                    "target": {"type": "string", "enum": ["local"]}
                }
            }),
            output_contract: "V1SearchResponse",
            safety: v1_read_only_agent_tool_safety(),
            evidence: v1_agent_tool_evidence(true, true),
        },
        V1AgentToolContract {
            name: "get_item",
            description: "Fetch one local library item and its Cerul open locator.",
            method: "GET",
            path: "/v1/items/{id}",
            stage: "a1",
            input_schema: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["id"],
                "properties": {
                    "id": {"type": "string", "minLength": 1}
                }
            }),
            output_contract: "V1ItemResponse",
            safety: v1_read_only_agent_tool_safety(),
            evidence: v1_agent_tool_evidence(true, true),
        },
        V1AgentToolContract {
            name: "get_chunks",
            description: "List evidence chunks for one local library item.",
            method: "GET",
            path: "/v1/items/{id}/chunks",
            stage: "a1",
            input_schema: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["id"],
                "properties": {
                    "id": {"type": "string", "minLength": 1},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100},
                    "cursor": {"type": "string"},
                    "from_sec": {"type": "number", "minimum": 0},
                    "to_sec": {"type": "number", "minimum": 0},
                    "type": {"type": "string"}
                }
            }),
            output_contract: "V1ItemChunksResponse",
            safety: v1_read_only_agent_tool_safety(),
            evidence: v1_agent_tool_evidence(true, true),
        },
        V1AgentToolContract {
            name: "get_frame",
            description: "Fetch a local frame preview for a chunk evidence id.",
            method: "GET",
            path: "/v1/chunks/{id}/frame",
            stage: "a1",
            input_schema: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["id"],
                "properties": {
                    "id": {"type": "string", "minLength": 1}
                }
            }),
            output_contract: "binary image response",
            safety: v1_read_only_agent_tool_safety(),
            evidence: v1_agent_tool_evidence(false, false),
        },
        V1AgentToolContract {
            name: "get_segment",
            description: "Fetch a local bounded video clip for a chunk evidence id.",
            method: "GET",
            path: "/v1/chunks/{id}/video-clip?before_sec=3&after_sec=5",
            stage: "a1",
            input_schema: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["id"],
                "properties": {
                    "id": {"type": "string", "minLength": 1}
                }
            }),
            output_contract: "binary video response",
            safety: v1_agent_tool_safety(false),
            evidence: v1_agent_tool_evidence(false, false),
        },
        V1AgentToolContract {
            name: "ask",
            description: "Answer from local indexed evidence in extractive mode.",
            method: "POST",
            path: "/v1/ask",
            stage: "a1",
            input_schema: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["question"],
                "properties": {
                    "question": {"type": "string", "minLength": 1},
                    "max_results": {"type": "integer", "minimum": 1, "maximum": 8},
                    "locale": {"type": "string"},
                    "mode": {"type": "string", "enum": ["extractive", "auto"]},
                    "target": {"type": "string", "enum": ["local"]}
                }
            }),
            output_contract: "V1AskResponse",
            safety: v1_read_only_agent_tool_safety(),
            evidence: v1_agent_tool_evidence(true, true),
        },
        V1AgentToolContract {
            name: "material_insight",
            description: "Heuristically group local indexed material into evidence-backed topics and usable-shot candidates.",
            method: "POST",
            path: "/v1/agent/material-insight",
            stage: "a2",
            input_schema: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["query"],
                "properties": {
                    "query": {"type": "string", "minLength": 1},
                    "max_results": {"type": "integer", "minimum": 1, "maximum": 20},
                    "target": {"type": "string", "enum": ["local"]}
                }
            }),
            output_contract: "V1MaterialInsightResponse",
            safety: v1_read_only_agent_tool_safety(),
            evidence: v1_agent_tool_evidence(true, true),
        },
        V1AgentToolContract {
            name: "build_storyboard",
            description: "Build a heuristic evidence-backed pre-edit storyboard and shot list from local indexed material.",
            method: "POST",
            path: "/v1/agent/storyboard",
            stage: "c1",
            input_schema: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["query"],
                "properties": {
                    "query": {"type": "string", "minLength": 1},
                    "title": {"type": "string"},
                    "max_results": {"type": "integer", "minimum": 1, "maximum": 20},
                    "target": {"type": "string", "enum": ["local"]}
                }
            }),
            output_contract: "V1StoryboardResponse",
            safety: v1_read_only_agent_tool_safety(),
            evidence: v1_agent_tool_evidence(true, true),
        },
        V1AgentToolContract {
            name: "search_broll",
            description: "Heuristically find visual b-roll candidates from local evidence for a pre-edit plan.",
            method: "POST",
            path: "/v1/agent/broll-search",
            stage: "c1",
            input_schema: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["query"],
                "properties": {
                    "query": {"type": "string", "minLength": 1},
                    "max_results": {"type": "integer", "minimum": 1, "maximum": 20},
                    "target": {"type": "string", "enum": ["local"]}
                }
            }),
            output_contract: "V1BrollSearchResponse",
            safety: v1_read_only_agent_tool_safety(),
            evidence: v1_agent_tool_evidence(true, true),
        },
        V1AgentToolContract {
            name: "export_edl",
            description: "Export a heuristic OTIO JSON planning timeline for external editing tools; Cerul does not render or edit media.",
            method: "POST",
            path: "/v1/agent/timeline-export",
            stage: "c1",
            input_schema: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["query"],
                "properties": {
                    "query": {"type": "string", "minLength": 1},
                    "title": {"type": "string"},
                    "format": {"type": "string", "enum": ["otio_json"]},
                    "max_results": {"type": "integer", "minimum": 1, "maximum": 20},
                    "target": {"type": "string", "enum": ["local"]}
                }
            }),
            output_contract: "V1TimelineExportResponse",
            safety: v1_read_only_agent_tool_safety(),
            evidence: v1_agent_tool_evidence(true, true),
        },
    ]
}

fn v1_read_only_agent_tool_safety() -> V1AgentToolSafety {
    v1_agent_tool_safety(true)
}

fn v1_agent_tool_safety(read_only: bool) -> V1AgentToolSafety {
    V1AgentToolSafety {
        read_only,
        billable: false,
        requires_confirmation: false,
        arbitrary_shell: false,
        arbitrary_file_write: false,
    }
}

fn v1_agent_tool_evidence(
    returns_evidence_locators: bool,
    opens_in_cerul: bool,
) -> V1AgentToolEvidence {
    V1AgentToolEvidence {
        returns_evidence_locators,
        opens_in_cerul,
    }
}

async fn v1_search(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<V1SearchRequest>,
) -> ApiResult<Json<V1SearchResponse>> {
    let query = first_non_empty_text([req.query, req.q])
        .ok_or_else(|| ApiError::bad_request("query cannot be empty"))?;
    validate_v1_local_target(req.target.as_deref())?;
    let query_execution = v1_query_execution(&state.paths);
    let limit = req.max_results.or(req.limit).unwrap_or(10).clamp(1, 50);
    let response = search_records(
        &state.paths,
        cerul_search::SearchRequest { q: query, limit },
    )
    .await?;
    let item_metadata = v1_search_item_metadata(&state.paths, &response.results)?;
    let existing_preview_chunk_ids =
        v1_existing_preview_chunk_ids(&state.paths, &response.results)?;
    let evidence_metadata = v1_evidence_metadata_for_results(&state.paths, &response.results)?;
    let base_url = v1_base_url(&headers, &state.paths);
    let results = response
        .results
        .iter()
        .map(|result| {
            v1_search_result(
                result,
                &item_metadata,
                &existing_preview_chunk_ids,
                &evidence_metadata,
                &base_url,
            )
        })
        .collect::<Vec<_>>();
    let result_count = results.len();

    Ok(Json(V1SearchResponse {
        request_id: new_id("req"),
        execution: query_execution.execution(),
        results,
        diagnostics: V1SearchDiagnostics {
            retrieval_mode: response.diagnostics.retrieval_mode,
            fallback_reason: response.diagnostics.fallback_reason,
            vector_hits: response.diagnostics.vector_hits_count,
            text_hits: response.diagnostics.fts_hits_count,
            result_count,
        },
        usage: V1Usage {
            billable: false,
            metered_events: query_execution.search_events(),
            credits_used: 0,
        },
    }))
}

async fn v1_material_insight(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<V1MaterialInsightRequest>,
) -> ApiResult<Json<V1MaterialInsightResponse>> {
    let query = first_non_empty_text([req.query, req.q])
        .ok_or_else(|| ApiError::bad_request("query cannot be empty"))?;
    validate_v1_local_target(req.target.as_deref())?;
    let limit = req.max_results.or(req.limit).unwrap_or(12).clamp(1, 20);
    let (query_execution, evidence) =
        v1_collect_material_evidence(&state.paths, &headers, &query, limit).await?;

    Ok(Json(V1MaterialInsightResponse {
        request_id: new_id("req"),
        execution: query_execution.execution(),
        summary: v1_material_insight_summary(&query, &evidence),
        topics: v1_material_insight_topics(&evidence),
        usable_shots: v1_material_usable_shots(&evidence),
        evidence,
        usage: V1Usage {
            billable: false,
            metered_events: query_execution.material_insight_events(),
            credits_used: 0,
        },
    }))
}

async fn v1_storyboard(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<V1PreEditRequest>,
) -> ApiResult<Json<V1StoryboardResponse>> {
    let query = first_non_empty_text([req.query, req.q])
        .ok_or_else(|| ApiError::bad_request("query cannot be empty"))?;
    validate_v1_local_target(req.target.as_deref())?;
    let title = v1_pre_edit_title(req.title.as_deref(), &query);
    let limit = req.max_results.or(req.limit).unwrap_or(10).clamp(1, 20);
    let (query_execution, evidence) =
        v1_collect_material_evidence(&state.paths, &headers, &query, limit).await?;
    let plan = v1_pre_edit_plan(&query, &title, &evidence);

    Ok(Json(V1StoryboardResponse {
        request_id: new_id("req"),
        execution: query_execution.execution(),
        storyboard: plan.storyboard,
        shot_list: plan.shot_list,
        broll_gaps: plan.broll_gaps,
        evidence,
        usage: V1Usage {
            billable: false,
            metered_events: query_execution.storyboard_events(),
            credits_used: 0,
        },
    }))
}

async fn v1_broll_search(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<V1PreEditRequest>,
) -> ApiResult<Json<V1BrollSearchResponse>> {
    let query = first_non_empty_text([req.query, req.q])
        .ok_or_else(|| ApiError::bad_request("query cannot be empty"))?;
    validate_v1_local_target(req.target.as_deref())?;
    let limit = req.max_results.or(req.limit).unwrap_or(10).clamp(1, 20);
    let (query_execution, evidence) =
        v1_collect_material_evidence(&state.paths, &headers, &query, limit).await?;
    let candidates = v1_broll_candidates(&evidence);

    Ok(Json(V1BrollSearchResponse {
        request_id: new_id("req"),
        execution: query_execution.execution(),
        query,
        candidates,
        evidence,
        usage: V1Usage {
            billable: false,
            metered_events: query_execution.broll_search_events(),
            credits_used: 0,
        },
    }))
}

async fn v1_timeline_export(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<V1PreEditRequest>,
) -> ApiResult<Json<V1TimelineExportResponse>> {
    let query = first_non_empty_text([req.query, req.q])
        .ok_or_else(|| ApiError::bad_request("query cannot be empty"))?;
    validate_v1_local_target(req.target.as_deref())?;
    let format = req
        .format
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("otio_json")
        .to_ascii_lowercase();
    if format != "otio_json" {
        return Err(ApiError::bad_request(
            "only otio_json timeline export is currently supported",
        ));
    }
    let title = v1_pre_edit_title(req.title.as_deref(), &query);
    let limit = req.max_results.or(req.limit).unwrap_or(10).clamp(1, 20);
    let (query_execution, evidence) =
        v1_collect_material_evidence(&state.paths, &headers, &query, limit).await?;
    let plan = v1_pre_edit_plan(&query, &title, &evidence);
    let timeline_export = v1_otio_timeline_export(&plan.storyboard.title, &plan.shot_list);

    Ok(Json(V1TimelineExportResponse {
        request_id: new_id("req"),
        execution: query_execution.execution(),
        timeline_export,
        storyboard: plan.storyboard,
        shot_list: plan.shot_list,
        evidence,
        usage: V1Usage {
            billable: false,
            metered_events: query_execution.timeline_export_events(),
            credits_used: 0,
        },
    }))
}

async fn v1_collect_material_evidence(
    paths: &AppPaths,
    headers: &HeaderMap,
    query: &str,
    limit: usize,
) -> ApiResult<(V1QueryExecution, Vec<V1SearchResult>)> {
    let query_execution = v1_query_execution(paths);
    let response = search_records(
        paths,
        cerul_search::SearchRequest {
            q: query.to_string(),
            limit,
        },
    )
    .await?;
    let raw_results = response.results.into_iter().take(limit).collect::<Vec<_>>();
    let item_metadata = v1_search_item_metadata(paths, &raw_results)?;
    let existing_preview_chunk_ids = v1_existing_preview_chunk_ids(paths, &raw_results)?;
    let evidence_metadata = v1_evidence_metadata_for_results(paths, &raw_results)?;
    let base_url = v1_base_url(headers, paths);
    let evidence = raw_results
        .iter()
        .map(|result| {
            v1_search_result(
                result,
                &item_metadata,
                &existing_preview_chunk_ids,
                &evidence_metadata,
                &base_url,
            )
        })
        .collect::<Vec<_>>();
    Ok((query_execution, evidence))
}

async fn v1_ask(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<V1AskRequest>,
) -> ApiResult<Json<V1AskResponse>> {
    let question = first_non_empty_text([req.question, req.query, req.q])
        .ok_or_else(|| ApiError::bad_request("question cannot be empty"))?;
    validate_v1_local_target(req.target.as_deref())?;
    let query_execution = v1_query_execution(&state.paths);
    let requested_mode = req
        .mode
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("extractive")
        .to_ascii_lowercase();
    if !matches!(requested_mode.as_str(), "extractive" | "auto") {
        return Err(ApiError::bad_request(
            "only extractive mode is currently supported by /v1/ask",
        ));
    }
    let limit = req.max_results.or(req.limit).unwrap_or(6).clamp(1, 8);
    let response = search_records(
        &state.paths,
        cerul_search::SearchRequest {
            q: question.clone(),
            limit,
        },
    )
    .await?;
    let filtered_results = response
        .results
        .into_iter()
        .filter(|result| !result.snippet.trim().is_empty())
        .take(limit)
        .collect::<Vec<_>>();
    let item_metadata = v1_search_item_metadata(&state.paths, &filtered_results)?;
    let existing_preview_chunk_ids =
        v1_existing_preview_chunk_ids(&state.paths, &filtered_results)?;
    let evidence_metadata = v1_evidence_metadata_for_results(&state.paths, &filtered_results)?;
    let base_url = v1_base_url(&headers, &state.paths);
    let citations = filtered_results
        .iter()
        .map(|result| {
            v1_search_result(
                result,
                &item_metadata,
                &existing_preview_chunk_ids,
                &evidence_metadata,
                &base_url,
            )
        })
        .collect::<Vec<_>>();
    let answer = v1_extractive_answer(&question, &citations, req.locale.as_deref());

    Ok(Json(V1AskResponse {
        request_id: new_id("req"),
        execution: query_execution.execution(),
        mode: "extractive",
        answer,
        citations,
        warnings: Vec::new(),
        usage: V1Usage {
            billable: false,
            metered_events: query_execution.ask_events(),
            credits_used: 0,
        },
    }))
}

async fn v1_list_items(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<V1ListItemsQuery>,
) -> ApiResult<Json<V1ItemsResponse>> {
    let limit = v1_page_limit(query.limit, 50, 100);
    let offset = v1_cursor_offset(query.cursor.as_deref())?;
    let fetch_limit = limit + 1;
    let statuses = split_filter_values(query.status.as_deref());
    let base_url = v1_base_url(&headers, &state.paths);
    let conn = cerul_storage::sqlite::open(&state.paths)?;
    let mut params: Vec<SqlValue> = Vec::new();
    let mut sql = v1_item_select_sql();
    sql.push_str(" WHERE i.status != 'deleting'");

    if !statuses.is_empty() {
        sql.push_str(" AND i.status IN (");
        sql.push_str(
            &std::iter::repeat_n("?", statuses.len())
                .collect::<Vec<_>>()
                .join(", "),
        );
        sql.push(')');
        params.extend(statuses.into_iter().map(SqlValue::from));
    }
    if let Some(source_id) = query.source_id.filter(|value| !value.trim().is_empty()) {
        sql.push_str(" AND i.source_id = ?");
        params.push(SqlValue::from(source_id));
    }
    if let Some(source_type) = query.source_type.filter(|value| !value.trim().is_empty()) {
        sql.push_str(" AND s.type = ?");
        params.push(SqlValue::from(source_type));
    }
    sql.push_str(
        r#"
        ORDER BY COALESCE(i.indexed_at, 0) DESC, i.id ASC
        LIMIT ? OFFSET ?
        "#,
    );
    params.push(SqlValue::from(fetch_limit as i64));
    params.push(SqlValue::from(offset as i64));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        rusqlite::params_from_iter(params.iter()),
        v1_item_row_from_row,
    )?;
    let mut rows = rows.collect::<Result<Vec<_>, _>>()?;
    let next_cursor = if rows.len() > limit {
        rows.truncate(limit);
        Some((offset + limit).to_string())
    } else {
        None
    };
    let items = rows
        .iter()
        .map(|item| v1_item_from_row(item, &base_url))
        .collect::<Vec<_>>();

    Ok(Json(V1ItemsResponse {
        request_id: new_id("req"),
        execution: V1Execution {
            target: "local",
            account_id: None,
            privacy: "local_only",
        },
        items,
        page: V1Page { limit, next_cursor },
    }))
}

async fn v1_get_item(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<V1ItemResponse>> {
    let base_url = v1_base_url(&headers, &state.paths);
    let item = v1_load_item(&state.paths, &id)?
        .ok_or_else(|| ApiError::not_found(format!("item not found: {id}")))?;

    Ok(Json(V1ItemResponse {
        request_id: new_id("req"),
        execution: V1Execution {
            target: "local",
            account_id: None,
            privacy: "local_only",
        },
        item: v1_item_from_row(&item, &base_url),
    }))
}

async fn v1_list_item_chunks(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<V1ItemChunksQuery>,
) -> ApiResult<Json<V1ItemChunksResponse>> {
    let limit = v1_page_limit(query.limit, 100, 250);
    let offset = v1_cursor_offset(query.cursor.as_deref())?;
    let fetch_limit = limit + 1;
    let base_url = v1_base_url(&headers, &state.paths);
    let item = v1_load_item(&state.paths, &id)?
        .ok_or_else(|| ApiError::not_found(format!("item not found: {id}")))?;

    if let Some(from_sec) = query.from_sec {
        if !from_sec.is_finite() || from_sec < 0.0 {
            return Err(ApiError::bad_request(
                "from_sec must be a finite non-negative number",
            ));
        }
    }
    if let Some(to_sec) = query.to_sec {
        if !to_sec.is_finite() || to_sec < 0.0 {
            return Err(ApiError::bad_request(
                "to_sec must be a finite non-negative number",
            ));
        }
    }
    if let (Some(from_sec), Some(to_sec)) = (query.from_sec, query.to_sec) {
        if to_sec < from_sec {
            return Err(ApiError::bad_request(
                "to_sec must be greater than or equal to from_sec",
            ));
        }
    }

    let mut params: Vec<SqlValue> = vec![SqlValue::from(id.clone())];
    let mut sql = String::from(
        r#"
        SELECT id, item_id, chunk_type, start_sec, end_sec, text, frame_path, metadata
        FROM chunks
        WHERE item_id = ?
        "#,
    );
    if let Some(chunk_type) = query.chunk_type.filter(|value| !value.trim().is_empty()) {
        let chunk_types = v1_chunk_type_filter_values(&chunk_type);
        if chunk_types.len() == 1 {
            sql.push_str(" AND chunk_type = ?");
        } else {
            sql.push_str(" AND chunk_type IN (");
            sql.push_str(
                &std::iter::repeat_n("?", chunk_types.len())
                    .collect::<Vec<_>>()
                    .join(","),
            );
            sql.push(')');
        }
        for chunk_type in chunk_types {
            params.push(SqlValue::from(chunk_type));
        }
    }
    if let Some(from_sec) = query.from_sec {
        sql.push_str(" AND COALESCE(end_sec, start_sec, 0) >= ?");
        params.push(SqlValue::from(from_sec));
    }
    if let Some(to_sec) = query.to_sec {
        sql.push_str(" AND COALESCE(start_sec, end_sec, 0) <= ?");
        params.push(SqlValue::from(to_sec));
    }
    sql.push_str(
        r#"
        ORDER BY COALESCE(start_sec, 0), id ASC
        LIMIT ? OFFSET ?
        "#,
    );
    params.push(SqlValue::from(fetch_limit as i64));
    params.push(SqlValue::from(offset as i64));

    let conn = cerul_storage::sqlite::open(&state.paths)?;
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), chunk_from_row)?;
    let mut rows = rows.collect::<Result<Vec<_>, _>>()?;
    let next_cursor = if rows.len() > limit {
        rows.truncate(limit);
        Some((offset + limit).to_string())
    } else {
        None
    };
    let chunks = rows
        .iter()
        .map(|chunk| v1_item_chunk(chunk, &item, &base_url))
        .collect::<Vec<_>>();

    Ok(Json(V1ItemChunksResponse {
        request_id: new_id("req"),
        execution: V1Execution {
            target: "local",
            account_id: None,
            privacy: "local_only",
        },
        item: v1_item_from_row(&item, &base_url),
        chunks,
        page: V1Page { limit, next_cursor },
    }))
}

fn v1_page_limit(limit: Option<usize>, default: usize, max: usize) -> usize {
    limit.unwrap_or(default).clamp(1, max)
}

fn v1_cursor_offset(cursor: Option<&str>) -> ApiResult<usize> {
    let Some(cursor) = cursor.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(0);
    };
    cursor
        .parse::<usize>()
        .map_err(|_| ApiError::bad_request("cursor must be a non-negative integer offset"))
}

fn first_non_empty_text(values: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    values
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

pub(crate) fn v1_chunk_type_filter_values(value: &str) -> Vec<String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "transcript" => vec!["transcript".to_string(), "transcript_line".to_string()],
        "visual" => vec![
            "keyframe".to_string(),
            "image".to_string(),
            "ocr".to_string(),
        ],
        "summary" => vec!["understanding".to_string()],
        raw => vec![raw.to_string()],
    }
}

fn local_source_file_exists(raw_path: &str) -> bool {
    let raw_path = raw_path.trim();
    !raw_path.is_empty() && FsPath::new(raw_path).is_file()
}

fn v1_query_execution(paths: &AppPaths) -> V1QueryExecution {
    match api_models::effective_query_inference_mode(paths) {
        Ok(mode) if mode == "remote" => V1QueryExecution::RemoteEmbedding,
        Ok(_) => V1QueryExecution::LocalOnly,
        Err(error) => {
            tracing::debug!(%error, "could not resolve v1 query execution mode; assuming local-only fallback");
            V1QueryExecution::LocalOnly
        }
    }
}

fn v1_read_only_query_execution(paths: &AppPaths) -> V1QueryExecution {
    match api_models::read_only_effective_query_inference_mode(paths).as_str() {
        "remote" => V1QueryExecution::RemoteEmbedding,
        _ => V1QueryExecution::LocalOnly,
    }
}

fn v1_load_item(paths: &AppPaths, id: &str) -> anyhow::Result<Option<V1ItemRow>> {
    let conn = cerul_storage::sqlite::open(paths)?;
    let sql = format!(
        r#"
        {}
        WHERE i.id = ?1
          AND i.status != 'deleting'
        "#,
        v1_item_select_sql()
    );
    conn.query_row(&sql, [id], v1_item_row_from_row)
        .optional()
        .map_err(Into::into)
}

fn v1_item_select_sql() -> String {
    r#"
        SELECT i.id, i.content_type, i.external_id, i.title,
               COALESCE(i.duration_sec, (
                   SELECT MAX(c2.end_sec)
                   FROM chunks c2
                   WHERE c2.item_id = i.id
               )) AS duration_sec,
               i.indexed_at, i.status, i.metadata,
               s.type AS source_type, s.config AS source_config,
               (
                   SELECT c.id
                   FROM chunks c
                   WHERE c.item_id = i.id
                     AND c.frame_path IS NOT NULL
                     AND TRIM(c.frame_path) <> ''
                   ORDER BY COALESCE(c.start_sec, 0), c.id
                   LIMIT 1
               ) AS thumbnail_chunk_id,
               (
                   SELECT c.frame_path
                   FROM chunks c
                   WHERE c.item_id = i.id
                     AND c.frame_path IS NOT NULL
                     AND TRIM(c.frame_path) <> ''
                   ORDER BY COALESCE(c.start_sec, 0), c.id
                   LIMIT 1
               ) AS thumbnail_frame_path,
               (
                   SELECT COUNT(*)
                   FROM chunks c
                   WHERE c.item_id = i.id
               ) AS chunk_count,
               i.raw_path
        FROM items i
        JOIN sources s ON s.id = i.source_id
    "#
    .to_string()
}

fn v1_item_row_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<V1ItemRow> {
    let title = row
        .get::<_, Option<String>>(3)?
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Untitled media".to_string());
    let metadata = row
        .get::<_, Option<String>>(7)?
        .as_deref()
        .map(parse_json)
        .unwrap_or_else(|| json!({}));
    let source_config = row
        .get::<_, Option<String>>(9)?
        .as_deref()
        .map(parse_json)
        .unwrap_or_else(|| json!({}));
    let thumbnail_chunk_id: Option<String> = row.get(10)?;
    let thumbnail_frame_path: Option<String> = row.get(11)?;
    let chunk_count = row.get::<_, i64>(12)?.max(0) as usize;
    let raw_path: Option<String> = row.get(13)?;
    let source_file_exists = raw_path.as_deref().is_some_and(local_source_file_exists);

    Ok(V1ItemRow {
        id: row.get(0)?,
        content_type: row.get(1)?,
        external_id: row.get(2)?,
        title,
        duration_sec: row.get(4)?,
        indexed_at: row.get(5)?,
        status: row.get(6)?,
        metadata,
        source_type: row.get(8)?,
        source_config,
        thumbnail_chunk_id,
        thumbnail_frame_path,
        chunk_count,
        source_file_exists,
    })
}

fn v1_item_from_row(item: &V1ItemRow, base_url: &str) -> V1Item {
    let thumbnail = item
        .thumbnail_chunk_id
        .as_deref()
        .zip(item.thumbnail_frame_path.as_deref())
        .filter(|(_, frame_path)| local_source_file_exists(frame_path))
        .map(|(chunk_id, _)| V1Locator {
            locator_type: "local",
            url: format!(
                "{}/chunks/{}/frame",
                base_url,
                encode_path_segment(chunk_id)
            ),
        });
    V1Item {
        id: item.id.clone(),
        title: item.title.clone(),
        content_type: item.content_type.clone(),
        source_type: item.source_type.clone(),
        source_url: v1_item_source_url(item),
        status: item.status.clone(),
        duration_sec: item.duration_sec,
        indexed_at: item.indexed_at,
        chunk_count: item.chunk_count,
        thumbnail,
        open_in_cerul: v1_open_item_in_cerul_link(&item.id),
    }
}

fn v1_item_chunk(chunk: &ChunkRecord, item: &V1ItemRow, base_url: &str) -> V1ItemChunk {
    V1ItemChunk {
        id: chunk.id.clone(),
        chunk_type: v1_result_type(&chunk.chunk_type).to_string(),
        source: "local_library",
        time: V1SearchTime {
            start_sec: chunk.start_sec.filter(|value| value.is_finite()),
            end_sec: chunk.end_sec.filter(|value| value.is_finite()),
            timestamp: chunk.start_sec.map(format_playback_timestamp),
        },
        text: V1ChunkText {
            content: chunk.text.clone(),
            snippet: chunk.text.as_deref().map(|text| trim_for_answer(text, 360)),
        },
        evidence: v1_chunk_evidence(
            &chunk.id,
            item,
            chunk.start_sec,
            chunk.frame_path.as_deref(),
            &chunk.metadata,
            base_url,
        ),
    }
}

fn v1_chunk_evidence(
    chunk_id: &str,
    item: &V1ItemRow,
    start_sec: Option<f64>,
    frame_path: Option<&str>,
    metadata: &Value,
    base_url: &str,
) -> V1Evidence {
    let clip = if item.source_file_exists
        && item.content_type == "video"
        && has_timed_video_clip_start(start_sec)
    {
        Some(V1Locator {
            locator_type: "local",
            url: format!(
                "{}/chunks/{}/video-clip?before_sec=3&after_sec=5",
                base_url,
                encode_path_segment(chunk_id)
            ),
        })
    } else {
        None
    };
    let preview = frame_path
        .map(str::trim)
        .filter(|path| local_source_file_exists(path))
        .map(|_| V1Locator {
            locator_type: "local",
            url: format!(
                "{}/chunks/{}/frame",
                base_url,
                encode_path_segment(chunk_id)
            ),
        });
    let page = evidence_page(metadata);
    let section = evidence_section(metadata);
    let evidence_kind = match (clip.is_some(), preview.is_some()) {
        (true, _) => "video_clip",
        (false, true) => "frame",
        (false, false) if item.content_type == "document" || page.is_some() => "document",
        (false, false) => "chunk",
    };

    V1Evidence {
        id: chunk_id.to_string(),
        kind: evidence_kind,
        clip,
        preview,
        page,
        section,
        open_in_cerul: v1_open_in_cerul_link_with_page(&item.id, chunk_id, start_sec, page),
    }
}

fn v1_item_source_url(item: &V1ItemRow) -> Option<String> {
    for key in [
        "webpage_url",
        "original_url",
        "source_url",
        "url",
        "enclosure_url",
        "feed_url",
        "channel_url",
    ] {
        if let Some(url) = item
            .metadata
            .get(key)
            .and_then(Value::as_str)
            .and_then(v1_http_url)
        {
            return Some(url);
        }
    }
    if item.source_type == "youtube" {
        if let Some(external_id) = item
            .external_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(format!("https://www.youtube.com/watch?v={external_id}"));
        }
    }
    for key in ["url", "feed_url", "channel_url"] {
        if let Some(url) = item
            .source_config
            .get(key)
            .and_then(Value::as_str)
            .and_then(v1_http_url)
        {
            return Some(url);
        }
    }
    None
}

fn v1_http_url(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
        Some(trimmed.to_string())
    } else {
        None
    }
}

fn validate_v1_local_target(target: Option<&str>) -> ApiResult<()> {
    let target = target
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("local")
        .to_ascii_lowercase();
    if matches!(target.as_str(), "local" | "auto") {
        return Ok(());
    }
    Err(ApiError::bad_request(
        "only local or auto target is currently supported by /v1",
    ))
}

fn v1_extractive_answer(
    question: &str,
    citations: &[V1SearchResult],
    locale: Option<&str>,
) -> String {
    let answer_in_english =
        !locale.is_some_and(|locale| locale.trim().to_ascii_lowercase().starts_with("zh"));
    if citations.is_empty() {
        if answer_in_english {
            format!(
                "I couldn't find a directly related moment for \"{}\" in the local index. Try another keyword or wait for current indexing jobs to finish.",
                question
            )
        } else {
            format!(
                "没有在本地索引里找到和「{}」直接相关的片段。可以先换一个关键词，或等当前索引任务完成后再问。",
                question
            )
        }
    } else {
        let mut sentences = Vec::new();
        for citation in citations.iter().take(3) {
            if answer_in_english {
                if let Some(location) = v1_document_answer_location(citation, true) {
                    sentences.push(format!(
                        "{} in \"{}\", the document says: {}",
                        location, citation.item.title, citation.text.snippet
                    ));
                } else {
                    let timestamp = citation.time.timestamp.as_deref().unwrap_or("0:00");
                    sentences.push(format!(
                        "Around {} in \"{}\", the index says: {}",
                        timestamp, citation.item.title, citation.text.snippet
                    ));
                }
            } else {
                if let Some(location) = v1_document_answer_location(citation, false) {
                    sentences.push(format!(
                        "在《{}》{}，文档里提到：{}",
                        citation.item.title, location, citation.text.snippet
                    ));
                } else {
                    let timestamp = citation.time.timestamp.as_deref().unwrap_or("0:00");
                    sentences.push(format!(
                        "在《{}》{} 附近，索引里提到：{}",
                        citation.item.title, timestamp, citation.text.snippet
                    ));
                }
            }
        }
        if answer_in_english {
            format!(
                "{} This answer is extractive and grounded only in the local search hits below.",
                sentences.join(" ")
            )
        } else {
            format!(
                "{} 本回答是抽取式回答，只基于下面这些本地检索命中。",
                sentences.join(" ")
            )
        }
    }
}

fn v1_document_answer_location(citation: &V1SearchResult, english: bool) -> Option<String> {
    let is_document = citation.item.content_type == "document"
        || citation.result_type == "document"
        || citation.evidence.kind == "document"
        || citation.evidence.page.is_some()
        || citation.evidence.section.is_some();
    if !is_document {
        return None;
    }
    let section = citation
        .evidence
        .section
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match (citation.evidence.page, section, english) {
        (Some(page), Some(section), true) => Some(format!("On page {page}, section \"{section}\"")),
        (Some(page), None, true) => Some(format!("On page {page}")),
        (None, Some(section), true) => Some(format!("In section \"{section}\"")),
        (None, None, true) => Some("In the indexed document".to_string()),
        (Some(page), Some(section), false) => Some(format!("第 {page} 页「{section}」部分")),
        (Some(page), None, false) => Some(format!("第 {page} 页")),
        (None, Some(section), false) => Some(format!("「{section}」部分")),
        (None, None, false) => Some("文档索引中".to_string()),
    }
}

fn v1_material_insight_summary(
    query: &str,
    evidence: &[V1SearchResult],
) -> V1MaterialInsightSummary {
    let item_count = evidence
        .iter()
        .map(|result| result.item.id.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let present_modalities = evidence
        .iter()
        .map(v1_material_modality)
        .collect::<BTreeSet<_>>();
    let modalities = V1_MATERIAL_MODALITY_ORDER
        .iter()
        .copied()
        .filter(|modality| present_modalities.contains(modality))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    V1MaterialInsightSummary {
        query: query.to_string(),
        result_count: evidence.len(),
        item_count,
        modalities,
    }
}

fn v1_material_insight_topics(evidence: &[V1SearchResult]) -> Vec<V1MaterialInsightTopic> {
    let mut groups: BTreeMap<&'static str, (BTreeSet<String>, Vec<String>)> = BTreeMap::new();
    for result in evidence {
        let modality = v1_material_modality(result);
        let (item_ids, evidence_ids) = groups.entry(modality).or_default();
        item_ids.insert(result.item.id.clone());
        evidence_ids.push(result.id.clone());
    }

    V1_MATERIAL_MODALITY_ORDER
        .iter()
        .copied()
        .filter_map(|modality| {
            groups
                .remove(modality)
                .map(|(item_ids, evidence_ids)| V1MaterialInsightTopic {
                    title: v1_material_topic_title(modality).to_string(),
                    modality: modality.to_string(),
                    item_count: item_ids.len(),
                    evidence_ids,
                })
        })
        .collect()
}

fn v1_material_usable_shots(evidence: &[V1SearchResult]) -> Vec<V1MaterialUsableShot> {
    evidence
        .iter()
        .filter_map(|result| {
            let modality = v1_material_modality(result);
            if modality == "document" {
                return None;
            }
            if !v1_material_result_has_usable_locator(result) {
                return None;
            }
            Some(V1MaterialUsableShot {
                evidence_id: result.id.clone(),
                item_id: result.item.id.clone(),
                item_title: result.item.title.clone(),
                modality: modality.to_string(),
                start_sec: result.time.start_sec,
                end_sec: result.time.end_sec,
                reason: v1_material_usable_shot_reason(modality).to_string(),
                open_in_cerul: result.evidence.open_in_cerul.clone(),
                clip_url: result
                    .evidence
                    .clip
                    .as_ref()
                    .map(|locator| locator.url.clone()),
                preview_url: result
                    .evidence
                    .preview
                    .as_ref()
                    .map(|locator| locator.url.clone()),
            })
        })
        .collect()
}

fn v1_material_result_has_usable_locator(result: &V1SearchResult) -> bool {
    result
        .time
        .start_sec
        .or(result.time.end_sec)
        .is_some_and(|value| value.is_finite())
        || result.evidence.clip.is_some()
        || result.evidence.preview.is_some()
}

const V1_MATERIAL_MODALITY_ORDER: &[&str] = &["video", "audio", "image", "document", "other"];

fn v1_material_modality(result: &V1SearchResult) -> &'static str {
    match result.item.content_type.as_str() {
        "video" => "video",
        "audio" => "audio",
        "image" => "image",
        "document" => "document",
        _ if result.result_type == "visual" => "image",
        _ if result.result_type == "document" => "document",
        _ => "other",
    }
}

fn v1_material_topic_title(modality: &str) -> &'static str {
    match modality {
        "video" => "Video evidence",
        "audio" => "Audio evidence",
        "image" => "Image evidence",
        "document" => "Document evidence",
        _ => "Other evidence",
    }
}

fn v1_material_usable_shot_reason(modality: &str) -> &'static str {
    match modality {
        "video" => "Timed video evidence with a replayable local clip.",
        "audio" => "Timed audio evidence that can anchor narration or interview beats.",
        "image" => "Visual evidence with a frame preview or OCR-backed match.",
        _ => "Indexed local evidence related to the requested material.",
    }
}

struct V1PreEditPlan {
    storyboard: V1Storyboard,
    shot_list: Vec<V1ShotListEntry>,
    broll_gaps: Vec<V1BrollGap>,
}

fn v1_pre_edit_plan(query: &str, title: &str, evidence: &[V1SearchResult]) -> V1PreEditPlan {
    let shot_evidence = evidence
        .iter()
        .filter(|result| v1_is_pre_edit_shot_candidate(result))
        .collect::<Vec<_>>();
    let beats = shot_evidence
        .iter()
        .enumerate()
        .map(|(index, result)| v1_storyboard_beat(index, result))
        .collect::<Vec<_>>();
    let shot_list = shot_evidence
        .iter()
        .enumerate()
        .map(|(index, result)| v1_shot_list_entry(index, result))
        .collect::<Vec<_>>();
    let broll_candidate_ids = evidence
        .iter()
        .filter(|result| v1_is_broll_candidate(result))
        .map(|result| result.id.clone())
        .collect::<Vec<_>>();
    let broll_gaps = shot_list
        .iter()
        .filter_map(|shot| v1_broll_gap(query, shot, &broll_candidate_ids))
        .collect::<Vec<_>>();

    V1PreEditPlan {
        storyboard: V1Storyboard {
            title: title.to_string(),
            intent: format!("Pre-edit plan for local material matching \"{query}\"."),
            boundary: "Heuristic pre-edit planning only; Cerul does not render media, provide a timeline editor, or run LLM/VLM reasoning for this endpoint.",
            beats,
        },
        shot_list,
        broll_gaps,
    }
}

fn v1_storyboard_beat(index: usize, result: &V1SearchResult) -> V1StoryboardBeat {
    let beat_id = v1_beat_id(index);
    let modality = v1_material_modality(result);
    V1StoryboardBeat {
        id: beat_id,
        title: format!(
            "{}: {}",
            v1_pretty_modality(modality),
            trim_for_answer(&result.item.title, 72)
        ),
        summary: trim_for_answer(&result.text.snippet, 180),
        evidence_ids: vec![result.id.clone()],
        open_in_cerul: result.evidence.open_in_cerul.clone(),
    }
}

fn v1_shot_list_entry(index: usize, result: &V1SearchResult) -> V1ShotListEntry {
    let modality = v1_material_modality(result);
    V1ShotListEntry {
        id: format!("shot-{:02}", index + 1),
        beat_id: v1_beat_id(index),
        evidence_id: result.id.clone(),
        item_id: result.item.id.clone(),
        item_title: result.item.title.clone(),
        modality: modality.to_string(),
        role: v1_shot_role(modality),
        start_sec: result.time.start_sec,
        end_sec: result.time.end_sec,
        note: trim_for_answer(&result.text.snippet, 180),
        open_in_cerul: result.evidence.open_in_cerul.clone(),
        clip_url: result
            .evidence
            .clip
            .as_ref()
            .map(|locator| locator.url.clone()),
        preview_url: result
            .evidence
            .preview
            .as_ref()
            .map(|locator| locator.url.clone()),
        media_target_url: result.source_file_url.clone(),
        item_duration_sec: result.item.duration_sec,
    }
}

fn v1_broll_gap(
    query: &str,
    shot: &V1ShotListEntry,
    broll_candidate_ids: &[String],
) -> Option<V1BrollGap> {
    if !matches!(shot.modality.as_str(), "audio" | "document") {
        return None;
    }
    let candidate_evidence_ids = broll_candidate_ids
        .iter()
        .filter(|evidence_id| *evidence_id != &shot.evidence_id)
        .take(3)
        .cloned()
        .collect::<Vec<_>>();
    let reason = if shot.modality == "audio" {
        "Audio-led beat needs visual coverage before editing."
    } else {
        "Document/reference beat needs visual material before editing."
    };

    Some(V1BrollGap {
        id: format!("broll-gap-{}", shot.id),
        beat_id: shot.beat_id.clone(),
        reason: reason.to_string(),
        search_query: format!("{query} b-roll {}", shot.item_title),
        candidate_evidence_ids,
    })
}

fn v1_broll_candidates(evidence: &[V1SearchResult]) -> Vec<V1BrollCandidate> {
    evidence
        .iter()
        .filter(|result| v1_is_broll_candidate(result))
        .map(|result| {
            let modality = v1_material_modality(result);
            V1BrollCandidate {
                evidence_id: result.id.clone(),
                item_id: result.item.id.clone(),
                item_title: result.item.title.clone(),
                modality: modality.to_string(),
                start_sec: result.time.start_sec,
                end_sec: result.time.end_sec,
                reason: v1_material_usable_shot_reason(modality).to_string(),
                open_in_cerul: result.evidence.open_in_cerul.clone(),
                clip_url: result
                    .evidence
                    .clip
                    .as_ref()
                    .map(|locator| locator.url.clone()),
                preview_url: result
                    .evidence
                    .preview
                    .as_ref()
                    .map(|locator| locator.url.clone()),
            }
        })
        .collect()
}

fn v1_otio_timeline_export(title: &str, shot_list: &[V1ShotListEntry]) -> V1TimelineExport {
    let video_shots = shot_list
        .iter()
        .filter(|shot| matches!(shot.modality.as_str(), "video" | "image"))
        .collect::<Vec<_>>();
    let audio_shots = shot_list
        .iter()
        .filter(|shot| shot.modality == "audio")
        .collect::<Vec<_>>();
    let mut tracks = Vec::new();
    if !video_shots.is_empty() {
        tracks.push(v1_otio_track("Video", "Visual storyboard", &video_shots));
    }
    if !audio_shots.is_empty() {
        tracks.push(v1_otio_track("Audio", "Audio anchors", &audio_shots));
    }
    let timeline = json!({
        "OTIO_SCHEMA": "Timeline.1",
        "name": title,
        "metadata": {
            "cerul": {
                "export_kind": "pre_edit_planning",
                "format": "otio_json",
                "shot_count": shot_list.len(),
                "compatibility_note": "Heuristic planning export only; Cerul does not render media or provide a timeline editor, and timeline clips reference local source files when available."
            }
        },
        "tracks": {
            "OTIO_SCHEMA": "Stack.1",
            "name": "Cerul pre-edit stack",
            "children": tracks
        }
    });

    V1TimelineExport {
        format: "otio_json",
        filename: v1_timeline_filename(title),
        mime_type: "application/vnd.opentimelineio+json",
        content: serde_json::to_string_pretty(&timeline).unwrap_or_else(|_| "{}".to_string()),
        compatibility_note:
            "OTIO JSON planning timeline only; Cerul does not render media or provide a timeline editor, and clips reference local source files when available.",
    }
}

fn v1_otio_track(kind: &'static str, name: &'static str, shots: &[&V1ShotListEntry]) -> Value {
    let mut accumulated_sec = 0.0;
    let clips = shots
        .iter()
        .filter_map(|shot| {
            let media_target_url = shot.media_target_url.as_deref()?;
            let (source_start_sec, duration_sec, available_duration_sec) =
                v1_shot_source_range_sec(shot)?;
            let clip = json!({
                "OTIO_SCHEMA": "Clip.2",
                "name": format!("{} - {}", shot.id, shot.item_title),
                "source_range": {
                    "OTIO_SCHEMA": "TimeRange.1",
                    "start_time": v1_otio_rational_time(source_start_sec),
                    "duration": v1_otio_rational_time(duration_sec)
                },
                "media_reference": {
                    "OTIO_SCHEMA": "ExternalReference.1",
                    "target_url": media_target_url,
                    "available_range": {
                        "OTIO_SCHEMA": "TimeRange.1",
                        "start_time": v1_otio_rational_time(0.0),
                        "duration": v1_otio_rational_time(available_duration_sec)
                    },
                    "metadata": {
                        "cerul": {
                            "evidence_id": shot.evidence_id,
                            "item_id": shot.item_id,
                            "modality": shot.modality,
                            "open_in_cerul": shot.open_in_cerul,
                            "clip_url": shot.clip_url,
                            "preview_url": shot.preview_url,
                            "source_file_url": media_target_url
                        }
                    }
                },
                "metadata": {
                    "cerul": {
                        "beat_id": shot.beat_id,
                        "role": shot.role,
                        "note": shot.note,
                        "open_in_cerul": shot.open_in_cerul,
                        "timeline_offset_sec": accumulated_sec
                    }
                }
            });
            accumulated_sec += duration_sec;
            Some(clip)
        })
        .collect::<Vec<_>>();
    json!({
        "OTIO_SCHEMA": "Track.1",
        "name": name,
        "kind": kind,
        "children": clips
    })
}

fn v1_pre_edit_title(title: Option<&str>, query: &str) -> String {
    title
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("Cerul pre-edit plan: {}", trim_for_answer(query, 80)))
}

fn v1_beat_id(index: usize) -> String {
    format!("beat-{:02}", index + 1)
}

fn v1_is_broll_candidate(result: &V1SearchResult) -> bool {
    matches!(v1_material_modality(result), "video" | "image")
        && (result.evidence.clip.is_some() || result.evidence.preview.is_some())
}

fn v1_is_pre_edit_shot_candidate(result: &V1SearchResult) -> bool {
    if result.source_file_url.is_none() {
        return false;
    }
    match v1_material_modality(result) {
        "video" | "audio" => {
            v1_timed_source_range(result.time.start_sec, result.time.end_sec).is_some()
        }
        "image" => true,
        _ => false,
    }
}

fn v1_shot_role(modality: &str) -> &'static str {
    match modality {
        "video" => "primary",
        "audio" => "audio_anchor",
        "image" => "broll",
        "document" => "reference",
        _ => "context",
    }
}

fn v1_pretty_modality(modality: &str) -> &'static str {
    match modality {
        "video" => "Video",
        "audio" => "Audio",
        "image" => "Image",
        "document" => "Document",
        _ => "Evidence",
    }
}

fn v1_shot_source_range_sec(shot: &V1ShotListEntry) -> Option<(f64, f64, f64)> {
    if matches!(shot.modality.as_str(), "video" | "audio") {
        let (start_sec, duration_sec) = v1_timed_source_range(shot.start_sec, shot.end_sec)?;
        let available_duration_sec = shot
            .item_duration_sec
            .filter(|duration| duration.is_finite() && *duration >= start_sec + duration_sec)
            .unwrap_or(start_sec + duration_sec);
        return Some((start_sec, duration_sec, available_duration_sec));
    }

    if shot.modality == "image" {
        return Some((0.0, 4.0, 4.0));
    }

    None
}

fn v1_timed_source_range(start_sec: Option<f64>, end_sec: Option<f64>) -> Option<(f64, f64)> {
    let start_sec = start_sec.filter(|value| value.is_finite() && *value >= 0.0)?;
    let end_sec = end_sec.filter(|value| value.is_finite() && *value > start_sec)?;
    let duration_sec = end_sec - start_sec;
    (duration_sec > 0.25).then_some((start_sec, duration_sec))
}

fn v1_otio_rational_time(seconds: f64) -> Value {
    let millis = (seconds.max(0.0) * 1000.0).round() as i64;
    json!({
        "OTIO_SCHEMA": "RationalTime.1",
        "value": millis,
        "rate": 1000
    })
}

fn v1_timeline_filename(title: &str) -> String {
    let mut slug = String::new();
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if matches!(ch, ' ' | '-' | '_') && !slug.ends_with('-') {
            slug.push('-');
        }
        if slug.len() >= 48 {
            break;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "cerul-pre-edit-plan.otio".to_string()
    } else {
        format!("{slug}.otio")
    }
}

fn v1_search_result(
    result: &cerul_search::SearchResult,
    item_metadata: &HashMap<String, V1SearchItemMetadata>,
    existing_preview_chunk_ids: &HashSet<String>,
    evidence_metadata: &HashMap<String, V1EvidenceMetadata>,
    base_url: &str,
) -> V1SearchResult {
    let metadata = item_metadata
        .get(&result.item_id)
        .cloned()
        .unwrap_or_else(|| fallback_v1_search_item_metadata(result));
    let start_sec = result.start_sec.filter(|value| value.is_finite());
    let end_sec = result.end_sec.filter(|value| value.is_finite());
    let preview_chunk_id = result.nearest_frame_chunk_id.as_deref().or_else(|| {
        result
            .frame_path
            .as_ref()
            .map(|_| result.playback_chunk_id.as_str())
    });
    let clip = if metadata.source_file_exists
        && metadata.item.content_type == "video"
        && has_timed_video_clip_start(start_sec)
    {
        Some(V1Locator {
            locator_type: "local",
            url: format!(
                "{}/chunks/{}/video-clip?before_sec=3&after_sec=5",
                base_url,
                encode_path_segment(&result.playback_chunk_id)
            ),
        })
    } else {
        None
    };
    let preview = preview_chunk_id.and_then(|chunk_id| {
        existing_preview_chunk_ids
            .contains(chunk_id)
            .then(|| V1Locator {
                locator_type: "local",
                url: format!(
                    "{}/chunks/{}/frame",
                    base_url,
                    encode_path_segment(chunk_id)
                ),
            })
    });
    let evidence_metadata = evidence_metadata
        .get(&result.playback_chunk_id)
        .cloned()
        .unwrap_or_default();
    let evidence_kind = match (clip.is_some(), preview.is_some()) {
        (true, _) => "video_clip",
        (false, true) => "frame",
        (false, false)
            if metadata.item.content_type == "document"
                || result.chunk_type == "document"
                || evidence_metadata.page.is_some() =>
        {
            "document"
        }
        (false, false) => "chunk",
    };

    V1SearchResult {
        id: result.playback_chunk_id.clone(),
        result_type: v1_result_type(&result.chunk_type),
        source: "local_library",
        item: metadata.item,
        time: V1SearchTime {
            start_sec,
            end_sec,
            timestamp: start_sec.map(format_playback_timestamp),
        },
        text: V1SearchText {
            snippet: trim_for_answer(&result.snippet, 360),
            quote: trim_for_answer(&result.snippet, 240),
        },
        evidence: V1Evidence {
            id: result.playback_chunk_id.clone(),
            kind: evidence_kind,
            clip,
            preview,
            page: evidence_metadata.page,
            section: evidence_metadata.section,
            open_in_cerul: v1_open_in_cerul_link_with_page(
                &result.item_id,
                &result.playback_chunk_id,
                start_sec,
                evidence_metadata.page,
            ),
        },
        score: V1Score {
            match_score: result.match_score,
            exact_match: result.exact_match,
            similarity: result.similarity_score,
        },
        source_file_url: metadata.source_file_url,
    }
}

fn v1_search_item_metadata(
    paths: &AppPaths,
    results: &[cerul_search::SearchResult],
) -> anyhow::Result<HashMap<String, V1SearchItemMetadata>> {
    let mut item_ids = results
        .iter()
        .map(|result| result.item_id.as_str())
        .collect::<Vec<_>>();
    item_ids.sort_unstable();
    item_ids.dedup();
    if item_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders = std::iter::repeat_n("?", item_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        r#"
        SELECT i.id, i.title, i.content_type, s.type, i.duration_sec,
               i.raw_path
        FROM items i
        JOIN sources s ON s.id = i.source_id
        WHERE i.id IN ({placeholders})
        "#
    );
    let conn = cerul_storage::sqlite::open(paths)?;
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(item_ids.iter()), |row| {
        let id: String = row.get(0)?;
        let title = row
            .get::<_, Option<String>>(1)?
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "Untitled media".to_string());
        let raw_path: Option<String> = row.get(5)?;
        let source_file_url = raw_path.as_deref().and_then(v1_source_file_url);
        let source_file_exists = source_file_url.is_some();
        Ok((
            id.clone(),
            V1SearchItemMetadata {
                item: V1SearchItem {
                    id,
                    title,
                    content_type: row.get(2)?,
                    source_type: row.get(3)?,
                    duration_sec: row.get(4)?,
                },
                source_file_exists,
                source_file_url,
            },
        ))
    })?;
    let mut metadata = HashMap::with_capacity(item_ids.len());
    for row in rows {
        let (id, item) = row?;
        metadata.insert(id, item);
    }
    Ok(metadata)
}

fn v1_existing_preview_chunk_ids(
    paths: &AppPaths,
    results: &[cerul_search::SearchResult],
) -> anyhow::Result<HashSet<String>> {
    let mut chunk_ids = results
        .iter()
        .filter_map(|result| {
            result.nearest_frame_chunk_id.as_deref().or_else(|| {
                result
                    .frame_path
                    .as_ref()
                    .map(|_| result.playback_chunk_id.as_str())
            })
        })
        .collect::<Vec<_>>();
    chunk_ids.sort_unstable();
    chunk_ids.dedup();
    if chunk_ids.is_empty() {
        return Ok(HashSet::new());
    }

    let placeholders = std::iter::repeat_n("?", chunk_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        r#"
        SELECT id, frame_path
        FROM chunks
        WHERE id IN ({placeholders})
          AND frame_path IS NOT NULL
          AND TRIM(frame_path) <> ''
        "#
    );
    let conn = cerul_storage::sqlite::open(paths)?;
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(chunk_ids.iter()), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut existing = HashSet::new();
    for row in rows {
        let (id, frame_path) = row?;
        if local_source_file_exists(&frame_path) {
            existing.insert(id);
        }
    }
    Ok(existing)
}

#[derive(Debug, Clone, Default)]
struct V1EvidenceMetadata {
    page: Option<u32>,
    section: Option<String>,
}

fn v1_evidence_metadata_for_results(
    paths: &AppPaths,
    results: &[cerul_search::SearchResult],
) -> anyhow::Result<HashMap<String, V1EvidenceMetadata>> {
    let mut chunk_ids = results
        .iter()
        .map(|result| result.playback_chunk_id.as_str())
        .collect::<Vec<_>>();
    chunk_ids.sort_unstable();
    chunk_ids.dedup();
    if chunk_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let placeholders = std::iter::repeat_n("?", chunk_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        r#"
        SELECT id, metadata
        FROM chunks
        WHERE id IN ({placeholders})
        "#
    );
    let conn = cerul_storage::sqlite::open(paths)?;
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(chunk_ids.iter()), |row| {
        let id: String = row.get(0)?;
        let metadata: Option<String> = row.get(1)?;
        let metadata = metadata
            .as_deref()
            .map(parse_json)
            .unwrap_or_else(|| json!({}));
        Ok((
            id,
            V1EvidenceMetadata {
                page: evidence_page(&metadata),
                section: evidence_section(&metadata),
            },
        ))
    })?;
    let mut metadata = HashMap::with_capacity(chunk_ids.len());
    for row in rows {
        let (id, value) = row?;
        metadata.insert(id, value);
    }
    Ok(metadata)
}

fn fallback_v1_search_item_metadata(result: &cerul_search::SearchResult) -> V1SearchItemMetadata {
    V1SearchItemMetadata {
        item: V1SearchItem {
            id: result.item_id.clone(),
            title: result
                .item_title
                .clone()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "Untitled media".to_string()),
            content_type: "unknown".to_string(),
            source_type: "unknown".to_string(),
            duration_sec: None,
        },
        source_file_exists: false,
        source_file_url: None,
    }
}

fn v1_source_file_url(raw_path: &str) -> Option<String> {
    let raw_path = raw_path.trim();
    if raw_path.is_empty() {
        return None;
    }
    let path = FsPath::new(raw_path);
    if !path.is_file() {
        return None;
    }
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let path = path.to_string_lossy().replace('\\', "/");
    let encoded_path = v1_file_url_encode_path(&path);
    if encoded_path.starts_with('/') {
        Some(format!("file://{encoded_path}"))
    } else {
        Some(format!("file:///{encoded_path}"))
    }
}

fn v1_file_url_encode_path(path: &str) -> String {
    path.bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}

fn v1_result_type(chunk_type: &str) -> &'static str {
    match chunk_type {
        "keyframe" | "image" | "ocr" => "visual",
        "understanding" => "summary",
        "document" => "document",
        _ => "transcript",
    }
}

fn v1_base_url(headers: &HeaderMap, paths: &AppPaths) -> String {
    if let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.contains('/'))
    {
        return format!("http://{host}/v1");
    }
    let port = configured_addr(paths)
        .map(|addr| addr.port())
        .unwrap_or(DEFAULT_API_PORT);
    format!("http://127.0.0.1:{port}/v1")
}

fn v1_open_in_cerul_link(item_id: &str, chunk_id: &str, start_sec: Option<f64>) -> String {
    let mut link = format!(
        "cerul-app://item/{}?playbackChunkId={}",
        encode_path_segment(item_id),
        encode_path_segment(chunk_id)
    );
    if let Some(start_sec) = start_sec.filter(|value| value.is_finite() && *value >= 0.0) {
        link.push_str("&t=");
        link.push_str(&format_seconds_param(start_sec));
    }
    link
}

fn v1_open_in_cerul_link_with_page(
    item_id: &str,
    chunk_id: &str,
    start_sec: Option<f64>,
    page: Option<u32>,
) -> String {
    let mut link = v1_open_in_cerul_link(item_id, chunk_id, start_sec);
    if let Some(page) = page {
        link.push_str("&page=");
        link.push_str(&page.to_string());
    }
    link
}

fn evidence_page(metadata: &Value) -> Option<u32> {
    metadata
        .get("page")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn evidence_section(metadata: &Value) -> Option<String> {
    metadata
        .get("section")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn v1_open_item_in_cerul_link(item_id: &str) -> String {
    format!("cerul-app://item/{}", encode_path_segment(item_id))
}
