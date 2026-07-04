//! Async wrapper around [`Collection`], gated on the `tokio` cargo feature.
//!
//! zvec's C API is synchronous and its operations can block for non-trivial
//! amounts of time (index compaction, disk flush, vector search on big
//! collections). Calling those blocking operations directly from a tokio
//! task starves the runtime. [`AsyncCollection`] owns an
//! `Arc<Collection>` and wraps every operation in
//! [`tokio::task::spawn_blocking`] so your reactor threads stay
//! responsive.
//!
//! ```no_run
//! # #[cfg(feature = "tokio")]
//! # async fn demo() -> zvec::Result<()> {
//! use zvec::{AsyncCollection, CollectionSchema, Doc, VectorQuery};
//! # let schema: CollectionSchema = unreachable!();
//!
//! let collection = AsyncCollection::create_and_open("./coll", schema, None).await?;
//!
//! let mut doc = Doc::new()?;
//! doc.set_pk("a")?;
//! doc.add_vector_fp32("embedding", &[0.1, 0.2, 0.3])?;
//! collection.insert(vec![doc]).await?;
//! collection.flush().await?;
//!
//! # let mut q = VectorQuery::new()?;
//! let results = collection.query(q).await?;
//! # let _ = results;
//! # Ok(()) }
//! ```

use std::sync::Arc;

use tokio::task::spawn_blocking;

use crate::collection::{Collection, DocSet, WriteResult, WriteSummary};
use crate::doc::Doc;
use crate::error::{ErrorCode, Result, ZvecError};
use crate::index_params::IndexParams;
use crate::options::CollectionOptions;
use crate::query::VectorQuery;
use crate::schema::{CollectionSchema, FieldSchema};
use crate::stats::CollectionStats;

/// Tokio-friendly handle to a [`Collection`]. Cheap to clone.
#[derive(Clone)]
pub struct AsyncCollection {
    inner: Arc<Collection>,
}

impl AsyncCollection {
    /// Create + open a new collection at `path`. Takes ownership of
    /// `schema` and `options` to satisfy `'static` bounds on the blocking
    /// task.
    pub async fn create_and_open(
        path: impl Into<String>,
        schema: CollectionSchema,
        options: Option<CollectionOptions>,
    ) -> Result<Self> {
        let path = path.into();
        let inner =
            run_blocking(move || Collection::create_and_open(&path, &schema, options.as_ref()))
                .await?;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// Open an existing collection at `path`.
    pub async fn open(path: impl Into<String>, options: Option<CollectionOptions>) -> Result<Self> {
        let path = path.into();
        let inner = run_blocking(move || Collection::open(&path, options.as_ref())).await?;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// Wrap an existing `Arc<Collection>`. Useful when you already have a
    /// shared `Collection` you want to use from async contexts.
    pub fn from_arc(inner: Arc<Collection>) -> Self {
        Self { inner }
    }

    /// Access the underlying `Arc<Collection>`. Escape hatch for callers
    /// that need APIs this wrapper doesn't cover yet.
    pub fn inner(&self) -> &Arc<Collection> {
        &self.inner
    }

    // ---------- lifecycle ----------

    pub async fn flush(&self) -> Result<()> {
        let inner = Arc::clone(&self.inner);
        run_blocking(move || inner.flush()).await
    }

    pub async fn optimize(&self) -> Result<()> {
        let inner = Arc::clone(&self.inner);
        run_blocking(move || inner.optimize()).await
    }

    // ---------- introspection ----------

    pub async fn schema(&self) -> Result<CollectionSchema> {
        let inner = Arc::clone(&self.inner);
        run_blocking(move || inner.schema()).await
    }

    pub async fn options(&self) -> Result<CollectionOptions> {
        let inner = Arc::clone(&self.inner);
        run_blocking(move || inner.options()).await
    }

    pub async fn stats(&self) -> Result<CollectionStats> {
        let inner = Arc::clone(&self.inner);
        run_blocking(move || inner.stats()).await
    }

    // ---------- index / column DDL ----------

    pub async fn create_index(
        &self,
        field_name: impl Into<String>,
        params: IndexParams,
    ) -> Result<()> {
        let inner = Arc::clone(&self.inner);
        let field_name = field_name.into();
        run_blocking(move || inner.create_index(&field_name, &params)).await
    }

    pub async fn drop_index(&self, field_name: impl Into<String>) -> Result<()> {
        let inner = Arc::clone(&self.inner);
        let field_name = field_name.into();
        run_blocking(move || inner.drop_index(&field_name)).await
    }

    pub async fn add_column(&self, field: FieldSchema, expression: Option<String>) -> Result<()> {
        let inner = Arc::clone(&self.inner);
        run_blocking(move || inner.add_column(&field, expression.as_deref())).await
    }

    pub async fn drop_column(&self, column_name: impl Into<String>) -> Result<()> {
        let inner = Arc::clone(&self.inner);
        let name = column_name.into();
        run_blocking(move || inner.drop_column(&name)).await
    }

    pub async fn alter_column(
        &self,
        column_name: impl Into<String>,
        new_name: Option<String>,
        new_schema: Option<FieldSchema>,
    ) -> Result<()> {
        let inner = Arc::clone(&self.inner);
        let name = column_name.into();
        run_blocking(move || inner.alter_column(&name, new_name.as_deref(), new_schema.as_ref()))
            .await
    }

    // ---------- DML ----------

    pub async fn insert(&self, docs: Vec<Doc>) -> Result<WriteSummary> {
        let inner = Arc::clone(&self.inner);
        run_blocking(move || {
            let refs: Vec<&Doc> = docs.iter().collect();
            inner.insert(&refs)
        })
        .await
    }

    pub async fn insert_with_results(&self, docs: Vec<Doc>) -> Result<Vec<WriteResult>> {
        let inner = Arc::clone(&self.inner);
        run_blocking(move || {
            let refs: Vec<&Doc> = docs.iter().collect();
            inner.insert_with_results(&refs)
        })
        .await
    }

    pub async fn update(&self, docs: Vec<Doc>) -> Result<WriteSummary> {
        let inner = Arc::clone(&self.inner);
        run_blocking(move || {
            let refs: Vec<&Doc> = docs.iter().collect();
            inner.update(&refs)
        })
        .await
    }

    pub async fn update_with_results(&self, docs: Vec<Doc>) -> Result<Vec<WriteResult>> {
        let inner = Arc::clone(&self.inner);
        run_blocking(move || {
            let refs: Vec<&Doc> = docs.iter().collect();
            inner.update_with_results(&refs)
        })
        .await
    }

    pub async fn upsert(&self, docs: Vec<Doc>) -> Result<WriteSummary> {
        let inner = Arc::clone(&self.inner);
        run_blocking(move || {
            let refs: Vec<&Doc> = docs.iter().collect();
            inner.upsert(&refs)
        })
        .await
    }

    pub async fn upsert_with_results(&self, docs: Vec<Doc>) -> Result<Vec<WriteResult>> {
        let inner = Arc::clone(&self.inner);
        run_blocking(move || {
            let refs: Vec<&Doc> = docs.iter().collect();
            inner.upsert_with_results(&refs)
        })
        .await
    }

    pub async fn delete(&self, pks: Vec<String>) -> Result<WriteSummary> {
        let inner = Arc::clone(&self.inner);
        run_blocking(move || {
            let refs: Vec<&str> = pks.iter().map(String::as_str).collect();
            inner.delete(&refs)
        })
        .await
    }

    pub async fn delete_by_filter(&self, filter: impl Into<String>) -> Result<()> {
        let inner = Arc::clone(&self.inner);
        let filter = filter.into();
        run_blocking(move || inner.delete_by_filter(&filter)).await
    }

    /// Streamed batched insert. The input iterator must be `Send + 'static`;
    /// callers producing docs lazily from a channel or stream can
    /// `.collect::<Vec<_>>().await` upstream.
    pub async fn insert_iter<I>(&self, docs: I, batch_size: usize) -> Result<WriteSummary>
    where
        I: IntoIterator<Item = Doc> + Send + 'static,
    {
        let inner = Arc::clone(&self.inner);
        run_blocking(move || inner.insert_iter(docs, batch_size)).await
    }

    pub async fn upsert_iter<I>(&self, docs: I, batch_size: usize) -> Result<WriteSummary>
    where
        I: IntoIterator<Item = Doc> + Send + 'static,
    {
        let inner = Arc::clone(&self.inner);
        run_blocking(move || inner.upsert_iter(docs, batch_size)).await
    }

    pub async fn update_iter<I>(&self, docs: I, batch_size: usize) -> Result<WriteSummary>
    where
        I: IntoIterator<Item = Doc> + Send + 'static,
    {
        let inner = Arc::clone(&self.inner);
        run_blocking(move || inner.update_iter(docs, batch_size)).await
    }

    // ---------- DQL ----------

    pub async fn query(&self, query: VectorQuery) -> Result<DocSet> {
        let inner = Arc::clone(&self.inner);
        run_blocking(move || inner.query(&query)).await
    }

    pub async fn fetch(&self, pks: Vec<String>) -> Result<DocSet> {
        let inner = Arc::clone(&self.inner);
        run_blocking(move || {
            let refs: Vec<&str> = pks.iter().map(String::as_str).collect();
            inner.fetch(&refs)
        })
        .await
    }
}

/// Shared `spawn_blocking` adapter. Converts a JoinError into a
/// `ZvecError::Internal` so callers see a single error type.
async fn run_blocking<F, T>(f: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    match spawn_blocking(f).await {
        Ok(r) => r,
        Err(join_err) => Err(ZvecError::with_message(
            ErrorCode::Internal,
            format!("tokio blocking task failed: {join_err}"),
        )),
    }
}
