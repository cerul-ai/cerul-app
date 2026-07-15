use std::{
    fs::File,
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use axum::{
    extract::{Path as AxumPath, State},
    Json,
};
use reqwest::{
    blocking::{Body, Client, RequestBuilder, Response},
    header, StatusCode,
};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{ApiError, ApiResult, ApiState};

const GEMINI_TIMEOUT: Duration = Duration::from_secs(600);
const RETRY_SLEEP: Duration = Duration::from_secs(2);
const MAX_RETRIES: usize = 3;
const FILE_POLL_SLEEP: Duration = Duration::from_secs(3);
const FILE_POLL_ATTEMPTS: usize = 120;
const STATUS_NOT_STARTED: &str = "not_started";
const STATUS_RUNNING: &str = "running";
const STATUS_COMPLETED: &str = "completed";
const STATUS_FAILED: &str = "failed";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VideoUnderstandingRecord {
    pub item_id: String,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub status: String,
    pub summary: Option<String>,
    pub display_title: Option<String>,
    pub chapters: Vec<VideoUnderstandingChapter>,
    pub events: Vec<VideoUnderstandingEvent>,
    pub topics: Vec<String>,
    pub searchable_text: Option<String>,
    pub error: Option<String>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VideoUnderstandingChapter {
    pub start_sec: Option<f64>,
    pub end_sec: Option<f64>,
    pub title: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VideoUnderstandingEvent {
    pub start_sec: Option<f64>,
    pub end_sec: Option<f64>,
    pub caption: String,
    pub visual: Option<String>,
    pub audio: Option<String>,
    pub actions: Vec<String>,
    pub entities: Vec<String>,
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone)]
struct GeminiVideoUnderstandingAnalyzer {
    provider: cerul_storage::providers::Provider,
    api_key: String,
    model: String,
}

#[derive(Debug, Clone)]
struct VideoInput {
    title: String,
    source: VideoInputSource,
}

#[derive(Debug, Clone)]
enum VideoInputSource {
    LocalFile { path: PathBuf, mime_type: String },
    YoutubeUrl { url: String },
}

#[derive(Debug, Clone)]
struct UploadedFile {
    name: String,
    uri: String,
    mime_type: String,
}

pub async fn get_item_understanding(
    State(state): State<ApiState>,
    AxumPath(id): AxumPath<String>,
) -> ApiResult<Json<VideoUnderstandingRecord>> {
    ensure_item_exists(&state.paths, &id)?;
    Ok(Json(read_understanding_record(&state.paths, &id)?))
}

pub async fn analyze_item_understanding(
    State(state): State<ApiState>,
    AxumPath(id): AxumPath<String>,
) -> ApiResult<Json<VideoUnderstandingRecord>> {
    ensure_item_exists(&state.paths, &id)?;
    let paths = state.paths.clone();
    let item_id = id.clone();
    let record = tokio::task::spawn_blocking(move || analyze_and_store(&paths, &item_id))
        .await
        .map_err(|error| ApiError::internal(anyhow::anyhow!(error)))??;
    Ok(Json(record))
}

fn analyze_and_store(
    paths: &cerul_storage::AppPaths,
    item_id: &str,
) -> anyhow::Result<VideoUnderstandingRecord> {
    let input = video_input(paths, item_id)?;
    let analyzer = selected_analyzer(paths)?;
    write_status_record(
        paths,
        item_id,
        Some(&analyzer.provider.id),
        Some(&analyzer.model),
        STATUS_RUNNING,
        None,
    )?;

    match analyzer.analyze(&input) {
        Ok(result) => {
            let record = write_completed_record(
                paths,
                item_id,
                &analyzer.provider.id,
                &analyzer.model,
                result,
            )?;
            record_video_understanding_usage(
                paths,
                item_id,
                &analyzer,
                STATUS_COMPLETED,
                json!({ "source": "manual_analysis" }),
            );
            Ok(record)
        }
        Err(error) => {
            let message = error.to_string();
            record_video_understanding_usage(
                paths,
                item_id,
                &analyzer,
                STATUS_FAILED,
                json!({ "source": "manual_analysis", "error": message }),
            );
            let _ = write_status_record(
                paths,
                item_id,
                Some(&analyzer.provider.id),
                Some(&analyzer.model),
                STATUS_FAILED,
                Some(&message),
            );
            Err(error)
        }
    }
}

fn selected_analyzer(
    paths: &cerul_storage::AppPaths,
) -> anyhow::Result<GeminiVideoUnderstandingAnalyzer> {
    let model = crate::models::selected_video_understanding_model_id(paths)
        .unwrap_or_else(|| crate::models::DEFAULT_VIDEO_UNDERSTANDING_MODEL_ID.to_string());
    let provider = provider_for_video_understanding(paths)?;
    let api_key =
        crate::providers::get_provider_key_for_provider(paths, &provider)?.ok_or_else(|| {
            anyhow::anyhow!(
                "video understanding provider {} has no API key configured",
                provider.label
            )
        })?;

    Ok(GeminiVideoUnderstandingAnalyzer {
        provider,
        api_key,
        model,
    })
}

fn record_video_understanding_usage(
    paths: &cerul_storage::AppPaths,
    item_id: &str,
    analyzer: &GeminiVideoUnderstandingAnalyzer,
    status: &str,
    metadata: Value,
) {
    let provider_mode = if analyzer.provider.provider_type == "local" {
        "local"
    } else {
        "remote"
    };
    let usage_status = if status == STATUS_COMPLETED {
        "succeeded"
    } else {
        "failed"
    };
    let mut event = cerul_storage::NewUsageEvent::new(provider_mode, "video_understanding");
    event.provider_id = Some(analyzer.provider.id.clone());
    event.provider_type = Some(analyzer.provider.provider_type.clone());
    event.model_id = Some(analyzer.model.clone());
    event.item_id = Some(item_id.to_string());
    event.status = usage_status.to_string();
    event.metadata = metadata;
    if let Err(error) = cerul_storage::record_usage_event(paths, event) {
        tracing::warn!(%error, item_id, "failed to record video understanding usage");
    }
}

fn provider_for_video_understanding(
    paths: &cerul_storage::AppPaths,
) -> anyhow::Result<cerul_storage::providers::Provider> {
    let providers = cerul_storage::providers::list_providers(paths)?;
    if let Some(provider_id) =
        crate::setting_string(paths, "video_understanding_provider_id")?.filter(|id| !id.is_empty())
    {
        let provider = providers
            .iter()
            .find(|provider| provider.id == provider_id)
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!("video understanding provider {provider_id} was not found")
            })?;
        anyhow::ensure!(
            provider.provider_type == "gemini",
            "video understanding provider {} has unsupported type {}; expected gemini",
            provider.label,
            provider.provider_type
        );
        return Ok(provider);
    }

    providers
        .into_iter()
        .find(|provider| {
            provider.id != cerul_storage::providers::LOCAL_PROVIDER_ID
                && provider.provider_type == "gemini"
                && crate::providers::has_provider_key_for_provider(paths, provider)
        })
        .ok_or_else(|| anyhow::anyhow!("Connect a Gemini provider before analyzing video."))
}

impl GeminiVideoUnderstandingAnalyzer {
    fn analyze(&self, input: &VideoInput) -> anyhow::Result<Value> {
        let client = http_client()?;
        let file_part = match &input.source {
            VideoInputSource::LocalFile { path, mime_type } => {
                let uploaded = self.upload_file(&client, path, mime_type, &input.title)?;
                let part = json!({
                    "file_data": {
                        "mime_type": uploaded.mime_type,
                        "file_uri": uploaded.uri,
                    }
                });
                let result = self.generate_content(&client, vec![part]);
                if let Err(error) = self.delete_file(&client, &uploaded.name) {
                    tracing::warn!(%error, file = %uploaded.name, "failed to delete Gemini uploaded file");
                }
                return result;
            }
            VideoInputSource::YoutubeUrl { url } => json!({
                "file_data": {
                    "file_uri": url,
                }
            }),
        };

        self.generate_content(&client, vec![file_part])
    }

    fn upload_file(
        &self,
        client: &Client,
        path: &Path,
        mime_type: &str,
        display_name: &str,
    ) -> anyhow::Result<UploadedFile> {
        let metadata = std::fs::metadata(path)?;
        let upload_start_url = gemini_upload_files_url(&provider_base_url(&self.provider)?)?;
        let upload_url = start_resumable_upload(
            client,
            &upload_start_url,
            self.api_key.trim(),
            metadata.len(),
            mime_type,
            display_name,
        )?;
        let file = File::open(path)?;
        let upload_response = client
            .post(upload_url)
            .header(header::CONTENT_LENGTH, metadata.len().to_string())
            .header("X-Goog-Upload-Offset", "0")
            .header("X-Goog-Upload-Command", "upload, finalize")
            .body(Body::new(file))
            .send()?;
        let mut file_value = response_json(upload_response)?
            .get("file")
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!("Gemini Files API upload response did not include file")
            })?;
        file_value = self.wait_for_file_ready(client, file_value)?;
        uploaded_file(file_value, mime_type)
    }

    fn wait_for_file_ready(&self, client: &Client, file: Value) -> anyhow::Result<Value> {
        let Some(name) = file
            .get("name")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
        else {
            return Ok(file);
        };
        let mut current = file;
        for _ in 0..FILE_POLL_ATTEMPTS {
            match current.get("state").and_then(Value::as_str) {
                Some("ACTIVE") | None => return Ok(current),
                Some("FAILED") => anyhow::bail!("Gemini file processing failed for {name}"),
                Some("PROCESSING") => {}
                Some(other) => {
                    tracing::debug!(state = other, file = %name, "Gemini file processing state")
                }
            }
            thread::sleep(FILE_POLL_SLEEP);
            current = self.get_file(client, &name)?;
        }

        anyhow::bail!("Gemini file processing timed out for {name}")
    }

    fn get_file(&self, client: &Client, name: &str) -> anyhow::Result<Value> {
        let url = format!(
            "{}/{}",
            provider_base_url(&self.provider)?,
            name.trim_start_matches('/')
        );
        let value = send_json_with_retry(|| {
            Ok(client
                .get(&url)
                .header("x-goog-api-key", self.api_key.trim()))
        })?;
        value
            .get("file")
            .cloned()
            .or_else(|| {
                if value.get("name").is_some() {
                    Some(value)
                } else {
                    None
                }
            })
            .ok_or_else(|| anyhow::anyhow!("Gemini Files API get response did not include file"))
    }

    fn delete_file(&self, client: &Client, name: &str) -> anyhow::Result<()> {
        let url = format!(
            "{}/{}",
            provider_base_url(&self.provider)?,
            name.trim_start_matches('/')
        );
        let response = client
            .delete(url)
            .header("x-goog-api-key", self.api_key.trim())
            .send()?;
        if response.status().is_success() || response.status() == StatusCode::NOT_FOUND {
            Ok(())
        } else {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            anyhow::bail!("Gemini Files API delete returned HTTP {status}: {body}")
        }
    }

    fn generate_content(&self, client: &Client, mut parts: Vec<Value>) -> anyhow::Result<Value> {
        parts.push(json!({ "text": video_understanding_prompt() }));
        let url = format!(
            "{}/models/{}:generateContent",
            provider_base_url(&self.provider)?,
            self.model.trim_start_matches("models/")
        );
        let body = json!({
            "contents": [{
                "role": "user",
                "parts": parts,
            }],
            "generationConfig": {
                "responseMimeType": "application/json",
                "responseSchema": video_understanding_schema(),
            }
        });
        let response = send_json_with_retry(|| {
            Ok(client
                .post(&url)
                .header("x-goog-api-key", self.api_key.trim())
                .json(&body))
        })?;
        let text = gemini_candidate_text(&response)?;
        parse_video_understanding_output(&text)
    }
}

fn start_resumable_upload(
    client: &Client,
    url: &str,
    api_key: &str,
    num_bytes: u64,
    mime_type: &str,
    display_name: &str,
) -> anyhow::Result<String> {
    let response = client
        .post(url)
        .header("x-goog-api-key", api_key)
        .header("X-Goog-Upload-Protocol", "resumable")
        .header("X-Goog-Upload-Command", "start")
        .header("X-Goog-Upload-Header-Content-Length", num_bytes.to_string())
        .header("X-Goog-Upload-Header-Content-Type", mime_type)
        .json(&json!({ "file": { "display_name": display_name } }))
        .send()?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        anyhow::bail!("Gemini Files API upload start returned HTTP {status}: {body}");
    }

    response
        .headers()
        .get("x-goog-upload-url")
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow::anyhow!("Gemini Files API did not return x-goog-upload-url"))
}

fn uploaded_file(value: Value, fallback_mime_type: &str) -> anyhow::Result<UploadedFile> {
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("Gemini uploaded file has no name"))?
        .to_string();
    let uri = value
        .get("uri")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("Gemini uploaded file has no uri"))?
        .to_string();
    let mime_type = value
        .get("mimeType")
        .or_else(|| value.get("mime_type"))
        .and_then(Value::as_str)
        .unwrap_or(fallback_mime_type)
        .to_string();

    Ok(UploadedFile {
        name,
        uri,
        mime_type,
    })
}

fn video_input(paths: &cerul_storage::AppPaths, item_id: &str) -> anyhow::Result<VideoInput> {
    let conn = cerul_storage::sqlite::open(paths)?;
    let (content_type, title, raw_path, metadata): (
        String,
        Option<String>,
        Option<String>,
        String,
    ) = conn.query_row(
        "SELECT content_type, title, raw_path, metadata FROM items WHERE id = ?1",
        [item_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    anyhow::ensure!(
        content_type == "video",
        "video understanding only supports video items"
    );

    let metadata = parse_json(&metadata);
    let title = title
        .or_else(|| metadata_string(&metadata, &["title"]))
        .unwrap_or_else(|| item_id.to_string());
    let raw_path = raw_path.or_else(|| metadata_string(&metadata, &["raw_path", "path"]));
    if let Some(raw_path) = raw_path {
        let path = PathBuf::from(raw_path);
        if path.exists() {
            let mime_type = video_mime_type(&path)?;
            return Ok(VideoInput {
                title,
                source: VideoInputSource::LocalFile { path, mime_type },
            });
        }
    }

    if let Some(url) = metadata_string(
        &metadata,
        &[
            "webpage_url",
            "original_url",
            "source_url",
            "url",
            "episode_url",
        ],
    )
    .filter(|url| is_youtube_url(url))
    {
        return Ok(VideoInput {
            title,
            source: VideoInputSource::YoutubeUrl { url },
        });
    }

    anyhow::bail!(
        "video source file is unavailable; re-index the item before running video understanding"
    )
}

fn write_completed_record(
    paths: &cerul_storage::AppPaths,
    item_id: &str,
    provider_id: &str,
    model_id: &str,
    result: Value,
) -> anyhow::Result<VideoUnderstandingRecord> {
    let summary = result
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let display_title = display_title_from_result(&result);
    let searchable_text = searchable_text_from_result(&result);
    let conn = cerul_storage::sqlite::open(paths)?;
    conn.execute(
        r#"
        INSERT INTO item_understandings
            (item_id, provider_id, model_id, status, summary, result, error, created_at, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, strftime('%s','now'), strftime('%s','now'))
        ON CONFLICT(item_id) DO UPDATE SET
            provider_id = excluded.provider_id,
            model_id = excluded.model_id,
            status = excluded.status,
            summary = excluded.summary,
            result = excluded.result,
            error = NULL,
            updated_at = excluded.updated_at
        "#,
        (
            item_id,
            provider_id,
            model_id,
            STATUS_COMPLETED,
            summary.as_deref(),
            result.to_string(),
        ),
    )?;
    write_item_display_title(paths, item_id, display_title.as_deref())?;
    replace_understanding_chunks(paths, item_id, &result, searchable_text.as_deref())?;
    crate::refresh_item_retrieval_units_after_understanding_update(
        paths, item_id, false, false, true,
    )?;
    read_understanding_record(paths, item_id)
}

fn write_item_display_title(
    paths: &cerul_storage::AppPaths,
    item_id: &str,
    display_title: Option<&str>,
) -> anyhow::Result<()> {
    cerul_storage::update_item_metadata(paths, item_id, |metadata| {
        if let Some(display_title) = display_title {
            metadata.insert(
                "display_title".to_string(),
                Value::String(display_title.to_string()),
            );
        } else {
            metadata.remove("display_title");
        }
    })
}

fn write_status_record(
    paths: &cerul_storage::AppPaths,
    item_id: &str,
    provider_id: Option<&str>,
    model_id: Option<&str>,
    status: &str,
    error: Option<&str>,
) -> anyhow::Result<VideoUnderstandingRecord> {
    let conn = cerul_storage::sqlite::open(paths)?;
    conn.execute(
        r#"
        INSERT INTO item_understandings
            (item_id, provider_id, model_id, status, summary, result, error, created_at, updated_at)
        VALUES (?1, ?2, ?3, ?4, NULL, '{}', ?5, strftime('%s','now'), strftime('%s','now'))
        ON CONFLICT(item_id) DO UPDATE SET
            provider_id = excluded.provider_id,
            model_id = excluded.model_id,
            status = excluded.status,
            summary = NULL,
            result = '{}',
            error = excluded.error,
            updated_at = excluded.updated_at
        "#,
        (item_id, provider_id, model_id, status, error),
    )?;
    if status == STATUS_FAILED {
        write_item_display_title(paths, item_id, None)?;
        replace_understanding_chunks(paths, item_id, &json!({}), None)?;
        crate::refresh_item_retrieval_units_after_understanding_update(
            paths, item_id, false, false, true,
        )?;
    }
    read_understanding_record(paths, item_id)
}

fn replace_understanding_chunks(
    paths: &cerul_storage::AppPaths,
    item_id: &str,
    result: &Value,
    searchable_text: Option<&str>,
) -> anyhow::Result<()> {
    let mut conn = cerul_storage::sqlite::open(paths)?;
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM chunks WHERE item_id = ?1 AND chunk_type = 'understanding'",
        [item_id],
    )?;
    if let Some(text) = searchable_text
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        tx.execute(
            r#"
            INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, text, metadata)
            VALUES (?1, ?2, 'understanding', NULL, NULL, ?3, ?4)
            "#,
            (
                format!("{item_id}:understanding:summary"),
                item_id,
                text,
                json!({ "kind": "summary", "source": "video_understanding" }).to_string(),
            ),
        )?;
    }

    for (index, chapter) in chapters_from_result(result).into_iter().enumerate() {
        tx.execute(
            r#"
            INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, text, metadata)
            VALUES (?1, ?2, 'understanding', ?3, ?4, ?5, ?6)
            "#,
            (
                format!("{item_id}:understanding:chapter:{index:04}"),
                item_id,
                chapter.start_sec,
                chapter.end_sec,
                format!("{} {}", chapter.title, chapter.summary)
                    .trim()
                    .to_string(),
                json!({ "kind": "chapter", "source": "video_understanding" }).to_string(),
            ),
        )?;
    }

    for (index, event) in events_from_result(result).into_iter().enumerate() {
        tx.execute(
            r#"
            INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, text, metadata)
            VALUES (?1, ?2, 'understanding', ?3, ?4, ?5, ?6)
            "#,
            (
                format!("{item_id}:understanding:event:{index:04}"),
                item_id,
                event.start_sec,
                event.end_sec,
                event_search_text(&event),
                json!({ "kind": "event", "source": "video_understanding" }).to_string(),
            ),
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub(crate) fn read_understanding_record(
    paths: &cerul_storage::AppPaths,
    item_id: &str,
) -> anyhow::Result<VideoUnderstandingRecord> {
    let conn = cerul_storage::sqlite::open(paths)?;
    let row = conn
        .query_row(
            r#"
            SELECT item_id, provider_id, model_id, status, summary, result, error, created_at, updated_at
            FROM item_understandings
            WHERE item_id = ?1
            "#,
            [item_id],
            |row| {
                let result: String = row.get(5)?;
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    result,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, Option<i64>>(8)?,
                ))
            },
        )
        .optional()?;

    let Some((
        item_id,
        provider_id,
        model_id,
        status,
        summary,
        result,
        error,
        created_at,
        updated_at,
    )) = row
    else {
        return Ok(VideoUnderstandingRecord {
            item_id: item_id.to_string(),
            provider_id: None,
            model_id: None,
            status: STATUS_NOT_STARTED.to_string(),
            summary: None,
            display_title: None,
            chapters: Vec::new(),
            events: Vec::new(),
            topics: Vec::new(),
            searchable_text: None,
            error: None,
            created_at: None,
            updated_at: None,
        });
    };
    let result = parse_json(&result);

    Ok(VideoUnderstandingRecord {
        item_id,
        provider_id,
        model_id,
        status,
        summary,
        display_title: display_title_from_result(&result),
        chapters: chapters_from_result(&result),
        events: events_from_result(&result),
        topics: string_array(result.get("topics")),
        searchable_text: searchable_text_from_result(&result),
        error,
        created_at,
        updated_at,
    })
}

fn ensure_item_exists(paths: &cerul_storage::AppPaths, item_id: &str) -> ApiResult<()> {
    let conn = cerul_storage::sqlite::open(paths)?;
    let exists: Option<i64> = conn
        .query_row("SELECT 1 FROM items WHERE id = ?1", [item_id], |row| {
            row.get(0)
        })
        .optional()?;
    if exists.is_some() {
        Ok(())
    } else {
        Err(ApiError::not_found(format!("item not found: {item_id}")))
    }
}

fn video_understanding_prompt() -> &'static str {
    "Analyze this video for a local media memory index. Return JSON only. Include what happens \
     visually and audibly. Use seconds for timestamps. Detect the video's primary spoken \
     language (or, if there is no speech, the dominant on-screen text language) and write every \
     natural-language field — display_title, summary, chapter titles and summaries, event captions, \
     visual and audio descriptions, topics, and searchable_text — in that same language. For example, \
     if the speech is Chinese, respond in Chinese. Use literal, factual, neutral descriptions \
     suitable for retrieval, not story narration or editorial copy. Describe only observable \
     visual and audible evidence. Do not infer plot, genre, intentions, identities, roles, or \
     relationships; labels such as protagonist, father, or uncle are allowed only when explicitly \
     spoken or shown as text. Keep chapter titles objective and avoid dramatic adjectives. Avoid \
     repeating the same wording across a chapter and its events. Prefer concise, searchable language. Make \
     display_title a short content title, not a source filename, URL, platform ID, model name, or \
     provider name. If exact timing is uncertain, estimate a short range. Do not invent people, \
     brands, or text that are not visible or audible."
}

fn video_understanding_schema() -> Value {
    // Gemini's `generationConfig.responseSchema` is an OpenAPI-style Schema
    // whose `type` is the `Type` enum (UPPERCASE: OBJECT/ARRAY/STRING/NUMBER),
    // not JSON Schema's lowercase strings.
    json!({
        "type": "OBJECT",
        "properties": {
            "summary": {
                "type": "STRING",
                "description": "A concise full-video summary."
            },
            "display_title": {
                "type": "STRING",
                "description": "A concise human-readable title for this video, written in the video's primary language. Do not include source filenames, URLs, platform IDs, or model/provider names."
            },
            "chapters": {
                "type": "ARRAY",
                "items": {
                    "type": "OBJECT",
                    "properties": {
                        "start_sec": { "type": "NUMBER" },
                        "end_sec": { "type": "NUMBER" },
                        "title": { "type": "STRING", "description": "A neutral factual label for the visible activity or setting; no dramatic or editorial language." },
                        "summary": { "type": "STRING", "description": "A concise factual summary grounded only in visible or audible evidence." }
                    },
                    "required": ["start_sec", "end_sec", "title", "summary"]
                }
            },
            "events": {
                "type": "ARRAY",
                "items": {
                    "type": "OBJECT",
                    "properties": {
                        "start_sec": { "type": "NUMBER" },
                        "end_sec": { "type": "NUMBER" },
                        "caption": { "type": "STRING", "description": "A neutral factual event label that does not repeat the chapter wording." },
                        "visual": { "type": "STRING", "description": "Only directly observable visual details, without inferred plot, roles, or relationships." },
                        "audio": { "type": "STRING", "description": "Only directly audible speech or sounds, without inferred context." },
                        "actions": {
                            "type": "ARRAY",
                            "items": { "type": "STRING" }
                        },
                        "entities": {
                            "type": "ARRAY",
                            "items": { "type": "STRING" }
                        },
                        "confidence": { "type": "NUMBER" }
                    },
                    "required": ["start_sec", "end_sec", "caption", "visual", "audio", "actions", "entities", "confidence"]
                }
            },
            "topics": {
                "type": "ARRAY",
                "items": { "type": "STRING" }
            },
            "searchable_text": {
                "type": "STRING",
                "description": "Dense search text combining summary, topics, chapters, and key visual/audio events."
            }
        },
        "required": ["summary", "display_title", "chapters", "events", "topics", "searchable_text"]
    })
}

fn parse_video_understanding_output(text: &str) -> anyhow::Result<Value> {
    let value: Value = serde_json::from_str(strip_json_fence(text)).map_err(|error| {
        anyhow::anyhow!("Gemini video understanding response was not valid JSON: {error}")
    })?;
    anyhow::ensure!(
        value.get("summary").and_then(Value::as_str).is_some(),
        "Gemini video understanding response did not include summary"
    );
    Ok(value)
}

fn searchable_text_from_result(result: &Value) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(display_title) = display_title_from_result(result) {
        push_search_part(&mut parts, display_title);
    }

    let explicit = result
        .get("searchable_text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(explicit) = explicit {
        push_search_part(&mut parts, explicit.to_string());
        return Some(parts.join("\n"));
    }

    if let Some(summary) = result.get("summary").and_then(Value::as_str) {
        push_search_part(&mut parts, summary);
    }
    parts.extend(chapters_from_result(result).into_iter().map(|chapter| {
        format!("{} {}", chapter.title, chapter.summary)
            .trim()
            .to_string()
    }));
    parts.extend(
        events_from_result(result)
            .into_iter()
            .map(|event| event_search_text(&event)),
    );
    parts.extend(string_array(result.get("topics")));
    let combined = parts
        .into_iter()
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if combined.is_empty() {
        None
    } else {
        Some(combined)
    }
}

fn push_search_part(parts: &mut Vec<String>, part: impl Into<String>) {
    let part = part.into().trim().to_string();
    if part.is_empty() || parts.iter().any(|existing| existing == &part) {
        return;
    }
    parts.push(part);
}

fn display_title_from_result(result: &Value) -> Option<String> {
    result
        .get("display_title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(120).collect::<String>())
}

fn chapters_from_result(result: &Value) -> Vec<VideoUnderstandingChapter> {
    result
        .get("chapters")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|chapter| {
            let title = string_field(chapter, "title")?;
            let summary = string_field(chapter, "summary").unwrap_or_default();
            Some(VideoUnderstandingChapter {
                start_sec: number_field(chapter, "start_sec")
                    .or_else(|| number_field(chapter, "start")),
                end_sec: number_field(chapter, "end_sec").or_else(|| number_field(chapter, "end")),
                title,
                summary,
            })
        })
        .collect()
}

fn events_from_result(result: &Value) -> Vec<VideoUnderstandingEvent> {
    result
        .get("events")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|event| {
            let caption = string_field(event, "caption")?;
            Some(VideoUnderstandingEvent {
                start_sec: number_field(event, "start_sec")
                    .or_else(|| number_field(event, "start")),
                end_sec: number_field(event, "end_sec").or_else(|| number_field(event, "end")),
                caption,
                visual: string_field(event, "visual"),
                audio: string_field(event, "audio"),
                actions: string_array(event.get("actions")),
                entities: string_array(event.get("entities")),
                confidence: number_field(event, "confidence"),
            })
        })
        .collect()
}

fn event_search_text(event: &VideoUnderstandingEvent) -> String {
    let mut parts = vec![event.caption.clone()];
    parts.extend(event.visual.clone());
    parts.extend(event.audio.clone());
    parts.extend(event.actions.clone());
    parts.extend(event.entities.clone());
    parts
        .into_iter()
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn number_field(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64)
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn parse_json(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| json!({}))
}

fn metadata_string(metadata: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| metadata.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn is_youtube_url(url: &str) -> bool {
    url.contains("youtube.com/") || url.contains("youtu.be/")
}

fn video_mime_type(path: &Path) -> anyhow::Result<String> {
    let Some(extension) = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
    else {
        anyhow::bail!("video file has no extension; Gemini video understanding needs a supported video MIME type");
    };
    let mime_type = match extension.as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mpeg" => "video/mpeg",
        "mov" => "video/mov",
        "avi" => "video/avi",
        "mpg" => "video/mpg",
        "webm" => "video/webm",
        "wmv" => "video/wmv",
        "3gp" | "3gpp" => "video/3gpp",
        other => anyhow::bail!(
            "video format .{other} is not supported by Gemini video understanding yet; use mp4, mov, avi, webm, wmv, mpg, mpeg, or 3gpp"
        ),
    };
    Ok(mime_type.to_string())
}

fn gemini_upload_files_url(base_url: &str) -> anyhow::Result<String> {
    let base = base_url.trim().trim_end_matches('/');
    anyhow::ensure!(
        !base.is_empty(),
        "Gemini provider has no base URL configured"
    );
    if base.ends_with("/upload/v1beta") {
        return Ok(format!("{base}/files"));
    }
    if let Some(prefix) = base.strip_suffix("/v1beta") {
        return Ok(format!("{prefix}/upload/v1beta/files"));
    }
    Ok(format!("{base}/upload/v1beta/files"))
}

fn provider_base_url(provider: &cerul_storage::providers::Provider) -> anyhow::Result<String> {
    provider
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(|url| url.trim_end_matches('/').to_string())
        .ok_or_else(|| anyhow::anyhow!("provider {} has no base_url configured", provider.label))
}

fn http_client() -> anyhow::Result<Client> {
    Ok(Client::builder().timeout(GEMINI_TIMEOUT).build()?)
}

fn send_json_with_retry<F>(mut build: F) -> anyhow::Result<Value>
where
    F: FnMut() -> anyhow::Result<RequestBuilder>,
{
    let mut last_error = None;
    for attempt in 1..=MAX_RETRIES {
        match build()?.send() {
            Ok(response) if response.status().is_success() => return response_json(response),
            Ok(response) => {
                let status = response.status();
                let body = response.text().unwrap_or_default();
                let message = format!("provider returned HTTP {status}: {body}");
                if !retryable_status(status) || attempt == MAX_RETRIES {
                    anyhow::bail!(message);
                }
                last_error = Some(message);
            }
            Err(error) => {
                let message = error.to_string();
                if attempt == MAX_RETRIES {
                    anyhow::bail!(
                        "provider request failed after {MAX_RETRIES} attempts: {message}"
                    );
                }
                last_error = Some(message);
            }
        }
        thread::sleep(RETRY_SLEEP);
    }

    anyhow::bail!(
        "provider request failed: {}",
        last_error.unwrap_or_else(|| "unknown error".to_string())
    )
}

fn retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn response_json(response: Response) -> anyhow::Result<Value> {
    let status = response.status();
    let value = response.json::<Value>()?;
    anyhow::ensure!(
        status.is_success(),
        "provider returned HTTP {status}: {value}"
    );
    Ok(value)
}

fn gemini_candidate_text(response: &Value) -> anyhow::Result<String> {
    response
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|candidates| candidates.first())
        .and_then(|candidate| candidate.get("content"))
        .and_then(|content| content.get("parts"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
        .pipe(|text| {
            if text.is_empty() {
                anyhow::bail!("Gemini video understanding response did not include text")
            } else {
                Ok(text)
            }
        })
}

fn strip_json_fence(text: &str) -> &str {
    let trimmed = text.trim();
    let Some(without_prefix) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    let without_lang = without_prefix
        .strip_prefix("json")
        .unwrap_or(without_prefix)
        .trim_start();
    without_lang
        .strip_suffix("```")
        .unwrap_or(without_lang)
        .trim()
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}

impl<T> Pipe for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn upload_url_uses_gemini_upload_host_path() {
        assert_eq!(
            gemini_upload_files_url("https://generativelanguage.googleapis.com/v1beta").unwrap(),
            "https://generativelanguage.googleapis.com/upload/v1beta/files"
        );
    }

    #[test]
    fn parses_structured_understanding_output() {
        let value = parse_video_understanding_output(
            r#"```json
            {
              "summary": "A screen recording shows an API key being configured.",
              "display_title": "Configuring an API key in settings",
              "chapters": [{"start_sec":0,"end_sec":30,"title":"Setup","summary":"The user opens settings."}],
              "events": [{"start_sec":4,"end_sec":8,"caption":"The settings page is opened.","visual":"A settings panel appears.","audio":"","actions":["open settings"],"entities":["settings"],"confidence":0.8}],
              "topics": ["settings", "api"],
              "searchable_text": "settings api key"
            }
            ```"#,
        )
        .unwrap();

        assert_eq!(
            value["summary"],
            "A screen recording shows an API key being configured."
        );
        assert_eq!(chapters_from_result(&value).len(), 1);
        assert_eq!(
            display_title_from_result(&value).as_deref(),
            Some("Configuring an API key in settings")
        );
        assert_eq!(events_from_result(&value)[0].actions, vec!["open settings"]);
        assert_eq!(
            searchable_text_from_result(&value).as_deref(),
            Some("Configuring an API key in settings\nsettings api key")
        );
    }

    #[test]
    fn understanding_prompt_requires_neutral_observable_descriptions() {
        let prompt = video_understanding_prompt();

        assert!(prompt.contains("literal, factual, neutral descriptions"));
        assert!(prompt.contains("Describe only observable visual and audible evidence"));
        assert!(prompt.contains("avoid dramatic adjectives"));
        assert!(prompt.contains("Avoid repeating the same wording"));
    }

    #[test]
    fn read_understanding_record_returns_not_started_when_missing() {
        let temp = tempfile::tempdir().unwrap();
        let paths = cerul_storage::AppPaths::from_data_dir(temp.path()).unwrap();
        let conn = Connection::open(&paths.db).unwrap();
        drop(conn);

        let record = read_understanding_record(&paths, "item-1").unwrap();

        assert_eq!(record.item_id, "item-1");
        assert_eq!(record.status, STATUS_NOT_STARTED);
        assert!(record.chapters.is_empty());
    }

    #[test]
    fn write_completed_record_queues_retrieval_refresh_without_replacing_live_units() {
        let temp = tempfile::tempdir().unwrap();
        let paths = cerul_storage::AppPaths::from_data_dir(temp.path()).unwrap();
        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        conn.execute(
            "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'folder_video', '{}', 'active')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO items (id, source_id, content_type, title, status, metadata) VALUES ('item-1', 'source-1', 'video', 'Demo video', 'indexed', '{}')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO jobs (id, item_id, job_type, status, progress) VALUES ('job-running', 'item-1', 'index_video', 'running', 0.5)",
            [],
        )
        .unwrap();
        drop(conn);
        cerul_storage::write_media_sqlite_chunks_with_ocr_and_lines(
            &paths,
            "item-1",
            &[cerul_storage::StorageTranscriptChunk {
                start: 0.0,
                end: 8.0,
                text: "previous searchable generation".to_string(),
            }],
            &[],
            &[],
            &[],
        )
        .unwrap();
        let profile = cerul_storage::vectors::ensure_active_embedding_profile(&paths).unwrap();
        let baseline_units =
            cerul_storage::rebuild_item_retrieval_units(&paths, "item-1", &profile.id).unwrap();
        cerul_storage::set_item_search_index_status(
            &paths,
            "item-1",
            "indexed",
            None,
            baseline_units.len(),
            1,
        )
        .unwrap();
        let conn = cerul_storage::sqlite::open(&paths).unwrap();

        let record = write_completed_record(
            &paths,
            "item-1",
            "provider-1",
            "model-1",
            json!({
                "summary": "A checkout flow highlights code XR-42.",
                "display_title": "Checkout flow with code XR-42",
                "chapters": [],
                "events": [{
                    "start_sec": 4.0,
                    "end_sec": 8.0,
                    "caption": "The checkout screen appears.",
                    "visual": "Visible code XR-42 is shown.",
                    "audio": "",
                    "actions": [],
                    "entities": ["XR-42"],
                    "confidence": 0.9
                }],
                "topics": ["checkout"],
                "searchable_text": "checkout visible code XR-42"
            }),
        )
        .unwrap();

        assert_eq!(record.status, STATUS_COMPLETED);
        assert_eq!(
            record.display_title.as_deref(),
            Some("Checkout flow with code XR-42")
        );
        let item_metadata: String = conn
            .query_row(
                "SELECT metadata FROM items WHERE id = 'item-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let item_metadata: Value = serde_json::from_str(&item_metadata).unwrap();
        assert_eq!(
            item_metadata.get("display_title").and_then(Value::as_str),
            Some("Checkout flow with code XR-42")
        );
        let retrieval_text: String = conn
            .query_row(
                "SELECT content_text FROM retrieval_units WHERE item_id = 'item-1' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(retrieval_text.contains("previous searchable generation"));
        assert!(!retrieval_text.contains("XR-42"));
        let item_index_state: (String, i64, i64) = conn
            .query_row(
                r#"
                SELECT search_index_status, search_index_unit_count, search_index_vector_count
                FROM items
                WHERE id = 'item-1'
                "#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            item_index_state,
            ("pending".to_string(), baseline_units.len() as i64, 1)
        );
        let (running_media_jobs, queued_media_jobs, queued_refresh_jobs): (i64, i64, i64) = conn
            .query_row(
                r#"
                SELECT
                  SUM(CASE WHEN job_type = 'index_video' AND status = 'running' THEN 1 ELSE 0 END),
                  SUM(CASE WHEN job_type = 'index_video' AND status = 'queued' THEN 1 ELSE 0 END),
                  SUM(CASE WHEN job_type = 'refresh_search_index' AND status = 'queued' THEN 1 ELSE 0 END)
                FROM jobs
                WHERE item_id = 'item-1'
                "#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(running_media_jobs, 1);
        assert_eq!(queued_media_jobs, 0);
        assert_eq!(queued_refresh_jobs, 1);

        write_status_record(
            &paths,
            "item-1",
            Some("provider-1"),
            Some("model-1"),
            STATUS_FAILED,
            Some("analysis failed"),
        )
        .unwrap();

        let remaining_understanding_chunks: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE item_id = 'item-1' AND chunk_type = 'understanding'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let remaining_retrieval_units: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM retrieval_units WHERE item_id = 'item-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining_understanding_chunks, 0);
        assert_eq!(remaining_retrieval_units, baseline_units.len() as i64);
        let item_metadata: String = conn
            .query_row(
                "SELECT metadata FROM items WHERE id = 'item-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let item_metadata: Value = serde_json::from_str(&item_metadata).unwrap();
        assert!(item_metadata.get("display_title").is_none());
    }

    #[test]
    fn write_completed_record_clears_stale_display_title_when_missing() {
        let temp = tempfile::tempdir().unwrap();
        let paths = cerul_storage::AppPaths::from_data_dir(temp.path()).unwrap();
        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        conn.execute(
            "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'folder_video', '{}', 'active')",
            [],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO items (id, source_id, content_type, title, status, metadata)
            VALUES (
              'item-1',
              'source-1',
              'video',
              'Demo video',
              'indexed',
              '{"display_title":"Old generated title"}'
            )
            "#,
            [],
        )
        .unwrap();
        drop(conn);
        cerul_storage::write_media_sqlite_chunks_with_ocr_and_lines(
            &paths,
            "item-1",
            &[cerul_storage::StorageTranscriptChunk {
                start: 0.0,
                end: 8.0,
                text: "baseline searchable content".to_string(),
            }],
            &[],
            &[],
            &[],
        )
        .unwrap();
        let profile = cerul_storage::vectors::ensure_active_embedding_profile(&paths).unwrap();
        let baseline_units =
            cerul_storage::rebuild_item_retrieval_units(&paths, "item-1", &profile.id).unwrap();
        cerul_storage::set_item_search_index_status(
            &paths,
            "item-1",
            "indexed",
            None,
            baseline_units.len(),
            1,
        )
        .unwrap();
        let conn = cerul_storage::sqlite::open(&paths).unwrap();

        let record = write_completed_record(
            &paths,
            "item-1",
            "provider-1",
            "model-1",
            json!({
                "summary": "Fresh analysis without a generated title.",
                "chapters": [],
                "events": [],
                "topics": ["fresh"],
                "searchable_text": "fresh searchable text"
            }),
        )
        .unwrap();

        assert_eq!(record.display_title, None);
        let item_metadata: String = conn
            .query_row(
                "SELECT metadata FROM items WHERE id = 'item-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let item_metadata: Value = serde_json::from_str(&item_metadata).unwrap();
        assert!(item_metadata.get("display_title").is_none());
        let retrieval_text: String = conn
            .query_row(
                "SELECT content_text FROM retrieval_units WHERE item_id = 'item-1' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(retrieval_text.contains("Old generated title"));
        assert!(retrieval_text.contains("baseline searchable content"));
        assert!(!retrieval_text.contains("fresh searchable text"));
        let (search_status, queued_refreshes): (String, i64) = conn
            .query_row(
                r#"
                SELECT i.search_index_status,
                       (SELECT COUNT(*) FROM jobs j
                        WHERE j.item_id = i.id
                          AND j.job_type = 'refresh_search_index'
                          AND j.status = 'queued')
                FROM items i
                WHERE i.id = 'item-1'
                "#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(search_status, "pending");
        assert_eq!(queued_refreshes, 1);
    }

    #[test]
    fn write_running_record_preserves_existing_search_vectors() {
        let temp = tempfile::tempdir().unwrap();
        let paths = cerul_storage::AppPaths::from_data_dir(temp.path()).unwrap();
        let conn = cerul_storage::sqlite::open(&paths).unwrap();
        conn.execute(
            "INSERT INTO sources (id, type, config, status) VALUES ('source-1', 'folder_video', '{}', 'active')",
            [],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO items (
                id, source_id, content_type, title, status, metadata,
                search_index_version, search_index_status, search_index_unit_count, search_index_vector_count
            )
            VALUES ('item-1', 'source-1', 'video', 'Demo video', 'indexed', '{}', ?1, 'indexed', 1, 3)
            "#,
            [cerul_storage::SEARCH_INDEX_VERSION],
        )
        .unwrap();
        cerul_storage::write_media_sqlite_chunks_with_ocr_and_lines(
            &paths,
            "item-1",
            &[cerul_storage::StorageTranscriptChunk {
                start: 0.0,
                end: 8.0,
                text: "existing transcript remains searchable".to_string(),
            }],
            &[],
            &[],
            &[],
        )
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO chunks (id, item_id, chunk_type, start_sec, end_sec, text, metadata)
            VALUES ('item-1:understanding:event:0000', 'item-1', 'understanding', 1, 2, 'old visual summary', '{}')
            "#,
            [],
        )
        .unwrap();

        write_status_record(
            &paths,
            "item-1",
            Some("provider-1"),
            Some("model-1"),
            STATUS_RUNNING,
            None,
        )
        .unwrap();

        let item_index_state: (String, i64, i64) = conn
            .query_row(
                r#"
                SELECT search_index_status, search_index_unit_count, search_index_vector_count
                FROM items
                WHERE id = 'item-1'
                "#,
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(item_index_state, ("indexed".to_string(), 1, 3));
        let remaining_understanding_chunks: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE item_id = 'item-1' AND chunk_type = 'understanding'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining_understanding_chunks, 1);
        let queued_jobs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM jobs WHERE item_id = 'item-1' AND job_type = 'index_video' AND status = 'queued'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(queued_jobs, 0);
    }
}
