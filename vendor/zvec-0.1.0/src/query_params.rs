//! Algorithm-specific query parameters (HNSW / IVF / Flat).
//!
//! Each builder owns its C-side object. When handed off to a
//! [`crate::query::VectorQuery`] or [`crate::query::GroupByVectorQuery`] via
//! their `set_*_params` methods, ownership transfers into the query and the
//! Rust wrapper is consumed.

use std::ptr::NonNull;

use crate::error::{check, ErrorCode, Result, ZvecError};
use crate::sys;

macro_rules! ensure_ptr {
    ($ptr:expr, $ctx:expr) => {
        NonNull::new($ptr).ok_or_else(|| {
            ZvecError::with_message(
                ErrorCode::ResourceExhausted,
                concat!($ctx, " returned NULL"),
            )
        })
    };
}

// -----------------------------------------------------------------------------
// HNSW
// -----------------------------------------------------------------------------

pub struct HnswQueryParams {
    ptr: NonNull<sys::zvec_hnsw_query_params_t>,
}

impl HnswQueryParams {
    pub fn new(ef: i32, radius: f32, is_linear: bool, is_using_refiner: bool) -> Result<Self> {
        let ptr =
            unsafe { sys::zvec_query_params_hnsw_create(ef, radius, is_linear, is_using_refiner) };
        Ok(Self {
            ptr: ensure_ptr!(ptr, "zvec_query_params_hnsw_create")?,
        })
    }

    pub fn set_ef(&mut self, ef: i32) -> Result<()> {
        check(unsafe { sys::zvec_query_params_hnsw_set_ef(self.ptr.as_ptr(), ef) })
    }
    pub fn ef(&self) -> i32 {
        unsafe { sys::zvec_query_params_hnsw_get_ef(self.ptr.as_ptr()) }
    }

    pub fn set_radius(&mut self, radius: f32) -> Result<()> {
        check(unsafe { sys::zvec_query_params_hnsw_set_radius(self.ptr.as_ptr(), radius) })
    }
    pub fn radius(&self) -> f32 {
        unsafe { sys::zvec_query_params_hnsw_get_radius(self.ptr.as_ptr()) }
    }

    pub fn set_is_linear(&mut self, is_linear: bool) -> Result<()> {
        check(unsafe { sys::zvec_query_params_hnsw_set_is_linear(self.ptr.as_ptr(), is_linear) })
    }
    pub fn is_linear(&self) -> bool {
        unsafe { sys::zvec_query_params_hnsw_get_is_linear(self.ptr.as_ptr()) }
    }

    pub fn set_is_using_refiner(&mut self, b: bool) -> Result<()> {
        check(unsafe { sys::zvec_query_params_hnsw_set_is_using_refiner(self.ptr.as_ptr(), b) })
    }
    pub fn is_using_refiner(&self) -> bool {
        unsafe { sys::zvec_query_params_hnsw_get_is_using_refiner(self.ptr.as_ptr()) }
    }

    pub(crate) fn into_raw(self) -> *mut sys::zvec_hnsw_query_params_t {
        let p = self.ptr.as_ptr();
        core::mem::forget(self);
        p
    }
}

impl Drop for HnswQueryParams {
    fn drop(&mut self) {
        unsafe { sys::zvec_query_params_hnsw_destroy(self.ptr.as_ptr()) };
    }
}

// SAFETY: pure builder; see IndexParams.
unsafe impl Send for HnswQueryParams {}
unsafe impl Sync for HnswQueryParams {}

// -----------------------------------------------------------------------------
// IVF
// -----------------------------------------------------------------------------

pub struct IvfQueryParams {
    ptr: NonNull<sys::zvec_ivf_query_params_t>,
}

impl IvfQueryParams {
    pub fn new(nprobe: i32, is_using_refiner: bool, scale_factor: f32) -> Result<Self> {
        let ptr =
            unsafe { sys::zvec_query_params_ivf_create(nprobe, is_using_refiner, scale_factor) };
        Ok(Self {
            ptr: ensure_ptr!(ptr, "zvec_query_params_ivf_create")?,
        })
    }

    pub fn set_nprobe(&mut self, nprobe: i32) -> Result<()> {
        check(unsafe { sys::zvec_query_params_ivf_set_nprobe(self.ptr.as_ptr(), nprobe) })
    }
    pub fn nprobe(&self) -> i32 {
        unsafe { sys::zvec_query_params_ivf_get_nprobe(self.ptr.as_ptr()) }
    }

    pub fn set_scale_factor(&mut self, f: f32) -> Result<()> {
        check(unsafe { sys::zvec_query_params_ivf_set_scale_factor(self.ptr.as_ptr(), f) })
    }
    pub fn scale_factor(&self) -> f32 {
        unsafe { sys::zvec_query_params_ivf_get_scale_factor(self.ptr.as_ptr()) }
    }

    pub fn set_radius(&mut self, r: f32) -> Result<()> {
        check(unsafe { sys::zvec_query_params_ivf_set_radius(self.ptr.as_ptr(), r) })
    }
    pub fn radius(&self) -> f32 {
        unsafe { sys::zvec_query_params_ivf_get_radius(self.ptr.as_ptr()) }
    }

    pub fn set_is_linear(&mut self, b: bool) -> Result<()> {
        check(unsafe { sys::zvec_query_params_ivf_set_is_linear(self.ptr.as_ptr(), b) })
    }
    pub fn is_linear(&self) -> bool {
        unsafe { sys::zvec_query_params_ivf_get_is_linear(self.ptr.as_ptr()) }
    }

    pub fn set_is_using_refiner(&mut self, b: bool) -> Result<()> {
        check(unsafe { sys::zvec_query_params_ivf_set_is_using_refiner(self.ptr.as_ptr(), b) })
    }
    pub fn is_using_refiner(&self) -> bool {
        unsafe { sys::zvec_query_params_ivf_get_is_using_refiner(self.ptr.as_ptr()) }
    }

    pub(crate) fn into_raw(self) -> *mut sys::zvec_ivf_query_params_t {
        let p = self.ptr.as_ptr();
        core::mem::forget(self);
        p
    }
}

impl Drop for IvfQueryParams {
    fn drop(&mut self) {
        unsafe { sys::zvec_query_params_ivf_destroy(self.ptr.as_ptr()) };
    }
}

// SAFETY: pure builder; see IndexParams.
unsafe impl Send for IvfQueryParams {}
unsafe impl Sync for IvfQueryParams {}

// -----------------------------------------------------------------------------
// Flat
// -----------------------------------------------------------------------------

pub struct FlatQueryParams {
    ptr: NonNull<sys::zvec_flat_query_params_t>,
}

impl FlatQueryParams {
    pub fn new(is_using_refiner: bool, scale_factor: f32) -> Result<Self> {
        let ptr = unsafe { sys::zvec_query_params_flat_create(is_using_refiner, scale_factor) };
        Ok(Self {
            ptr: ensure_ptr!(ptr, "zvec_query_params_flat_create")?,
        })
    }

    pub fn set_scale_factor(&mut self, f: f32) -> Result<()> {
        check(unsafe { sys::zvec_query_params_flat_set_scale_factor(self.ptr.as_ptr(), f) })
    }
    pub fn scale_factor(&self) -> f32 {
        unsafe { sys::zvec_query_params_flat_get_scale_factor(self.ptr.as_ptr()) }
    }

    pub fn set_radius(&mut self, r: f32) -> Result<()> {
        check(unsafe { sys::zvec_query_params_flat_set_radius(self.ptr.as_ptr(), r) })
    }
    pub fn radius(&self) -> f32 {
        unsafe { sys::zvec_query_params_flat_get_radius(self.ptr.as_ptr()) }
    }

    pub fn set_is_linear(&mut self, b: bool) -> Result<()> {
        check(unsafe { sys::zvec_query_params_flat_set_is_linear(self.ptr.as_ptr(), b) })
    }
    pub fn is_linear(&self) -> bool {
        unsafe { sys::zvec_query_params_flat_get_is_linear(self.ptr.as_ptr()) }
    }

    pub fn set_is_using_refiner(&mut self, b: bool) -> Result<()> {
        check(unsafe { sys::zvec_query_params_flat_set_is_using_refiner(self.ptr.as_ptr(), b) })
    }
    pub fn is_using_refiner(&self) -> bool {
        unsafe { sys::zvec_query_params_flat_get_is_using_refiner(self.ptr.as_ptr()) }
    }

    pub(crate) fn into_raw(self) -> *mut sys::zvec_flat_query_params_t {
        let p = self.ptr.as_ptr();
        core::mem::forget(self);
        p
    }
}

impl Drop for FlatQueryParams {
    fn drop(&mut self) {
        unsafe { sys::zvec_query_params_flat_destroy(self.ptr.as_ptr()) };
    }
}

// SAFETY: pure builder; see IndexParams.
unsafe impl Send for FlatQueryParams {}
unsafe impl Sync for FlatQueryParams {}
