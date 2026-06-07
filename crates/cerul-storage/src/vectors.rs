use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Mutex, OnceLock},
    time::Duration,
};

use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::paths::AppPaths;

pub const MODEL_INDEX_VERSION: i32 = 3;
pub const DEFAULT_EMBEDDING_PROFILE_ID: &str = "gemini-embedding-2-3072";
pub const LOCAL_EMBEDDING_PROFILE_ID: &str = "qwen3-vl-local-2048";
pub const LEGACY_DEFAULT_EMBEDDING_PROFILE_ID: &str = "qwen3-vl-2b-2048";
pub const LEGACY_QWEN3_EMBEDDING_PROFILE_ID: &str = "qwen3-vl-embedding-2b-2048";
pub const DEFAULT_EMBEDDING_PROVIDER_ID: &str = "gemini";
pub const DEFAULT_EMBEDDING_MODEL_ID: &str = "gemini-embedding-2";
pub const LOCAL_EMBEDDING_PROVIDER_ID: &str = "local";
pub const LOCAL_EMBEDDING_MODEL_ID: &str = "mlx-community/Qwen3-VL-Embedding-2B-6bit";
pub const DEFAULT_VECTOR_DIMENSIONS: i32 = 3072;
pub const LOCAL_VECTOR_DIMENSIONS: i32 = 2048;
pub const VECTOR_DIMENSIONS: i32 = DEFAULT_VECTOR_DIMENSIONS;
const DEFAULT_DISTANCE_METRIC: &str = "cosine";
const ACTIVE_EMBEDDING_PROFILE_SETTING: &str = "active_embedding_profile";
const DEFAULT_QDRANT_URL: &str = "http://127.0.0.1:6333";
const VECTOR_BATCH_SIZE: usize = 256;

static QDRANT_PROCESS: OnceLock<Mutex<Option<Child>>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingProfile {
    pub id: String,
    pub provider_id: String,
    pub model_id: String,
    pub model_revision: Option<String>,
    pub output_dimension: i32,
    pub distance_metric: String,
    pub instruction_template: Option<String>,
    pub index_version: i32,
    pub status: String,
}

impl EmbeddingProfile {
    pub fn table_names(&self) -> VectorCollectionNames {
        VectorCollectionNames::for_profile(&self.id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorCollectionNames {
    pub text: String,
    pub image: String,
}

impl VectorCollectionNames {
    pub fn for_profile(profile_id: &str) -> Self {
        Self::for_profile_in_namespace(profile_id, "cerul")
    }

    fn for_profile_in_namespace(profile_id: &str, namespace: &str) -> Self {
        let sanitized = sanitize_profile_id(profile_id);
        Self {
            text: format!("{namespace}__text_chunks__{sanitized}"),
            image: format!("{namespace}__image_chunks__{sanitized}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VectorRecord {
    pub chunk_id: String,
    pub item_id: String,
    pub vector: Vec<f32>,
}

impl VectorRecord {
    pub fn new(chunk_id: String, item_id: String, vector: Vec<f32>) -> anyhow::Result<Self> {
        Self::new_for_dimensions(chunk_id, item_id, vector, VECTOR_DIMENSIONS)
    }

    pub fn new_for_dimensions(
        chunk_id: String,
        item_id: String,
        vector: Vec<f32>,
        expected_dimensions: i32,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            vector.len() == expected_dimensions as usize,
            "vector for chunk {chunk_id} has {} dimensions, expected {expected_dimensions}",
            vector.len()
        );

        Ok(Self {
            chunk_id,
            item_id,
            vector,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VectorHit {
    pub chunk_id: String,
    pub score: f32,
}

#[derive(Debug, Clone)]
struct QdrantConfig {
    url: String,
    api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QdrantEnvelope<T> {
    status: String,
    result: T,
}

#[derive(Debug, Deserialize)]
struct QdrantCountResult {
    count: usize,
}

#[derive(Debug, Deserialize)]
struct QdrantScoredPoint {
    score: f32,
    payload: Option<HashMap<String, Value>>,
}

pub async fn ensure_collections(paths: &AppPaths) -> anyhow::Result<()> {
    let profile = ensure_active_embedding_profile(paths)?;
    ensure_collections_for_profile(paths, &profile).await
}

pub fn shutdown_qdrant_sidecar() {
    let Some(mutex) = QDRANT_PROCESS.get() else {
        return;
    };
    let mut guard = mutex.lock().expect("Qdrant process mutex poisoned");
    let Some(mut child) = guard.take() else {
        return;
    };

    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    if let Err(error) = child.kill() {
        tracing::warn!(%error, "failed to stop local Qdrant sidecar");
    }
    if let Err(error) = child.wait() {
        tracing::warn!(%error, "failed to wait for local Qdrant sidecar shutdown");
    }
}

pub async fn ensure_collections_for_profile(
    paths: &AppPaths,
    profile: &EmbeddingProfile,
) -> anyhow::Result<()> {
    ensure_qdrant_ready(paths).await?;
    let collections = collection_names(paths, profile);
    for collection in [&collections.text, &collections.image] {
        ensure_collection(paths, collection, profile).await?;
    }
    Ok(())
}

pub async fn replace_item_embeddings(
    paths: &AppPaths,
    item_id: &str,
    text_records: &[VectorRecord],
    image_records: &[VectorRecord],
) -> anyhow::Result<()> {
    let profile = ensure_active_embedding_profile(paths)?;
    replace_item_embeddings_for_profile(paths, item_id, text_records, image_records, &profile).await
}

pub async fn replace_item_embeddings_for_profile(
    paths: &AppPaths,
    item_id: &str,
    text_records: &[VectorRecord],
    image_records: &[VectorRecord],
    profile: &EmbeddingProfile,
) -> anyhow::Result<()> {
    ensure_collections_for_profile(paths, profile).await?;
    let collections = collection_names(paths, profile);

    replace_collection_item_embeddings(paths, &collections.text, item_id, text_records).await?;
    replace_collection_item_embeddings(paths, &collections.image, item_id, image_records).await?;
    Ok(())
}

pub async fn delete_item_embeddings(paths: &AppPaths, item_id: &str) -> anyhow::Result<()> {
    ensure_qdrant_ready(paths).await?;
    let profiles = list_embedding_profiles(paths)?;

    for profile in profiles {
        let collections = collection_names(paths, &profile);
        for collection in [collections.text, collections.image] {
            if collection_exists(paths, &collection).await? {
                delete_collection_item_embeddings(paths, &collection, item_id).await?;
            }
        }
    }

    Ok(())
}

pub async fn collection_point_count(paths: &AppPaths, collection: &str) -> anyhow::Result<usize> {
    ensure_qdrant_ready(paths).await?;
    if !collection_exists(paths, collection).await? {
        return Ok(0);
    }

    let result: QdrantCountResult = qdrant_post(
        paths,
        &format!("/collections/{collection}/points/count"),
        Some(&[("wait", "true")]),
        &json!({ "exact": true }),
    )
    .await?;
    Ok(result.count)
}

pub fn collection_names(paths: &AppPaths, profile: &EmbeddingProfile) -> VectorCollectionNames {
    VectorCollectionNames::for_profile_in_namespace(&profile.id, &collection_namespace(paths))
}

pub async fn search_collection(
    paths: &AppPaths,
    collection: &str,
    query_vector: &[f32],
    limit: usize,
) -> anyhow::Result<Vec<VectorHit>> {
    ensure_qdrant_ready(paths).await?;
    if !collection_exists(paths, collection).await? {
        return Ok(Vec::new());
    }

    let points: Vec<QdrantScoredPoint> = qdrant_post(
        paths,
        &format!("/collections/{collection}/points/search"),
        None,
        &json!({
            "vector": query_vector,
            "limit": limit.max(1),
            "with_payload": true,
            "with_vector": false
        }),
    )
    .await?;

    Ok(points
        .into_iter()
        .filter_map(|point| {
            let chunk_id = point
                .payload?
                .get("chunk_id")?
                .as_str()
                .map(ToOwned::to_owned)?;
            Some(VectorHit {
                chunk_id,
                score: point.score,
            })
        })
        .collect())
}

async fn replace_collection_item_embeddings(
    paths: &AppPaths,
    collection: &str,
    item_id: &str,
    records: &[VectorRecord],
) -> anyhow::Result<()> {
    delete_collection_item_embeddings(paths, collection, item_id).await?;

    for batch in records.chunks(VECTOR_BATCH_SIZE) {
        if batch.is_empty() {
            continue;
        }

        let points = batch
            .iter()
            .map(|record| {
                json!({
                    "id": point_id(&record.chunk_id),
                    "vector": record.vector,
                    "payload": {
                        "chunk_id": record.chunk_id,
                        "item_id": record.item_id
                    }
                })
            })
            .collect::<Vec<_>>();

        let _: Value = qdrant_put(
            paths,
            &format!("/collections/{collection}/points"),
            Some(&[("wait", "true")]),
            &json!({ "points": points }),
        )
        .await?;
    }

    Ok(())
}

async fn delete_collection_item_embeddings(
    paths: &AppPaths,
    collection: &str,
    item_id: &str,
) -> anyhow::Result<()> {
    let _: Value = qdrant_post(
        paths,
        &format!("/collections/{collection}/points/delete"),
        Some(&[("wait", "true")]),
        &json!({
            "filter": {
                "must": [
                    {
                        "key": "item_id",
                        "match": { "value": item_id }
                    }
                ]
            }
        }),
    )
    .await?;
    Ok(())
}

async fn ensure_collection(
    paths: &AppPaths,
    collection: &str,
    profile: &EmbeddingProfile,
) -> anyhow::Result<()> {
    if collection_exists(paths, collection).await? {
        return Ok(());
    }

    let _: Value = qdrant_put(
        paths,
        &format!("/collections/{collection}"),
        Some(&[("wait", "true")]),
        &json!({
            "vectors": {
                "size": profile.output_dimension,
                "distance": qdrant_distance(&profile.distance_metric)?,
                "on_disk": true
            },
            "on_disk_payload": true
        }),
    )
    .await?;

    ensure_payload_index(paths, collection, "item_id").await;
    ensure_payload_index(paths, collection, "chunk_id").await;
    Ok(())
}

async fn ensure_payload_index(paths: &AppPaths, collection: &str, field: &str) {
    let result: anyhow::Result<Value> = qdrant_put(
        paths,
        &format!("/collections/{collection}/index"),
        Some(&[("wait", "true")]),
        &json!({
            "field_name": field,
            "field_schema": "keyword"
        }),
    )
    .await;

    if let Err(error) = result {
        tracing::warn!(%error, collection, field, "failed to create Qdrant payload index");
    }
}

async fn collection_exists(paths: &AppPaths, collection: &str) -> anyhow::Result<bool> {
    match qdrant_get::<Value>(paths, &format!("/collections/{collection}")).await {
        Ok(_) => Ok(true),
        Err(error) if qdrant_error_is_not_found(&error) => Ok(false),
        Err(error) => Err(error),
    }
}

async fn ensure_qdrant_ready(paths: &AppPaths) -> anyhow::Result<()> {
    if qdrant_health().await {
        return Ok(());
    }

    let config = qdrant_config();
    if !qdrant_autostart_enabled(&config) {
        anyhow::bail!(
            "Qdrant is not reachable at {}. Start Qdrant or set CERUL_QDRANT_URL.",
            config.url
        );
    }

    maybe_spawn_qdrant(paths, &config)?;

    for _ in 0..50 {
        if qdrant_health().await {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    anyhow::bail!("Qdrant did not become ready at {}", config.url)
}

async fn qdrant_health() -> bool {
    let config = qdrant_config();
    let client = qdrant_client();
    let root = config.url.trim_end_matches('/');

    for endpoint in ["/healthz", "/"] {
        let mut request = client.get(format!("{root}{endpoint}"));
        if let Some(api_key) = &config.api_key {
            request = request.header("api-key", api_key);
        }
        if matches!(request.send().await, Ok(response) if response.status().is_success()) {
            return true;
        }
    }

    false
}

fn maybe_spawn_qdrant(paths: &AppPaths, config: &QdrantConfig) -> anyhow::Result<()> {
    ensure_data_dirs(paths)?;

    let mutex = QDRANT_PROCESS.get_or_init(|| Mutex::new(None));
    let mut guard = mutex.lock().expect("Qdrant process mutex poisoned");
    if guard
        .as_mut()
        .is_some_and(|child| child.try_wait().ok().flatten().is_none())
    {
        return Ok(());
    }

    let binary = find_qdrant_binary().ok_or_else(|| {
        anyhow::anyhow!(
            "Qdrant is not reachable at {} and no qdrant binary was found. Run scripts/fetch-binaries.sh or set CERUL_QDRANT_BIN.",
            config.url
        )
    })?;
    let parsed = reqwest::Url::parse(&config.url)?;
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| anyhow::anyhow!("Qdrant URL has no port: {}", config.url))?;
    let grpc_port = port.saturating_add(1);

    let mut command = Command::new(binary);
    command
        .current_dir(&paths.qdrant)
        .env(
            "QDRANT__STORAGE__STORAGE_PATH",
            paths.qdrant.to_string_lossy().into_owned(),
        )
        .env(
            "QDRANT__STORAGE__SNAPSHOTS_PATH",
            paths
                .qdrant
                .join("snapshots")
                .to_string_lossy()
                .into_owned(),
        )
        .env("QDRANT__SERVICE__HTTP_PORT", port.to_string())
        .env("QDRANT__SERVICE__GRPC_PORT", grpc_port.to_string())
        .env("QDRANT__LOG_LEVEL", "WARN")
        .env("QDRANT__TELEMETRY_DISABLED", "true")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    tracing::info!(%port, storage = %paths.qdrant.display(), "starting local Qdrant sidecar");
    let child = command.spawn()?;
    *guard = Some(child);
    Ok(())
}

fn find_qdrant_binary() -> Option<PathBuf> {
    let exe = qdrant_exe_name();
    if let Some(path) = std::env::var_os("CERUL_QDRANT_BIN").map(PathBuf::from) {
        if path.is_file() {
            return Some(path);
        }
    }

    for root in candidate_roots() {
        let candidate = root.join("third-party").join(host_target()).join(&exe);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(target_os = "macos")]
        {
            let candidate = root
                .join("Contents")
                .join("Resources")
                .join("third-party")
                .join(host_target())
                .join(&exe);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    find_on_path(&exe)
}

fn candidate_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        push_ancestors(&mut roots, &cwd);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            push_ancestors(&mut roots, parent);
        }
    }
    roots
}

fn push_ancestors(roots: &mut Vec<PathBuf>, start: &Path) {
    for path in start.ancestors().take(8) {
        let candidate = path.to_path_buf();
        if !roots.contains(&candidate) {
            roots.push(candidate);
        }
    }
}

fn find_on_path(exe: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|path| path.join(exe))
        .find(|candidate| candidate.is_file())
}

fn qdrant_exe_name() -> String {
    if cfg!(target_os = "windows") {
        "qdrant.exe".to_string()
    } else {
        "qdrant".to_string()
    }
}

fn host_target() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        _ => "unsupported",
    }
}

fn qdrant_autostart_enabled(config: &QdrantConfig) -> bool {
    match std::env::var("CERUL_QDRANT_AUTOSTART") {
        Ok(value) => !matches!(value.as_str(), "0" | "false" | "FALSE" | "no" | "NO"),
        Err(_) => config.url == DEFAULT_QDRANT_URL,
    }
}

fn qdrant_config() -> QdrantConfig {
    QdrantConfig {
        url: std::env::var("CERUL_QDRANT_URL").unwrap_or_else(|_| DEFAULT_QDRANT_URL.to_string()),
        api_key: std::env::var("CERUL_QDRANT_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty()),
    }
}

fn qdrant_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("Qdrant reqwest client should build")
}

async fn qdrant_get<T: for<'de> Deserialize<'de>>(
    paths: &AppPaths,
    path: &str,
) -> anyhow::Result<T> {
    ensure_data_dirs(paths)?;
    let config = qdrant_config();
    let mut request = qdrant_client().get(qdrant_url(&config, path, None));
    if let Some(api_key) = &config.api_key {
        request = request.header("api-key", api_key);
    }
    qdrant_send(request).await
}

async fn qdrant_put<T: for<'de> Deserialize<'de>>(
    paths: &AppPaths,
    path: &str,
    query: Option<&[(&str, &str)]>,
    body: &Value,
) -> anyhow::Result<T> {
    ensure_data_dirs(paths)?;
    let config = qdrant_config();
    let mut request = qdrant_client()
        .put(qdrant_url(&config, path, query))
        .json(body);
    if let Some(api_key) = &config.api_key {
        request = request.header("api-key", api_key);
    }
    qdrant_send(request).await
}

async fn qdrant_post<T: for<'de> Deserialize<'de>>(
    paths: &AppPaths,
    path: &str,
    query: Option<&[(&str, &str)]>,
    body: &Value,
) -> anyhow::Result<T> {
    ensure_data_dirs(paths)?;
    let config = qdrant_config();
    let mut request = qdrant_client()
        .post(qdrant_url(&config, path, query))
        .json(body);
    if let Some(api_key) = &config.api_key {
        request = request.header("api-key", api_key);
    }
    qdrant_send(request).await
}

async fn qdrant_send<T: for<'de> Deserialize<'de>>(
    request: reqwest::RequestBuilder,
) -> anyhow::Result<T> {
    let response = request.send().await?;
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        anyhow::bail!("Qdrant request failed ({status}): {text}");
    }
    let envelope = serde_json::from_str::<QdrantEnvelope<T>>(&text)?;
    anyhow::ensure!(
        envelope.status == "ok",
        "Qdrant returned non-ok status: {}",
        envelope.status
    );
    Ok(envelope.result)
}

fn qdrant_url(config: &QdrantConfig, path: &str, query: Option<&[(&str, &str)]>) -> String {
    let mut url = format!("{}{}", config.url.trim_end_matches('/'), path);
    if let Some(query) = query {
        let query = query
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("&");
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query);
        }
    }
    url
}

fn ensure_data_dirs(paths: &AppPaths) -> anyhow::Result<()> {
    std::fs::create_dir_all(&paths.qdrant)?;
    Ok(())
}

fn qdrant_error_is_not_found(error: &anyhow::Error) -> bool {
    let message = error.to_string();
    message.contains(StatusCode::NOT_FOUND.as_str()) || message.contains("Not found")
}

fn qdrant_distance(metric: &str) -> anyhow::Result<&'static str> {
    match metric.to_ascii_lowercase().as_str() {
        "cosine" => Ok("Cosine"),
        "dot" | "ip" => Ok("Dot"),
        "euclid" | "euclidean" | "l2" => Ok("Euclid"),
        other => anyhow::bail!("unsupported Qdrant distance metric {other:?}"),
    }
}

fn point_id(chunk_id: &str) -> String {
    Uuid::new_v5(&Uuid::NAMESPACE_URL, chunk_id.as_bytes()).to_string()
}

fn collection_namespace(paths: &AppPaths) -> String {
    let data_dir = paths.data.to_string_lossy();
    format!(
        "cerul_{}",
        Uuid::new_v5(&Uuid::NAMESPACE_URL, data_dir.as_bytes()).simple()
    )
}

pub fn ensure_active_embedding_profile(paths: &AppPaths) -> anyhow::Result<EmbeddingProfile> {
    let conn = crate::sqlite::open(paths)?;
    ensure_builtin_embedding_profiles(&conn)?;
    archive_legacy_default_embedding_profile(&conn)?;

    let selected = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [ACTIVE_EMBEDDING_PROFILE_SETTING],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|value| parse_setting_string(&value));

    let profile_id = selected
        .as_deref()
        .map(canonical_embedding_profile_id)
        .unwrap_or_else(|| DEFAULT_EMBEDDING_PROFILE_ID.to_string());
    let profile =
        load_embedding_profile(&conn, &profile_id)?.unwrap_or_else(default_embedding_profile);

    conn.execute(
        r#"
        INSERT INTO settings (key, value, updated_at)
        VALUES (?1, ?2, strftime('%s','now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at
        "#,
        (
            ACTIVE_EMBEDDING_PROFILE_SETTING,
            serde_json::Value::String(profile.id.clone()).to_string(),
        ),
    )?;

    Ok(profile)
}

pub fn ensure_embedding_profile_for_inference_mode(
    paths: &AppPaths,
    inference_mode: &str,
) -> anyhow::Result<EmbeddingProfile> {
    let profile_id = if inference_mode.trim().eq_ignore_ascii_case("local") {
        LOCAL_EMBEDDING_PROFILE_ID
    } else {
        DEFAULT_EMBEDDING_PROFILE_ID
    };
    set_active_embedding_profile(paths, profile_id)
}

pub fn embedding_profile_for_inference_mode(
    paths: &AppPaths,
    inference_mode: &str,
) -> anyhow::Result<EmbeddingProfile> {
    let profile_id = if inference_mode.trim().eq_ignore_ascii_case("local") {
        LOCAL_EMBEDDING_PROFILE_ID
    } else {
        DEFAULT_EMBEDDING_PROFILE_ID
    };
    embedding_profile_by_id(paths, profile_id)
}

pub fn embedding_profile_by_id(
    paths: &AppPaths,
    profile_id: &str,
) -> anyhow::Result<EmbeddingProfile> {
    let conn = crate::sqlite::open(paths)?;
    ensure_builtin_embedding_profiles(&conn)?;
    archive_legacy_default_embedding_profile(&conn)?;
    let profile_id = canonical_embedding_profile_id(profile_id);
    Ok(load_embedding_profile(&conn, &profile_id)?.unwrap_or_else(default_embedding_profile))
}

pub fn set_active_embedding_profile(
    paths: &AppPaths,
    profile_id: &str,
) -> anyhow::Result<EmbeddingProfile> {
    let conn = crate::sqlite::open(paths)?;
    ensure_builtin_embedding_profiles(&conn)?;
    archive_legacy_default_embedding_profile(&conn)?;
    let profile_id = canonical_embedding_profile_id(profile_id);
    let profile =
        load_embedding_profile(&conn, &profile_id)?.unwrap_or_else(default_embedding_profile);

    conn.execute(
        r#"
        INSERT INTO settings (key, value, updated_at)
        VALUES (?1, ?2, strftime('%s','now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at
        "#,
        (
            ACTIVE_EMBEDDING_PROFILE_SETTING,
            serde_json::Value::String(profile.id.clone()).to_string(),
        ),
    )?;

    Ok(profile)
}

pub fn list_embedding_profiles(paths: &AppPaths) -> anyhow::Result<Vec<EmbeddingProfile>> {
    let conn = crate::sqlite::open(paths)?;
    ensure_builtin_embedding_profiles(&conn)?;
    archive_legacy_default_embedding_profile(&conn)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT id, provider_id, model_id, model_revision, output_dimension, distance_metric,
               instruction_template, index_version, status
        FROM embedding_profiles
        ORDER BY
            CASE status
                WHEN 'active' THEN 0
                WHEN 'building' THEN 1
                WHEN 'failed' THEN 2
                WHEN 'stale' THEN 3
                WHEN 'archived' THEN 4
                ELSE 5
            END,
            id
        "#,
    )?;

    let profiles = stmt
        .query_map([], profile_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(profiles)
}

pub fn active_embedding_profile_id(paths: &AppPaths) -> anyhow::Result<String> {
    Ok(ensure_active_embedding_profile(paths)?.id)
}

pub fn is_default_embedding_profile_id(profile_id: &str) -> bool {
    profile_id == DEFAULT_EMBEDDING_PROFILE_ID
}

fn canonical_embedding_profile_id(profile_id: &str) -> String {
    match profile_id {
        LEGACY_DEFAULT_EMBEDDING_PROFILE_ID | LEGACY_QWEN3_EMBEDDING_PROFILE_ID => {
            DEFAULT_EMBEDDING_PROFILE_ID.to_string()
        }
        _ => profile_id.to_string(),
    }
}

fn ensure_builtin_embedding_profiles(conn: &rusqlite::Connection) -> anyhow::Result<()> {
    for profile in [default_embedding_profile(), local_embedding_profile()] {
        upsert_embedding_profile(conn, &profile)?;
    }
    Ok(())
}

fn upsert_embedding_profile(
    conn: &rusqlite::Connection,
    profile: &EmbeddingProfile,
) -> anyhow::Result<()> {
    conn.execute(
        r#"
        INSERT INTO embedding_profiles (
            id, provider_id, model_id, model_revision, output_dimension, distance_metric,
            instruction_template, index_version, status, updated_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, strftime('%s','now'))
        ON CONFLICT(id) DO UPDATE SET
            provider_id = excluded.provider_id,
            model_id = excluded.model_id,
            model_revision = excluded.model_revision,
            output_dimension = excluded.output_dimension,
            distance_metric = excluded.distance_metric,
            instruction_template = excluded.instruction_template,
            index_version = excluded.index_version,
            status = CASE
                WHEN embedding_profiles.status IN ('building', 'failed', 'archived')
                THEN embedding_profiles.status
                ELSE excluded.status
            END,
            updated_at = excluded.updated_at
        "#,
        (
            &profile.id,
            &profile.provider_id,
            &profile.model_id,
            &profile.model_revision,
            profile.output_dimension,
            &profile.distance_metric,
            &profile.instruction_template,
            profile.index_version,
            &profile.status,
        ),
    )?;
    Ok(())
}

fn archive_legacy_default_embedding_profile(conn: &rusqlite::Connection) -> anyhow::Result<()> {
    conn.execute(
        r#"
        UPDATE embedding_profiles
        SET status = 'archived',
            updated_at = strftime('%s','now')
        WHERE id IN (?1, ?2)
          AND status = 'active'
        "#,
        [
            LEGACY_DEFAULT_EMBEDDING_PROFILE_ID,
            LEGACY_QWEN3_EMBEDDING_PROFILE_ID,
        ],
    )?;
    Ok(())
}

fn load_embedding_profile(
    conn: &rusqlite::Connection,
    id: &str,
) -> anyhow::Result<Option<EmbeddingProfile>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, provider_id, model_id, model_revision, output_dimension, distance_metric,
               instruction_template, index_version, status
        FROM embedding_profiles
        WHERE id = ?1
        "#,
    )?;
    let mut rows = stmt.query([id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    Ok(Some(profile_from_row(row)?))
}

fn profile_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EmbeddingProfile> {
    Ok(EmbeddingProfile {
        id: row.get(0)?,
        provider_id: row.get(1)?,
        model_id: row.get(2)?,
        model_revision: row.get(3)?,
        output_dimension: row.get(4)?,
        distance_metric: row.get(5)?,
        instruction_template: row.get(6)?,
        index_version: row.get(7)?,
        status: row.get(8)?,
    })
}

fn default_embedding_profile() -> EmbeddingProfile {
    EmbeddingProfile {
        id: DEFAULT_EMBEDDING_PROFILE_ID.to_string(),
        provider_id: DEFAULT_EMBEDDING_PROVIDER_ID.to_string(),
        model_id: DEFAULT_EMBEDDING_MODEL_ID.to_string(),
        model_revision: None,
        output_dimension: DEFAULT_VECTOR_DIMENSIONS,
        distance_metric: DEFAULT_DISTANCE_METRIC.to_string(),
        instruction_template: Some("title: none | text: {content}".to_string()),
        index_version: MODEL_INDEX_VERSION,
        status: "active".to_string(),
    }
}

fn local_embedding_profile() -> EmbeddingProfile {
    EmbeddingProfile {
        id: LOCAL_EMBEDDING_PROFILE_ID.to_string(),
        provider_id: LOCAL_EMBEDDING_PROVIDER_ID.to_string(),
        model_id: LOCAL_EMBEDDING_MODEL_ID.to_string(),
        model_revision: None,
        output_dimension: LOCAL_VECTOR_DIMENSIONS,
        distance_metric: DEFAULT_DISTANCE_METRIC.to_string(),
        instruction_template: Some(
            "Represent this multimodal memory chunk for retrieval.".to_string(),
        ),
        index_version: MODEL_INDEX_VERSION,
        status: "active".to_string(),
    }
}

fn parse_setting_string(raw: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .or_else(|| Some(raw.to_string()))
}

fn sanitize_profile_id(profile_id: &str) -> String {
    profile_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}
