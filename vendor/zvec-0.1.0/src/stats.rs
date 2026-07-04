//! [`CollectionStats`] — statistics returned by [`crate::collection::Collection::stats`].

use std::ptr::NonNull;

use crate::ffi_util::cstr_to_string;
use crate::sys;

pub struct CollectionStats {
    ptr: NonNull<sys::zvec_collection_stats_t>,
}

impl CollectionStats {
    pub(crate) fn from_raw(ptr: *mut sys::zvec_collection_stats_t) -> Option<Self> {
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    fn raw(&self) -> *const sys::zvec_collection_stats_t {
        self.ptr.as_ptr() as *const _
    }

    pub fn doc_count(&self) -> u64 {
        unsafe { sys::zvec_collection_stats_get_doc_count(self.raw()) }
    }

    pub fn index_count(&self) -> usize {
        unsafe { sys::zvec_collection_stats_get_index_count(self.raw()) }
    }

    pub fn index_name(&self, idx: usize) -> Option<String> {
        unsafe { cstr_to_string(sys::zvec_collection_stats_get_index_name(self.raw(), idx)) }
    }

    pub fn index_completeness(&self, idx: usize) -> f32 {
        unsafe { sys::zvec_collection_stats_get_index_completeness(self.raw(), idx) }
    }

    /// Returns all `(name, completeness)` pairs.
    pub fn indexes(&self) -> Vec<(String, f32)> {
        let n = self.index_count();
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let name = self.index_name(i).unwrap_or_default();
            out.push((name, self.index_completeness(i)));
        }
        out
    }
}

impl Drop for CollectionStats {
    fn drop(&mut self) {
        unsafe { sys::zvec_collection_stats_destroy(self.ptr.as_ptr()) };
    }
}

// SAFETY: CollectionStats is a read-only snapshot produced by
// `Collection::stats`; it exposes no mutators.
unsafe impl Send for CollectionStats {}
unsafe impl Sync for CollectionStats {}
