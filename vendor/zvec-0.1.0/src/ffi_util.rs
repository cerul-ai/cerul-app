//! Small helpers that bridge between safe Rust types and the raw FFI surface.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use crate::error::{ErrorCode, Result, ZvecError};

/// Reinterpret a `&[T]` as `&[u8]` for passing to the zvec C API. The caller
/// guarantees `T` has no padding bytes — only primitives like `f32`, `i64`,
/// etc. are used here.
#[inline]
pub(crate) fn slice_as_bytes<T: Copy>(s: &[T]) -> &[u8] {
    // SAFETY: T: Copy + primitive-sized; all bit patterns are valid for the
    // u8 view and lifetime is tied to `s`.
    unsafe { core::slice::from_raw_parts(s.as_ptr() as *const u8, core::mem::size_of_val(s)) }
}

/// Convert a Rust string to a heap-allocated `CString`. Returns an error if
/// the string contains an interior NUL byte.
pub(crate) fn cstring(s: &str) -> Result<CString> {
    CString::new(s).map_err(|e| {
        ZvecError::with_message(
            ErrorCode::InvalidArgument,
            format!("string contains NUL byte at position {}", e.nul_position()),
        )
    })
}

/// Borrow a `NULL`-able C string as `Option<&str>`, copying to `String` on
/// demand.
///
/// # Safety
///
/// `ptr` must either be NULL or a pointer to a valid NUL-terminated C string
/// that will outlive the conversion.
pub(crate) unsafe fn cstr_to_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        None
    } else {
        Some(CStr::from_ptr(ptr).to_string_lossy().into_owned())
    }
}

/// Borrow a `NULL`-able C string without copying.
///
/// # Safety
///
/// Same as [`cstr_to_string`]. The returned `&str` is valid only for the
/// lifetime of the underlying buffer.
pub(crate) unsafe fn cstr_as_str<'a>(ptr: *const c_char) -> Option<&'a str> {
    if ptr.is_null() {
        None
    } else {
        CStr::from_ptr(ptr).to_str().ok()
    }
}
