use std::ffi::CStr;

use crate::sys;

/// Returns the full version string reported by the linked zvec library.
///
/// The string is managed by zvec; this wrapper copies it into an owned
/// `String`.
pub fn version() -> String {
    unsafe {
        let ptr = sys::zvec_get_version();
        if ptr.is_null() {
            return String::new();
        }
        CStr::from_ptr(ptr).to_string_lossy().into_owned()
    }
}

pub fn version_major() -> i32 {
    unsafe { sys::zvec_get_version_major() as i32 }
}

pub fn version_minor() -> i32 {
    unsafe { sys::zvec_get_version_minor() as i32 }
}

pub fn version_patch() -> i32 {
    unsafe { sys::zvec_get_version_patch() as i32 }
}

/// Returns true if the linked zvec library is at least `major.minor.patch`.
pub fn check_version(major: i32, minor: i32, patch: i32) -> bool {
    unsafe { sys::zvec_check_version(major, minor, patch) }
}
