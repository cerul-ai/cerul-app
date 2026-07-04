//! [`CollectionOptions`] — per-collection runtime settings.

use std::ptr::NonNull;

use crate::error::{check, ErrorCode, Result, ZvecError};
use crate::sys;

pub struct CollectionOptions {
    ptr: NonNull<sys::zvec_collection_options_t>,
}

impl CollectionOptions {
    pub fn new() -> Result<Self> {
        let ptr = unsafe { sys::zvec_collection_options_create() };
        NonNull::new(ptr).map(|ptr| Self { ptr }).ok_or_else(|| {
            ZvecError::with_message(
                ErrorCode::ResourceExhausted,
                "zvec_collection_options_create returned NULL",
            )
        })
    }

    pub(crate) fn as_ptr(&self) -> *const sys::zvec_collection_options_t {
        self.ptr.as_ptr() as *const _
    }

    pub(crate) fn from_raw(ptr: *mut sys::zvec_collection_options_t) -> Option<Self> {
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    pub fn enable_mmap(&self) -> bool {
        unsafe { sys::zvec_collection_options_get_enable_mmap(self.as_ptr()) }
    }
    pub fn set_enable_mmap(&mut self, enable: bool) -> Result<()> {
        check(unsafe { sys::zvec_collection_options_set_enable_mmap(self.ptr.as_ptr(), enable) })
    }

    pub fn max_buffer_size(&self) -> usize {
        unsafe { sys::zvec_collection_options_get_max_buffer_size(self.as_ptr()) }
    }
    pub fn set_max_buffer_size(&mut self, size: usize) -> Result<()> {
        check(unsafe { sys::zvec_collection_options_set_max_buffer_size(self.ptr.as_ptr(), size) })
    }

    pub fn read_only(&self) -> bool {
        unsafe { sys::zvec_collection_options_get_read_only(self.as_ptr()) }
    }
    pub fn set_read_only(&mut self, ro: bool) -> Result<()> {
        check(unsafe { sys::zvec_collection_options_set_read_only(self.ptr.as_ptr(), ro) })
    }
}

impl Drop for CollectionOptions {
    fn drop(&mut self) {
        unsafe { sys::zvec_collection_options_destroy(self.ptr.as_ptr()) };
    }
}

// SAFETY: plain options builder; mutation requires `&mut self`.
unsafe impl Send for CollectionOptions {}
unsafe impl Sync for CollectionOptions {}
