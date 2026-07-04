//! [`VectorQuery`] and [`GroupByVectorQuery`] — the knobs for similarity
//! search.

use std::ffi::CString;
use std::ptr::NonNull;

use crate::error::{check, ErrorCode, Result, ZvecError};
use crate::ffi_util::{cstr_to_string, cstring, slice_as_bytes};
use crate::query_params::{FlatQueryParams, HnswQueryParams, IvfQueryParams};
use crate::sys;

fn field_name_vec_to_c(fields: &[&str]) -> Result<(Vec<CString>, Vec<*const core::ffi::c_char>)> {
    let mut c_strings = Vec::with_capacity(fields.len());
    for f in fields {
        c_strings.push(cstring(f)?);
    }
    let ptrs = c_strings.iter().map(|s| s.as_ptr()).collect();
    Ok((c_strings, ptrs))
}

// -----------------------------------------------------------------------------
// VectorQuery
// -----------------------------------------------------------------------------

pub struct VectorQuery {
    ptr: NonNull<sys::zvec_vector_query_t>,
}

impl VectorQuery {
    pub fn new() -> Result<Self> {
        let ptr = unsafe { sys::zvec_vector_query_create() };
        NonNull::new(ptr).map(|ptr| Self { ptr }).ok_or_else(|| {
            ZvecError::with_message(
                ErrorCode::ResourceExhausted,
                "zvec_vector_query_create returned NULL",
            )
        })
    }

    pub(crate) fn as_ptr(&self) -> *const sys::zvec_vector_query_t {
        self.ptr.as_ptr() as *const _
    }

    pub fn topk(&self) -> i32 {
        unsafe { sys::zvec_vector_query_get_topk(self.as_ptr()) }
    }
    pub fn set_topk(&mut self, topk: i32) -> Result<()> {
        check(unsafe { sys::zvec_vector_query_set_topk(self.ptr.as_ptr(), topk) })
    }

    pub fn field_name(&self) -> Option<String> {
        unsafe { cstr_to_string(sys::zvec_vector_query_get_field_name(self.as_ptr())) }
    }

    pub fn set_field_name(&mut self, name: &str) -> Result<()> {
        let c = cstring(name)?;
        check(unsafe { sys::zvec_vector_query_set_field_name(self.ptr.as_ptr(), c.as_ptr()) })
    }

    pub fn set_query_vector_raw(&mut self, bytes: &[u8]) -> Result<()> {
        check(unsafe {
            sys::zvec_vector_query_set_query_vector(
                self.ptr.as_ptr(),
                bytes.as_ptr() as *const core::ffi::c_void,
                bytes.len(),
            )
        })
    }

    pub fn set_query_vector_fp32(&mut self, vec: &[f32]) -> Result<()> {
        self.set_query_vector_raw(slice_as_bytes(vec))
    }

    pub fn set_query_vector_fp64(&mut self, vec: &[f64]) -> Result<()> {
        self.set_query_vector_raw(slice_as_bytes(vec))
    }

    /// Set a half-precision query vector directly from `&[half::f16]`.
    ///
    /// Available with the `half` cargo feature.
    #[cfg(feature = "half")]
    #[cfg_attr(docsrs, doc(cfg(feature = "half")))]
    pub fn set_query_vector_fp16(&mut self, vec: &[half::f16]) -> Result<()> {
        // SAFETY: `half::f16` is `#[repr(transparent)]` over `u16`.
        let bits: &[u16] =
            unsafe { core::slice::from_raw_parts(vec.as_ptr() as *const u16, vec.len()) };
        self.set_query_vector_raw(slice_as_bytes(bits))
    }

    pub fn filter(&self) -> Option<String> {
        unsafe { cstr_to_string(sys::zvec_vector_query_get_filter(self.as_ptr())) }
    }

    pub fn set_filter(&mut self, filter: &str) -> Result<()> {
        let c = cstring(filter)?;
        check(unsafe { sys::zvec_vector_query_set_filter(self.ptr.as_ptr(), c.as_ptr()) })
    }

    pub fn include_vector(&self) -> bool {
        unsafe { sys::zvec_vector_query_get_include_vector(self.as_ptr()) }
    }
    pub fn set_include_vector(&mut self, b: bool) -> Result<()> {
        check(unsafe { sys::zvec_vector_query_set_include_vector(self.ptr.as_ptr(), b) })
    }

    pub fn include_doc_id(&self) -> bool {
        unsafe { sys::zvec_vector_query_get_include_doc_id(self.as_ptr()) }
    }
    pub fn set_include_doc_id(&mut self, b: bool) -> Result<()> {
        check(unsafe { sys::zvec_vector_query_set_include_doc_id(self.ptr.as_ptr(), b) })
    }

    pub fn set_output_fields(&mut self, fields: &[&str]) -> Result<()> {
        let (keep, ptrs) = field_name_vec_to_c(fields)?;
        let rc = unsafe {
            sys::zvec_vector_query_set_output_fields(
                self.ptr.as_ptr(),
                ptrs.as_ptr() as *mut *const core::ffi::c_char,
                ptrs.len(),
            )
        };
        drop(keep);
        check(rc)
    }

    pub fn output_fields(&self) -> Result<Vec<String>> {
        let mut arr: *mut *const core::ffi::c_char = core::ptr::null_mut();
        let mut count: usize = 0;
        check(unsafe {
            sys::zvec_vector_query_get_output_fields(self.as_ptr(), &mut arr, &mut count)
        })?;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let p = unsafe { *arr.add(i) };
            if let Some(s) = unsafe { cstr_to_string(p) } {
                out.push(s);
            }
        }
        if !arr.is_null() {
            unsafe { sys::zvec_free(arr as *mut _) };
        }
        Ok(out)
    }

    pub fn set_hnsw_params(&mut self, params: HnswQueryParams) -> Result<()> {
        let raw = params.into_raw();
        check(unsafe { sys::zvec_vector_query_set_hnsw_params(self.ptr.as_ptr(), raw) })
    }

    pub fn set_ivf_params(&mut self, params: IvfQueryParams) -> Result<()> {
        let raw = params.into_raw();
        check(unsafe { sys::zvec_vector_query_set_ivf_params(self.ptr.as_ptr(), raw) })
    }

    pub fn set_flat_params(&mut self, params: FlatQueryParams) -> Result<()> {
        let raw = params.into_raw();
        check(unsafe { sys::zvec_vector_query_set_flat_params(self.ptr.as_ptr(), raw) })
    }
}

impl Drop for VectorQuery {
    fn drop(&mut self) {
        unsafe { sys::zvec_vector_query_destroy(self.ptr.as_ptr()) };
    }
}

// SAFETY: VectorQuery owns its C object exclusively. Sending across threads
// is safe. Not Sync because setters mutate internal state.
unsafe impl Send for VectorQuery {}

// -----------------------------------------------------------------------------
// GroupByVectorQuery
// -----------------------------------------------------------------------------

pub struct GroupByVectorQuery {
    ptr: NonNull<sys::zvec_group_by_vector_query_t>,
}

impl GroupByVectorQuery {
    pub fn new() -> Result<Self> {
        let ptr = unsafe { sys::zvec_group_by_vector_query_create() };
        NonNull::new(ptr).map(|ptr| Self { ptr }).ok_or_else(|| {
            ZvecError::with_message(
                ErrorCode::ResourceExhausted,
                "zvec_group_by_vector_query_create returned NULL",
            )
        })
    }

    pub(crate) fn as_ptr(&self) -> *const sys::zvec_group_by_vector_query_t {
        self.ptr.as_ptr() as *const _
    }

    pub fn field_name(&self) -> Option<String> {
        unsafe {
            cstr_to_string(sys::zvec_group_by_vector_query_get_field_name(
                self.as_ptr(),
            ))
        }
    }
    pub fn set_field_name(&mut self, name: &str) -> Result<()> {
        let c = cstring(name)?;
        check(unsafe {
            sys::zvec_group_by_vector_query_set_field_name(self.ptr.as_ptr(), c.as_ptr())
        })
    }

    pub fn group_by_field_name(&self) -> Option<String> {
        unsafe {
            cstr_to_string(sys::zvec_group_by_vector_query_get_group_by_field_name(
                self.as_ptr(),
            ))
        }
    }
    pub fn set_group_by_field_name(&mut self, name: &str) -> Result<()> {
        let c = cstring(name)?;
        check(unsafe {
            sys::zvec_group_by_vector_query_set_group_by_field_name(self.ptr.as_ptr(), c.as_ptr())
        })
    }

    pub fn group_count(&self) -> u32 {
        unsafe { sys::zvec_group_by_vector_query_get_group_count(self.as_ptr()) }
    }
    pub fn set_group_count(&mut self, n: u32) -> Result<()> {
        check(unsafe { sys::zvec_group_by_vector_query_set_group_count(self.ptr.as_ptr(), n) })
    }

    pub fn group_topk(&self) -> u32 {
        unsafe { sys::zvec_group_by_vector_query_get_group_topk(self.as_ptr()) }
    }
    pub fn set_group_topk(&mut self, n: u32) -> Result<()> {
        check(unsafe { sys::zvec_group_by_vector_query_set_group_topk(self.ptr.as_ptr(), n) })
    }

    pub fn set_query_vector_raw(&mut self, bytes: &[u8]) -> Result<()> {
        check(unsafe {
            sys::zvec_group_by_vector_query_set_query_vector(
                self.ptr.as_ptr(),
                bytes.as_ptr() as *const core::ffi::c_void,
                bytes.len(),
            )
        })
    }

    pub fn set_query_vector_fp32(&mut self, vec: &[f32]) -> Result<()> {
        self.set_query_vector_raw(slice_as_bytes(vec))
    }

    pub fn filter(&self) -> Option<String> {
        unsafe { cstr_to_string(sys::zvec_group_by_vector_query_get_filter(self.as_ptr())) }
    }
    pub fn set_filter(&mut self, filter: &str) -> Result<()> {
        let c = cstring(filter)?;
        check(unsafe { sys::zvec_group_by_vector_query_set_filter(self.ptr.as_ptr(), c.as_ptr()) })
    }

    pub fn include_vector(&self) -> bool {
        unsafe { sys::zvec_group_by_vector_query_get_include_vector(self.as_ptr()) }
    }
    pub fn set_include_vector(&mut self, b: bool) -> Result<()> {
        check(unsafe { sys::zvec_group_by_vector_query_set_include_vector(self.ptr.as_ptr(), b) })
    }

    pub fn set_output_fields(&mut self, fields: &[&str]) -> Result<()> {
        let (keep, ptrs) = field_name_vec_to_c(fields)?;
        let rc = unsafe {
            sys::zvec_group_by_vector_query_set_output_fields(
                self.ptr.as_ptr(),
                ptrs.as_ptr() as *mut *const core::ffi::c_char,
                ptrs.len(),
            )
        };
        drop(keep);
        check(rc)
    }

    pub fn output_fields(&self) -> Result<Vec<String>> {
        let mut arr: *mut *const core::ffi::c_char = core::ptr::null_mut();
        let mut count: usize = 0;
        check(unsafe {
            sys::zvec_group_by_vector_query_get_output_fields(
                self.ptr.as_ptr(),
                &mut arr,
                &mut count,
            )
        })?;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let p = unsafe { *arr.add(i) };
            if let Some(s) = unsafe { cstr_to_string(p) } {
                out.push(s);
            }
        }
        if !arr.is_null() {
            unsafe { sys::zvec_free(arr as *mut _) };
        }
        Ok(out)
    }

    pub fn set_hnsw_params(&mut self, params: HnswQueryParams) -> Result<()> {
        let raw = params.into_raw();
        check(unsafe { sys::zvec_group_by_vector_query_set_hnsw_params(self.ptr.as_ptr(), raw) })
    }

    pub fn set_ivf_params(&mut self, params: IvfQueryParams) -> Result<()> {
        let raw = params.into_raw();
        check(unsafe { sys::zvec_group_by_vector_query_set_ivf_params(self.ptr.as_ptr(), raw) })
    }

    pub fn set_flat_params(&mut self, params: FlatQueryParams) -> Result<()> {
        let raw = params.into_raw();
        check(unsafe { sys::zvec_group_by_vector_query_set_flat_params(self.ptr.as_ptr(), raw) })
    }
}

impl Drop for GroupByVectorQuery {
    fn drop(&mut self) {
        unsafe { sys::zvec_group_by_vector_query_destroy(self.ptr.as_ptr()) };
    }
}

// SAFETY: see VectorQuery.
unsafe impl Send for GroupByVectorQuery {}
