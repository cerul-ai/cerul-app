use core::fmt;
use std::ffi::CStr;

use crate::sys;
use crate::sys::zvec_error_code_t as raw;

/// Strongly-typed mirror of [`sys::zvec_error_code_t`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    Ok,
    NotFound,
    AlreadyExists,
    InvalidArgument,
    PermissionDenied,
    FailedPrecondition,
    ResourceExhausted,
    Unavailable,
    Internal,
    NotSupported,
    Unknown,
    /// An error code the bindings did not recognise (e.g. from a newer zvec).
    Other(i32),
}

impl ErrorCode {
    pub fn from_raw(code: sys::zvec_error_code_t::Type) -> Self {
        match code {
            raw::ZVEC_OK => ErrorCode::Ok,
            raw::ZVEC_ERROR_NOT_FOUND => ErrorCode::NotFound,
            raw::ZVEC_ERROR_ALREADY_EXISTS => ErrorCode::AlreadyExists,
            raw::ZVEC_ERROR_INVALID_ARGUMENT => ErrorCode::InvalidArgument,
            raw::ZVEC_ERROR_PERMISSION_DENIED => ErrorCode::PermissionDenied,
            raw::ZVEC_ERROR_FAILED_PRECONDITION => ErrorCode::FailedPrecondition,
            raw::ZVEC_ERROR_RESOURCE_EXHAUSTED => ErrorCode::ResourceExhausted,
            raw::ZVEC_ERROR_UNAVAILABLE => ErrorCode::Unavailable,
            raw::ZVEC_ERROR_INTERNAL_ERROR => ErrorCode::Internal,
            raw::ZVEC_ERROR_NOT_SUPPORTED => ErrorCode::NotSupported,
            raw::ZVEC_ERROR_UNKNOWN => ErrorCode::Unknown,
            other => ErrorCode::Other(other as i32),
        }
    }

    pub fn to_raw(self) -> sys::zvec_error_code_t::Type {
        match self {
            ErrorCode::Ok => raw::ZVEC_OK,
            ErrorCode::NotFound => raw::ZVEC_ERROR_NOT_FOUND,
            ErrorCode::AlreadyExists => raw::ZVEC_ERROR_ALREADY_EXISTS,
            ErrorCode::InvalidArgument => raw::ZVEC_ERROR_INVALID_ARGUMENT,
            ErrorCode::PermissionDenied => raw::ZVEC_ERROR_PERMISSION_DENIED,
            ErrorCode::FailedPrecondition => raw::ZVEC_ERROR_FAILED_PRECONDITION,
            ErrorCode::ResourceExhausted => raw::ZVEC_ERROR_RESOURCE_EXHAUSTED,
            ErrorCode::Unavailable => raw::ZVEC_ERROR_UNAVAILABLE,
            ErrorCode::Internal => raw::ZVEC_ERROR_INTERNAL_ERROR,
            ErrorCode::NotSupported => raw::ZVEC_ERROR_NOT_SUPPORTED,
            ErrorCode::Unknown => raw::ZVEC_ERROR_UNKNOWN,
            ErrorCode::Other(n) => n as sys::zvec_error_code_t::Type,
        }
    }

    pub fn is_ok(self) -> bool {
        matches!(self, ErrorCode::Ok)
    }

    /// Return the description zvec attaches to this code (via
    /// [`sys::zvec_error_code_to_string`]).
    pub fn description(self) -> &'static str {
        unsafe {
            let ptr = sys::zvec_error_code_to_string(self.to_raw());
            if ptr.is_null() {
                return "";
            }
            // SAFETY: zvec documents these as static strings owned by the library.
            CStr::from_ptr(ptr).to_str().unwrap_or("")
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorCode::Ok => f.write_str("ok"),
            ErrorCode::NotFound => f.write_str("not found"),
            ErrorCode::AlreadyExists => f.write_str("already exists"),
            ErrorCode::InvalidArgument => f.write_str("invalid argument"),
            ErrorCode::PermissionDenied => f.write_str("permission denied"),
            ErrorCode::FailedPrecondition => f.write_str("failed precondition"),
            ErrorCode::ResourceExhausted => f.write_str("resource exhausted"),
            ErrorCode::Unavailable => f.write_str("unavailable"),
            ErrorCode::Internal => f.write_str("internal error"),
            ErrorCode::NotSupported => f.write_str("not supported"),
            ErrorCode::Unknown => f.write_str("unknown error"),
            ErrorCode::Other(n) => write!(f, "error code {n}"),
        }
    }
}

/// Error returned by fallible safe wrappers over the zvec C API.
///
/// The `message` field, when present, is the last-error message retrieved via
/// [`sys::zvec_get_last_error`] at the time the error was constructed.
#[derive(Debug, Clone)]
pub struct ZvecError {
    pub code: ErrorCode,
    pub message: Option<String>,
}

/// Convenience alias for `Result<T, ZvecError>` used throughout the crate.
pub type Result<T> = core::result::Result<T, ZvecError>;

impl ZvecError {
    /// Construct an error from a raw code, pulling the current thread-local
    /// message out of the C API if one is set.
    pub fn from_code(code: sys::zvec_error_code_t::Type) -> Self {
        let message = unsafe { take_last_error_message() };
        Self {
            code: ErrorCode::from_raw(code),
            message,
        }
    }

    /// Construct an error with a static message (used for cases where the C
    /// API did not provide a detailed message, e.g. `NULL`-returning creators).
    pub fn with_message(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: Some(message.into()),
        }
    }
}

impl fmt::Display for ZvecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.message {
            Some(msg) => write!(f, "{}: {msg}", self.code),
            None => write!(f, "{}", self.code),
        }
    }
}

impl std::error::Error for ZvecError {}

/// Convert a raw error code returned by a zvec C API call into a `Result`.
pub(crate) fn check(code: sys::zvec_error_code_t::Type) -> Result<()> {
    if code == raw::ZVEC_OK {
        Ok(())
    } else {
        Err(ZvecError::from_code(code))
    }
}

/// Clear the current thread-local last-error slot.
pub fn clear_last_error() {
    unsafe { sys::zvec_clear_error() };
}

/// Pulls the last error message out of zvec and frees the underlying buffer.
///
/// # Safety
///
/// Requires that the zvec C API has been linked in.
unsafe fn take_last_error_message() -> Option<String> {
    let mut buf: *mut std::os::raw::c_char = core::ptr::null_mut();
    let code = sys::zvec_get_last_error(&mut buf as *mut _);
    if code != raw::ZVEC_OK || buf.is_null() {
        if !buf.is_null() {
            sys::zvec_free(buf as *mut _);
        }
        return None;
    }
    let msg = CStr::from_ptr(buf).to_string_lossy().into_owned();
    sys::zvec_free(buf as *mut _);
    Some(msg)
}
