use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zvec::{
    Collection, CollectionSchema, DataType, Doc, FieldSchema, IndexParams, IndexType, MetricType,
    VectorQuery,
};

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
const VECTOR_BATCH_SIZE: usize = 256;
const CHUNK_ID_FIELD: &str = "chunk_id";
const ITEM_ID_FIELD: &str = "item_id";
const VECTOR_FIELD: &str = "vector";
const ZVEC_HNSW_M: i32 = 16;
const ZVEC_HNSW_EF_CONSTRUCTION: i32 = 200;
const ZVEC_COLLECTION_CONFIG_SUFFIX: &str = ".cerul.json";

type CollectionHandle = Arc<Mutex<Collection>>;
type CollectionCache = HashMap<PathBuf, CollectionHandle>;

static ZVEC_COLLECTIONS: OnceLock<Mutex<CollectionCache>> = OnceLock::new();

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
    pub point_key: String,
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
        Self::new_for_dimensions_with_point_key(
            chunk_id.clone(),
            chunk_id,
            item_id,
            vector,
            expected_dimensions,
        )
    }

    pub fn new_for_dimensions_with_point_key(
        point_key: String,
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
            point_key,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ZvecCollectionConfig {
    collection: String,
    profile_id: String,
    output_dimension: i32,
    distance_metric: String,
}

pub async fn ensure_collections(paths: &AppPaths) -> anyhow::Result<()> {
    let profile = ensure_active_embedding_profile(paths)?;
    ensure_unified_collection_for_profile(paths, &profile, crate::SEARCH_INDEX_VERSION).await
}

pub fn shutdown_vector_index() {
    let Some(cache) = ZVEC_COLLECTIONS.get() else {
        return;
    };
    if let Ok(mut guard) = cache.lock() {
        guard.clear();
    }
}

pub async fn ensure_collections_for_profile(
    paths: &AppPaths,
    profile: &EmbeddingProfile,
) -> anyhow::Result<()> {
    let collections = collection_names(paths, profile);
    for collection in [&collections.text, &collections.image] {
        ensure_collection(paths, collection, profile)?;
    }
    Ok(())
}

pub async fn ensure_unified_collection_for_profile(
    paths: &AppPaths,
    profile: &EmbeddingProfile,
    index_version: i32,
) -> anyhow::Result<()> {
    let collection = unified_collection_name(paths, profile, index_version);
    ensure_collection(paths, &collection, profile)?;
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

pub async fn replace_item_unified_embeddings_for_profile(
    paths: &AppPaths,
    item_id: &str,
    records: &[VectorRecord],
    profile: &EmbeddingProfile,
    index_version: i32,
) -> anyhow::Result<()> {
    let collection = unified_collection_name(paths, profile, index_version);
    ensure_unified_collection_for_profile(paths, profile, index_version).await?;
    replace_collection_item_embeddings(paths, &collection, item_id, records).await
}

pub async fn upsert_item_unified_embeddings_for_profile(
    paths: &AppPaths,
    records: &[VectorRecord],
    profile: &EmbeddingProfile,
    index_version: i32,
) -> anyhow::Result<()> {
    let collection = unified_collection_name(paths, profile, index_version);
    ensure_unified_collection_for_profile(paths, profile, index_version).await?;
    upsert_collection_embeddings(paths, &collection, records).await
}

pub async fn delete_stale_item_unified_embeddings_for_profile(
    paths: &AppPaths,
    item_id: &str,
    keep_records: &[VectorRecord],
    profile: &EmbeddingProfile,
    index_version: i32,
) -> anyhow::Result<usize> {
    let collection = unified_collection_name(paths, profile, index_version);
    if !collection_exists(paths, &collection).await? {
        return Ok(0);
    }
    upsert_collection_embeddings(paths, &collection, keep_records).await?;
    delete_stale_collection_item_embeddings(paths, &collection, item_id, keep_records).await
}

pub async fn delete_item_embeddings(paths: &AppPaths, item_id: &str) -> anyhow::Result<()> {
    let profiles = list_embedding_profiles(paths)?;

    for profile in profiles {
        let collections = collection_names(paths, &profile);
        for collection in [collections.text, collections.image] {
            if collection_exists(paths, &collection).await? {
                delete_collection_item_embeddings(paths, &collection, item_id).await?;
            }
        }
        let unified = unified_collection_name(paths, &profile, crate::SEARCH_INDEX_VERSION);
        if collection_exists(paths, &unified).await? {
            delete_collection_item_embeddings(paths, &unified, item_id).await?;
        }
    }

    Ok(())
}

pub async fn collection_point_count(paths: &AppPaths, collection: &str) -> anyhow::Result<usize> {
    let Some(handle) = collection_handle_existing(paths, collection)? else {
        return Ok(0);
    };
    collection_doc_count(handle)
}

pub async fn collection_point_count_for_profile(
    paths: &AppPaths,
    collection: &str,
    profile: &EmbeddingProfile,
) -> anyhow::Result<usize> {
    let Some(handle) = collection_handle_existing_for_profile(paths, collection, profile)? else {
        return Ok(0);
    };
    collection_doc_count(handle)
}

fn collection_doc_count(handle: CollectionHandle) -> anyhow::Result<usize> {
    let guard = handle
        .lock()
        .map_err(|_| anyhow::anyhow!("zvec collection mutex poisoned"))?;
    Ok(guard.stats()?.doc_count() as usize)
}

pub fn collection_names(paths: &AppPaths, profile: &EmbeddingProfile) -> VectorCollectionNames {
    VectorCollectionNames::for_profile_in_namespace(&profile.id, &collection_namespace(paths))
}

pub fn unified_collection_name(
    paths: &AppPaths,
    profile: &EmbeddingProfile,
    index_version: i32,
) -> String {
    let namespace = collection_namespace(paths);
    let sanitized = sanitize_profile_id(&profile.id);
    format!("{namespace}__retrieval_units_v{index_version}__{sanitized}")
}

pub async fn search_collection_for_profile(
    paths: &AppPaths,
    collection: &str,
    query_vector: &[f32],
    limit: usize,
    profile: &EmbeddingProfile,
) -> anyhow::Result<Vec<VectorHit>> {
    let Some(handle) = collection_handle_existing_for_profile(paths, collection, profile)? else {
        return Ok(Vec::new());
    };
    search_collection_handle(handle, query_vector, limit)
}

fn search_collection_handle(
    handle: CollectionHandle,
    query_vector: &[f32],
    limit: usize,
) -> anyhow::Result<Vec<VectorHit>> {
    let mut query = VectorQuery::new()?;
    query.set_field_name(VECTOR_FIELD)?;
    query.set_query_vector_fp32(query_vector)?;
    query.set_topk(limit.max(1).min(i32::MAX as usize) as i32)?;
    query.set_include_vector(false)?;
    query.set_include_doc_id(false)?;
    query.set_output_fields(&[CHUNK_ID_FIELD])?;

    let docs = {
        let guard = handle
            .lock()
            .map_err(|_| anyhow::anyhow!("zvec collection mutex poisoned"))?;
        guard.query(&query)?
    };

    Ok(docs
        .iter()
        .filter_map(|doc| {
            let chunk_id = doc.get_string(CHUNK_ID_FIELD).ok().flatten()?;
            Some(VectorHit {
                chunk_id,
                score: doc.score(),
            })
        })
        .collect())
}

pub async fn retrieve_collection_vectors(
    paths: &AppPaths,
    collection: &str,
    chunk_ids: &[String],
) -> anyhow::Result<HashMap<String, Vec<Vec<f32>>>> {
    if chunk_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let Some(handle) = collection_handle_existing(paths, collection)? else {
        return Ok(HashMap::new());
    };
    retrieve_collection_vectors_from_handle(handle, chunk_ids)
}

pub async fn retrieve_collection_vectors_for_profile(
    paths: &AppPaths,
    collection: &str,
    chunk_ids: &[String],
    profile: &EmbeddingProfile,
) -> anyhow::Result<HashMap<String, Vec<Vec<f32>>>> {
    if chunk_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let Some(handle) = collection_handle_existing_for_profile(paths, collection, profile)? else {
        return Ok(HashMap::new());
    };
    retrieve_collection_vectors_from_handle(handle, chunk_ids)
}

fn retrieve_collection_vectors_from_handle(
    handle: CollectionHandle,
    chunk_ids: &[String],
) -> anyhow::Result<HashMap<String, Vec<Vec<f32>>>> {
    let mut ids = Vec::with_capacity(chunk_ids.len() * 2);
    let mut seen_ids = HashSet::new();
    for id in chunk_ids {
        for point_key in [id.clone(), format!("{id}:image")] {
            let zvec_id = point_id(&point_key);
            if seen_ids.insert(zvec_id.clone()) {
                ids.push(zvec_id);
            }
        }
    }
    let refs = ids.iter().map(String::as_str).collect::<Vec<_>>();
    let docs = {
        let guard = handle
            .lock()
            .map_err(|_| anyhow::anyhow!("zvec collection mutex poisoned"))?;
        guard.fetch(&refs)?
    };

    let mut vectors = HashMap::new();
    for doc in docs.iter() {
        let Some(chunk_id) = doc.get_string(CHUNK_ID_FIELD).ok().flatten() else {
            continue;
        };
        let Ok(vector) = doc.get_vector_fp32(VECTOR_FIELD) else {
            continue;
        };
        vectors
            .entry(chunk_id)
            .or_insert_with(Vec::new)
            .push(vector);
    }
    Ok(vectors)
}

async fn replace_collection_item_embeddings(
    paths: &AppPaths,
    collection: &str,
    item_id: &str,
    records: &[VectorRecord],
) -> anyhow::Result<()> {
    delete_collection_item_embeddings(paths, collection, item_id).await?;
    upsert_collection_embeddings(paths, collection, records).await
}

async fn upsert_collection_embeddings(
    paths: &AppPaths,
    collection: &str,
    records: &[VectorRecord],
) -> anyhow::Result<()> {
    for batch in records.chunks(VECTOR_BATCH_SIZE) {
        if batch.is_empty() {
            continue;
        }
        let docs = batch
            .iter()
            .map(record_to_doc)
            .collect::<anyhow::Result<Vec<_>>>()?;
        let refs = docs.iter().collect::<Vec<_>>();
        let handle = collection_handle_existing(paths, collection)?.ok_or_else(|| {
            anyhow::anyhow!("zvec collection {collection} does not exist before vector upsert")
        })?;
        let guard = handle
            .lock()
            .map_err(|_| anyhow::anyhow!("zvec collection mutex poisoned"))?;
        let summary = guard.upsert(&refs)?;
        anyhow::ensure!(
            summary.error == 0,
            "zvec upsert for collection {collection} failed for {} records",
            summary.error
        );
        guard.flush()?;
    }
    Ok(())
}

async fn delete_collection_item_embeddings(
    paths: &AppPaths,
    collection: &str,
    item_id: &str,
) -> anyhow::Result<()> {
    let Some(handle) = collection_handle_existing(paths, collection)? else {
        return Ok(());
    };
    let filter = equality_filter(ITEM_ID_FIELD, item_id);
    let guard = handle
        .lock()
        .map_err(|_| anyhow::anyhow!("zvec collection mutex poisoned"))?;
    guard.delete_by_filter(&filter)?;
    guard.flush()?;
    Ok(())
}

async fn delete_stale_collection_item_embeddings(
    paths: &AppPaths,
    collection: &str,
    item_id: &str,
    keep_records: &[VectorRecord],
) -> anyhow::Result<usize> {
    let Some(handle) = collection_handle_existing(paths, collection)? else {
        return Ok(0);
    };
    let filter = stale_item_filter(item_id, keep_records);
    let stale_point_ids = stale_sibling_point_ids(keep_records);
    let guard = handle
        .lock()
        .map_err(|_| anyhow::anyhow!("zvec collection mutex poisoned"))?;
    let before_filter_delete = guard.stats()?.doc_count() as usize;
    guard.delete_by_filter(&filter)?;
    guard.flush()?;
    let after_filter_delete = guard.stats()?.doc_count() as usize;
    let filter_deleted = before_filter_delete.saturating_sub(after_filter_delete);

    let stale_siblings = existing_point_ids(&guard, &stale_point_ids)?;
    let sibling_deleted = stale_siblings.len();
    if !stale_siblings.is_empty() {
        let refs = stale_siblings
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let summary = guard.delete(&refs)?;
        anyhow::ensure!(
            summary.error == 0,
            "zvec stale delete for collection {collection} failed for {} records",
            summary.error
        );
        guard.flush()?;
    }
    Ok(filter_deleted + sibling_deleted)
}

fn ensure_collection(
    paths: &AppPaths,
    collection: &str,
    profile: &EmbeddingProfile,
) -> anyhow::Result<()> {
    collection_handle_for_profile(paths, collection, profile).map(|_| ())
}

async fn collection_exists(paths: &AppPaths, collection: &str) -> anyhow::Result<bool> {
    Ok(collection_handle_existing(paths, collection)?.is_some())
}

fn collection_handle_for_profile(
    paths: &AppPaths,
    collection: &str,
    profile: &EmbeddingProfile,
) -> anyhow::Result<CollectionHandle> {
    let path = collection_path(paths, collection);
    let cache = ZVEC_COLLECTIONS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache
        .lock()
        .map_err(|_| anyhow::anyhow!("zvec collection cache mutex poisoned"))?;

    if let Some(handle) = guard.get(&path) {
        if collection_path_has_data(&path)? && collection_config_is_readable(&path) {
            validate_collection_config(paths, collection, profile, handle)?;
            return Ok(Arc::clone(handle));
        }
        guard.remove(&path);
    }

    let collection_handle = Arc::new(Mutex::new(open_or_create_collection(
        &path, collection, profile,
    )?));
    validate_collection_config(paths, collection, profile, &collection_handle)?;
    guard.insert(path, Arc::clone(&collection_handle));
    Ok(collection_handle)
}

fn collection_handle_existing(
    paths: &AppPaths,
    collection: &str,
) -> anyhow::Result<Option<CollectionHandle>> {
    collection_handle_existing_with_profile(paths, collection, None)
}

fn collection_handle_existing_for_profile(
    paths: &AppPaths,
    collection: &str,
    profile: &EmbeddingProfile,
) -> anyhow::Result<Option<CollectionHandle>> {
    collection_handle_existing_with_profile(paths, collection, Some(profile))
}

fn collection_handle_existing_with_profile(
    paths: &AppPaths,
    collection: &str,
    profile: Option<&EmbeddingProfile>,
) -> anyhow::Result<Option<CollectionHandle>> {
    let path = collection_path(paths, collection);
    let cache = ZVEC_COLLECTIONS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache
        .lock()
        .map_err(|_| anyhow::anyhow!("zvec collection cache mutex poisoned"))?;

    if let Some(handle) = guard.get(&path) {
        if collection_path_has_data(&path)? && collection_config_is_readable(&path) {
            if let Some(profile) = profile {
                validate_collection_config(paths, collection, profile, handle)?;
            }
            return Ok(Some(Arc::clone(handle)));
        }
        guard.remove(&path);
    }

    if !collection_path_has_data(&path)? || !collection_config_is_readable(&path) {
        return Ok(None);
    }

    let collection_path = path_to_string(&path)?;
    let collection_handle = Arc::new(Mutex::new(
        Collection::open(&collection_path, None).with_context(|| {
            format!(
                "failed to open zvec collection {collection} at {}",
                path.display()
            )
        })?,
    ));
    if let Some(profile) = profile {
        validate_collection_config(paths, collection, profile, &collection_handle)?;
    }
    guard.insert(path, Arc::clone(&collection_handle));
    Ok(Some(collection_handle))
}

fn open_or_create_collection(
    path: &Path,
    collection: &str,
    profile: &EmbeddingProfile,
) -> anyhow::Result<Collection> {
    fs::create_dir_all(&profile_index_root(path))?;
    let collection_path = path_to_string(path)?;
    if collection_path_has_data(path)? {
        if collection_config_is_readable(path) {
            return Collection::open(&collection_path, None).with_context(|| {
                format!(
                    "failed to open zvec collection {collection} at {}",
                    path.display()
                )
            });
        }
        fs::remove_dir_all(path).with_context(|| {
            format!(
                "failed to remove zvec collection {collection} with missing or invalid config at {}",
                path.display()
            )
        })?;
    }
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    let schema = zvec_collection_schema(collection, profile)?;
    let handle =
        Collection::create_and_open(&collection_path, &schema, None).with_context(|| {
            format!(
                "failed to create zvec collection {collection} at {}",
                path.display()
            )
        })?;
    write_collection_config(path, collection, profile)?;
    Ok(handle)
}

fn zvec_collection_schema(
    collection: &str,
    profile: &EmbeddingProfile,
) -> anyhow::Result<CollectionSchema> {
    let mut schema = CollectionSchema::new(&collection_schema_name(collection))?;

    let mut invert = IndexParams::new(IndexType::Invert)?;
    invert.set_invert_params(true, false)?;

    let mut item_field = FieldSchema::new(ITEM_ID_FIELD, DataType::String, false, 0)?;
    item_field.set_index_params(&invert)?;
    schema.add_field(&item_field)?;

    let mut chunk_field = FieldSchema::new(CHUNK_ID_FIELD, DataType::String, false, 0)?;
    chunk_field.set_index_params(&invert)?;
    schema.add_field(&chunk_field)?;

    let mut hnsw = IndexParams::new(IndexType::Hnsw)?;
    hnsw.set_metric_type(zvec_metric(&profile.distance_metric)?)?;
    hnsw.set_hnsw_params(ZVEC_HNSW_M, ZVEC_HNSW_EF_CONSTRUCTION)?;

    let mut vector_field = FieldSchema::new(
        VECTOR_FIELD,
        DataType::VectorFp32,
        false,
        profile.output_dimension as u32,
    )?;
    vector_field.set_index_params(&hnsw)?;
    schema.add_field(&vector_field)?;
    schema.validate()?;
    Ok(schema)
}

fn validate_collection_config(
    paths: &AppPaths,
    collection: &str,
    profile: &EmbeddingProfile,
    handle: &CollectionHandle,
) -> anyhow::Result<()> {
    let guard = handle
        .lock()
        .map_err(|_| anyhow::anyhow!("zvec collection mutex poisoned"))?;
    let schema = guard.schema()?;
    let vector_field = schema
        .vector_field(VECTOR_FIELD)?
        .ok_or_else(|| anyhow::anyhow!("zvec collection {collection} is missing vector field"))?;
    anyhow::ensure!(
        vector_field.dimension() == profile.output_dimension as u32,
        "zvec collection {collection} has vector dimension {}, expected {} for profile {}",
        vector_field.dimension(),
        profile.output_dimension,
        profile.id
    );
    let config =
        read_collection_config(&collection_config_path(&collection_path(paths, collection)))?;
    let expected_metric = canonical_distance_metric(&profile.distance_metric)?;
    anyhow::ensure!(
        config.collection == collection
            && config.profile_id == profile.id
            && config.output_dimension == profile.output_dimension
            && config.distance_metric == expected_metric,
        "zvec collection {collection} config does not match profile {}; stored collection={} profile={} dimensions={} metric={}, expected dimensions={} metric={}",
        profile.id,
        config.collection,
        config.profile_id,
        config.output_dimension,
        config.distance_metric,
        profile.output_dimension,
        expected_metric
    );
    Ok(())
}

fn record_to_doc(record: &VectorRecord) -> anyhow::Result<Doc> {
    let mut doc = Doc::new()?;
    doc.set_pk(&point_id(&record.point_key))?;
    doc.add_string(CHUNK_ID_FIELD, &record.chunk_id)?;
    doc.add_string(ITEM_ID_FIELD, &record.item_id)?;
    doc.add_vector_fp32(VECTOR_FIELD, &record.vector)?;
    Ok(doc)
}

fn zvec_metric(metric: &str) -> anyhow::Result<MetricType> {
    match canonical_distance_metric(metric)?.as_str() {
        "cosine" => Ok(MetricType::Cosine),
        "dot" => Ok(MetricType::Ip),
        "l2" => Ok(MetricType::L2),
        other => anyhow::bail!("unsupported zvec distance metric {other:?}"),
    }
}

fn canonical_distance_metric(metric: &str) -> anyhow::Result<String> {
    match metric.to_ascii_lowercase().as_str() {
        "cosine" => Ok("cosine".to_string()),
        "dot" | "ip" => Ok("dot".to_string()),
        "euclid" | "euclidean" | "l2" => Ok("l2".to_string()),
        other => anyhow::bail!("unsupported zvec distance metric {other:?}"),
    }
}

fn collection_path(paths: &AppPaths, collection: &str) -> PathBuf {
    paths
        .vector_index
        .join("collections")
        .join(sanitize_collection_path(collection))
}

fn collection_config_path(collection_path: &Path) -> PathBuf {
    let file_name = collection_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("collection");
    collection_path.with_file_name(format!("{file_name}{ZVEC_COLLECTION_CONFIG_SUFFIX}"))
}

fn collection_config_is_readable(collection_path: &Path) -> bool {
    read_collection_config(&collection_config_path(collection_path)).is_ok()
}

fn read_collection_config(path: &Path) -> anyhow::Result<ZvecCollectionConfig> {
    let raw = fs::read_to_string(path).with_context(|| {
        format!(
            "zvec collection config is missing at {}; rebuild the vector index",
            path.display()
        )
    })?;
    serde_json::from_str(&raw).with_context(|| {
        format!(
            "failed to parse zvec collection config at {}",
            path.display()
        )
    })
}

fn write_collection_config(
    collection_path: &Path,
    collection: &str,
    profile: &EmbeddingProfile,
) -> anyhow::Result<()> {
    let config = ZvecCollectionConfig {
        collection: collection.to_string(),
        profile_id: profile.id.clone(),
        output_dimension: profile.output_dimension,
        distance_metric: canonical_distance_metric(&profile.distance_metric)?,
    };
    let config_path = collection_config_path(collection_path);
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = collection_config_tmp_path(&config_path);
    let _ = fs::remove_file(&tmp_path);
    fs::write(&tmp_path, serde_json::to_vec_pretty(&config)?)?;
    if let Err(error) = fs::rename(&tmp_path, &config_path) {
        if config_path.exists() {
            fs::remove_file(&config_path)?;
            fs::rename(&tmp_path, &config_path)?;
        } else {
            return Err(error.into());
        }
    }
    Ok(())
}

fn collection_config_tmp_path(config_path: &Path) -> PathBuf {
    let file_name = config_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("collection.cerul.json");
    config_path.with_file_name(format!("{file_name}.{}.tmp", std::process::id()))
}

fn profile_index_root(path: &Path) -> PathBuf {
    path.parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| path.to_path_buf())
}

fn collection_path_has_data(path: &Path) -> anyhow::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    Ok(fs::read_dir(path)?.next().transpose()?.is_some())
}

fn collection_schema_name(collection: &str) -> String {
    format!(
        "cerul_{}",
        Uuid::new_v5(&Uuid::NAMESPACE_URL, collection.as_bytes()).simple()
    )
}

fn sanitize_collection_path(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "collection".to_string()
    } else {
        sanitized
    }
}

fn equality_filter(field: &str, value: &str) -> String {
    format!(
        "{field} = '{}'",
        value.replace('\\', "\\\\").replace('\'', "\\'")
    )
}

fn stale_item_filter(item_id: &str, keep_records: &[VectorRecord]) -> String {
    let mut filter = equality_filter(ITEM_ID_FIELD, item_id);
    let mut keep_chunk_ids = keep_records
        .iter()
        .map(|record| record.chunk_id.as_str())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    keep_chunk_ids.sort_unstable();
    if !keep_chunk_ids.is_empty() {
        let keep_values = keep_chunk_ids
            .into_iter()
            .map(quoted_filter_value)
            .collect::<Vec<_>>()
            .join(", ");
        filter.push_str(&format!(" AND {CHUNK_ID_FIELD} NOT IN ({keep_values})"));
    }
    filter
}

fn stale_sibling_point_ids(keep_records: &[VectorRecord]) -> Vec<String> {
    let keep_point_keys = keep_records
        .iter()
        .map(|record| record.point_key.as_str())
        .collect::<HashSet<_>>();
    let mut keep_chunk_ids = keep_records
        .iter()
        .map(|record| record.chunk_id.as_str())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    keep_chunk_ids.sort_unstable();

    let mut stale_ids = Vec::new();
    for chunk_id in keep_chunk_ids {
        for point_key in [chunk_id.to_string(), format!("{chunk_id}:image")] {
            if !keep_point_keys.contains(point_key.as_str()) {
                stale_ids.push(point_id(&point_key));
            }
        }
    }
    stale_ids
}

fn existing_point_ids(
    collection: &Collection,
    point_ids: &[String],
) -> anyhow::Result<Vec<String>> {
    if point_ids.is_empty() {
        return Ok(Vec::new());
    }
    let refs = point_ids.iter().map(String::as_str).collect::<Vec<_>>();
    let docs = collection.fetch(&refs)?;
    let mut ids = Vec::new();
    for doc in docs.iter() {
        if let Some(id) = doc.pk_copy() {
            ids.push(id);
        }
    }
    Ok(ids)
}

fn quoted_filter_value(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn path_to_string(path: &Path) -> anyhow::Result<String> {
    path.to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow::anyhow!("path is not valid UTF-8: {}", path.display()))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_profile(distance_metric: &str) -> EmbeddingProfile {
        EmbeddingProfile {
            id: "test-profile".to_string(),
            provider_id: "test".to_string(),
            model_id: "test-model".to_string(),
            model_revision: None,
            output_dimension: 3,
            distance_metric: distance_metric.to_string(),
            instruction_template: None,
            index_version: MODEL_INDEX_VERSION,
            status: "active".to_string(),
        }
    }

    fn test_record(
        point_key: &str,
        chunk_id: &str,
        item_id: &str,
        vector: [f32; 3],
    ) -> VectorRecord {
        VectorRecord::new_for_dimensions_with_point_key(
            point_key.to_string(),
            chunk_id.to_string(),
            item_id.to_string(),
            vector.to_vec(),
            3,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn zvec_cosine_search_scores_are_distances() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let profile = test_profile("cosine");
        let same = test_record("chunk-same", "chunk-same", "item-1", [1.0, 0.0, 0.0]);
        let orthogonal = test_record(
            "chunk-orthogonal",
            "chunk-orthogonal",
            "item-1",
            [0.0, 1.0, 0.0],
        );
        replace_item_unified_embeddings_for_profile(
            &paths,
            "item-1",
            &[same, orthogonal],
            &profile,
            crate::SEARCH_INDEX_VERSION,
        )
        .await
        .unwrap();

        let collection = unified_collection_name(&paths, &profile, crate::SEARCH_INDEX_VERSION);
        let hits =
            search_collection_for_profile(&paths, &collection, &[1.0, 0.0, 0.0], 2, &profile)
                .await
                .unwrap();

        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].chunk_id, "chunk-same");
        assert!(hits[0].score.abs() < 0.001);
        assert!(hits[0].score < hits[1].score);
    }

    #[tokio::test]
    async fn stale_unified_delete_preserves_keep_records() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let profile = test_profile("cosine");
        let stale = test_record("chunk-stale", "chunk-stale", "item-1", [1.0, 0.0, 0.0]);
        let keep = test_record("chunk-keep", "chunk-keep", "item-1", [0.0, 1.0, 0.0]);
        replace_item_unified_embeddings_for_profile(
            &paths,
            "item-1",
            &[stale.clone(), keep.clone()],
            &profile,
            crate::SEARCH_INDEX_VERSION,
        )
        .await
        .unwrap();

        let deleted = delete_stale_item_unified_embeddings_for_profile(
            &paths,
            "item-1",
            std::slice::from_ref(&keep),
            &profile,
            crate::SEARCH_INDEX_VERSION,
        )
        .await
        .unwrap();
        assert_eq!(deleted, 1);

        let collection = unified_collection_name(&paths, &profile, crate::SEARCH_INDEX_VERSION);
        assert_eq!(
            collection_point_count(&paths, &collection).await.unwrap(),
            1
        );
        let vectors = retrieve_collection_vectors(
            &paths,
            &collection,
            &["chunk-stale".to_string(), "chunk-keep".to_string()],
        )
        .await
        .unwrap();
        assert!(!vectors.contains_key("chunk-stale"));
        assert_eq!(vectors.get("chunk-keep").map(Vec::len), Some(1));
    }

    #[tokio::test]
    async fn stale_unified_delete_removes_dropped_sibling_point_keys() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let profile = test_profile("cosine");
        let text = test_record("chunk-1", "chunk-1", "item-1", [1.0, 0.0, 0.0]);
        let image = test_record("chunk-1:image", "chunk-1", "item-1", [0.0, 1.0, 0.0]);
        replace_item_unified_embeddings_for_profile(
            &paths,
            "item-1",
            &[text.clone(), image],
            &profile,
            crate::SEARCH_INDEX_VERSION,
        )
        .await
        .unwrap();

        let deleted = delete_stale_item_unified_embeddings_for_profile(
            &paths,
            "item-1",
            std::slice::from_ref(&text),
            &profile,
            crate::SEARCH_INDEX_VERSION,
        )
        .await
        .unwrap();

        assert_eq!(deleted, 1);
        let collection = unified_collection_name(&paths, &profile, crate::SEARCH_INDEX_VERSION);
        assert_eq!(
            collection_point_count(&paths, &collection).await.unwrap(),
            1
        );
        let vectors = retrieve_collection_vectors(&paths, &collection, &["chunk-1".to_string()])
            .await
            .unwrap();
        assert_eq!(
            vectors.get("chunk-1").cloned(),
            Some(vec![vec![1.0, 0.0, 0.0]])
        );
    }

    #[tokio::test]
    async fn ensure_collection_rejects_distance_metric_mismatch() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let profile = test_profile("cosine");
        ensure_unified_collection_for_profile(&paths, &profile, crate::SEARCH_INDEX_VERSION)
            .await
            .unwrap();

        let mismatched = test_profile("l2");
        let error =
            ensure_unified_collection_for_profile(&paths, &mismatched, crate::SEARCH_INDEX_VERSION)
                .await
                .unwrap_err()
                .to_string();
        assert!(error.contains("config does not match profile"));
    }

    #[tokio::test]
    async fn ensure_collection_recreates_when_config_is_missing() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let profile = test_profile("cosine");
        let record = test_record("chunk-1", "chunk-1", "item-1", [1.0, 0.0, 0.0]);
        replace_item_unified_embeddings_for_profile(
            &paths,
            "item-1",
            std::slice::from_ref(&record),
            &profile,
            crate::SEARCH_INDEX_VERSION,
        )
        .await
        .unwrap();

        let collection = unified_collection_name(&paths, &profile, crate::SEARCH_INDEX_VERSION);
        let path = collection_path(&paths, &collection);
        let config = collection_config_path(&path);
        assert!(config.exists());
        fs::remove_file(&config).unwrap();

        ensure_unified_collection_for_profile(&paths, &profile, crate::SEARCH_INDEX_VERSION)
            .await
            .unwrap();

        assert!(config.exists());
        assert_eq!(
            collection_point_count(&paths, &collection).await.unwrap(),
            0
        );
        replace_item_unified_embeddings_for_profile(
            &paths,
            "item-1",
            &[record],
            &profile,
            crate::SEARCH_INDEX_VERSION,
        )
        .await
        .unwrap();
        assert_eq!(
            collection_point_count(&paths, &collection).await.unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn ensure_collection_recreates_when_config_is_corrupt() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let profile = test_profile("cosine");
        let record = test_record("chunk-1", "chunk-1", "item-1", [1.0, 0.0, 0.0]);
        replace_item_unified_embeddings_for_profile(
            &paths,
            "item-1",
            std::slice::from_ref(&record),
            &profile,
            crate::SEARCH_INDEX_VERSION,
        )
        .await
        .unwrap();

        let collection = unified_collection_name(&paths, &profile, crate::SEARCH_INDEX_VERSION);
        let path = collection_path(&paths, &collection);
        let config = collection_config_path(&path);
        fs::write(&config, b"{").unwrap();

        ensure_unified_collection_for_profile(&paths, &profile, crate::SEARCH_INDEX_VERSION)
            .await
            .unwrap();

        assert_eq!(
            collection_point_count(&paths, &collection).await.unwrap(),
            0
        );
        replace_item_unified_embeddings_for_profile(
            &paths,
            "item-1",
            &[record],
            &profile,
            crate::SEARCH_INDEX_VERSION,
        )
        .await
        .unwrap();
        assert_eq!(
            collection_point_count(&paths, &collection).await.unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn search_collection_for_profile_rejects_distance_metric_mismatch() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let profile = test_profile("cosine");
        let record = test_record("chunk-1", "chunk-1", "item-1", [1.0, 0.0, 0.0]);
        replace_item_unified_embeddings_for_profile(
            &paths,
            "item-1",
            &[record],
            &profile,
            crate::SEARCH_INDEX_VERSION,
        )
        .await
        .unwrap();

        let collection = unified_collection_name(&paths, &profile, crate::SEARCH_INDEX_VERSION);
        let mismatched = test_profile("l2");
        let error =
            search_collection_for_profile(&paths, &collection, &[1.0, 0.0, 0.0], 1, &mismatched)
                .await
                .unwrap_err()
                .to_string();
        assert!(error.contains("config does not match profile"));
    }
}
