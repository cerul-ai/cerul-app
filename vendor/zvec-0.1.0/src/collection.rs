//! [`Collection`] — a zvec collection.

use std::ffi::CString;
use std::ptr::NonNull;

use crate::doc::{Doc, DocRef};
use crate::error::{check, ErrorCode, Result, ZvecError};
use crate::ffi_util::{cstr_to_string, cstring};
use crate::index_params::IndexParams;
use crate::options::CollectionOptions;
use crate::query::VectorQuery;
use crate::schema::{CollectionSchema, FieldSchema};
use crate::stats::CollectionStats;
use crate::sys;

fn pks_to_c(pks: &[&str]) -> Result<(Vec<CString>, Vec<*const core::ffi::c_char>)> {
    let mut c_strings = Vec::with_capacity(pks.len());
    for pk in pks {
        c_strings.push(cstring(pk)?);
    }
    let ptrs = c_strings.iter().map(|s| s.as_ptr()).collect();
    Ok((c_strings, ptrs))
}

fn docs_to_c(docs: &[&Doc]) -> Vec<*const sys::zvec_doc_t> {
    docs.iter().map(|d| d.as_ptr()).collect()
}

/// Result summary returned by batch write APIs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WriteSummary {
    pub success: usize,
    pub error: usize,
}

/// Per-document status returned by the `*_with_results` batch write APIs.
#[derive(Debug, Clone)]
pub struct WriteResult {
    pub code: crate::error::ErrorCode,
    pub message: Option<String>,
}

/// Result set returned by [`Collection::query`] or [`Collection::fetch`].
///
/// The slot owns a zvec-allocated `zvec_doc_t**` array and frees it on drop.
pub struct DocSet {
    ptr: *mut *mut sys::zvec_doc_t,
    len: usize,
}

impl DocSet {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, idx: usize) -> Option<DocRef<'_>> {
        if idx >= self.len {
            return None;
        }
        let p = unsafe { *self.ptr.add(idx) };
        DocRef::from_ptr(p)
    }

    pub fn iter(&self) -> impl Iterator<Item = DocRef<'_>> {
        (0..self.len).filter_map(move |i| self.get(i))
    }

    /// Convert this result set into a flat `Vec<Hit>` suitable for
    /// feeding into [`crate::rerank::RrfReRanker`] or
    /// [`crate::rerank::WeightedReRanker`]. Rows whose primary key
    /// cannot be read are skipped.
    pub fn to_hits(&self) -> Vec<crate::rerank::Hit> {
        self.iter()
            .filter_map(|r| {
                r.pk_copy().map(|pk| crate::rerank::Hit {
                    pk,
                    score: r.score(),
                })
            })
            .collect()
    }
}

impl Drop for DocSet {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { sys::zvec_docs_free(self.ptr, self.len) };
        }
    }
}

pub struct Collection {
    ptr: NonNull<sys::zvec_collection_t>,
}

impl Collection {
    /// Create (and open) a new collection at `path` using the given schema.
    /// Passing `None` for `options` uses zvec's defaults.
    pub fn create_and_open(
        path: &str,
        schema: &CollectionSchema,
        options: Option<&CollectionOptions>,
    ) -> Result<Self> {
        let c_path = cstring(path)?;
        let options_ptr = options.map_or(core::ptr::null(), |o| o.as_ptr());
        let mut out: *mut sys::zvec_collection_t = core::ptr::null_mut();
        check(unsafe {
            sys::zvec_collection_create_and_open(
                c_path.as_ptr(),
                schema.as_ptr(),
                options_ptr,
                &mut out,
            )
        })?;
        NonNull::new(out).map(|ptr| Self { ptr }).ok_or_else(|| {
            ZvecError::with_message(
                ErrorCode::Internal,
                "zvec_collection_create_and_open returned NULL",
            )
        })
    }

    /// Open an existing collection at `path`.
    pub fn open(path: &str, options: Option<&CollectionOptions>) -> Result<Self> {
        let c_path = cstring(path)?;
        let options_ptr = options.map_or(core::ptr::null(), |o| o.as_ptr());
        let mut out: *mut sys::zvec_collection_t = core::ptr::null_mut();
        check(unsafe { sys::zvec_collection_open(c_path.as_ptr(), options_ptr, &mut out) })?;
        NonNull::new(out).map(|ptr| Self { ptr }).ok_or_else(|| {
            ZvecError::with_message(ErrorCode::Internal, "zvec_collection_open returned NULL")
        })
    }

    pub(crate) fn as_ptr(&self) -> *const sys::zvec_collection_t {
        self.ptr.as_ptr() as *const _
    }

    /// Flush buffered writes to disk.
    pub fn flush(&self) -> Result<()> {
        check(unsafe { sys::zvec_collection_flush(self.ptr.as_ptr()) })
    }

    /// Rebuild indexes and merge segments.
    pub fn optimize(&self) -> Result<()> {
        check(unsafe { sys::zvec_collection_optimize(self.ptr.as_ptr()) })
    }

    /// Close the underlying handle. After this the collection cannot be used.
    /// Normally closing happens in `Drop`, but callers may want an explicit
    /// close to check the result.
    pub fn close(self) -> Result<()> {
        let ptr = self.ptr.as_ptr();
        core::mem::forget(self);
        check(unsafe { sys::zvec_collection_close(ptr) })
    }

    // ---------- introspection ----------

    pub fn schema(&self) -> Result<CollectionSchema> {
        let mut out: *mut sys::zvec_collection_schema_t = core::ptr::null_mut();
        check(unsafe { sys::zvec_collection_get_schema(self.as_ptr(), &mut out) })?;
        CollectionSchema::from_raw(out).ok_or_else(|| {
            ZvecError::with_message(
                ErrorCode::Internal,
                "zvec_collection_get_schema returned NULL",
            )
        })
    }

    pub fn options(&self) -> Result<CollectionOptions> {
        let mut out: *mut sys::zvec_collection_options_t = core::ptr::null_mut();
        check(unsafe { sys::zvec_collection_get_options(self.as_ptr(), &mut out) })?;
        CollectionOptions::from_raw(out).ok_or_else(|| {
            ZvecError::with_message(
                ErrorCode::Internal,
                "zvec_collection_get_options returned NULL",
            )
        })
    }

    pub fn stats(&self) -> Result<CollectionStats> {
        let mut out: *mut sys::zvec_collection_stats_t = core::ptr::null_mut();
        check(unsafe { sys::zvec_collection_get_stats(self.as_ptr(), &mut out) })?;
        CollectionStats::from_raw(out).ok_or_else(|| {
            ZvecError::with_message(
                ErrorCode::Internal,
                "zvec_collection_get_stats returned NULL",
            )
        })
    }

    // ---------- index / column DDL ----------

    pub fn create_index(&self, field_name: &str, params: &IndexParams) -> Result<()> {
        let c = cstring(field_name)?;
        check(unsafe {
            sys::zvec_collection_create_index(self.ptr.as_ptr(), c.as_ptr(), params.as_ptr())
        })
    }

    pub fn drop_index(&self, field_name: &str) -> Result<()> {
        let c = cstring(field_name)?;
        check(unsafe { sys::zvec_collection_drop_index(self.ptr.as_ptr(), c.as_ptr()) })
    }

    pub fn add_column(&self, field: &FieldSchema, expression: Option<&str>) -> Result<()> {
        let expr_c = match expression {
            Some(e) => Some(cstring(e)?),
            None => None,
        };
        let expr_ptr = expr_c.as_ref().map_or(core::ptr::null(), |c| c.as_ptr());
        // The field pointer needs to be `const zvec_field_schema_t*`; our
        // wrapper exposes such a borrow via as_ptr().
        check(unsafe {
            sys::zvec_collection_add_column(self.ptr.as_ptr(), field.as_ptr() as *const _, expr_ptr)
        })
    }

    pub fn drop_column(&self, column_name: &str) -> Result<()> {
        let c = cstring(column_name)?;
        check(unsafe { sys::zvec_collection_drop_column(self.ptr.as_ptr(), c.as_ptr()) })
    }

    pub fn alter_column(
        &self,
        column_name: &str,
        new_name: Option<&str>,
        new_schema: Option<&FieldSchema>,
    ) -> Result<()> {
        let col_c = cstring(column_name)?;
        let new_name_c = match new_name {
            Some(n) => Some(cstring(n)?),
            None => None,
        };
        let new_name_ptr = new_name_c
            .as_ref()
            .map_or(core::ptr::null(), |c| c.as_ptr());
        let schema_ptr = new_schema.map_or(core::ptr::null(), |s| s.as_ptr() as *const _);
        check(unsafe {
            sys::zvec_collection_alter_column(
                self.ptr.as_ptr(),
                col_c.as_ptr(),
                new_name_ptr,
                schema_ptr,
            )
        })
    }

    // ---------- DML ----------

    pub fn insert(&self, docs: &[&Doc]) -> Result<WriteSummary> {
        let mut c_docs = docs_to_c(docs);
        let mut success = 0usize;
        let mut error = 0usize;
        check(unsafe {
            sys::zvec_collection_insert(
                self.ptr.as_ptr(),
                c_docs.as_mut_ptr(),
                c_docs.len(),
                &mut success,
                &mut error,
            )
        })?;
        Ok(WriteSummary { success, error })
    }

    pub fn insert_with_results(&self, docs: &[&Doc]) -> Result<Vec<WriteResult>> {
        self.batch_results(docs, sys::zvec_collection_insert_with_results)
    }

    pub fn update(&self, docs: &[&Doc]) -> Result<WriteSummary> {
        let mut c_docs = docs_to_c(docs);
        let mut success = 0usize;
        let mut error = 0usize;
        check(unsafe {
            sys::zvec_collection_update(
                self.ptr.as_ptr(),
                c_docs.as_mut_ptr(),
                c_docs.len(),
                &mut success,
                &mut error,
            )
        })?;
        Ok(WriteSummary { success, error })
    }

    pub fn update_with_results(&self, docs: &[&Doc]) -> Result<Vec<WriteResult>> {
        self.batch_results(docs, sys::zvec_collection_update_with_results)
    }

    pub fn upsert(&self, docs: &[&Doc]) -> Result<WriteSummary> {
        let mut c_docs = docs_to_c(docs);
        let mut success = 0usize;
        let mut error = 0usize;
        check(unsafe {
            sys::zvec_collection_upsert(
                self.ptr.as_ptr(),
                c_docs.as_mut_ptr(),
                c_docs.len(),
                &mut success,
                &mut error,
            )
        })?;
        Ok(WriteSummary { success, error })
    }

    pub fn upsert_with_results(&self, docs: &[&Doc]) -> Result<Vec<WriteResult>> {
        self.batch_results(docs, sys::zvec_collection_upsert_with_results)
    }

    /// Stream [`Doc`]s from an iterator into the collection in batches of
    /// `batch_size`. Accumulates per-batch [`WriteSummary`]s into a single
    /// returned summary; stops on the first error.
    ///
    /// Useful for loading large corpora where collecting every `Doc` into a
    /// `Vec` up front would be wasteful or impossible.
    pub fn insert_iter<I>(&self, docs: I, batch_size: usize) -> Result<WriteSummary>
    where
        I: IntoIterator<Item = Doc>,
    {
        self.batched_write(docs, batch_size, Collection::insert)
    }

    /// As [`Self::insert_iter`] but using [`Self::update`] for each batch.
    pub fn update_iter<I>(&self, docs: I, batch_size: usize) -> Result<WriteSummary>
    where
        I: IntoIterator<Item = Doc>,
    {
        self.batched_write(docs, batch_size, Collection::update)
    }

    /// As [`Self::insert_iter`] but using [`Self::upsert`] for each batch.
    pub fn upsert_iter<I>(&self, docs: I, batch_size: usize) -> Result<WriteSummary>
    where
        I: IntoIterator<Item = Doc>,
    {
        self.batched_write(docs, batch_size, Collection::upsert)
    }

    fn batched_write<I, F>(&self, docs: I, batch_size: usize, op: F) -> Result<WriteSummary>
    where
        I: IntoIterator<Item = Doc>,
        F: Fn(&Collection, &[&Doc]) -> Result<WriteSummary>,
    {
        assert!(batch_size > 0, "batch_size must be > 0");
        let mut total = WriteSummary::default();
        let mut batch: Vec<Doc> = Vec::with_capacity(batch_size);
        for doc in docs {
            batch.push(doc);
            if batch.len() >= batch_size {
                let refs: Vec<&Doc> = batch.iter().collect();
                let s = op(self, &refs)?;
                total.success += s.success;
                total.error += s.error;
                batch.clear();
            }
        }
        if !batch.is_empty() {
            let refs: Vec<&Doc> = batch.iter().collect();
            let s = op(self, &refs)?;
            total.success += s.success;
            total.error += s.error;
        }
        Ok(total)
    }

    fn batch_results(
        &self,
        docs: &[&Doc],
        call: unsafe extern "C" fn(
            *mut sys::zvec_collection_t,
            *mut *const sys::zvec_doc_t,
            usize,
            *mut *mut sys::zvec_write_result_t,
            *mut usize,
        ) -> sys::zvec_error_code_t::Type,
    ) -> Result<Vec<WriteResult>> {
        let mut c_docs = docs_to_c(docs);
        let mut results: *mut sys::zvec_write_result_t = core::ptr::null_mut();
        let mut count: usize = 0;
        check(unsafe {
            call(
                self.ptr.as_ptr(),
                c_docs.as_mut_ptr(),
                c_docs.len(),
                &mut results,
                &mut count,
            )
        })?;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let r = unsafe { &*results.add(i) };
            out.push(WriteResult {
                code: crate::error::ErrorCode::from_raw(r.code),
                message: unsafe { cstr_to_string(r.message) },
            });
        }
        if !results.is_null() {
            unsafe { sys::zvec_write_results_free(results, count) };
        }
        Ok(out)
    }

    pub fn delete(&self, pks: &[&str]) -> Result<WriteSummary> {
        let (keep, ptrs) = pks_to_c(pks)?;
        let mut success = 0usize;
        let mut error = 0usize;
        let rc = unsafe {
            sys::zvec_collection_delete(
                self.ptr.as_ptr(),
                ptrs.as_ptr(),
                ptrs.len(),
                &mut success,
                &mut error,
            )
        };
        drop(keep);
        check(rc)?;
        Ok(WriteSummary { success, error })
    }

    pub fn delete_with_results(&self, pks: &[&str]) -> Result<Vec<WriteResult>> {
        let (keep, ptrs) = pks_to_c(pks)?;
        let mut results: *mut sys::zvec_write_result_t = core::ptr::null_mut();
        let mut count: usize = 0;
        let rc = unsafe {
            sys::zvec_collection_delete_with_results(
                self.ptr.as_ptr(),
                ptrs.as_ptr(),
                ptrs.len(),
                &mut results,
                &mut count,
            )
        };
        drop(keep);
        check(rc)?;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let r = unsafe { &*results.add(i) };
            out.push(WriteResult {
                code: crate::error::ErrorCode::from_raw(r.code),
                message: unsafe { cstr_to_string(r.message) },
            });
        }
        if !results.is_null() {
            unsafe { sys::zvec_write_results_free(results, count) };
        }
        Ok(out)
    }

    pub fn delete_by_filter(&self, filter: &str) -> Result<()> {
        let c = cstring(filter)?;
        check(unsafe { sys::zvec_collection_delete_by_filter(self.ptr.as_ptr(), c.as_ptr()) })
    }

    // ---------- DQL ----------

    pub fn query(&self, query: &VectorQuery) -> Result<DocSet> {
        let mut results: *mut *mut sys::zvec_doc_t = core::ptr::null_mut();
        let mut count: usize = 0;
        check(unsafe {
            sys::zvec_collection_query(self.as_ptr(), query.as_ptr(), &mut results, &mut count)
        })?;
        Ok(DocSet {
            ptr: results,
            len: count,
        })
    }

    pub fn fetch(&self, pks: &[&str]) -> Result<DocSet> {
        let (keep, ptrs) = pks_to_c(pks)?;
        let mut results: *mut *mut sys::zvec_doc_t = core::ptr::null_mut();
        let mut count: usize = 0;
        let rc = unsafe {
            sys::zvec_collection_fetch(
                self.ptr.as_ptr(),
                ptrs.as_ptr(),
                ptrs.len(),
                &mut results,
                &mut count,
            )
        };
        drop(keep);
        check(rc)?;
        Ok(DocSet {
            ptr: results,
            len: count,
        })
    }
}

impl Drop for Collection {
    fn drop(&mut self) {
        // Close the handle only. `zvec_collection_destroy` drops the on-disk
        // collection data, so it must not run from ordinary handle teardown.
        unsafe { sys::zvec_collection_close(self.ptr.as_ptr()) };
    }
}

// SAFETY: A `Collection` is an in-process handle wrapping a
// `std::shared_ptr<zvec::Collection>`. zvec is documented as an embedded
// engine usable from multiple threads; all of our mutating APIs (insert /
// update / delete / flush / optimize) go through the C API, which is
// responsible for its own synchronisation.
unsafe impl Send for Collection {}
unsafe impl Sync for Collection {}

// SAFETY: A `DocSet` owns a zvec-allocated array of non-null document
// pointers. Sending to another thread is fine. We do not implement `Sync`
// because `zvec_docs_free` runs in `Drop` and the underlying documents are
// accessed by pointer.
unsafe impl Send for DocSet {}
