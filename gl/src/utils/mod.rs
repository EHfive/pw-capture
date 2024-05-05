mod egl;
mod logger;
mod x11_lib;

use core::ffi::{c_void, CStr};

pub use egl::*;
pub use logger::*;
pub use x11_lib::*;

#[cfg(debug_assertions)]
#[macro_export]
macro_rules! cstr {
    ($bytes:expr) => {
        ::core::ffi::CStr::from_bytes_with_nul($bytes).unwrap()
    };
}
#[cfg(not(debug_assertions))]
#[macro_export]
macro_rules! cstr {
    ($bytes:expr) => {
        ::core::ffi::CStr::from_bytes_with_nul_unchecked($bytes)
    };
}
pub use cstr;

pub unsafe fn dlopen(filenames: &[&CStr]) -> Option<*mut c_void> {
    for filename in filenames {
        let h = libc::dlopen(filename.as_ptr(), libc::RTLD_LAZY);
        if !h.is_null() {
            return Some(h);
        }
    }
    if !filenames.is_empty() {
        log::warn!(
            "failed to load {}",
            filenames[filenames.len() - 1].to_string_lossy()
        );
    }
    None
}
