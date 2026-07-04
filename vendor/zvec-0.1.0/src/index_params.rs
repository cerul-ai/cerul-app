//! Index parameter builders.
//!
//! `IndexParams` represents zvec's `zvec_index_params_t` — the index-type
//! specific configuration that accompanies a field when creating or altering
//! a collection schema. Setter functions are type-specific (`set_hnsw_params`
//! only valid on HNSW params, etc.) but we expose them uniformly and return
//! a `Result` so zvec can reject invalid combinations.

use std::ptr::NonNull;

use crate::error::{check, ErrorCode, Result, ZvecError};
use crate::sys;
use crate::types::{IndexType, MetricType, QuantizeType};

pub struct IndexParams {
    ptr: NonNull<sys::zvec_index_params_t>,
}

impl IndexParams {
    pub fn new(ty: IndexType) -> Result<Self> {
        let ptr = unsafe { sys::zvec_index_params_create(ty.to_raw()) };
        NonNull::new(ptr).map(|ptr| Self { ptr }).ok_or_else(|| {
            ZvecError::with_message(
                ErrorCode::ResourceExhausted,
                "zvec_index_params_create returned NULL",
            )
        })
    }

    pub(crate) fn as_ptr(&self) -> *const sys::zvec_index_params_t {
        self.ptr.as_ptr() as *const _
    }

    pub fn index_type(&self) -> IndexType {
        IndexType::from_raw(unsafe { sys::zvec_index_params_get_type(self.as_ptr()) })
    }

    pub fn metric_type(&self) -> MetricType {
        MetricType::from_raw(unsafe { sys::zvec_index_params_get_metric_type(self.as_ptr()) })
    }

    pub fn set_metric_type(&mut self, metric: MetricType) -> Result<()> {
        check(unsafe { sys::zvec_index_params_set_metric_type(self.ptr.as_ptr(), metric.to_raw()) })
    }

    pub fn quantize_type(&self) -> QuantizeType {
        QuantizeType::from_raw(unsafe { sys::zvec_index_params_get_quantize_type(self.as_ptr()) })
    }

    pub fn set_quantize_type(&mut self, q: QuantizeType) -> Result<()> {
        check(unsafe { sys::zvec_index_params_set_quantize_type(self.ptr.as_ptr(), q.to_raw()) })
    }

    /// Set HNSW-specific parameters. Only valid when `index_type() == Hnsw`.
    pub fn set_hnsw_params(&mut self, m: i32, ef_construction: i32) -> Result<()> {
        check(unsafe {
            sys::zvec_index_params_set_hnsw_params(self.ptr.as_ptr(), m, ef_construction)
        })
    }

    pub fn hnsw_m(&self) -> i32 {
        unsafe { sys::zvec_index_params_get_hnsw_m(self.as_ptr()) }
    }

    pub fn hnsw_ef_construction(&self) -> i32 {
        unsafe { sys::zvec_index_params_get_hnsw_ef_construction(self.as_ptr()) }
    }

    /// Set IVF-specific parameters.
    pub fn set_ivf_params(&mut self, n_list: i32, n_iters: i32, use_soar: bool) -> Result<()> {
        check(unsafe {
            sys::zvec_index_params_set_ivf_params(self.ptr.as_ptr(), n_list, n_iters, use_soar)
        })
    }

    pub fn ivf_params(&self) -> Result<(i32, i32, bool)> {
        let mut n_list = 0i32;
        let mut n_iters = 0i32;
        let mut soar = false;
        check(unsafe {
            sys::zvec_index_params_get_ivf_params(
                self.as_ptr(),
                &mut n_list,
                &mut n_iters,
                &mut soar,
            )
        })?;
        Ok((n_list, n_iters, soar))
    }

    /// Set inverted-index-specific parameters.
    pub fn set_invert_params(
        &mut self,
        enable_range_opt: bool,
        enable_wildcard: bool,
    ) -> Result<()> {
        check(unsafe {
            sys::zvec_index_params_set_invert_params(
                self.ptr.as_ptr(),
                enable_range_opt,
                enable_wildcard,
            )
        })
    }

    pub fn invert_params(&self) -> Result<(bool, bool)> {
        let mut range_opt = false;
        let mut wildcard = false;
        check(unsafe {
            sys::zvec_index_params_get_invert_params(self.as_ptr(), &mut range_opt, &mut wildcard)
        })?;
        Ok((range_opt, wildcard))
    }
}

impl Drop for IndexParams {
    fn drop(&mut self) {
        unsafe { sys::zvec_index_params_destroy(self.ptr.as_ptr()) };
    }
}

// SAFETY: IndexParams is a pure configuration object with no internal
// shared state; mutation is gated by `&mut self`.
unsafe impl Send for IndexParams {}
unsafe impl Sync for IndexParams {}
