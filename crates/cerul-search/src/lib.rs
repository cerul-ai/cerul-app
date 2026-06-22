use std::collections::{HashMap, HashSet};

use cerul_storage::AppPaths;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchRequest {
    pub q: String,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    pub chunk_id: String,
    pub item_id: String,
    pub chunk_type: String,
    pub start_sec: Option<f64>,
    pub end_sec: Option<f64>,
    pub snippet: String,
    pub frame_path: Option<String>,
    /// User-facing match score derived from the final fused ranking score.
    /// Calibrated to 0.0..=1.0 after dedupe so the UI can display the same
    /// signal that placed the result without making the top hit always 100%.
    pub match_score: f32,
    pub score: f32,
    pub similarity_score: Option<f32>,
    #[serde(default)]
    pub exact_match: bool,
    #[serde(skip)]
    source_mask: u8,
    /// Item title, joined in so the UI can label a result without a separate
    /// items fetch (which can be empty/stale and leave the row showing a raw id).
    pub item_title: Option<String>,
    /// Keyframe/image chunk that should be used for the result thumbnail.
    /// Transcript rows use the frame nearest their timestamp; visual rows point
    /// at their representative frame chunk.
    pub nearest_frame_chunk_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchDiagnostics {
    pub retrieval_mode: String,
    pub fallback_reason: Option<String>,
    pub vector_hits_count: usize,
    pub text_vector_hits_count: usize,
    pub image_vector_hits_count: usize,
    pub fts_hits_count: usize,
    pub embedding_profile_id: Option<String>,
    pub qdrant_collection: Option<String>,
    pub qdrant_point_count: Option<usize>,
    pub retrieval_unit_count: Option<usize>,
    pub indexed_item_count: Option<usize>,
    pub items_needing_rebuild: Option<usize>,
    pub qdrant_text_collection: Option<String>,
    pub qdrant_image_collection: Option<String>,
    pub qdrant_text_points: Option<usize>,
    pub qdrant_image_points: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub diagnostics: SearchDiagnostics,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawHit {
    pub chunk_id: String,
    pub score: f32,
    pub similarity_score: Option<f32>,
    exact_match: bool,
    source_mask: u8,
}

#[derive(Debug, Clone, PartialEq)]
struct LegacyChunkHit {
    chunk_id: String,
    score: f32,
    exact_match: bool,
    source_mask: u8,
}

const SOURCE_TEXT: u8 = 1 << 0;
const SOURCE_TEXT_VECTOR: u8 = 1 << 1;
const SOURCE_EXACT: u8 = 1 << 2;
const PER_ITEM_CAP: usize = 3;

pub fn crate_ready() -> bool {
    true
}

pub async fn search(req: SearchRequest) -> anyhow::Result<Vec<SearchResult>> {
    let paths = AppPaths::resolve()?;
    search_with_paths(&paths, req).await
}

pub async fn search_with_paths(
    paths: &AppPaths,
    req: SearchRequest,
) -> anyhow::Result<Vec<SearchResult>> {
    Ok(search_with_paths_diagnostics(paths, req).await?.results)
}

pub async fn search_with_paths_diagnostics(
    paths: &AppPaths,
    req: SearchRequest,
) -> anyhow::Result<SearchResponse> {
    let mut text_query_vectors = match cerul_embed::embed_texts(&[req.q.as_str()]) {
        Ok(vectors) => vectors,
        Err(error) => {
            tracing::warn!(
                %error,
                "semantic search unavailable; falling back to SQLite FTS"
            );
            return search_fts_only_with_diagnostics(
                paths,
                req,
                Some("embedding_unavailable".to_string()),
            )
            .await;
        }
    };
    let query_vector = text_query_vectors
        .pop()
        .ok_or_else(|| anyhow::anyhow!("embedder returned no query vector"))?;

    // The query vector searches the unified retrieval-unit collection.
    search_with_vectors_diagnostics(paths, req, query_vector.clone(), query_vector).await
}

pub async fn search_with_vector(
    paths: &AppPaths,
    req: SearchRequest,
    query_vector: Vec<f32>,
) -> anyhow::Result<Vec<SearchResult>> {
    search_with_vectors(paths, req, query_vector.clone(), query_vector).await
}

pub async fn search_with_vector_for_profile(
    paths: &AppPaths,
    req: SearchRequest,
    query_vector: Vec<f32>,
    profile: &cerul_storage::vectors::EmbeddingProfile,
) -> anyhow::Result<Vec<SearchResult>> {
    search_with_vectors_for_profile(paths, req, query_vector.clone(), query_vector, profile).await
}

pub async fn search_with_vector_for_profile_diagnostics(
    paths: &AppPaths,
    req: SearchRequest,
    query_vector: Vec<f32>,
    profile: &cerul_storage::vectors::EmbeddingProfile,
) -> anyhow::Result<SearchResponse> {
    search_with_vectors_for_profile_diagnostics(
        paths,
        req,
        query_vector.clone(),
        query_vector,
        profile,
    )
    .await
}

pub async fn search_fts_only(
    paths: &AppPaths,
    req: SearchRequest,
) -> anyhow::Result<Vec<SearchResult>> {
    Ok(search_fts_only_with_diagnostics(paths, req, None)
        .await?
        .results)
}

pub async fn search_fts_only_with_diagnostics(
    paths: &AppPaths,
    req: SearchRequest,
    fallback_reason: Option<String>,
) -> anyhow::Result<SearchResponse> {
    let limit = req.limit.clamp(1, 50);
    let hits = sqlite_text_search(paths, &req.q, retrieval_limit(limit)).await?;
    let mut fts_hits_count = hits.len();
    let mut results = hydrate(paths, &hits, &req.q)?;
    let mut fallback_reason = fallback_reason;
    if should_try_legacy_fts(paths) {
        let legacy_results =
            legacy_sqlite_text_results(paths, &req.q, retrieval_limit(limit)).await?;
        if !legacy_results.is_empty() {
            fts_hits_count += legacy_results.len();
            results.extend(legacy_results);
            fallback_reason.get_or_insert_with(|| "search_index_rebuilding_legacy_fts".to_string());
        }
    }
    let results = finalize_results(results, limit);
    Ok(SearchResponse {
        results,
        diagnostics: SearchDiagnostics::fts_only(fts_hits_count, fallback_reason),
    })
}

pub async fn search_with_vectors(
    paths: &AppPaths,
    req: SearchRequest,
    text_query_vector: Vec<f32>,
    _image_query_vector: Vec<f32>,
) -> anyhow::Result<Vec<SearchResult>> {
    Ok(
        search_with_vectors_diagnostics(paths, req, text_query_vector, Vec::new())
            .await?
            .results,
    )
}

pub async fn search_with_vectors_diagnostics(
    paths: &AppPaths,
    req: SearchRequest,
    text_query_vector: Vec<f32>,
    _image_query_vector: Vec<f32>,
) -> anyhow::Result<SearchResponse> {
    let profile = cerul_storage::vectors::ensure_active_embedding_profile(paths)?;
    search_with_vectors_for_profile_diagnostics(paths, req, text_query_vector, Vec::new(), &profile)
        .await
}

pub async fn search_with_vectors_for_profile(
    paths: &AppPaths,
    req: SearchRequest,
    text_query_vector: Vec<f32>,
    _image_query_vector: Vec<f32>,
    profile: &cerul_storage::vectors::EmbeddingProfile,
) -> anyhow::Result<Vec<SearchResult>> {
    Ok(search_with_vectors_for_profile_diagnostics(
        paths,
        req,
        text_query_vector,
        Vec::new(),
        profile,
    )
    .await?
    .results)
}

pub async fn search_with_vectors_for_profile_diagnostics(
    paths: &AppPaths,
    req: SearchRequest,
    text_query_vector: Vec<f32>,
    _image_query_vector: Vec<f32>,
    profile: &cerul_storage::vectors::EmbeddingProfile,
) -> anyhow::Result<SearchResponse> {
    anyhow::ensure!(
        text_query_vector.len() == profile.output_dimension as usize,
        "text query vector has {} dimensions, expected {}",
        text_query_vector.len(),
        profile.output_dimension
    );

    let limit = req.limit.clamp(1, 50);
    let retrieval_limit = retrieval_limit(limit);
    let collection = cerul_storage::vectors::unified_collection_name(
        paths,
        profile,
        cerul_storage::SEARCH_INDEX_VERSION,
    );
    let (lexical_hits, vector_hits) = tokio::try_join!(
        sqlite_text_search(paths, &req.q, retrieval_limit),
        qdrant_vector_search(
            paths,
            &collection,
            &text_query_vector,
            retrieval_limit,
            &profile.distance_metric
        ),
    )?;
    let mut fts_hits_count = lexical_hits.len();
    let vector_hits_count = vector_hits.len();
    let qdrant_point_count = cerul_storage::vectors::collection_point_count(paths, &collection)
        .await
        .ok();
    let mut fallback_reason = if vector_hits_count == 0 {
        match qdrant_point_count {
            Some(0) => Some("unified_vector_index_empty".to_string()),
            Some(_) => Some("no_unified_vector_hits".to_string()),
            None => Some("qdrant_health_check_failed".to_string()),
        }
    } else {
        None
    };
    let top_hits = merge_unified_hits(
        paths,
        &collection,
        &text_query_vector,
        &profile.distance_metric,
        vector_hits,
        lexical_hits,
        retrieval_limit,
    )
    .await?;

    let mut results = hydrate(paths, &top_hits, &req.q)?;
    let mut used_legacy_fts = false;
    if should_try_legacy_fts(paths) {
        let legacy_results = legacy_sqlite_text_results(paths, &req.q, retrieval_limit).await?;
        if !legacy_results.is_empty() {
            fts_hits_count += legacy_results.len();
            used_legacy_fts = results.is_empty();
            results.extend(legacy_results);
            fallback_reason.get_or_insert_with(|| "search_index_rebuilding_legacy_fts".to_string());
        }
    }
    let results = finalize_results(results, limit);
    let retrieval_mode = if used_legacy_fts {
        "fts_fallback".to_string()
    } else {
        retrieval_mode(vector_hits_count, fts_hits_count, qdrant_point_count)
    };
    Ok(SearchResponse {
        results,
        diagnostics: SearchDiagnostics {
            retrieval_mode,
            fallback_reason,
            vector_hits_count,
            text_vector_hits_count: vector_hits_count,
            image_vector_hits_count: 0,
            fts_hits_count,
            embedding_profile_id: Some(profile.id.clone()),
            qdrant_collection: Some(collection.clone()),
            qdrant_point_count,
            retrieval_unit_count: cerul_storage::retrieval_unit_count(paths).ok(),
            indexed_item_count: cerul_storage::indexed_item_count(paths).ok(),
            items_needing_rebuild: cerul_storage::items_needing_rebuild_count(paths).ok(),
            qdrant_text_collection: None,
            qdrant_image_collection: None,
            qdrant_text_points: qdrant_point_count,
            qdrant_image_points: Some(0),
        },
    })
}

impl SearchDiagnostics {
    fn fts_only(fts_hits_count: usize, fallback_reason: Option<String>) -> Self {
        Self {
            retrieval_mode: if fallback_reason.is_some() {
                "fts_fallback".to_string()
            } else if fts_hits_count == 0 {
                "empty".to_string()
            } else {
                "fts".to_string()
            },
            fallback_reason,
            vector_hits_count: 0,
            text_vector_hits_count: 0,
            image_vector_hits_count: 0,
            fts_hits_count,
            embedding_profile_id: None,
            qdrant_collection: None,
            qdrant_point_count: None,
            retrieval_unit_count: None,
            indexed_item_count: None,
            items_needing_rebuild: None,
            qdrant_text_collection: None,
            qdrant_image_collection: None,
            qdrant_text_points: None,
            qdrant_image_points: None,
        }
    }
}

fn retrieval_mode(
    vector_hits_count: usize,
    fts_hits_count: usize,
    qdrant_point_count: Option<usize>,
) -> String {
    match (
        vector_hits_count > 0,
        fts_hits_count > 0,
        qdrant_point_count,
    ) {
        (true, _, _) => "unified_vector",
        (false, true, _) => "unified_vector",
        (false, false, Some(0)) => "empty",
        (false, false, _) => "empty",
    }
    .to_string()
}

fn retrieval_limit(limit: usize) -> usize {
    limit.saturating_mul(4).max(limit).max(1)
}

async fn sqlite_text_search(
    paths: &AppPaths,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<RawHit>> {
    let mut hits = sqlite_fts_search(paths, query, limit).await?;
    let mut seen = hits
        .iter()
        .map(|hit| hit.chunk_id.clone())
        .collect::<HashSet<_>>();

    for hit in sqlite_literal_search(paths, query, limit).await? {
        if seen.insert(hit.chunk_id.clone()) {
            hits.push(hit);
        }
        if hits.len() >= limit {
            break;
        }
    }

    Ok(hits)
}

async fn sqlite_fts_search(
    paths: &AppPaths,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<RawHit>> {
    let Some(match_query) = fts_match_query(query) else {
        return Ok(Vec::new());
    };
    let conn = cerul_storage::sqlite::open(paths)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT u.id, bm25(retrieval_units_fts) AS rank_score
        FROM retrieval_units_fts
        JOIN retrieval_units u ON u.rowid = retrieval_units_fts.rowid
        JOIN items i ON i.id = u.item_id
        WHERE retrieval_units_fts MATCH ?1
          AND i.status != 'deleting'
          AND u.index_version = ?2
        ORDER BY rank_score
        LIMIT ?3
        "#,
    )?;
    let rows = stmt.query_map(
        (
            &match_query,
            cerul_storage::SEARCH_INDEX_VERSION,
            limit as i64,
        ),
        |row| {
            let chunk_id: String = row.get(0)?;
            let rank_score: f64 = row.get(1)?;
            Ok(RawHit {
                chunk_id,
                score: (-rank_score) as f32,
                similarity_score: None,
                exact_match: false,
                source_mask: SOURCE_TEXT,
            })
        },
    )?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

async fn sqlite_literal_search(
    paths: &AppPaths,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<RawHit>> {
    let Some(pattern) = sqlite_like_pattern(query) else {
        return Ok(Vec::new());
    };
    let strong_exact = strong_exact_intent(query);
    let conn = cerul_storage::sqlite::open(paths)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT u.id
        FROM retrieval_units u
        JOIN items i ON i.id = u.item_id
        WHERE u.content_text IS NOT NULL
          AND i.status != 'deleting'
          AND u.index_version = ?2
          AND u.content_text LIKE ?1 ESCAPE '\'
        ORDER BY
          CASE u.unit_kind
            WHEN 'moment' THEN 0
            WHEN 'summary' THEN 1
            WHEN 'visual' THEN 2
            WHEN 'image' THEN 3
            ELSE 4
          END,
          COALESCE(u.start_sec, 9223372036854775807),
          u.id
        LIMIT ?3
        "#,
    )?;
    let rows = stmt.query_map(
        (&pattern, cerul_storage::SEARCH_INDEX_VERSION, limit as i64),
        |row| {
            let chunk_id: String = row.get(0)?;
            Ok(RawHit {
                chunk_id,
                score: 0.01,
                similarity_score: None,
                exact_match: strong_exact,
                source_mask: SOURCE_TEXT | if strong_exact { SOURCE_EXACT } else { 0 },
            })
        },
    )?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn should_try_legacy_fts(paths: &AppPaths) -> bool {
    cerul_storage::items_needing_rebuild_count(paths).unwrap_or(0) > 0
}

async fn legacy_sqlite_text_results(
    paths: &AppPaths,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<SearchResult>> {
    let mut hits = legacy_sqlite_fts_search(paths, query, limit).await?;
    let mut seen = hits
        .iter()
        .map(|hit| hit.chunk_id.clone())
        .collect::<HashSet<_>>();

    for hit in legacy_sqlite_literal_search(paths, query, limit).await? {
        if seen.insert(hit.chunk_id.clone()) {
            hits.push(hit);
        }
        if hits.len() >= limit {
            break;
        }
    }

    hydrate_legacy_chunks(paths, &hits)
}

async fn legacy_sqlite_fts_search(
    paths: &AppPaths,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<LegacyChunkHit>> {
    let Some(match_query) = fts_match_query(query) else {
        return Ok(Vec::new());
    };
    let conn = cerul_storage::sqlite::open(paths)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT c.id, bm25(chunks_fts) AS rank_score
        FROM chunks_fts
        JOIN chunks c ON c.rowid = chunks_fts.rowid
        JOIN items i ON i.id = c.item_id
        WHERE chunks_fts MATCH ?1
          AND i.status != 'deleting'
          AND c.chunk_type IN ('transcript', 'transcript_line', 'audio', 'ocr', 'understanding')
        ORDER BY rank_score
        LIMIT ?2
        "#,
    )?;
    let rows = stmt.query_map((&match_query, limit as i64), |row| {
        let chunk_id: String = row.get(0)?;
        let rank_score: f64 = row.get(1)?;
        Ok(LegacyChunkHit {
            chunk_id,
            score: (-rank_score) as f32,
            exact_match: false,
            source_mask: SOURCE_TEXT,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

async fn legacy_sqlite_literal_search(
    paths: &AppPaths,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<LegacyChunkHit>> {
    let Some(pattern) = sqlite_like_pattern(query) else {
        return Ok(Vec::new());
    };
    let strong_exact = strong_exact_intent(query);
    let conn = cerul_storage::sqlite::open(paths)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT c.id
        FROM chunks c
        JOIN items i ON i.id = c.item_id
        WHERE c.text IS NOT NULL
          AND TRIM(c.text) <> ''
          AND i.status != 'deleting'
          AND c.chunk_type IN ('transcript', 'transcript_line', 'audio', 'ocr', 'understanding')
          AND c.text LIKE ?1 ESCAPE '\'
        ORDER BY
          CASE c.chunk_type
            WHEN 'transcript_line' THEN 0
            WHEN 'transcript' THEN 1
            WHEN 'audio' THEN 2
            WHEN 'ocr' THEN 3
            WHEN 'understanding' THEN 4
            ELSE 5
          END,
          COALESCE(c.start_sec, 9223372036854775807),
          c.id
        LIMIT ?2
        "#,
    )?;
    let rows = stmt.query_map((&pattern, limit as i64), |row| {
        let chunk_id: String = row.get(0)?;
        Ok(LegacyChunkHit {
            chunk_id,
            score: 0.01,
            exact_match: strong_exact,
            source_mask: SOURCE_TEXT | if strong_exact { SOURCE_EXACT } else { 0 },
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

async fn qdrant_vector_search(
    paths: &AppPaths,
    collection: &str,
    query_vector: &[f32],
    limit: usize,
    distance_metric: &str,
) -> anyhow::Result<Vec<RawHit>> {
    let hits =
        cerul_storage::vectors::search_collection(paths, collection, query_vector, limit).await?;
    Ok(hits
        .into_iter()
        .map(|hit| RawHit {
            chunk_id: hit.chunk_id,
            score: similarity_from_qdrant_score(hit.score, distance_metric),
            similarity_score: Some(similarity_from_qdrant_score(hit.score, distance_metric)),
            exact_match: false,
            source_mask: SOURCE_TEXT_VECTOR,
        })
        .collect())
}

async fn merge_unified_hits(
    paths: &AppPaths,
    collection: &str,
    query_vector: &[f32],
    distance_metric: &str,
    vector_hits: Vec<RawHit>,
    lexical_hits: Vec<RawHit>,
    limit: usize,
) -> anyhow::Result<Vec<RawHit>> {
    let mut candidates = HashMap::<String, RawHit>::new();
    for hit in vector_hits {
        merge_candidate_hit(&mut candidates, hit);
    }

    let lexical_only_ids = lexical_hits
        .iter()
        .filter(|hit| !candidates.contains_key(&hit.chunk_id))
        .map(|hit| hit.chunk_id.clone())
        .collect::<Vec<_>>();
    let lexical_vectors =
        cerul_storage::vectors::retrieve_collection_vectors(paths, collection, &lexical_only_ids)
            .await
            .unwrap_or_default();

    for mut hit in lexical_hits {
        if let Some(existing) = candidates.get_mut(&hit.chunk_id) {
            existing.source_mask |= hit.source_mask;
            existing.exact_match |= hit.exact_match;
            continue;
        }
        if let Some(vector) = lexical_vectors.get(&hit.chunk_id) {
            let score = vector_similarity(query_vector, vector, distance_metric);
            hit.score = score;
            hit.similarity_score = Some(score);
        } else if hit.exact_match {
            hit.score = 0.0001;
        } else {
            hit.score = 0.0;
        }
        merge_candidate_hit(&mut candidates, hit);
    }

    let mut hits = candidates.into_values().collect::<Vec<_>>();
    hits.sort_by(|left, right| {
        right
            .exact_match
            .cmp(&left.exact_match)
            .then_with(|| {
                boosted_score(right)
                    .partial_cmp(&boosted_score(left))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
    });
    hits.truncate(limit);
    Ok(hits)
}

fn merge_candidate_hit(candidates: &mut HashMap<String, RawHit>, hit: RawHit) {
    if let Some(existing) = candidates.get_mut(&hit.chunk_id) {
        existing.source_mask |= hit.source_mask;
        existing.exact_match |= hit.exact_match;
        if boosted_score(&hit) > boosted_score(existing) {
            existing.score = hit.score;
            existing.similarity_score = hit.similarity_score;
        }
        return;
    }
    candidates.insert(hit.chunk_id.clone(), hit);
}

fn boosted_score(hit: &RawHit) -> f32 {
    let lexical_boost = if hit.source_mask & SOURCE_TEXT != 0 {
        if hit.exact_match {
            0.25
        } else {
            0.03
        }
    } else {
        0.0
    };
    (hit.score + lexical_boost).clamp(0.0, 1.0)
}

fn vector_similarity(query_vector: &[f32], vector: &[f32], distance_metric: &str) -> f32 {
    if query_vector.len() != vector.len() || query_vector.is_empty() {
        return 0.0;
    }
    match distance_metric.to_ascii_lowercase().as_str() {
        "dot" | "ip" => query_vector
            .iter()
            .zip(vector)
            .map(|(left, right)| left * right)
            .sum::<f32>()
            .clamp(0.0, 1.0),
        "euclid" | "euclidean" | "l2" => {
            let distance = query_vector
                .iter()
                .zip(vector)
                .map(|(left, right)| (left - right).powi(2))
                .sum::<f32>()
                .sqrt();
            (1.0 / (1.0 + distance)).clamp(0.0, 1.0)
        }
        _ => {
            let dot = query_vector
                .iter()
                .zip(vector)
                .map(|(left, right)| left * right)
                .sum::<f32>();
            let left_norm = query_vector
                .iter()
                .map(|value| value * value)
                .sum::<f32>()
                .sqrt();
            let right_norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
            if left_norm == 0.0 || right_norm == 0.0 {
                0.0
            } else {
                (dot / (left_norm * right_norm)).clamp(0.0, 1.0)
            }
        }
    }
}

fn similarity_from_qdrant_score(score: f32, distance_metric: &str) -> f32 {
    if !score.is_finite() {
        return 0.0;
    }

    match distance_metric.to_ascii_lowercase().as_str() {
        "cosine" => score.clamp(0.0, 1.0),
        "dot" | "ip" => score.clamp(0.0, 1.0),
        "euclid" | "euclidean" | "l2" => (1.0 / (1.0 + score.max(0.0))).clamp(0.0, 1.0),
        _ => score.clamp(0.0, 1.0),
    }
}

fn hydrate(paths: &AppPaths, hits: &[RawHit], query: &str) -> anyhow::Result<Vec<SearchResult>> {
    if hits.is_empty() {
        return Ok(Vec::new());
    }
    let conn = cerul_storage::sqlite::open(paths)?;
    let units = load_units_for_hits(&conn, hits)?;
    let mut results = Vec::with_capacity(hits.len());

    for hit in hits {
        let Some(unit) = units.get(&hit.chunk_id) else {
            continue;
        };
        let (snippet, matched_field) = best_snippet(unit, query);
        let visual_sub_unit = if matched_field.is_some_and(SnippetField::prefers_visual_playback) {
            cerul_storage::best_visual_sub_unit_for_query(
                paths,
                &unit.item_id,
                unit.start_sec,
                unit.end_sec,
                query,
            )
            .ok()
            .flatten()
        } else {
            None
        };
        let spoken_sub_unit = if matched_field == Some(SnippetField::Transcript) {
            cerul_storage::best_sub_unit_for_query(
                paths,
                &unit.item_id,
                unit.start_sec,
                unit.end_sec,
                query,
            )
            .ok()
            .flatten()
            .map(|(chunk_id, start)| (chunk_id, Some(start)))
        } else {
            None
        };
        let (playback_chunk_id, start_sec) = match visual_sub_unit.or(spoken_sub_unit) {
            Some((chunk_id, start)) => (chunk_id, start.or(unit.start_sec)),
            None => (
                unit.representative_chunk_id
                    .clone()
                    .unwrap_or_else(|| unit.id.clone()),
                unit.start_sec,
            ),
        };
        let chunk_type =
            chunk_type_for_id(&conn, &playback_chunk_id)?.unwrap_or_else(|| unit.unit_kind.clone());
        let nearest_frame_chunk_id = if unit.representative_frame_path.is_some()
            && unit
                .representative_chunk_id
                .as_deref()
                .is_some_and(|id| id.contains(":keyframe:") || id.contains(":image:"))
        {
            unit.representative_chunk_id.clone()
        } else {
            None
        };

        results.push(SearchResult {
            chunk_id: playback_chunk_id,
            item_id: unit.item_id.clone(),
            chunk_type,
            start_sec,
            end_sec: unit.end_sec,
            snippet,
            frame_path: None,
            match_score: 0.0,
            score: hit.score,
            similarity_score: hit.similarity_score,
            exact_match: hit.exact_match,
            source_mask: hit.source_mask,
            item_title: unit.item_title.clone(),
            nearest_frame_chunk_id,
        });
    }

    attach_nearest_frame_chunk_ids(&conn, &mut results)?;
    Ok(results)
}

fn hydrate_legacy_chunks(
    paths: &AppPaths,
    hits: &[LegacyChunkHit],
) -> anyhow::Result<Vec<SearchResult>> {
    if hits.is_empty() {
        return Ok(Vec::new());
    }
    let conn = cerul_storage::sqlite::open(paths)?;
    let chunk_ids = hits
        .iter()
        .map(|hit| hit.chunk_id.clone())
        .collect::<Vec<_>>();
    let placeholders = std::iter::repeat("?")
        .take(chunk_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        r#"
        SELECT c.id, c.item_id, c.chunk_type, c.start_sec, c.end_sec, c.text, c.frame_path, i.title
        FROM chunks c
        JOIN items i ON i.id = c.item_id
        WHERE c.id IN ({placeholders})
          AND i.status != 'deleting'
        "#
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(chunk_ids.iter()), |row| {
        let chunk_id = row.get::<_, String>(0)?;
        let item_id = row.get(1)?;
        let chunk_type = row.get::<_, String>(2)?;
        let start_sec = row.get(3)?;
        let end_sec = row.get(4)?;
        let snippet = row
            .get::<_, Option<String>>(5)?
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| fallback_snippet(&chunk_type, start_sec));
        let frame_path = row.get(6)?;
        let item_title = row.get(7)?;
        Ok((
            chunk_id.clone(),
            SearchResult {
                chunk_id,
                item_id,
                chunk_type,
                start_sec,
                end_sec,
                snippet,
                frame_path,
                match_score: 0.0,
                score: 0.0,
                similarity_score: None,
                exact_match: false,
                source_mask: SOURCE_TEXT,
                item_title,
                nearest_frame_chunk_id: None,
            },
        ))
    })?;

    let mut by_id = HashMap::with_capacity(chunk_ids.len());
    for row in rows {
        let (chunk_id, result) = row?;
        by_id.insert(chunk_id, result);
    }

    let mut results = Vec::with_capacity(hits.len());
    for hit in hits {
        let Some(mut result) = by_id.remove(&hit.chunk_id) else {
            continue;
        };
        result.score = hit.score;
        result.exact_match = hit.exact_match;
        result.source_mask = hit.source_mask;
        if result.frame_path.is_some() {
            result.nearest_frame_chunk_id = Some(result.chunk_id.clone());
        }
        results.push(result);
    }

    attach_nearest_frame_chunk_ids(&conn, &mut results)?;
    Ok(results)
}

fn finalize_results(results: Vec<SearchResult>, limit: usize) -> Vec<SearchResult> {
    let mut results = dedupe_results(results, limit);
    apply_match_scores(&mut results);
    results
}

fn apply_match_scores(results: &mut [SearchResult]) {
    for result in results {
        result.match_score =
            unified_match_score(result.score, result.exact_match, result.source_mask);
    }
}

fn unified_match_score(score: f32, exact_match: bool, source_mask: u8) -> f32 {
    if exact_match {
        return score.max(0.92).min(0.98);
    }
    let lexical_boost = if source_mask & SOURCE_TEXT != 0 {
        0.03
    } else {
        0.0
    };
    (score + lexical_boost).clamp(0.0, 0.91)
}

#[derive(Debug, Clone)]
struct HydratedUnit {
    id: String,
    item_id: String,
    unit_kind: String,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
    content_text: String,
    transcript_text: Option<String>,
    ocr_text: Option<String>,
    visual_text: Option<String>,
    summary_text: Option<String>,
    representative_chunk_id: Option<String>,
    representative_frame_path: Option<String>,
    item_title: Option<String>,
}

fn load_units_for_hits(
    conn: &rusqlite::Connection,
    hits: &[RawHit],
) -> anyhow::Result<HashMap<String, HydratedUnit>> {
    let mut seen = HashSet::new();
    let unit_ids = hits
        .iter()
        .filter_map(|hit| {
            if seen.insert(hit.chunk_id.as_str()) {
                Some(hit.chunk_id.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if unit_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let placeholders = std::iter::repeat_n("?", unit_ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        r#"
        SELECT
            u.id,
            u.item_id,
            u.unit_kind,
            u.start_sec,
            u.end_sec,
            u.content_text,
            u.transcript_text,
            u.ocr_text,
            u.visual_text,
            u.summary_text,
            u.representative_chunk_id,
            u.representative_frame_path,
            i.title
        FROM retrieval_units u
        JOIN items i ON i.id = u.item_id
        WHERE u.id IN ({placeholders})
          AND i.status != 'deleting'
        "#,
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(unit_ids.iter()), |row| {
        Ok(HydratedUnit {
            id: row.get(0)?,
            item_id: row.get(1)?,
            unit_kind: row.get(2)?,
            start_sec: row.get(3)?,
            end_sec: row.get(4)?,
            content_text: row.get(5)?,
            transcript_text: row.get(6)?,
            ocr_text: row.get(7)?,
            visual_text: row.get(8)?,
            summary_text: row.get(9)?,
            representative_chunk_id: row.get(10)?,
            representative_frame_path: row.get(11)?,
            item_title: row.get(12)?,
        })
    })?;

    let mut units = HashMap::with_capacity(unit_ids.len());
    for unit in rows {
        let unit = unit?;
        units.insert(unit.id.clone(), unit);
    }
    Ok(units)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SnippetField {
    Transcript,
    Ocr,
    Visual,
    Summary,
    Content,
}

impl SnippetField {
    fn prefers_visual_playback(self) -> bool {
        matches!(self, Self::Ocr | Self::Visual)
    }

    fn snippet_tiebreak_priority(self) -> usize {
        match self {
            Self::Ocr => 4,
            Self::Visual => 3,
            Self::Summary => 2,
            Self::Transcript => 1,
            Self::Content => 0,
        }
    }
}

fn best_snippet(unit: &HydratedUnit, query: &str) -> (String, Option<SnippetField>) {
    let structured_fields = [
        (SnippetField::Transcript, unit.transcript_text.as_deref()),
        (SnippetField::Ocr, unit.ocr_text.as_deref()),
        (SnippetField::Visual, unit.visual_text.as_deref()),
        (SnippetField::Summary, unit.summary_text.as_deref()),
    ];

    let pattern = literal_pattern_for_terms(query);
    let terms = literal_terms_for_query(query);
    let mut best_match = None::<(SnippetField, &str, usize)>;
    for (field, text) in structured_fields.iter().copied() {
        let Some(text) = text else {
            continue;
        };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        let score = query_text_score(trimmed, pattern.as_deref(), &terms);
        if score > 0
            && best_match
                .as_ref()
                .is_none_or(|(best_field, _, best_score)| {
                    score > *best_score
                        || (score == *best_score
                            && terms.len() > 1
                            && field.snippet_tiebreak_priority()
                                > best_field.snippet_tiebreak_priority())
                })
        {
            best_match = Some((field, trimmed, score));
        }
    }

    if let Some((field, text, _)) = best_match {
        return (text.chars().take(320).collect(), Some(field));
    }

    let content = unit.content_text.trim();
    if !content.is_empty() {
        let score = query_text_score(content, pattern.as_deref(), &terms);
        if score > 0 {
            return (
                content.chars().take(320).collect(),
                Some(SnippetField::Content),
            );
        }
    }

    for (_, text) in structured_fields.iter().copied() {
        let Some(text) = text else {
            continue;
        };
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return (trimmed.chars().take(320).collect(), None);
        }
    }
    if !unit.content_text.trim().is_empty() {
        return (unit.content_text.trim().chars().take(320).collect(), None);
    }
    (fallback_snippet(&unit.unit_kind, unit.start_sec), None)
}

fn literal_pattern_for_terms(query: &str) -> Option<String> {
    let trimmed = query.trim().trim_matches('"').to_lowercase();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn literal_terms_for_query(query: &str) -> Vec<String> {
    query
        .trim()
        .trim_matches('"')
        .to_lowercase()
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn query_text_score(text: &str, pattern: Option<&str>, terms: &[String]) -> usize {
    let normalized = text.to_lowercase();
    let term_weight_sum = terms
        .iter()
        .map(|term| query_term_weight(term))
        .sum::<usize>();
    let exact_score = pattern
        .filter(|pattern| normalized.contains(*pattern))
        .map_or(0, |_| term_weight_sum.max(1) + 1);
    let term_score = terms
        .iter()
        .filter(|term| normalized.contains(term.as_str()))
        .map(|term| query_term_weight(term))
        .sum();
    exact_score.max(term_score)
}

fn query_term_weight(term: &str) -> usize {
    if term.chars().any(|ch| ch.is_ascii_digit()) {
        4
    } else if term.chars().any(|ch| !ch.is_alphanumeric()) {
        3
    } else {
        1
    }
}

fn chunk_type_for_id(
    conn: &rusqlite::Connection,
    chunk_id: &str,
) -> anyhow::Result<Option<String>> {
    conn.query_row(
        "SELECT chunk_type FROM chunks WHERE id = ?1",
        [chunk_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}

fn attach_nearest_frame_chunk_ids(
    conn: &rusqlite::Connection,
    results: &mut [SearchResult],
) -> anyhow::Result<()> {
    let mut seen = HashSet::new();
    let item_ids = results
        .iter()
        .filter(|result| result.frame_path.is_none() && result.start_sec.is_some())
        .filter_map(|result| {
            if seen.insert(result.item_id.as_str()) {
                Some(result.item_id.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if item_ids.is_empty() {
        return Ok(());
    }

    let frames_by_item = load_frame_chunks_by_item(conn, &item_ids)?;
    for result in results {
        let Some(target) = result.start_sec else {
            continue;
        };
        if result.frame_path.is_some() {
            continue;
        }
        let Some(frames) = frames_by_item.get(&result.item_id) else {
            continue;
        };
        result.nearest_frame_chunk_id = frames
            .iter()
            .min_by(|left, right| {
                (left.0 - target)
                    .abs()
                    .partial_cmp(&(right.0 - target).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(_, chunk_id)| chunk_id.clone());
    }
    Ok(())
}

fn load_frame_chunks_by_item(
    conn: &rusqlite::Connection,
    item_ids: &[String],
) -> anyhow::Result<HashMap<String, Vec<(f64, String)>>> {
    if item_ids.is_empty() {
        return Ok(HashMap::new());
    };
    let placeholders = std::iter::repeat_n("?", item_ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        r#"
        SELECT c.item_id, c.start_sec, c.id
        FROM chunks c
        JOIN items i ON i.id = c.item_id
        WHERE c.item_id IN ({placeholders})
          AND i.status != 'deleting'
          AND c.chunk_type IN ('keyframe', 'image')
          AND c.frame_path IS NOT NULL
          AND c.start_sec IS NOT NULL
        "#,
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(item_ids.iter()), |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, f64>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;

    let mut frames_by_item: HashMap<String, Vec<(f64, String)>> = HashMap::new();
    for row in rows {
        let (item_id, start_sec, chunk_id) = row?;
        frames_by_item
            .entry(item_id)
            .or_default()
            .push((start_sec, chunk_id));
    }
    Ok(frames_by_item)
}

fn fallback_snippet(chunk_type: &str, start_sec: Option<f64>) -> String {
    let timestamp = start_sec.map(format_timestamp);
    match (chunk_type, timestamp) {
        ("keyframe" | "image" | "ocr", Some(timestamp)) => {
            format!("Visual frame at {timestamp}")
        }
        ("keyframe" | "image" | "ocr", None) => "Visual match".to_string(),
        ("understanding", Some(timestamp)) => format!("Video understanding at {timestamp}"),
        ("understanding", None) => "Video understanding match".to_string(),
        (_, Some(timestamp)) => format!("Search match at {timestamp}"),
        _ => "Search match".to_string(),
    }
}

fn format_timestamp(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

fn dedupe_results(results: Vec<SearchResult>, limit: usize) -> Vec<SearchResult> {
    let mut kept = Vec::with_capacity(limit.min(results.len()));
    let mut per_item_counts = HashMap::<String, usize>::new();

    for result in results {
        if per_item_counts
            .get(&result.item_id)
            .copied()
            .unwrap_or_default()
            >= PER_ITEM_CAP
        {
            continue;
        }
        if kept
            .iter()
            .any(|existing| is_near_duplicate(existing, &result))
        {
            continue;
        }
        *per_item_counts.entry(result.item_id.clone()).or_default() += 1;
        kept.push(result);
        if kept.len() >= limit {
            break;
        }
    }

    kept
}

fn is_near_duplicate(left: &SearchResult, right: &SearchResult) -> bool {
    if left.item_id != right.item_id {
        return false;
    }
    if left.chunk_id == right.chunk_id {
        return true;
    }
    match (left.start_sec, right.start_sec) {
        (Some(left_start), Some(right_start)) => (left_start - right_start).abs() < 30.0,
        _ => left.chunk_type == right.chunk_type,
    }
}

fn fts_match_query(query: &str) -> Option<String> {
    let terms = query
        .split_whitespace()
        .map(|term| {
            term.trim_matches(|ch: char| {
                !ch.is_alphanumeric() && ch != '_' && ch != '-' && ch != '"'
            })
        })
        .filter(|term| !term.is_empty())
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>();

    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

fn sqlite_like_pattern(query: &str) -> Option<String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut escaped = String::with_capacity(trimmed.len() + 2);
    escaped.push('%');
    for ch in trimmed.chars() {
        match ch {
            '%' | '_' | '\\' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }
    escaped.push('%');
    Some(escaped)
}

fn strong_exact_intent(query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.len() < 2 {
        return false;
    }
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() > 2 {
        return true;
    }
    let alnum_chars = trimmed.chars().filter(|ch| ch.is_alphanumeric()).count();
    let has_model_like_char = trimmed
        .chars()
        .any(|ch| matches!(ch, '-' | '_' | '/' | '.' | '#') || ch.is_ascii_digit());
    let has_cjk = trimmed
        .chars()
        .any(|ch| matches!(ch as u32, 0x4E00..=0x9FFF));
    let token_count = trimmed
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .count();

    has_model_like_char || alnum_chars >= 8 || (has_cjk && alnum_chars >= 4) || token_count >= 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use cerul_storage::vectors::{EmbeddingProfile, VectorRecord};
    use cerul_storage::{sqlite, StorageImageChunk, StorageRetrievalUnit, StorageTranscriptChunk};
    use std::time::{Duration, Instant};

    #[test]
    fn dedupe_results_collapses_adjacent_item_hits() {
        let results = vec![
            result("chunk-a", "item-1", "transcript", Some(10.0), 0.04),
            result("chunk-b", "item-1", "understanding", Some(25.0), 0.03),
            result("chunk-c", "item-1", "transcript", Some(90.0), 0.02),
            result("chunk-d", "item-2", "transcript", Some(12.0), 0.01),
        ];

        let deduped = dedupe_results(results, 10);

        assert_eq!(
            deduped
                .iter()
                .map(|hit| hit.chunk_id.as_str())
                .collect::<Vec<_>>(),
            vec!["chunk-a", "chunk-c", "chunk-d"]
        );
    }

    #[test]
    fn match_score_uses_unified_scores_and_exact_floor() {
        let mut semantic = result("chunk-a", "item-1", "moment", Some(10.0), 0.97);
        semantic.source_mask = SOURCE_TEXT_VECTOR;
        let mut lexical = result("chunk-b", "item-2", "moment", Some(20.0), 0.40);
        lexical.source_mask = SOURCE_TEXT;
        let mut exact = result("chunk-c", "item-3", "moment", Some(30.0), 0.01);
        exact.source_mask = SOURCE_TEXT | SOURCE_EXACT;
        exact.exact_match = true;

        let scored = finalize_results(vec![semantic, lexical, exact], 10);

        assert!((scored[0].match_score - 0.91).abs() < 0.001);
        assert!((scored[1].match_score - 0.43).abs() < 0.001);
        assert!((scored[2].match_score - 0.92).abs() < 0.001);
    }

    #[test]
    fn merge_candidate_hit_keeps_best_duplicate_vector_score() {
        let mut candidates = HashMap::new();
        merge_candidate_hit(
            &mut candidates,
            RawHit {
                chunk_id: "item-1:unit:v2:000000".to_string(),
                score: 0.82,
                similarity_score: Some(0.82),
                exact_match: false,
                source_mask: SOURCE_TEXT_VECTOR,
            },
        );
        merge_candidate_hit(
            &mut candidates,
            RawHit {
                chunk_id: "item-1:unit:v2:000000".to_string(),
                score: 0.41,
                similarity_score: Some(0.41),
                exact_match: false,
                source_mask: SOURCE_TEXT_VECTOR,
            },
        );

        let hit = candidates.get("item-1:unit:v2:000000").unwrap();
        assert_eq!(hit.score, 0.82);
        assert_eq!(hit.similarity_score, Some(0.82));
    }

    #[test]
    fn strong_exact_intent_is_bounded() {
        assert!(strong_exact_intent("\"short\""));
        assert!(strong_exact_intent("PX-1000"));
        assert!(strong_exact_intent("地下室光源"));
        assert!(!strong_exact_intent("地下室"));
        assert!(!strong_exact_intent("ai"));
    }

    #[tokio::test]
    async fn end_to_end() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        insert_item(&paths);
        let chunks = vec![
            StorageTranscriptChunk {
                start: 12.0,
                end: 30.0,
                text: "needle phrase appears here".to_string(),
            },
            StorageTranscriptChunk {
                start: 40.0,
                end: 60.0,
                text: "unrelated transcript text".to_string(),
            },
        ];
        index_video_units(&paths, "item-1", &chunks, |unit| {
            if unit.content_text.contains("needle phrase") {
                fake_vector(0)
            } else {
                fake_vector(1)
            }
        })
        .await;

        let results = search_with_vector(
            &paths,
            SearchRequest {
                q: "needle phrase".to_string(),
                limit: 2,
            },
            fake_vector(0),
        )
        .await
        .unwrap();

        assert_eq!(
            results.first().unwrap().chunk_id,
            "item-1:transcript:000000"
        );
        assert_eq!(results.first().unwrap().start_sec, Some(12.0));
        assert!(results.first().unwrap().snippet.contains("needle phrase"));
    }

    #[test]
    fn search_hydration_excludes_deleting_items() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        insert_item(&paths);
        let conn = sqlite::open(&paths).unwrap();
        conn.execute(
            r#"
            INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, text, metadata)
            VALUES ('item-1:transcript:000000', 'item-1', 'transcript', 4, 9, 'deleted item should not appear in search', '{}')
            "#,
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE items SET status = 'deleting' WHERE id = 'item-1'",
            [],
        )
        .unwrap();
        let profile = cerul_storage::vectors::ensure_active_embedding_profile(&paths).unwrap();
        let units = vec![manual_unit(
            "item-1:unit:v2:000000",
            "item-1",
            0,
            Some(4.0),
            Some(9.0),
            "deleted item should not appear in search",
            Some("item-1:transcript:000000"),
            &profile,
        )];
        cerul_storage::replace_item_retrieval_units(&paths, "item-1", &units).unwrap();

        let results = hydrate(
            &paths,
            &[RawHit {
                chunk_id: "item-1:unit:v2:000000".to_string(),
                score: 1.0,
                similarity_score: Some(1.0),
                exact_match: false,
                source_mask: SOURCE_TEXT,
            }],
            "deleted",
        )
        .unwrap();

        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn search_with_paths_falls_back_to_fts_when_embedder_is_not_initialized() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        insert_item(&paths);
        let chunks = vec![StorageTranscriptChunk {
            start: 4.0,
            end: 9.0,
            text: "fallback search should still find exact transcript text".to_string(),
        }];
        write_video_chunks_and_units(&paths, "item-1", &chunks);

        let results = search_with_paths(
            &paths,
            SearchRequest {
                q: "fallback transcript".to_string(),
                limit: 5,
            },
        )
        .await
        .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk_id, "item-1:transcript:000000");
        assert!(results[0].snippet.contains("fallback search"));
    }

    #[tokio::test]
    async fn fts_fallback_reports_search_diagnostics() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        insert_item(&paths);
        let chunks = vec![StorageTranscriptChunk {
            start: 4.0,
            end: 9.0,
            text: "fallback search should still report diagnostics".to_string(),
        }];
        write_video_chunks_and_units(&paths, "item-1", &chunks);

        let response = search_fts_only_with_diagnostics(
            &paths,
            SearchRequest {
                q: "fallback diagnostics".to_string(),
                limit: 5,
            },
            Some("query_embedding_failed".to_string()),
        )
        .await
        .unwrap();

        assert_eq!(response.diagnostics.retrieval_mode, "fts_fallback");
        assert_eq!(
            response.diagnostics.fallback_reason.as_deref(),
            Some("query_embedding_failed")
        );
        assert_eq!(response.diagnostics.vector_hits_count, 0);
        assert!(response.diagnostics.fts_hits_count >= 1);
        assert_eq!(response.results[0].chunk_id, "item-1:transcript:000000");
    }

    #[tokio::test]
    async fn hydrate_preserves_hit_order_with_batch_query() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        insert_item(&paths);
        let profile = cerul_storage::vectors::ensure_active_embedding_profile(&paths).unwrap();
        let units = vec![
            manual_unit(
                "item-1:unit:v2:000000",
                "item-1",
                0,
                Some(1.0),
                Some(2.0),
                "first chunk",
                Some("item-1:transcript:000000"),
                &profile,
            ),
            manual_unit(
                "item-1:unit:v2:000001",
                "item-1",
                1,
                Some(3.0),
                Some(4.0),
                "second chunk",
                Some("item-1:transcript:000001"),
                &profile,
            ),
        ];
        cerul_storage::replace_item_retrieval_units(&paths, "item-1", &units).unwrap();

        let results = hydrate(
            &paths,
            &[
                RawHit {
                    chunk_id: "item-1:unit:v2:000001".to_string(),
                    score: 0.9,
                    similarity_score: Some(0.9),
                    exact_match: false,
                    source_mask: SOURCE_TEXT,
                },
                RawHit {
                    chunk_id: "item-1:unit:v2:000000".to_string(),
                    score: 0.8,
                    similarity_score: Some(0.8),
                    exact_match: false,
                    source_mask: SOURCE_TEXT,
                },
            ],
            "chunk",
        )
        .unwrap();

        assert_eq!(
            results
                .iter()
                .map(|result| result.chunk_id.as_str())
                .collect::<Vec<_>>(),
            vec!["item-1:transcript:000001", "item-1:transcript:000000"]
        );
        assert_eq!(results[0].score, 0.9);
        assert_eq!(results[1].score, 0.8);
    }

    #[test]
    fn hydrate_preserves_playback_chunk_type() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        insert_item(&paths);
        let conn = sqlite::open(&paths).unwrap();
        conn.execute(
            r#"
            INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, text, metadata)
            VALUES ('item-1:audio:000000', 'item-1', 'audio', 12, 18, 'matched spoken audio', '{}')
            "#,
            [],
        )
        .unwrap();
        let profile = cerul_storage::vectors::ensure_active_embedding_profile(&paths).unwrap();
        let units = vec![manual_unit(
            "item-1:unit:v2:000000",
            "item-1",
            0,
            Some(12.0),
            Some(18.0),
            "matched spoken audio",
            Some("item-1:audio:000000"),
            &profile,
        )];
        cerul_storage::replace_item_retrieval_units(&paths, "item-1", &units).unwrap();

        let results = hydrate(
            &paths,
            &[RawHit {
                chunk_id: "item-1:unit:v2:000000".to_string(),
                score: 1.0,
                similarity_score: Some(1.0),
                exact_match: false,
                source_mask: SOURCE_TEXT,
            }],
            "spoken",
        )
        .unwrap();

        assert_eq!(results[0].chunk_id, "item-1:audio:000000");
        assert_eq!(results[0].chunk_type, "audio");
    }

    #[test]
    fn hydrate_uses_snippet_from_matched_unit_field() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        insert_item(&paths);
        let conn = sqlite::open(&paths).unwrap();
        conn.execute(
            r#"
            INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, text, metadata)
            VALUES ('item-1:transcript:000000', 'item-1', 'transcript', 20, 40, 'spoken text that does not include the visible code', '{}')
            "#,
            [],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO chunks (id, item_id, chunk_type, text, frame_path, metadata)
            VALUES ('item-1:ocr:000000', 'item-1', 'ocr', 'checkout display shows XR-42', '/tmp/frame.jpg', '{}')
            "#,
            [],
        )
        .unwrap();
        let profile = cerul_storage::vectors::ensure_active_embedding_profile(&paths).unwrap();
        let mut unit = manual_unit(
            "item-1:unit:v2:000000",
            "item-1",
            0,
            Some(20.0),
            Some(40.0),
            "spoken text that does not include the visible code",
            Some("item-1:transcript:000000"),
            &profile,
        );
        unit.ocr_text = Some("checkout display shows XR-42".to_string());
        unit.content_text =
            "Transcript: spoken text that does not include the visible code\nOn-screen text: checkout display shows XR-42".to_string();
        cerul_storage::replace_item_retrieval_units(&paths, "item-1", &[unit]).unwrap();

        let results = hydrate(
            &paths,
            &[RawHit {
                chunk_id: "item-1:unit:v2:000000".to_string(),
                score: 1.0,
                similarity_score: None,
                exact_match: false,
                source_mask: SOURCE_TEXT,
            }],
            "XR-42",
        )
        .unwrap();

        assert_eq!(results[0].snippet, "checkout display shows XR-42");
        assert_eq!(results[0].chunk_id, "item-1:ocr:000000");
        assert_eq!(results[0].chunk_type, "ocr");
    }

    #[test]
    fn hydrate_prefers_visual_partial_match_and_uses_ocr_frame_time() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        insert_item(&paths);
        let frame_path = temp
            .path()
            .join("frame_000034.jpg")
            .to_string_lossy()
            .into_owned();
        let conn = sqlite::open(&paths).unwrap();
        conn.execute(
            r#"
            INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, text, metadata)
            VALUES ('item-1:transcript:000000', 'item-1', 'transcript', 20, 50, 'checkout flow narration only', '{}')
            "#,
            [],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, frame_path, metadata)
            VALUES ('item-1:keyframe:000034', 'item-1', 'keyframe', 34, 44, ?1, '{}')
            "#,
            [frame_path.as_str()],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO chunks (id, item_id, chunk_type, text, frame_path, metadata)
            VALUES ('item-1:ocr:000034', 'item-1', 'ocr', 'XR-42 appears on the display', ?1, '{}')
            "#,
            [frame_path.as_str()],
        )
        .unwrap();
        let profile = cerul_storage::vectors::ensure_active_embedding_profile(&paths).unwrap();
        let mut unit = manual_unit(
            "item-1:unit:v2:000000",
            "item-1",
            0,
            Some(20.0),
            Some(50.0),
            "checkout flow narration only",
            Some("item-1:transcript:000000"),
            &profile,
        );
        unit.ocr_text = Some("XR-42 appears on the display".to_string());
        unit.content_text = "Transcript: checkout flow narration only\nOn-screen text: XR-42 appears on the display".to_string();
        cerul_storage::replace_item_retrieval_units(&paths, "item-1", &[unit]).unwrap();

        let results = hydrate(
            &paths,
            &[RawHit {
                chunk_id: "item-1:unit:v2:000000".to_string(),
                score: 1.0,
                similarity_score: None,
                exact_match: false,
                source_mask: SOURCE_TEXT,
            }],
            "checkout XR-42",
        )
        .unwrap();

        assert_eq!(results[0].snippet, "XR-42 appears on the display");
        assert_eq!(results[0].chunk_id, "item-1:ocr:000034");
        assert_eq!(results[0].chunk_type, "ocr");
        assert_eq!(results[0].start_sec, Some(34.0));
        assert_eq!(
            results[0].nearest_frame_chunk_id.as_deref(),
            Some("item-1:keyframe:000034")
        );
    }

    #[tokio::test]
    async fn search_with_paths_falls_back_to_literal_chinese_text() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        insert_item(&paths);
        let chunks = vec![StorageTranscriptChunk {
            start: 7.0,
            end: 12.0,
            text: "所有光源只能出现在地下室".to_string(),
        }];
        write_video_chunks_and_units(&paths, "item-1", &chunks);

        assert!(sqlite_fts_search(&paths, "地下室", 5)
            .await
            .unwrap()
            .is_empty());

        let results = search_with_paths(
            &paths,
            SearchRequest {
                q: "地下室".to_string(),
                limit: 5,
            },
        )
        .await
        .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk_id, "item-1:transcript:000000");
        assert!(results[0].snippet.contains("地下室"));
    }

    #[tokio::test]
    async fn fts_fallback_uses_legacy_chunks_while_units_rebuild() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        insert_item(&paths);
        let chunks = vec![StorageTranscriptChunk {
            start: 11.0,
            end: 19.0,
            text: "legacy transcript survives retrieval rebuild".to_string(),
        }];
        cerul_storage::write_media_sqlite_chunks_with_ocr_and_lines(
            &paths,
            "item-1",
            &chunks,
            &[],
            &[],
            &[],
        )
        .unwrap();

        let response = search_fts_only_with_diagnostics(
            &paths,
            SearchRequest {
                q: "legacy transcript".to_string(),
                limit: 5,
            },
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            response.diagnostics.fallback_reason.as_deref(),
            Some("search_index_rebuilding_legacy_fts")
        );
        assert_eq!(response.results[0].chunk_id, "item-1:transcript:000000");
        assert!(response.results[0].snippet.contains("legacy transcript"));
    }

    #[tokio::test]
    async fn fts_fallback_appends_legacy_chunks_during_partial_rebuild() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        insert_item(&paths);
        insert_item_with_type(&paths, "item-2", "video", "folder_video", "Legacy item");
        let profile = cerul_storage::vectors::ensure_active_embedding_profile(&paths).unwrap();
        let units = vec![manual_unit(
            "item-1:unit:v2:000000",
            "item-1",
            0,
            Some(2.0),
            Some(8.0),
            "shared phrase rebuilt unit",
            None,
            &profile,
        )];
        cerul_storage::replace_item_retrieval_units(&paths, "item-1", &units).unwrap();
        cerul_storage::write_media_sqlite_chunks_with_ocr_and_lines(
            &paths,
            "item-2",
            &[StorageTranscriptChunk {
                start: 30.0,
                end: 40.0,
                text: "shared phrase legacy chunk".to_string(),
            }],
            &[],
            &[],
            &[],
        )
        .unwrap();

        let response = search_fts_only_with_diagnostics(
            &paths,
            SearchRequest {
                q: "shared phrase".to_string(),
                limit: 5,
            },
            None,
        )
        .await
        .unwrap();
        let item_ids = response
            .results
            .iter()
            .map(|result| result.item_id.as_str())
            .collect::<Vec<_>>();

        assert!(item_ids.contains(&"item-1"));
        assert!(item_ids.contains(&"item-2"));
        assert_eq!(
            response.diagnostics.fallback_reason.as_deref(),
            Some("search_index_rebuilding_legacy_fts")
        );
    }

    #[tokio::test]
    async fn sqlite_text_search_does_not_filter_by_active_embedding_profile() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        insert_item(&paths);
        let profile = cerul_storage::vectors::ensure_active_embedding_profile(&paths).unwrap();
        let mut unit = manual_unit(
            "item-1:unit:v2:000000",
            "item-1",
            0,
            Some(2.0),
            Some(8.0),
            "old profile lexical match",
            None,
            &profile,
        );
        unit.embedding_profile_id = "old-profile-before-switch".to_string();
        cerul_storage::replace_item_retrieval_units(&paths, "item-1", &[unit]).unwrap();

        let hits = sqlite_text_search(&paths, "old profile", 5).await.unwrap();

        assert_eq!(
            hits.first().map(|hit| hit.chunk_id.as_str()),
            Some("item-1:unit:v2:000000")
        );
    }

    #[tokio::test]
    async fn search_with_vectors_uses_unified_query_vector() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        insert_item_with_type(&paths, "item-1", "image", "folder_image", "Sample image");
        let frames = temp.path().join("frames");
        std::fs::create_dir(&frames).unwrap();

        index_image_units(
            &paths,
            "item-1",
            &[
                StorageImageChunk::keyframe(frames.join("red.jpg")),
                StorageImageChunk::keyframe(frames.join("blue.jpg")),
            ],
            |unit| {
                if unit.representative_chunk_id.as_deref() == Some("item-1:keyframe:000001") {
                    fake_vector(21)
                } else {
                    fake_vector(20)
                }
            },
        )
        .await;

        let results = search_with_vectors(
            &paths,
            SearchRequest {
                q: "not in transcript".to_string(),
                limit: 1,
            },
            fake_vector(21),
            fake_vector(20),
        )
        .await
        .unwrap();

        let first = results.first().unwrap();
        assert_eq!(first.chunk_id, "item-1:keyframe:000001");
        assert_eq!(
            first.nearest_frame_chunk_id.as_deref(),
            Some("item-1:keyframe:000001")
        );
        assert_eq!(first.frame_path, None);
    }

    #[tokio::test]
    async fn vector_search_reports_search_diagnostics() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        insert_item_with_type(&paths, "item-1", "image", "folder_image", "Sample image");
        let frames = temp.path().join("frames");
        std::fs::create_dir(&frames).unwrap();

        index_image_units(
            &paths,
            "item-1",
            &[
                StorageImageChunk::keyframe(frames.join("red.jpg")),
                StorageImageChunk::keyframe(frames.join("blue.jpg")),
            ],
            |unit| {
                if unit.representative_chunk_id.as_deref() == Some("item-1:keyframe:000001") {
                    fake_vector(21)
                } else {
                    fake_vector(20)
                }
            },
        )
        .await;

        let response = search_with_vectors_diagnostics(
            &paths,
            SearchRequest {
                q: "not in transcript".to_string(),
                limit: 1,
            },
            fake_vector(21),
            fake_vector(0),
        )
        .await
        .unwrap();

        assert_eq!(response.diagnostics.retrieval_mode, "unified_vector");
        assert!(response.diagnostics.vector_hits_count >= 1);
        assert_eq!(response.diagnostics.fts_hits_count, 0);
        assert!(response.diagnostics.embedding_profile_id.is_some());
        assert!(response
            .diagnostics
            .qdrant_collection
            .as_deref()
            .unwrap()
            .contains("retrieval_units"));
        assert_eq!(response.diagnostics.qdrant_text_collection, None);
        assert_eq!(response.diagnostics.qdrant_image_points, Some(0));
        assert_eq!(response.results[0].chunk_id, "item-1:keyframe:000001");
    }

    #[tokio::test]
    async fn vector_search_uses_profile_distance_metric() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        insert_item_with_type(&paths, "item-1", "image", "folder_image", "Sample image");
        let frames = temp.path().join("frames");
        std::fs::create_dir(&frames).unwrap();

        index_image_units(
            &paths,
            "item-1",
            &[
                StorageImageChunk::keyframe(frames.join("same-direction.jpg")),
                StorageImageChunk::keyframe(frames.join("orthogonal.jpg")),
            ],
            |unit| {
                if unit.representative_chunk_id.as_deref() == Some("item-1:keyframe:000000") {
                    scaled_vector(0, 10.0)
                } else {
                    fake_vector(1)
                }
            },
        )
        .await;

        let results = search_with_vectors(
            &paths,
            SearchRequest {
                q: "not in transcript".to_string(),
                limit: 1,
            },
            fake_vector(0),
            fake_vector(0),
        )
        .await
        .unwrap();

        let first = results.first().unwrap();
        assert_eq!(first.chunk_id, "item-1:keyframe:000000");
        assert!(first.similarity_score.unwrap() > 0.99);
        assert_eq!(
            first.nearest_frame_chunk_id.as_deref(),
            Some("item-1:keyframe:000000")
        );
    }

    #[tokio::test]
    #[ignore = "release smoke; run scripts/smoke-search-latency.sh"]
    async fn search_latency_smoke() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        seed_latency_library(&paths, 100, 20).await;

        let mut timings = Vec::new();
        for topic in 0..20 {
            let query = format!("latency-topic-{topic:02}");
            let start = Instant::now();
            let results = search_with_vector(
                &paths,
                SearchRequest {
                    q: query.clone(),
                    limit: 5,
                },
                fake_vector(topic),
            )
            .await
            .unwrap();
            let elapsed = start.elapsed();

            assert!(
                !results.is_empty(),
                "expected results for latency smoke query {query}"
            );
            assert!(
                results.iter().any(|result| result.snippet.contains(&query)),
                "expected at least one snippet containing {query}, got {results:?}"
            );
            timings.push(elapsed);
        }

        timings.sort();
        let p50 = percentile(&timings, 50);
        let p99 = percentile(&timings, 99);
        let p50_limit = Duration::from_millis(30);
        let p99_limit = Duration::from_millis(100);

        println!(
            "search_latency_smoke p50={}ms p99={}ms queries={} items=100",
            p50.as_secs_f64() * 1000.0,
            p99.as_secs_f64() * 1000.0,
            timings.len()
        );

        assert!(
            p50 < p50_limit,
            "search p50 {:?} exceeded {:?}",
            p50,
            p50_limit
        );
        assert!(
            p99 < p99_limit,
            "search p99 {:?} exceeded {:?}",
            p99,
            p99_limit
        );
    }

    fn insert_item(paths: &AppPaths) {
        insert_item_with_type(paths, "item-1", "video", "folder_video", "Sample");
    }

    fn insert_item_with_type(
        paths: &AppPaths,
        item_id: &str,
        content_type: &str,
        source_type: &str,
        title: &str,
    ) {
        let conn = sqlite::open(paths).unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO sources (id, type, config, status) VALUES ('source-1', ?1, '{}', 'active')",
            [source_type],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO items (id, source_id, content_type, status, title) VALUES (?1, 'source-1', ?2, 'indexed', ?3)",
            (item_id, content_type, title),
        )
        .unwrap();
    }

    fn write_video_chunks_and_units(
        paths: &AppPaths,
        item_id: &str,
        chunks: &[StorageTranscriptChunk],
    ) -> Vec<StorageRetrievalUnit> {
        cerul_storage::write_media_sqlite_chunks_with_ocr_and_lines(
            paths,
            item_id,
            chunks,
            &[],
            &[],
            &[],
        )
        .unwrap();
        rebuild_units(paths, item_id).1
    }

    async fn index_video_units<F>(
        paths: &AppPaths,
        item_id: &str,
        chunks: &[StorageTranscriptChunk],
        vector_for_unit: F,
    ) -> Vec<StorageRetrievalUnit>
    where
        F: Fn(&StorageRetrievalUnit) -> Vec<f32>,
    {
        write_video_chunks_and_units(paths, item_id, chunks);
        let (profile, units) = rebuild_units(paths, item_id);
        write_unified_vectors(paths, item_id, &profile, &units, vector_for_unit).await;
        units
    }

    async fn index_image_units<F>(
        paths: &AppPaths,
        item_id: &str,
        images: &[StorageImageChunk],
        vector_for_unit: F,
    ) -> Vec<StorageRetrievalUnit>
    where
        F: Fn(&StorageRetrievalUnit) -> Vec<f32>,
    {
        cerul_storage::write_media_sqlite_chunks(paths, item_id, &[], images).unwrap();
        let (profile, units) = rebuild_units(paths, item_id);
        write_unified_vectors(paths, item_id, &profile, &units, vector_for_unit).await;
        units
    }

    fn rebuild_units(
        paths: &AppPaths,
        item_id: &str,
    ) -> (EmbeddingProfile, Vec<StorageRetrievalUnit>) {
        let profile = cerul_storage::vectors::ensure_active_embedding_profile(paths).unwrap();
        let units =
            cerul_storage::rebuild_item_retrieval_units(paths, item_id, &profile.id).unwrap();
        assert!(!units.is_empty(), "expected retrieval units for {item_id}");
        (profile, units)
    }

    async fn write_unified_vectors<F>(
        paths: &AppPaths,
        item_id: &str,
        profile: &EmbeddingProfile,
        units: &[StorageRetrievalUnit],
        vector_for_unit: F,
    ) where
        F: Fn(&StorageRetrievalUnit) -> Vec<f32>,
    {
        let records = units
            .iter()
            .map(|unit| {
                VectorRecord::new_for_dimensions(
                    unit.id.clone(),
                    item_id.to_string(),
                    vector_for_unit(unit),
                    profile.output_dimension,
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        cerul_storage::vectors::replace_item_unified_embeddings_for_profile(
            paths,
            item_id,
            &records,
            profile,
            cerul_storage::SEARCH_INDEX_VERSION,
        )
        .await
        .unwrap();
        cerul_storage::set_item_search_index_status(
            paths,
            item_id,
            "indexed",
            None,
            units.len(),
            records.len(),
        )
        .unwrap();
    }

    fn manual_unit(
        id: &str,
        item_id: &str,
        unit_index: i64,
        start_sec: Option<f64>,
        end_sec: Option<f64>,
        text: &str,
        representative_chunk_id: Option<&str>,
        profile: &EmbeddingProfile,
    ) -> StorageRetrievalUnit {
        StorageRetrievalUnit {
            id: id.to_string(),
            item_id: item_id.to_string(),
            unit_index,
            unit_kind: "moment".to_string(),
            start_sec,
            end_sec,
            content_text: format!("Transcript: {text}"),
            transcript_text: Some(text.to_string()),
            ocr_text: None,
            visual_text: None,
            summary_text: None,
            representative_chunk_id: representative_chunk_id.map(ToOwned::to_owned),
            representative_frame_path: None,
            embedding_profile_id: profile.id.clone(),
            index_version: cerul_storage::SEARCH_INDEX_VERSION,
            metadata: Default::default(),
        }
    }

    async fn seed_latency_library(paths: &AppPaths, item_count: usize, topic_count: usize) {
        let conn = sqlite::open(paths).unwrap();
        conn.execute(
            "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'folder_video', '{}', 'active')",
            [],
        )
        .unwrap();
        drop(conn);

        for item_index in 0..item_count {
            let item_id = format!("item-{item_index:03}");
            let topic = item_index % topic_count;
            let conn = sqlite::open(paths).unwrap();
            conn.execute(
                r#"
                INSERT INTO items (id, source_id, content_type, status, title)
                VALUES (?1, 'source-1', 'video', 'indexed', ?2)
                "#,
                (&item_id, format!("Latency smoke item {item_index:03}")),
            )
            .unwrap();
            drop(conn);

            let chunks = vec![StorageTranscriptChunk {
                start: (item_index * 30) as f64,
                end: (item_index * 30 + 30) as f64,
                text: format!(
                    "latency-topic-{topic:02} retrieval phrase for indexed video {item_index:03}"
                ),
            }];
            index_video_units(paths, &item_id, &chunks, |_| fake_vector(topic)).await;
        }
    }

    fn percentile(sorted_timings: &[Duration], percentile: usize) -> Duration {
        assert!(!sorted_timings.is_empty());
        let rank = ((sorted_timings.len() * percentile).saturating_sub(1)) / 100;
        sorted_timings[rank.min(sorted_timings.len() - 1)]
    }

    fn fake_vector(seed: usize) -> Vec<f32> {
        scaled_vector(seed, 1.0)
    }

    fn scaled_vector(seed: usize, value: f32) -> Vec<f32> {
        let mut vector = vec![0.0; cerul_storage::vectors::VECTOR_DIMENSIONS as usize];
        vector[seed] = value;
        vector
    }

    fn result(
        chunk_id: &str,
        item_id: &str,
        chunk_type: &str,
        start_sec: Option<f64>,
        score: f32,
    ) -> SearchResult {
        SearchResult {
            chunk_id: chunk_id.to_string(),
            item_id: item_id.to_string(),
            chunk_type: chunk_type.to_string(),
            start_sec,
            end_sec: start_sec.map(|start| start + 10.0),
            snippet: chunk_id.to_string(),
            frame_path: None,
            match_score: 0.0,
            score,
            similarity_score: None,
            exact_match: false,
            source_mask: SOURCE_TEXT,
            item_title: None,
            nearest_frame_chunk_id: None,
        }
    }
}
