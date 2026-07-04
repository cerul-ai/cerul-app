//! Logging and global configuration objects passed to [`initialize`].

use std::ptr::NonNull;

use crate::error::{check, ErrorCode, Result, ZvecError};
use crate::ffi_util::{cstr_to_string, cstring};
use crate::sys;
use crate::types::{LogLevel, LogType};

/// Initialize the zvec library.
///
/// Passing `None` uses zvec's default configuration. Safe to call once per
/// process; subsequent calls may return an error.
pub fn initialize(config: Option<&Config>) -> Result<()> {
    let ptr = config.map_or(core::ptr::null(), |c| c.as_ptr());
    check(unsafe { sys::zvec_initialize(ptr) })
}

/// Tear down any process-global state created by [`initialize`].
pub fn shutdown() -> Result<()> {
    check(unsafe { sys::zvec_shutdown() })
}

/// Returns whether [`initialize`] has been called successfully.
pub fn is_initialized() -> bool {
    unsafe { sys::zvec_is_initialized() }
}

// -----------------------------------------------------------------------------
// LogConfig
// -----------------------------------------------------------------------------

/// Logging sink + level configuration.
///
/// Ownership of a `LogConfig` is transferred to [`Config::set_log_config`];
/// after that point the underlying pointer belongs to the config and must not
/// be dropped.
pub struct LogConfig {
    ptr: NonNull<sys::zvec_log_config_t>,
}

impl LogConfig {
    /// Create a console log config with the given level.
    pub fn console(level: LogLevel) -> Result<Self> {
        let ptr = unsafe { sys::zvec_config_log_create_console(level.to_raw()) };
        Self::from_ptr(ptr, "zvec_config_log_create_console")
    }

    /// Create a file-backed log config.
    pub fn file(
        level: LogLevel,
        dir: &str,
        basename: &str,
        file_size_mb: u32,
        overdue_days: u32,
    ) -> Result<Self> {
        let c_dir = cstring(dir)?;
        let c_basename = cstring(basename)?;
        let ptr = unsafe {
            sys::zvec_config_log_create_file(
                level.to_raw(),
                c_dir.as_ptr(),
                c_basename.as_ptr(),
                file_size_mb,
                overdue_days,
            )
        };
        Self::from_ptr(ptr, "zvec_config_log_create_file")
    }

    fn from_ptr(ptr: *mut sys::zvec_log_config_t, ctx: &'static str) -> Result<Self> {
        NonNull::new(ptr).map(|ptr| Self { ptr }).ok_or_else(|| {
            ZvecError::with_message(ErrorCode::ResourceExhausted, format!("{ctx} returned NULL"))
        })
    }

    pub fn level(&self) -> LogLevel {
        LogLevel::from_raw(unsafe { sys::zvec_config_log_get_level(self.ptr.as_ptr()) })
    }

    pub fn set_level(&mut self, level: LogLevel) -> Result<()> {
        check(unsafe { sys::zvec_config_log_set_level(self.ptr.as_ptr(), level.to_raw()) })
    }

    pub fn is_file_type(&self) -> bool {
        unsafe { sys::zvec_config_log_is_file_type(self.ptr.as_ptr()) }
    }

    pub fn dir(&self) -> Option<String> {
        unsafe { cstr_to_string(sys::zvec_config_log_get_dir(self.ptr.as_ptr())) }
    }

    pub fn set_dir(&mut self, dir: &str) -> Result<()> {
        let c = cstring(dir)?;
        check(unsafe { sys::zvec_config_log_set_dir(self.ptr.as_ptr(), c.as_ptr()) })
    }

    pub fn basename(&self) -> Option<String> {
        unsafe { cstr_to_string(sys::zvec_config_log_get_basename(self.ptr.as_ptr())) }
    }

    pub fn set_basename(&mut self, basename: &str) -> Result<()> {
        let c = cstring(basename)?;
        check(unsafe { sys::zvec_config_log_set_basename(self.ptr.as_ptr(), c.as_ptr()) })
    }

    pub fn file_size_mb(&self) -> u32 {
        unsafe { sys::zvec_config_log_get_file_size(self.ptr.as_ptr()) }
    }

    pub fn set_file_size_mb(&mut self, size: u32) -> Result<()> {
        check(unsafe { sys::zvec_config_log_set_file_size(self.ptr.as_ptr(), size) })
    }

    pub fn overdue_days(&self) -> u32 {
        unsafe { sys::zvec_config_log_get_overdue_days(self.ptr.as_ptr()) }
    }

    pub fn set_overdue_days(&mut self, days: u32) -> Result<()> {
        check(unsafe { sys::zvec_config_log_set_overdue_days(self.ptr.as_ptr(), days) })
    }

    /// Consume the wrapper and return the raw pointer, bypassing `Drop`.
    fn into_raw(self) -> *mut sys::zvec_log_config_t {
        let ptr = self.ptr.as_ptr();
        core::mem::forget(self);
        ptr
    }
}

impl Drop for LogConfig {
    fn drop(&mut self) {
        unsafe { sys::zvec_config_log_destroy(self.ptr.as_ptr()) };
    }
}

// SAFETY: LogConfig is a plain config builder. All mutating methods require
// `&mut self`, and the underlying C object has no hidden shared state.
unsafe impl Send for LogConfig {}
unsafe impl Sync for LogConfig {}

// -----------------------------------------------------------------------------
// Config (global runtime configuration)
// -----------------------------------------------------------------------------

/// Global zvec runtime configuration, passed to [`initialize`].
pub struct Config {
    ptr: NonNull<sys::zvec_config_data_t>,
}

impl Config {
    pub fn new() -> Result<Self> {
        let ptr = unsafe { sys::zvec_config_data_create() };
        NonNull::new(ptr).map(|ptr| Self { ptr }).ok_or_else(|| {
            ZvecError::with_message(
                ErrorCode::ResourceExhausted,
                "zvec_config_data_create returned NULL",
            )
        })
    }

    pub(crate) fn as_ptr(&self) -> *const sys::zvec_config_data_t {
        self.ptr.as_ptr() as *const _
    }

    pub fn memory_limit_bytes(&self) -> u64 {
        unsafe { sys::zvec_config_data_get_memory_limit(self.ptr.as_ptr()) }
    }

    pub fn set_memory_limit_bytes(&mut self, bytes: u64) -> Result<()> {
        check(unsafe { sys::zvec_config_data_set_memory_limit(self.ptr.as_ptr(), bytes) })
    }

    pub fn log_type(&self) -> LogType {
        LogType::from_raw(unsafe { sys::zvec_config_data_get_log_type(self.ptr.as_ptr()) })
    }

    /// Take ownership of `log` and install it on this config.
    pub fn set_log_config(&mut self, log: LogConfig) -> Result<()> {
        let raw = log.into_raw();
        let rc = unsafe { sys::zvec_config_data_set_log_config(self.ptr.as_ptr(), raw) };
        // NOTE: ownership was transferred whether or not the call succeeded.
        check(rc)
    }

    pub fn query_thread_count(&self) -> u32 {
        unsafe { sys::zvec_config_data_get_query_thread_count(self.ptr.as_ptr()) }
    }

    pub fn set_query_thread_count(&mut self, n: u32) -> Result<()> {
        check(unsafe { sys::zvec_config_data_set_query_thread_count(self.ptr.as_ptr(), n) })
    }

    pub fn invert_to_forward_scan_ratio(&self) -> f32 {
        unsafe { sys::zvec_config_data_get_invert_to_forward_scan_ratio(self.ptr.as_ptr()) }
    }

    pub fn set_invert_to_forward_scan_ratio(&mut self, ratio: f32) -> Result<()> {
        check(unsafe {
            sys::zvec_config_data_set_invert_to_forward_scan_ratio(self.ptr.as_ptr(), ratio)
        })
    }

    pub fn brute_force_by_keys_ratio(&self) -> f32 {
        unsafe { sys::zvec_config_data_get_brute_force_by_keys_ratio(self.ptr.as_ptr()) }
    }

    pub fn set_brute_force_by_keys_ratio(&mut self, ratio: f32) -> Result<()> {
        check(unsafe {
            sys::zvec_config_data_set_brute_force_by_keys_ratio(self.ptr.as_ptr(), ratio)
        })
    }

    pub fn optimize_thread_count(&self) -> u32 {
        unsafe { sys::zvec_config_data_get_optimize_thread_count(self.ptr.as_ptr()) }
    }

    pub fn set_optimize_thread_count(&mut self, n: u32) -> Result<()> {
        check(unsafe { sys::zvec_config_data_set_optimize_thread_count(self.ptr.as_ptr(), n) })
    }
}

impl Drop for Config {
    fn drop(&mut self) {
        unsafe { sys::zvec_config_data_destroy(self.ptr.as_ptr()) };
    }
}

// SAFETY: see `LogConfig` justification.
unsafe impl Send for Config {}
unsafe impl Sync for Config {}
