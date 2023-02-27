use core::ffi::{c_uchar, c_void, CStr};

#[cfg(debug_assertions)]
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
pub(crate) use cstr;

pub unsafe fn dlopen(filenames: &[&CStr]) -> Option<*mut c_void> {
    for filename in filenames {
        let h = libc::dlopen(filename.as_ptr(), libc::RTLD_LAZY);
        if !h.is_null() {
            return Some(h);
        }
    }
    if filenames.len() > 0 {
        log::warn!(
            "failed to load {}",
            filenames[filenames.len() - 1].to_string_lossy()
        );
    }
    None
}

pub unsafe fn pointer_is_dereferencable(p: *mut c_void) -> bool {
    if p.is_null() {
        return false;
    }
    let page_size = libc::sysconf(libc::_SC_PAGE_SIZE);
    if page_size <= 0 {
        return false;
    }
    let addr = (p as usize) & !(page_size as usize - 1);

    let mut valid: c_uchar = 0;
    let res = libc::mincore(addr as _, page_size as _, &mut valid);

    return res >= 0;
}
