//! Document type — rows in a zvec collection.
//!
//! Field values can be added either raw (by byte slice, for callers that
//! already know the wire layout the C API expects) or via the typed helpers
//! [`Doc::add_string`], [`Doc::add_float`], [`Doc::add_vector_fp32`], etc.

use std::marker::PhantomData;
use std::ptr::NonNull;

use crate::error::{check, ErrorCode, Result, ZvecError};
use crate::ffi_util::{cstr_as_str, cstr_to_string, cstring, slice_as_bytes};
use crate::schema::CollectionSchema;
use crate::sys;
use crate::types::{DataType, DocOperator};

/// Owning document.
pub struct Doc {
    ptr: NonNull<sys::zvec_doc_t>,
}

impl Doc {
    pub fn new() -> Result<Self> {
        let ptr = unsafe { sys::zvec_doc_create() };
        NonNull::new(ptr).map(|ptr| Self { ptr }).ok_or_else(|| {
            ZvecError::with_message(
                ErrorCode::ResourceExhausted,
                "zvec_doc_create returned NULL",
            )
        })
    }

    pub(crate) fn as_ptr(&self) -> *const sys::zvec_doc_t {
        self.ptr.as_ptr() as *const _
    }

    pub(crate) fn from_raw(ptr: *mut sys::zvec_doc_t) -> Option<Self> {
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Transfer ownership to the caller; skips the destructor.
    #[allow(dead_code)]
    pub(crate) fn into_raw(self) -> *mut sys::zvec_doc_t {
        let p = self.ptr.as_ptr();
        core::mem::forget(self);
        p
    }

    /// Borrow this document non-mutably, e.g. to read fields.
    pub fn borrow(&self) -> DocRef<'_> {
        DocRef {
            ptr: self.ptr,
            _marker: PhantomData,
        }
    }

    pub fn clear(&mut self) {
        unsafe { sys::zvec_doc_clear(self.ptr.as_ptr()) };
    }

    // --- setters ---

    pub fn set_pk(&mut self, pk: &str) -> Result<()> {
        let c = cstring(pk)?;
        unsafe { sys::zvec_doc_set_pk(self.ptr.as_ptr(), c.as_ptr()) };
        Ok(())
    }

    pub fn set_doc_id(&mut self, id: u64) {
        unsafe { sys::zvec_doc_set_doc_id(self.ptr.as_ptr(), id) };
    }

    pub fn set_score(&mut self, score: f32) {
        unsafe { sys::zvec_doc_set_score(self.ptr.as_ptr(), score) };
    }

    pub fn set_operator(&mut self, op: DocOperator) {
        unsafe { sys::zvec_doc_set_operator(self.ptr.as_ptr(), op.to_raw()) };
    }

    pub fn set_field_null(&mut self, field_name: &str) -> Result<()> {
        let c = cstring(field_name)?;
        check(unsafe { sys::zvec_doc_set_field_null(self.ptr.as_ptr(), c.as_ptr()) })
    }

    /// Raw field setter — `value` is the wire layout expected by zvec for
    /// `data_type`. See [`Self::add_string`], [`Self::add_vector_fp32`] etc.
    /// for typed wrappers.
    pub fn add_field_raw(
        &mut self,
        field_name: &str,
        data_type: DataType,
        value: &[u8],
    ) -> Result<()> {
        let c = cstring(field_name)?;
        check(unsafe {
            sys::zvec_doc_add_field_by_value(
                self.ptr.as_ptr(),
                c.as_ptr(),
                data_type.to_raw(),
                value.as_ptr() as *const core::ffi::c_void,
                value.len(),
            )
        })
    }

    pub fn add_string(&mut self, field_name: &str, value: &str) -> Result<()> {
        self.add_field_raw(field_name, DataType::String, value.as_bytes())
    }

    pub fn add_bool(&mut self, field_name: &str, value: bool) -> Result<()> {
        let v: u8 = if value { 1 } else { 0 };
        self.add_field_raw(field_name, DataType::Bool, &[v])
    }

    pub fn add_int32(&mut self, field_name: &str, value: i32) -> Result<()> {
        self.add_field_raw(field_name, DataType::Int32, &value.to_ne_bytes())
    }

    pub fn add_int64(&mut self, field_name: &str, value: i64) -> Result<()> {
        self.add_field_raw(field_name, DataType::Int64, &value.to_ne_bytes())
    }

    pub fn add_uint32(&mut self, field_name: &str, value: u32) -> Result<()> {
        self.add_field_raw(field_name, DataType::UInt32, &value.to_ne_bytes())
    }

    pub fn add_uint64(&mut self, field_name: &str, value: u64) -> Result<()> {
        self.add_field_raw(field_name, DataType::UInt64, &value.to_ne_bytes())
    }

    pub fn add_float(&mut self, field_name: &str, value: f32) -> Result<()> {
        self.add_field_raw(field_name, DataType::Float, &value.to_ne_bytes())
    }

    pub fn add_double(&mut self, field_name: &str, value: f64) -> Result<()> {
        self.add_field_raw(field_name, DataType::Double, &value.to_ne_bytes())
    }

    pub fn add_binary(&mut self, field_name: &str, value: &[u8]) -> Result<()> {
        self.add_field_raw(field_name, DataType::Binary, value)
    }

    pub fn add_vector_fp32(&mut self, field_name: &str, vector: &[f32]) -> Result<()> {
        self.add_field_raw(field_name, DataType::VectorFp32, slice_as_bytes(vector))
    }

    pub fn add_vector_fp64(&mut self, field_name: &str, vector: &[f64]) -> Result<()> {
        self.add_field_raw(field_name, DataType::VectorFp64, slice_as_bytes(vector))
    }

    pub fn add_vector_int8(&mut self, field_name: &str, vector: &[i8]) -> Result<()> {
        self.add_field_raw(field_name, DataType::VectorInt8, slice_as_bytes(vector))
    }

    /// Add an INT16 vector field. Values are stored as-is in native byte
    /// order.
    pub fn add_vector_int16(&mut self, field_name: &str, vector: &[i16]) -> Result<()> {
        self.add_field_raw(field_name, DataType::VectorInt16, slice_as_bytes(vector))
    }

    /// Add an FP16 vector field. Each `u16` is the raw bit pattern of an
    /// IEEE-754 half-precision float, as zvec's C API accepts them; pair with
    /// a crate like `half` to produce them from `f32`.
    pub fn add_vector_fp16_bits(&mut self, field_name: &str, vector: &[u16]) -> Result<()> {
        self.add_field_raw(field_name, DataType::VectorFp16, slice_as_bytes(vector))
    }

    /// Add an FP16 vector field directly from a slice of `half::f16`.
    ///
    /// Available with the `half` cargo feature.
    #[cfg(feature = "half")]
    #[cfg_attr(docsrs, doc(cfg(feature = "half")))]
    pub fn add_vector_fp16(&mut self, field_name: &str, vector: &[half::f16]) -> Result<()> {
        // SAFETY: `half::f16` is `#[repr(transparent)]` over `u16` — see the
        // `half` crate docs — so a `&[half::f16]` is bitwise a `&[u16]` of
        // the same length.
        let bits: &[u16] =
            unsafe { core::slice::from_raw_parts(vector.as_ptr() as *const u16, vector.len()) };
        self.add_vector_fp16_bits(field_name, bits)
    }

    /// Add an INT4 vector field. INT4 is nibble-packed — two values per byte
    /// — and zvec expects the caller to hand it pre-packed bytes.
    pub fn add_vector_int4_packed(&mut self, field_name: &str, packed: &[u8]) -> Result<()> {
        self.add_field_raw(field_name, DataType::VectorInt4, packed)
    }

    /// Add a binary32 vector field (bit-packed, 32 bits per word).
    pub fn add_vector_binary32(&mut self, field_name: &str, words: &[u32]) -> Result<()> {
        self.add_field_raw(field_name, DataType::VectorBinary32, slice_as_bytes(words))
    }

    /// Add a binary64 vector field (bit-packed, 64 bits per word).
    pub fn add_vector_binary64(&mut self, field_name: &str, words: &[u64]) -> Result<()> {
        self.add_field_raw(field_name, DataType::VectorBinary64, slice_as_bytes(words))
    }

    /// Add an `array<int32>` field.
    pub fn add_array_int32(&mut self, field_name: &str, values: &[i32]) -> Result<()> {
        self.add_field_raw(field_name, DataType::ArrayInt32, slice_as_bytes(values))
    }

    /// Add an `array<int64>` field.
    pub fn add_array_int64(&mut self, field_name: &str, values: &[i64]) -> Result<()> {
        self.add_field_raw(field_name, DataType::ArrayInt64, slice_as_bytes(values))
    }

    /// Add an `array<uint32>` field.
    pub fn add_array_uint32(&mut self, field_name: &str, values: &[u32]) -> Result<()> {
        self.add_field_raw(field_name, DataType::ArrayUInt32, slice_as_bytes(values))
    }

    /// Add an `array<uint64>` field.
    pub fn add_array_uint64(&mut self, field_name: &str, values: &[u64]) -> Result<()> {
        self.add_field_raw(field_name, DataType::ArrayUInt64, slice_as_bytes(values))
    }

    /// Add an `array<float>` field.
    pub fn add_array_float(&mut self, field_name: &str, values: &[f32]) -> Result<()> {
        self.add_field_raw(field_name, DataType::ArrayFloat, slice_as_bytes(values))
    }

    /// Add an `array<double>` field.
    pub fn add_array_double(&mut self, field_name: &str, values: &[f64]) -> Result<()> {
        self.add_field_raw(field_name, DataType::ArrayDouble, slice_as_bytes(values))
    }

    pub fn remove_field(&mut self, field_name: &str) -> Result<()> {
        let c = cstring(field_name)?;
        check(unsafe { sys::zvec_doc_remove_field(self.ptr.as_ptr(), c.as_ptr()) })
    }

    pub fn merge(&mut self, other: &Doc) {
        unsafe { sys::zvec_doc_merge(self.ptr.as_ptr(), other.as_ptr()) };
    }

    pub fn validate(&self, schema: &CollectionSchema, is_update: bool) -> Result<()> {
        let mut err_msg: *mut core::ffi::c_char = core::ptr::null_mut();
        let rc = unsafe {
            sys::zvec_doc_validate(self.as_ptr(), schema.as_ptr(), is_update, &mut err_msg)
        };
        if rc == sys::zvec_error_code_t::ZVEC_OK {
            if !err_msg.is_null() {
                unsafe { sys::zvec_free(err_msg as *mut _) };
            }
            Ok(())
        } else {
            let msg = unsafe { cstr_to_string(err_msg) };
            if !err_msg.is_null() {
                unsafe { sys::zvec_free(err_msg as *mut _) };
            }
            Err(ZvecError {
                code: crate::error::ErrorCode::from_raw(rc),
                message: msg,
            })
        }
    }

    pub fn serialize(&self) -> Result<Vec<u8>> {
        let mut data: *mut u8 = core::ptr::null_mut();
        let mut size: usize = 0;
        check(unsafe { sys::zvec_doc_serialize(self.as_ptr(), &mut data, &mut size) })?;
        let out = if data.is_null() {
            Vec::new()
        } else {
            let slice = unsafe { core::slice::from_raw_parts(data, size) };
            slice.to_vec()
        };
        if !data.is_null() {
            unsafe { sys::zvec_free_uint8_array(data) };
        }
        Ok(out)
    }

    pub fn deserialize(data: &[u8]) -> Result<Self> {
        let mut doc: *mut sys::zvec_doc_t = core::ptr::null_mut();
        check(unsafe { sys::zvec_doc_deserialize(data.as_ptr(), data.len(), &mut doc) })?;
        Doc::from_raw(doc).ok_or_else(|| {
            ZvecError::with_message(
                ErrorCode::Internal,
                "zvec_doc_deserialize succeeded but returned NULL",
            )
        })
    }

    pub fn to_detail_string(&self) -> Result<String> {
        let mut s: *mut core::ffi::c_char = core::ptr::null_mut();
        check(unsafe { sys::zvec_doc_to_detail_string(self.as_ptr(), &mut s) })?;
        let out = unsafe { cstr_to_string(s) }.unwrap_or_default();
        if !s.is_null() {
            unsafe { sys::zvec_free(s as *mut _) };
        }
        Ok(out)
    }

    // Forward reads to DocRef.
    pub fn pk(&self) -> Option<String> {
        self.borrow().pk_copy()
    }
    pub fn doc_id(&self) -> u64 {
        self.borrow().doc_id()
    }
    pub fn score(&self) -> f32 {
        self.borrow().score()
    }
    pub fn operator(&self) -> DocOperator {
        self.borrow().operator()
    }
    pub fn field_count(&self) -> usize {
        self.borrow().field_count()
    }
    pub fn is_empty(&self) -> bool {
        self.borrow().is_empty()
    }
    pub fn has_field(&self, name: &str) -> bool {
        self.borrow().has_field(name)
    }
    pub fn has_field_value(&self, name: &str) -> bool {
        self.borrow().has_field_value(name)
    }
    pub fn is_field_null(&self, name: &str) -> bool {
        self.borrow().is_field_null(name)
    }
    pub fn memory_usage(&self) -> usize {
        self.borrow().memory_usage()
    }
    pub fn field_names(&self) -> Result<Vec<String>> {
        self.borrow().field_names()
    }

    pub fn get_vector_fp32(&self, field_name: &str) -> Result<Vec<f32>> {
        self.borrow().get_vector_fp32(field_name)
    }
    pub fn get_string(&self, field_name: &str) -> Result<Option<String>> {
        self.borrow().get_string(field_name)
    }
    pub fn get_int64(&self, field_name: &str) -> Result<i64> {
        self.borrow().get_int64(field_name)
    }
    pub fn get_float(&self, field_name: &str) -> Result<f32> {
        self.borrow().get_float(field_name)
    }
}

impl Drop for Doc {
    fn drop(&mut self) {
        unsafe { sys::zvec_doc_destroy(self.ptr.as_ptr()) };
    }
}

// SAFETY: A `Doc` owns its underlying C object exclusively. Sending across
// threads is fine. We deliberately do NOT impl `Sync`: zvec's doc mutators
// (`zvec_doc_add_field_*`, `zvec_doc_set_pk`, etc.) are not documented as
// thread-safe, and Rust's `&self` accessors on `DocRef` could otherwise run
// concurrently with those mutators through aliased pointers.
unsafe impl Send for Doc {}

/// Non-owning document reference (e.g. rows returned from a query).
#[derive(Clone, Copy)]
pub struct DocRef<'a> {
    ptr: NonNull<sys::zvec_doc_t>,
    _marker: PhantomData<&'a sys::zvec_doc_t>,
}

impl<'a> DocRef<'a> {
    pub(crate) fn from_ptr(ptr: *mut sys::zvec_doc_t) -> Option<Self> {
        NonNull::new(ptr).map(|ptr| Self {
            ptr,
            _marker: PhantomData,
        })
    }

    fn raw(self) -> *const sys::zvec_doc_t {
        self.ptr.as_ptr() as *const _
    }

    /// Borrow the PK without copying. The returned slice is invalidated when
    /// the underlying document is destroyed.
    pub fn pk_ref(&self) -> Option<&'a str> {
        unsafe { cstr_as_str(sys::zvec_doc_get_pk_pointer(self.raw())) }
    }

    /// Copy the PK into an owned `String`.
    pub fn pk_copy(&self) -> Option<String> {
        unsafe {
            let ptr = sys::zvec_doc_get_pk_copy(self.raw()) as *mut core::ffi::c_char;
            let out = cstr_to_string(ptr);
            if !ptr.is_null() {
                sys::zvec_free(ptr as *mut _);
            }
            out
        }
    }

    pub fn doc_id(&self) -> u64 {
        unsafe { sys::zvec_doc_get_doc_id(self.raw()) }
    }
    pub fn score(&self) -> f32 {
        unsafe { sys::zvec_doc_get_score(self.raw()) }
    }
    pub fn operator(&self) -> DocOperator {
        DocOperator::from_raw(unsafe { sys::zvec_doc_get_operator(self.raw()) })
    }
    pub fn field_count(&self) -> usize {
        unsafe { sys::zvec_doc_get_field_count(self.raw()) }
    }
    pub fn is_empty(&self) -> bool {
        unsafe { sys::zvec_doc_is_empty(self.raw()) }
    }

    pub fn has_field(&self, name: &str) -> bool {
        let Ok(c) = cstring(name) else { return false };
        unsafe { sys::zvec_doc_has_field(self.raw(), c.as_ptr()) }
    }

    pub fn has_field_value(&self, name: &str) -> bool {
        let Ok(c) = cstring(name) else { return false };
        unsafe { sys::zvec_doc_has_field_value(self.raw(), c.as_ptr()) }
    }

    pub fn is_field_null(&self, name: &str) -> bool {
        let Ok(c) = cstring(name) else { return false };
        unsafe { sys::zvec_doc_is_field_null(self.raw(), c.as_ptr()) }
    }

    pub fn memory_usage(&self) -> usize {
        unsafe { sys::zvec_doc_memory_usage(self.raw()) }
    }

    pub fn field_names(&self) -> Result<Vec<String>> {
        let mut names: *mut *mut core::ffi::c_char = core::ptr::null_mut();
        let mut count: usize = 0;
        check(unsafe { sys::zvec_doc_get_field_names(self.raw(), &mut names, &mut count) })?;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let p = unsafe { *names.add(i) };
            if let Some(s) = unsafe { cstr_to_string(p) } {
                out.push(s);
            }
        }
        if !names.is_null() {
            unsafe { sys::zvec_free_str_array(names, count) };
        }
        Ok(out)
    }

    /// Fetch a field value into a buffer using the "basic" variant — supports
    /// BOOL / INT32 / INT64 / UINT32 / UINT64 / FLOAT / DOUBLE.
    pub fn get_basic<T: Copy>(&self, name: &str, ty: DataType) -> Result<T> {
        let c = cstring(name)?;
        let mut out = core::mem::MaybeUninit::<T>::uninit();
        check(unsafe {
            sys::zvec_doc_get_field_value_basic(
                self.raw(),
                c.as_ptr(),
                ty.to_raw(),
                out.as_mut_ptr() as *mut core::ffi::c_void,
                core::mem::size_of::<T>(),
            )
        })?;
        Ok(unsafe { out.assume_init() })
    }

    pub fn get_int32(&self, name: &str) -> Result<i32> {
        self.get_basic(name, DataType::Int32)
    }
    pub fn get_int64(&self, name: &str) -> Result<i64> {
        self.get_basic(name, DataType::Int64)
    }
    pub fn get_uint32(&self, name: &str) -> Result<u32> {
        self.get_basic(name, DataType::UInt32)
    }
    pub fn get_uint64(&self, name: &str) -> Result<u64> {
        self.get_basic(name, DataType::UInt64)
    }
    pub fn get_float(&self, name: &str) -> Result<f32> {
        self.get_basic(name, DataType::Float)
    }
    pub fn get_double(&self, name: &str) -> Result<f64> {
        self.get_basic(name, DataType::Double)
    }
    pub fn get_bool(&self, name: &str) -> Result<bool> {
        let v: u8 = self.get_basic(name, DataType::Bool)?;
        Ok(v != 0)
    }

    /// Copy a field value (for variable-size types). Returns raw bytes which
    /// the caller can reinterpret.
    pub fn get_copy(&self, name: &str, ty: DataType) -> Result<Vec<u8>> {
        let c = cstring(name)?;
        let mut value: *mut core::ffi::c_void = core::ptr::null_mut();
        let mut size: usize = 0;
        check(unsafe {
            sys::zvec_doc_get_field_value_copy(
                self.raw(),
                c.as_ptr(),
                ty.to_raw(),
                &mut value,
                &mut size,
            )
        })?;
        let out = if value.is_null() || size == 0 {
            Vec::new()
        } else {
            unsafe { core::slice::from_raw_parts(value as *const u8, size) }.to_vec()
        };
        if !value.is_null() {
            // Per zvec docs: basic types + strings go through zvec_free.
            // Binary goes through zvec_free_uint8_array, but the typed helpers
            // below handle that case; the generic path uses zvec_free.
            unsafe { sys::zvec_free(value) };
        }
        Ok(out)
    }

    pub fn get_string(&self, name: &str) -> Result<Option<String>> {
        let c = cstring(name)?;
        let mut ptr: *const core::ffi::c_void = core::ptr::null();
        let mut size: usize = 0;
        check(unsafe {
            sys::zvec_doc_get_field_value_pointer(
                self.raw(),
                c.as_ptr(),
                DataType::String.to_raw(),
                &mut ptr,
                &mut size,
            )
        })?;
        if ptr.is_null() {
            return Ok(None);
        }
        let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, size) };
        Ok(Some(String::from_utf8_lossy(bytes).into_owned()))
    }

    pub fn get_vector_fp32(&self, name: &str) -> Result<Vec<f32>> {
        let bytes = self.get_copy(name, DataType::VectorFp32)?;
        let elems = bytes.len() / core::mem::size_of::<f32>();
        let mut out = Vec::with_capacity(elems);
        for chunk in bytes.chunks_exact(core::mem::size_of::<f32>()) {
            out.push(f32::from_ne_bytes(chunk.try_into().unwrap()));
        }
        Ok(out)
    }

    pub fn get_vector_fp64(&self, name: &str) -> Result<Vec<f64>> {
        let bytes = self.get_copy(name, DataType::VectorFp64)?;
        let elems = bytes.len() / core::mem::size_of::<f64>();
        let mut out = Vec::with_capacity(elems);
        for chunk in bytes.chunks_exact(core::mem::size_of::<f64>()) {
            out.push(f64::from_ne_bytes(chunk.try_into().unwrap()));
        }
        Ok(out)
    }

    /// Retrieve an INT8 vector field.
    pub fn get_vector_int8(&self, name: &str) -> Result<Vec<i8>> {
        let bytes = self.get_copy(name, DataType::VectorInt8)?;
        Ok(bytes.into_iter().map(|b| b as i8).collect())
    }

    /// Retrieve an INT16 vector field.
    pub fn get_vector_int16(&self, name: &str) -> Result<Vec<i16>> {
        let bytes = self.get_copy(name, DataType::VectorInt16)?;
        let mut out = Vec::with_capacity(bytes.len() / 2);
        for chunk in bytes.chunks_exact(core::mem::size_of::<i16>()) {
            out.push(i16::from_ne_bytes(chunk.try_into().unwrap()));
        }
        Ok(out)
    }

    /// Retrieve an FP16 vector field as its raw 16-bit bit patterns.
    pub fn get_vector_fp16_bits(&self, name: &str) -> Result<Vec<u16>> {
        let bytes = self.get_copy(name, DataType::VectorFp16)?;
        let mut out = Vec::with_capacity(bytes.len() / 2);
        for chunk in bytes.chunks_exact(core::mem::size_of::<u16>()) {
            out.push(u16::from_ne_bytes(chunk.try_into().unwrap()));
        }
        Ok(out)
    }

    /// Retrieve an FP16 vector field as `Vec<half::f16>`.
    ///
    /// Available with the `half` cargo feature.
    #[cfg(feature = "half")]
    #[cfg_attr(docsrs, doc(cfg(feature = "half")))]
    pub fn get_vector_fp16(&self, name: &str) -> Result<Vec<half::f16>> {
        Ok(self
            .get_vector_fp16_bits(name)?
            .into_iter()
            .map(half::f16::from_bits)
            .collect())
    }

    /// Retrieve a nibble-packed INT4 vector field as raw bytes (2 values per
    /// byte).
    pub fn get_vector_int4_packed(&self, name: &str) -> Result<Vec<u8>> {
        self.get_copy(name, DataType::VectorInt4)
    }

    pub fn get_vector_binary32(&self, name: &str) -> Result<Vec<u32>> {
        let bytes = self.get_copy(name, DataType::VectorBinary32)?;
        let mut out = Vec::with_capacity(bytes.len() / 4);
        for chunk in bytes.chunks_exact(core::mem::size_of::<u32>()) {
            out.push(u32::from_ne_bytes(chunk.try_into().unwrap()));
        }
        Ok(out)
    }

    pub fn get_vector_binary64(&self, name: &str) -> Result<Vec<u64>> {
        let bytes = self.get_copy(name, DataType::VectorBinary64)?;
        let mut out = Vec::with_capacity(bytes.len() / 8);
        for chunk in bytes.chunks_exact(core::mem::size_of::<u64>()) {
            out.push(u64::from_ne_bytes(chunk.try_into().unwrap()));
        }
        Ok(out)
    }

    pub fn get_array_int32(&self, name: &str) -> Result<Vec<i32>> {
        let bytes = self.get_copy(name, DataType::ArrayInt32)?;
        let mut out = Vec::with_capacity(bytes.len() / 4);
        for chunk in bytes.chunks_exact(core::mem::size_of::<i32>()) {
            out.push(i32::from_ne_bytes(chunk.try_into().unwrap()));
        }
        Ok(out)
    }

    pub fn get_array_int64(&self, name: &str) -> Result<Vec<i64>> {
        let bytes = self.get_copy(name, DataType::ArrayInt64)?;
        let mut out = Vec::with_capacity(bytes.len() / 8);
        for chunk in bytes.chunks_exact(core::mem::size_of::<i64>()) {
            out.push(i64::from_ne_bytes(chunk.try_into().unwrap()));
        }
        Ok(out)
    }

    pub fn get_array_uint32(&self, name: &str) -> Result<Vec<u32>> {
        let bytes = self.get_copy(name, DataType::ArrayUInt32)?;
        let mut out = Vec::with_capacity(bytes.len() / 4);
        for chunk in bytes.chunks_exact(core::mem::size_of::<u32>()) {
            out.push(u32::from_ne_bytes(chunk.try_into().unwrap()));
        }
        Ok(out)
    }

    pub fn get_array_uint64(&self, name: &str) -> Result<Vec<u64>> {
        let bytes = self.get_copy(name, DataType::ArrayUInt64)?;
        let mut out = Vec::with_capacity(bytes.len() / 8);
        for chunk in bytes.chunks_exact(core::mem::size_of::<u64>()) {
            out.push(u64::from_ne_bytes(chunk.try_into().unwrap()));
        }
        Ok(out)
    }

    pub fn get_array_float(&self, name: &str) -> Result<Vec<f32>> {
        let bytes = self.get_copy(name, DataType::ArrayFloat)?;
        let mut out = Vec::with_capacity(bytes.len() / 4);
        for chunk in bytes.chunks_exact(core::mem::size_of::<f32>()) {
            out.push(f32::from_ne_bytes(chunk.try_into().unwrap()));
        }
        Ok(out)
    }

    pub fn get_array_double(&self, name: &str) -> Result<Vec<f64>> {
        let bytes = self.get_copy(name, DataType::ArrayDouble)?;
        let mut out = Vec::with_capacity(bytes.len() / 8);
        for chunk in bytes.chunks_exact(core::mem::size_of::<f64>()) {
            out.push(f64::from_ne_bytes(chunk.try_into().unwrap()));
        }
        Ok(out)
    }

    /// Retrieve a binary field (opaque bytes). Suitable for `DataType::Binary`.
    pub fn get_binary(&self, name: &str) -> Result<Vec<u8>> {
        self.get_copy(name, DataType::Binary)
    }
}

// SAFETY: DocRef is a borrowed view whose lifetime is tied to a parent `Doc`
// or `DocSet`; accessors are `&self` read-only queries.
unsafe impl Send for DocRef<'_> {}
unsafe impl Sync for DocRef<'_> {}
