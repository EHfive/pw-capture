mod logger;
mod x11_lib;

pub use logger::*;
pub use x11_lib::*;

#[macro_export]
macro_rules! cstr {
    ($bytes:expr) => {
        ::core::ffi::CStr::from_bytes_with_nul_unchecked($bytes)
    };
}
pub use cstr;
