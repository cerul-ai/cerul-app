use std::collections::{HashMap, HashSet};

use cerul_storage::AppPaths;
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
    /// User-facing match score derived from the final fused ranking score for
    /// this query. Normalized to 0.0..=1.0 after dedupe so the UI can display
    /// and sort by the same signal that placed the result.
    pub match_score: f32,
    pub score: f32,
    pub similarity_score: Option<f32>,
    /// Item title, joined in so the UI can label a result without a separate
    /// items fetch (which can be empty/stale and leave the row showing a raw id).
    pub item_title: Option<String>,
    /// For transcript rows (which carry no frame of their own), the keyframe
    /// chunk nearest this result's timestamp — i.e. what was on screen when the
    /// line was spoken. `None` for visual rows (they use their own `frame_path`).
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
}

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

    // Qwen3-VL embeds text and images into one space, so the text query vector
    // drives both the transcript and the frame indexes.
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
    let fts_hits_count = hits.len();
    let results = hydrate(paths, &hits)?;
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
    image_query_vector: Vec<f32>,
) -> anyhow::Result<Vec<SearchResult>> {
    Ok(
        search_with_vectors_diagnostics(paths, req, text_query_vector, image_query_vector)
            .await?
            .results,
    )
}

pub async fn search_with_vectors_diagnostics(
    paths: &AppPaths,
    req: SearchRequest,
    text_query_vector: Vec<f32>,
    image_query_vector: Vec<f32>,
) -> anyhow::Result<SearchResponse> {
    let profile = cerul_storage::vectors::ensure_active_embedding_profile(paths)?;
    search_with_vectors_for_profile_diagnostics(
        paths,
        req,
        text_query_vector,
        image_query_vector,
        &profile,
    )
    .await
}

pub async fn search_with_vectors_for_profile(
    paths: &AppPaths,
    req: SearchRequest,
    text_query_vector: Vec<f32>,
    image_query_vector: Vec<f32>,
    profile: &cerul_storage::vectors::EmbeddingProfile,
) -> anyhow::Result<Vec<SearchResult>> {
    Ok(search_with_vectors_for_profile_diagnostics(
        paths,
        req,
        text_query_vector,
        image_query_vector,
        profile,
    )
    .await?
    .results)
}

pub async fn search_with_vectors_for_profile_diagnostics(
    paths: &AppPaths,
    req: SearchRequest,
    text_query_vector: Vec<f32>,
    image_query_vector: Vec<f32>,
    profile: &cerul_storage::vectors::EmbeddingProfile,
) -> anyhow::Result<SearchResponse> {
    anyhow::ensure!(
        text_query_vector.len() == profile.output_dimension as usize,
        "text query vector has {} dimensions, expected {}",
        text_query_vector.len(),
        profile.output_dimension
    );
    anyhow::ensure!(
        image_query_vector.len() == profile.output_dimension as usize,
        "image query vector has {} dimensions, expected {}",
        image_query_vector.len(),
        profile.output_dimension
    );

    let limit = req.limit.clamp(1, 50);
    let retrieval_limit = retrieval_limit(limit);
    let collections = cerul_storage::vectors::collection_names(paths, profile);
    let (bm25_hits, text_hits, image_hits) = tokio::try_join!(
        sqlite_text_search(paths, &req.q, retrieval_limit),
        qdrant_vector_search(
            paths,
            &collections.text,
            &text_query_vector,
            retrieval_limit,
            &profile.distance_metric
        ),
        qdrant_vector_search(
            paths,
            &collections.image,
            &image_query_vector,
            retrieval_limit,
            &profile.distance_metric
        ),
    )?;
    let fts_hits_count = bm25_hits.len();
    let text_vector_hits_count = text_hits.len();
    let image_vector_hits_count = image_hits.len();
    let vector_hits_count = text_vector_hits_count + image_vector_hits_count;
    let (qdrant_text_points, qdrant_image_points, fallback_reason) =
        vector_health_hint(paths, &collections, vector_hits_count).await;
    let merged = rrf_merge(vec![bm25_hits, text_hits, image_hits], 60);
    let top_hits = merged.into_iter().take(retrieval_limit).collect::<Vec<_>>();

    let results = hydrate(paths, &top_hits)?;
    let results = finalize_results(results, limit);
    let retrieval_mode = retrieval_mode(vector_hits_count, fts_hits_count);
    Ok(SearchResponse {
        results,
        diagnostics: SearchDiagnostics {
            retrieval_mode,
            fallback_reason,
            vector_hits_count,
            text_vector_hits_count,
            image_vector_hits_count,
            fts_hits_count,
            embedding_profile_id: Some(profile.id.clone()),
            qdrant_collection: Some(collections.text.clone()),
            qdrant_text_collection: Some(collections.text),
            qdrant_image_collection: Some(collections.image),
            qdrant_text_points,
            qdrant_image_points,
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
            qdrant_text_collection: None,
            qdrant_image_collection: None,
            qdrant_text_points: None,
            qdrant_image_points: None,
        }
    }
}

fn retrieval_mode(vector_hits_count: usize, fts_hits_count: usize) -> String {
    match (vector_hits_count > 0, fts_hits_count > 0) {
        (true, true) => "hybrid",
        (true, false) => "vector",
        (false, true) => "fts",
        (false, false) => "empty",
    }
    .to_string()
}

async fn vector_health_hint(
    paths: &AppPaths,
    collections: &cerul_storage::vectors::VectorCollectionNames,
    vector_hits_count: usize,
) -> (Option<usize>, Option<usize>, Option<String>) {
    if vector_hits_count > 0 {
        return (None, None, None);
    }

    let text_points =
        cerul_storage::vectors::collection_point_count(paths, &collections.text).await;
    let image_points =
        cerul_storage::vectors::collection_point_count(paths, &collections.image).await;

    match (text_points, image_points) {
        (Ok(text), Ok(image)) if text == 0 && image == 0 => (
            Some(text),
            Some(image),
            Some("vector_index_empty".to_string()),
        ),
        (Ok(text), Ok(image)) => (Some(text), Some(image), Some("no_vector_hits".to_string())),
        (text_result, image_result) => {
            if let Err(error) = &text_result {
                tracing::warn!(%error, collection = %collections.text, "failed to count Qdrant text points for search diagnostics");
            }
            if let Err(error) = &image_result {
                tracing::warn!(%error, collection = %collections.image, "failed to count Qdrant image points for search diagnostics");
            }
            (
                text_result.ok(),
                image_result.ok(),
                Some("qdrant_health_check_failed".to_string()),
            )
        }
    }
}

fn retrieval_limit(limit: usize) -> usize {
    limit.saturating_mul(4).max(limit).max(1)
}

pub fn rrf_merge(sources: Vec<Vec<RawHit>>, k: usize) -> Vec<RawHit> {
    let mut scores = HashMap::<String, f32>::new();
    let mut similarity_scores = HashMap::<String, f32>::new();

    for source in sources {
        let mut seen_in_source = HashSet::new();
        for (rank, hit) in source.into_iter().enumerate() {
            if !seen_in_source.insert(hit.chunk_id.clone()) {
                continue;
            }

            let chunk_id = hit.chunk_id;
            if let Some(similarity_score) = hit.similarity_score {
                similarity_scores
                    .entry(chunk_id.clone())
                    .and_modify(|current| {
                        if similarity_score > *current {
                            *current = similarity_score;
                        }
                    })
                    .or_insert(similarity_score);
            }
            *scores.entry(chunk_id).or_default() += 1.0 / (k as f32 + rank as f32 + 1.0);
        }
    }

    let mut hits = scores
        .into_iter()
        .map(|(chunk_id, score)| RawHit {
            similarity_score: similarity_scores.remove(&chunk_id),
            chunk_id,
            score,
        })
        .collect::<Vec<_>>();
    hits.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
    });
    hits
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
        SELECT c.id, bm25(chunks_fts) AS rank_score
        FROM chunks_fts
        JOIN chunks c ON c.rowid = chunks_fts.rowid
        JOIN items i ON i.id = c.item_id
        WHERE chunks_fts MATCH ?1
          AND i.status != 'deleting'
        ORDER BY rank_score
        LIMIT ?2
        "#,
    )?;
    let rows = stmt.query_map((&match_query, limit as i64), |row| {
        let chunk_id: String = row.get(0)?;
        let rank_score: f64 = row.get(1)?;
        Ok(RawHit {
            chunk_id,
            score: (-rank_score) as f32,
            similarity_score: None,
        })
    })?;

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
    let conn = cerul_storage::sqlite::open(paths)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT c.id
        FROM chunks c
        JOIN items i ON i.id = c.item_id
        WHERE c.text IS NOT NULL
          AND i.status != 'deleting'
          AND c.chunk_type IN ('transcript', 'transcript_line', 'audio', 'ocr', 'understanding')
          AND c.text LIKE ?1 ESCAPE '\'
        ORDER BY
          CASE c.chunk_type
            WHEN 'transcript_line' THEN 0
            WHEN 'transcript' THEN 1
            WHEN 'audio' THEN 2
            WHEN 'ocr' THEN 3
            ELSE 4
          END,
          COALESCE(c.start_sec, 9223372036854775807),
          c.id
        LIMIT ?2
        "#,
    )?;
    let rows = stmt.query_map((&pattern, limit as i64), |row| {
        let chunk_id: String = row.get(0)?;
        Ok(RawHit {
            chunk_id,
            score: 0.01,
            similarity_score: None,
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
            score: 0.0,
            similarity_score: Some(similarity_from_qdrant_score(hit.score, distance_metric)),
        })
        .collect())
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

fn hydrate(paths: &AppPaths, hits: &[RawHit]) -> anyhow::Result<Vec<SearchResult>> {
    if hits.is_empty() {
        return Ok(Vec::new());
    }
    let conn = cerul_storage::sqlite::open(paths)?;
    let chunks = load_chunks_for_hits(&conn, hits)?;
    let mut results = Vec::with_capacity(hits.len());

    for hit in hits {
        let Some(chunk) = chunks.get(&hit.chunk_id) else {
            continue;
        };
        let snippet = chunk
            .text
            .clone()
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| fallback_snippet(&chunk.chunk_type, chunk.start_sec));

        results.push(SearchResult {
            chunk_id: chunk.id.clone(),
            item_id: chunk.item_id.clone(),
            chunk_type: chunk.chunk_type.clone(),
            start_sec: chunk.start_sec,
            end_sec: chunk.end_sec,
            snippet,
            frame_path: chunk.frame_path.clone(),
            match_score: 0.0,
            score: hit.score,
            similarity_score: hit.similarity_score,
            item_title: chunk.item_title.clone(),
            nearest_frame_chunk_id: None,
        });
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
    let best_score = results
        .iter()
        .filter_map(|result| {
            if result.score.is_finite() && result.score > 0.0 {
                Some(result.score)
            } else {
                None
            }
        })
        .max_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));

    let Some(best_score) = best_score else {
        for result in results {
            result.match_score = 0.0;
        }
        return;
    };

    for result in results {
        result.match_score = if result.score.is_finite() && result.score > 0.0 {
            (result.score / best_score).clamp(0.0, 1.0)
        } else {
            0.0
        };
    }
}

#[derive(Debug, Clone)]
struct HydratedChunk {
    id: String,
    item_id: String,
    chunk_type: String,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
    text: Option<String>,
    frame_path: Option<String>,
    item_title: Option<String>,
}

fn load_chunks_for_hits(
    conn: &rusqlite::Connection,
    hits: &[RawHit],
) -> anyhow::Result<HashMap<String, HydratedChunk>> {
    let mut seen = HashSet::new();
    let chunk_ids = hits
        .iter()
        .filter_map(|hit| {
            if seen.insert(hit.chunk_id.as_str()) {
                Some(hit.chunk_id.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if chunk_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let placeholders = std::iter::repeat_n("?", chunk_ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        r#"
        SELECT
            c.id,
            c.item_id,
            c.chunk_type,
            c.start_sec,
            c.end_sec,
            c.text,
            c.frame_path,
            i.title
        FROM chunks c
        JOIN items i ON i.id = c.item_id
        WHERE c.id IN ({placeholders})
          AND i.status != 'deleting'
        "#,
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(chunk_ids.iter()), |row| {
        Ok(HydratedChunk {
            id: row.get(0)?,
            item_id: row.get(1)?,
            chunk_type: row.get(2)?,
            start_sec: row.get(3)?,
            end_sec: row.get(4)?,
            text: row.get(5)?,
            frame_path: row.get(6)?,
            item_title: row.get(7)?,
        })
    })?;

    let mut chunks = HashMap::with_capacity(chunk_ids.len());
    for chunk in rows {
        let chunk = chunk?;
        chunks.insert(chunk.id.clone(), chunk);
    }
    Ok(chunks)
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

    for result in results {
        if kept
            .iter()
            .any(|existing| is_near_duplicate(existing, &result))
        {
            continue;
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use cerul_storage::{sqlite, StorageImageChunk, StorageTranscriptChunk};
    use std::time::{Duration, Instant};

    #[test]
    fn rrf_synthetic() {
        let merged = rrf_merge(
            vec![
                vec![
                    RawHit {
                        chunk_id: "shared".to_string(),
                        score: 0.0,
                        similarity_score: Some(0.91),
                    },
                    RawHit {
                        chunk_id: "single".to_string(),
                        score: 0.0,
                        similarity_score: Some(0.74),
                    },
                ],
                vec![RawHit {
                    chunk_id: "shared".to_string(),
                    score: 0.0,
                    similarity_score: Some(0.87),
                }],
                vec![RawHit {
                    chunk_id: "shared".to_string(),
                    score: 0.0,
                    similarity_score: None,
                }],
            ],
            60,
        );

        assert_eq!(merged.first().unwrap().chunk_id, "shared");
        assert!(merged[0].score > merged[1].score);
        assert_eq!(merged[0].similarity_score, Some(0.91));
    }

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
    fn match_score_normalizes_final_fused_score() {
        let results = vec![
            result("chunk-a", "item-1", "transcript", Some(10.0), 0.04),
            result("chunk-b", "item-2", "keyframe", Some(20.0), 0.02),
            result("chunk-c", "item-3", "transcript", Some(30.0), 0.0),
        ];

        let scored = finalize_results(results, 10);

        assert_eq!(
            scored
                .iter()
                .map(|result| result.chunk_id.as_str())
                .collect::<Vec<_>>(),
            vec!["chunk-a", "chunk-b", "chunk-c"]
        );
        assert_eq!(scored[0].match_score, 1.0);
        assert_eq!(scored[1].match_score, 0.5);
        assert_eq!(scored[2].match_score, 0.0);
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
        cerul_storage::write_video_chunks(
            &paths,
            "item-1",
            &chunks,
            &[],
            &[fake_vector(0), fake_vector(1)],
            &[],
        )
        .await
        .unwrap();

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

        let results = hydrate(
            &paths,
            &[RawHit {
                chunk_id: "item-1:transcript:000000".to_string(),
                score: 1.0,
                similarity_score: Some(1.0),
            }],
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
        cerul_storage::write_video_chunks(&paths, "item-1", &chunks, &[], &[fake_vector(0)], &[])
            .await
            .unwrap();

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
        cerul_storage::write_video_chunks(&paths, "item-1", &chunks, &[], &[fake_vector(0)], &[])
            .await
            .unwrap();

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
        let chunks = vec![
            StorageTranscriptChunk {
                start: 1.0,
                end: 2.0,
                text: "first chunk".to_string(),
            },
            StorageTranscriptChunk {
                start: 3.0,
                end: 4.0,
                text: "second chunk".to_string(),
            },
        ];
        cerul_storage::write_video_chunks(
            &paths,
            "item-1",
            &chunks,
            &[],
            &[fake_vector(0), fake_vector(1)],
            &[],
        )
        .await
        .unwrap();

        let results = hydrate(
            &paths,
            &[
                RawHit {
                    chunk_id: "item-1:transcript:000001".to_string(),
                    score: 0.9,
                    similarity_score: Some(0.9),
                },
                RawHit {
                    chunk_id: "item-1:transcript:000000".to_string(),
                    score: 0.8,
                    similarity_score: Some(0.8),
                },
            ],
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
        cerul_storage::write_video_chunks(&paths, "item-1", &chunks, &[], &[fake_vector(0)], &[])
            .await
            .unwrap();

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
    async fn search_with_vectors_uses_separate_image_query_vector() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        insert_item(&paths);
        let frames = temp.path().join("frames");
        std::fs::create_dir(&frames).unwrap();

        cerul_storage::write_media_chunks(
            &paths,
            "item-1",
            &[],
            &[
                StorageImageChunk::keyframe(frames.join("red.jpg")),
                StorageImageChunk::keyframe(frames.join("blue.jpg")),
            ],
            &[],
            &[fake_vector(20), fake_vector(21)],
        )
        .await
        .unwrap();

        let results = search_with_vectors(
            &paths,
            SearchRequest {
                q: "not in transcript".to_string(),
                limit: 1,
            },
            fake_vector(0),
            fake_vector(21),
        )
        .await
        .unwrap();

        assert_eq!(results.first().unwrap().chunk_id, "item-1:keyframe:000001");
        assert!(results
            .first()
            .unwrap()
            .frame_path
            .as_deref()
            .unwrap()
            .ends_with("blue.jpg"));
    }

    #[tokio::test]
    async fn vector_search_reports_search_diagnostics() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        insert_item(&paths);
        let frames = temp.path().join("frames");
        std::fs::create_dir(&frames).unwrap();

        cerul_storage::write_media_chunks(
            &paths,
            "item-1",
            &[],
            &[
                StorageImageChunk::keyframe(frames.join("red.jpg")),
                StorageImageChunk::keyframe(frames.join("blue.jpg")),
            ],
            &[],
            &[fake_vector(20), fake_vector(21)],
        )
        .await
        .unwrap();

        let response = search_with_vectors_diagnostics(
            &paths,
            SearchRequest {
                q: "not in transcript".to_string(),
                limit: 1,
            },
            fake_vector(0),
            fake_vector(21),
        )
        .await
        .unwrap();

        assert_eq!(response.diagnostics.retrieval_mode, "vector");
        assert!(response.diagnostics.vector_hits_count >= 1);
        assert_eq!(response.diagnostics.fts_hits_count, 0);
        assert!(response.diagnostics.embedding_profile_id.is_some());
        assert!(response
            .diagnostics
            .qdrant_text_collection
            .as_deref()
            .unwrap()
            .contains("text_chunks"));
        assert_eq!(response.results[0].chunk_id, "item-1:keyframe:000001");
    }

    #[tokio::test]
    async fn vector_search_uses_profile_distance_metric() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        insert_item(&paths);
        let frames = temp.path().join("frames");
        std::fs::create_dir(&frames).unwrap();

        cerul_storage::write_media_chunks(
            &paths,
            "item-1",
            &[],
            &[
                StorageImageChunk::keyframe(frames.join("same-direction.jpg")),
                StorageImageChunk::keyframe(frames.join("orthogonal.jpg")),
            ],
            &[],
            &[scaled_vector(0, 10.0), fake_vector(1)],
        )
        .await
        .unwrap();

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
        assert!(first
            .frame_path
            .as_deref()
            .unwrap()
            .ends_with("same-direction.jpg"));
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
        let conn = sqlite::open(paths).unwrap();
        conn.execute(
            "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'folder_video', '{}', 'active')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO items (id, source_id, content_type, status, title) VALUES ('item-1', 'source-1', 'video', 'indexed', 'Sample')",
            [],
        )
        .unwrap();
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
            cerul_storage::write_video_chunks(
                paths,
                &item_id,
                &chunks,
                &[],
                &[fake_vector(topic)],
                &[],
            )
            .await
            .unwrap();
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
            item_title: None,
            nearest_frame_chunk_id: None,
        }
    }
}
