//! Field and collection schema types.

use std::marker::PhantomData;
use std::ptr::NonNull;

use crate::error::{check, ErrorCode, Result, ZvecError};
use crate::ffi_util::{cstr_to_string, cstring};
use crate::index_params::IndexParams;
use crate::sys;
use crate::types::{DataType, IndexType};

// -----------------------------------------------------------------------------
// FieldSchema (owning)
// -----------------------------------------------------------------------------

pub struct FieldSchema {
    ptr: NonNull<sys::zvec_field_schema_t>,
}

impl FieldSchema {
    pub fn new(name: &str, data_type: DataType, nullable: bool, dimension: u32) -> Result<Self> {
        let c_name = cstring(name)?;
        let ptr = unsafe {
            sys::zvec_field_schema_create(c_name.as_ptr(), data_type.to_raw(), nullable, dimension)
        };
        NonNull::new(ptr).map(|ptr| Self { ptr }).ok_or_else(|| {
            ZvecError::with_message(
                ErrorCode::ResourceExhausted,
                "zvec_field_schema_create returned NULL",
            )
        })
    }

    pub(crate) fn as_ptr(&self) -> *const sys::zvec_field_schema_t {
        self.ptr.as_ptr() as *const _
    }

    /// Borrow as a non-owning [`FieldSchemaRef`].
    pub fn borrow(&self) -> FieldSchemaRef<'_> {
        FieldSchemaRef {
            ptr: self.ptr,
            _marker: PhantomData,
        }
    }

    pub fn name(&self) -> Option<String> {
        self.borrow().name()
    }

    pub fn set_name(&mut self, name: &str) -> Result<()> {
        let c = cstring(name)?;
        check(unsafe { sys::zvec_field_schema_set_name(self.ptr.as_ptr(), c.as_ptr()) })
    }

    pub fn data_type(&self) -> DataType {
        self.borrow().data_type()
    }

    pub fn set_data_type(&mut self, t: DataType) -> Result<()> {
        check(unsafe { sys::zvec_field_schema_set_data_type(self.ptr.as_ptr(), t.to_raw()) })
    }

    pub fn element_data_type(&self) -> DataType {
        self.borrow().element_data_type()
    }

    pub fn element_data_size(&self) -> usize {
        self.borrow().element_data_size()
    }

    pub fn is_vector_field(&self) -> bool {
        self.borrow().is_vector_field()
    }

    pub fn is_dense_vector(&self) -> bool {
        self.borrow().is_dense_vector()
    }

    pub fn is_sparse_vector(&self) -> bool {
        self.borrow().is_sparse_vector()
    }

    pub fn is_nullable(&self) -> bool {
        self.borrow().is_nullable()
    }

    pub fn set_nullable(&mut self, nullable: bool) -> Result<()> {
        check(unsafe { sys::zvec_field_schema_set_nullable(self.ptr.as_ptr(), nullable) })
    }

    pub fn has_invert_index(&self) -> bool {
        self.borrow().has_invert_index()
    }

    pub fn is_array_type(&self) -> bool {
        self.borrow().is_array_type()
    }

    pub fn dimension(&self) -> u32 {
        self.borrow().dimension()
    }

    pub fn set_dimension(&mut self, d: u32) -> Result<()> {
        check(unsafe { sys::zvec_field_schema_set_dimension(self.ptr.as_ptr(), d) })
    }

    pub fn index_type(&self) -> IndexType {
        self.borrow().index_type()
    }

    pub fn has_index(&self) -> bool {
        self.borrow().has_index()
    }

    /// Install a (deep-copied) index parameters object on this field.
    pub fn set_index_params(&mut self, params: &IndexParams) -> Result<()> {
        check(unsafe {
            sys::zvec_field_schema_set_index_params(self.ptr.as_ptr(), params.as_ptr())
        })
    }

    pub fn validate(&self) -> Result<()> {
        let mut err_str: *mut sys::zvec_string_t = core::ptr::null_mut();
        let rc = unsafe { sys::zvec_field_schema_validate(self.as_ptr(), &mut err_str) };
        if rc == sys::zvec_error_code_t::ZVEC_OK {
            if !err_str.is_null() {
                unsafe { sys::zvec_free_string(err_str) };
            }
            Ok(())
        } else {
            let message = unsafe { take_zvec_string(err_str) };
            Err(ZvecError {
                code: crate::error::ErrorCode::from_raw(rc),
                message,
            })
        }
    }
}

impl Drop for FieldSchema {
    fn drop(&mut self) {
        unsafe { sys::zvec_field_schema_destroy(self.ptr.as_ptr()) };
    }
}

// SAFETY: FieldSchema is a builder/description object with no hidden shared
// state; mutation requires `&mut self`.
unsafe impl Send for FieldSchema {}
unsafe impl Sync for FieldSchema {}

// SAFETY: FieldSchemaRef is a borrowed pointer whose lifetime is tied to the
// parent schema; all accessors are `&self` read-only queries.
unsafe impl Send for FieldSchemaRef<'_> {}
unsafe impl Sync for FieldSchemaRef<'_> {}

// -----------------------------------------------------------------------------
// FieldSchemaRef (non-owning)
// -----------------------------------------------------------------------------

/// Non-owning borrow of a field schema returned by schema accessors.
///
/// The underlying pointer belongs to the parent [`CollectionSchema`]; the
/// borrow's lifetime is tied to the parent.
#[derive(Clone, Copy)]
pub struct FieldSchemaRef<'a> {
    ptr: NonNull<sys::zvec_field_schema_t>,
    _marker: PhantomData<&'a sys::zvec_field_schema_t>,
}

impl<'a> FieldSchemaRef<'a> {
    fn from_ptr(ptr: *mut sys::zvec_field_schema_t) -> Option<Self> {
        NonNull::new(ptr).map(|ptr| Self {
            ptr,
            _marker: PhantomData,
        })
    }

    fn raw(self) -> *const sys::zvec_field_schema_t {
        self.ptr.as_ptr() as *const _
    }

    pub fn name(&self) -> Option<String> {
        unsafe { cstr_to_string(sys::zvec_field_schema_get_name(self.raw())) }
    }

    pub fn data_type(&self) -> DataType {
        DataType::from_raw(unsafe { sys::zvec_field_schema_get_data_type(self.raw()) })
    }

    pub fn element_data_type(&self) -> DataType {
        DataType::from_raw(unsafe { sys::zvec_field_schema_get_element_data_type(self.raw()) })
    }

    pub fn element_data_size(&self) -> usize {
        unsafe { sys::zvec_field_schema_get_element_data_size(self.raw()) }
    }

    pub fn is_vector_field(&self) -> bool {
        unsafe { sys::zvec_field_schema_is_vector_field(self.raw()) }
    }
    pub fn is_dense_vector(&self) -> bool {
        unsafe { sys::zvec_field_schema_is_dense_vector(self.raw()) }
    }
    pub fn is_sparse_vector(&self) -> bool {
        unsafe { sys::zvec_field_schema_is_sparse_vector(self.raw()) }
    }
    pub fn is_nullable(&self) -> bool {
        unsafe { sys::zvec_field_schema_is_nullable(self.raw()) }
    }
    pub fn has_invert_index(&self) -> bool {
        unsafe { sys::zvec_field_schema_has_invert_index(self.raw()) }
    }
    pub fn is_array_type(&self) -> bool {
        unsafe { sys::zvec_field_schema_is_array_type(self.raw()) }
    }
    pub fn dimension(&self) -> u32 {
        unsafe { sys::zvec_field_schema_get_dimension(self.raw()) }
    }
    pub fn index_type(&self) -> IndexType {
        IndexType::from_raw(unsafe { sys::zvec_field_schema_get_index_type(self.raw()) })
    }
    pub fn has_index(&self) -> bool {
        unsafe { sys::zvec_field_schema_has_index(self.raw()) }
    }
}

// -----------------------------------------------------------------------------
// CollectionSchema
// -----------------------------------------------------------------------------

pub struct CollectionSchema {
    ptr: NonNull<sys::zvec_collection_schema_t>,
}

impl CollectionSchema {
    pub fn new(name: &str) -> Result<Self> {
        let c = cstring(name)?;
        let ptr = unsafe { sys::zvec_collection_schema_create(c.as_ptr()) };
        NonNull::new(ptr).map(|ptr| Self { ptr }).ok_or_else(|| {
            ZvecError::with_message(
                ErrorCode::ResourceExhausted,
                "zvec_collection_schema_create returned NULL",
            )
        })
    }

    pub(crate) fn as_ptr(&self) -> *const sys::zvec_collection_schema_t {
        self.ptr.as_ptr() as *const _
    }

    pub(crate) fn from_raw(ptr: *mut sys::zvec_collection_schema_t) -> Option<Self> {
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    pub fn name(&self) -> Option<String> {
        unsafe { cstr_to_string(sys::zvec_collection_schema_get_name(self.as_ptr())) }
    }

    pub fn set_name(&mut self, name: &str) -> Result<()> {
        let c = cstring(name)?;
        check(unsafe { sys::zvec_collection_schema_set_name(self.ptr.as_ptr(), c.as_ptr()) })
    }

    pub fn add_field(&mut self, field: &FieldSchema) -> Result<()> {
        check(unsafe { sys::zvec_collection_schema_add_field(self.ptr.as_ptr(), field.as_ptr()) })
    }

    pub fn alter_field(&mut self, field_name: &str, new_field: &FieldSchema) -> Result<()> {
        let c = cstring(field_name)?;
        check(unsafe {
            sys::zvec_collection_schema_alter_field(
                self.ptr.as_ptr(),
                c.as_ptr(),
                new_field.as_ptr(),
            )
        })
    }

    pub fn drop_field(&mut self, field_name: &str) -> Result<()> {
        let c = cstring(field_name)?;
        check(unsafe { sys::zvec_collection_schema_drop_field(self.ptr.as_ptr(), c.as_ptr()) })
    }

    pub fn has_field(&self, field_name: &str) -> bool {
        let Ok(c) = cstring(field_name) else {
            return false;
        };
        unsafe { sys::zvec_collection_schema_has_field(self.as_ptr(), c.as_ptr()) }
    }

    pub fn field(&self, field_name: &str) -> Result<Option<FieldSchemaRef<'_>>> {
        let c = cstring(field_name)?;
        let p = unsafe { sys::zvec_collection_schema_get_field(self.as_ptr(), c.as_ptr()) };
        Ok(FieldSchemaRef::from_ptr(p))
    }

    pub fn forward_field(&self, field_name: &str) -> Result<Option<FieldSchemaRef<'_>>> {
        let c = cstring(field_name)?;
        let p = unsafe { sys::zvec_collection_schema_get_forward_field(self.as_ptr(), c.as_ptr()) };
        Ok(FieldSchemaRef::from_ptr(p))
    }

    pub fn vector_field(&self, field_name: &str) -> Result<Option<FieldSchemaRef<'_>>> {
        let c = cstring(field_name)?;
        let p = unsafe { sys::zvec_collection_schema_get_vector_field(self.as_ptr(), c.as_ptr()) };
        Ok(FieldSchemaRef::from_ptr(p))
    }

    fn collect_fields(
        &self,
        getter: unsafe extern "C" fn(
            *const sys::zvec_collection_schema_t,
            *mut *mut *mut sys::zvec_field_schema_t,
            *mut usize,
        ) -> sys::zvec_error_code_t::Type,
    ) -> Result<Vec<FieldSchemaRef<'_>>> {
        let mut arr: *mut *mut sys::zvec_field_schema_t = core::ptr::null_mut();
        let mut count: usize = 0;
        check(unsafe { getter(self.as_ptr(), &mut arr, &mut count) })?;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let p = unsafe { *arr.add(i) };
            if let Some(r) = FieldSchemaRef::from_ptr(p) {
                out.push(r);
            }
        }
        if !arr.is_null() {
            unsafe { sys::zvec_free(arr as *mut _) };
        }
        Ok(out)
    }

    pub fn forward_fields(&self) -> Result<Vec<FieldSchemaRef<'_>>> {
        self.collect_fields(sys::zvec_collection_schema_get_forward_fields)
    }

    pub fn forward_fields_with_index(&self) -> Result<Vec<FieldSchemaRef<'_>>> {
        self.collect_fields(sys::zvec_collection_schema_get_forward_fields_with_index)
    }

    pub fn vector_fields(&self) -> Result<Vec<FieldSchemaRef<'_>>> {
        self.collect_fields(sys::zvec_collection_schema_get_vector_fields)
    }

    fn collect_names(
        &self,
        getter: unsafe extern "C" fn(
            *const sys::zvec_collection_schema_t,
            *mut *mut *const std::os::raw::c_char,
            *mut usize,
        ) -> sys::zvec_error_code_t::Type,
    ) -> Result<Vec<String>> {
        let mut arr: *mut *const std::os::raw::c_char = core::ptr::null_mut();
        let mut count: usize = 0;
        check(unsafe { getter(self.as_ptr(), &mut arr, &mut count) })?;
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

    pub fn forward_field_names(&self) -> Result<Vec<String>> {
        self.collect_names(sys::zvec_collection_schema_get_forward_field_names)
    }

    pub fn forward_field_names_with_index(&self) -> Result<Vec<String>> {
        self.collect_names(sys::zvec_collection_schema_get_forward_field_names_with_index)
    }

    pub fn all_field_names(&self) -> Result<Vec<String>> {
        self.collect_names(sys::zvec_collection_schema_get_all_field_names)
    }

    pub fn max_doc_count_per_segment(&self) -> u64 {
        unsafe { sys::zvec_collection_schema_get_max_doc_count_per_segment(self.as_ptr()) }
    }

    pub fn set_max_doc_count_per_segment(&mut self, n: u64) -> Result<()> {
        check(unsafe {
            sys::zvec_collection_schema_set_max_doc_count_per_segment(self.ptr.as_ptr(), n)
        })
    }

    pub fn validate(&self) -> Result<()> {
        let mut err_str: *mut sys::zvec_string_t = core::ptr::null_mut();
        let rc = unsafe { sys::zvec_collection_schema_validate(self.as_ptr(), &mut err_str) };
        if rc == sys::zvec_error_code_t::ZVEC_OK {
            if !err_str.is_null() {
                unsafe { sys::zvec_free_string(err_str) };
            }
            Ok(())
        } else {
            let message = unsafe { take_zvec_string(err_str) };
            Err(ZvecError {
                code: crate::error::ErrorCode::from_raw(rc),
                message,
            })
        }
    }

    pub fn add_index(&mut self, field_name: &str, params: &IndexParams) -> Result<()> {
        let c = cstring(field_name)?;
        check(unsafe {
            sys::zvec_collection_schema_add_index(self.ptr.as_ptr(), c.as_ptr(), params.as_ptr())
        })
    }

    pub fn drop_index(&mut self, field_name: &str) -> Result<()> {
        let c = cstring(field_name)?;
        check(unsafe { sys::zvec_collection_schema_drop_index(self.ptr.as_ptr(), c.as_ptr()) })
    }

    pub fn has_index(&self, field_name: &str) -> bool {
        let Ok(c) = cstring(field_name) else {
            return false;
        };
        unsafe { sys::zvec_collection_schema_has_index(self.as_ptr(), c.as_ptr()) }
    }
}

impl Drop for CollectionSchema {
    fn drop(&mut self) {
        unsafe { sys::zvec_collection_schema_destroy(self.ptr.as_ptr()) };
    }
}

// SAFETY: see FieldSchema.
unsafe impl Send for CollectionSchema {}
unsafe impl Sync for CollectionSchema {}

unsafe fn take_zvec_string(ptr: *mut sys::zvec_string_t) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let cptr = sys::zvec_string_c_str(ptr);
    let out = cstr_to_string(cptr);
    sys::zvec_free_string(ptr);
    out
}
