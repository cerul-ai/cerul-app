use std::{
    collections::{HashMap, HashSet},
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
        .route("/search", post(v1_search))
        .route("/ask", post(v1_ask))
        .route("/items", get(v1_list_items))
        .route("/items/:id", get(v1_get_item))
        .route("/items/:id/chunks", get(v1_list_item_chunks))
        .route("/chunks/:id/frame", get(crate::get_chunk_frame))
        .route(
            "/chunks/:id/video-segment",
            get(crate::get_chunk_video_segment),
        )
        .route("/chunks/:id/video-clip", get(crate::get_chunk_video_clip))
}

async fn v1_openapi_json() -> Json<Value> {
    Json(openapi_document("Cerul Agent API", API_PATHS))
}

async fn v1_status(State(state): State<ApiState>) -> ApiResult<Json<V1StatusResponse>> {
    let indexing = jobs::indexing_diagnostics(&state.paths)?;
    let search = search_health_diagnostics(&state.paths).await?;
    let text_ready = search.fts_row_count > 0 || search.retrieval_unit_fts_row_count > 0;
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
        capabilities: vec!["status", "openapi", "search", "ask", "items", "chunks"],
    }))
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
    let base_url = v1_base_url(&headers, &state.paths);
    let results = response
        .results
        .iter()
        .map(|result| {
            v1_search_result(
                result,
                &item_metadata,
                &existing_preview_chunk_ids,
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
    let base_url = v1_base_url(&headers, &state.paths);
    let citations = filtered_results
        .iter()
        .map(|result| {
            v1_search_result(
                result,
                &item_metadata,
                &existing_preview_chunk_ids,
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
            base_url,
        ),
    }
}

fn v1_chunk_evidence(
    chunk_id: &str,
    item: &V1ItemRow,
    start_sec: Option<f64>,
    frame_path: Option<&str>,
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
    let evidence_kind = match (clip.is_some(), preview.is_some()) {
        (true, _) => "video_clip",
        (false, true) => "frame",
        (false, false) => "chunk",
    };

    V1Evidence {
        id: chunk_id.to_string(),
        kind: evidence_kind,
        clip,
        preview,
        open_in_cerul: v1_open_in_cerul_link(&item.id, chunk_id, start_sec),
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
            let timestamp = citation.time.timestamp.as_deref().unwrap_or("0:00");
            if answer_in_english {
                sentences.push(format!(
                    "Around {} in \"{}\", the index says: {}",
                    timestamp, citation.item.title, citation.text.snippet
                ));
            } else {
                sentences.push(format!(
                    "在《{}》{} 附近，索引里提到：{}",
                    citation.item.title, timestamp, citation.text.snippet
                ));
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

fn v1_search_result(
    result: &cerul_search::SearchResult,
    item_metadata: &HashMap<String, V1SearchItemMetadata>,
    existing_preview_chunk_ids: &HashSet<String>,
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
    let evidence_kind = match (clip.is_some(), preview.is_some()) {
        (true, _) => "video_clip",
        (false, true) => "frame",
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
            open_in_cerul: v1_open_in_cerul_link(
                &result.item_id,
                &result.playback_chunk_id,
                start_sec,
            ),
        },
        score: V1Score {
            match_score: result.match_score,
            exact_match: result.exact_match,
            similarity: result.similarity_score,
        },
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
        let source_file_exists = raw_path.as_deref().is_some_and(local_source_file_exists);
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
    }
}

fn v1_result_type(chunk_type: &str) -> &'static str {
    match chunk_type {
        "keyframe" | "image" | "ocr" => "visual",
        "understanding" => "summary",
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

fn v1_open_item_in_cerul_link(item_id: &str) -> String {
    format!("cerul-app://item/{}", encode_path_segment(item_id))
}
