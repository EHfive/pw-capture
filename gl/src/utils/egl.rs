use super::{cstr, dlopen};

use core::ffi::{c_int, c_uchar, c_void};
use core::ptr;
use std::env;

use pw_capture_gl_sys::prelude::egl_sys;

const EGL_DEFAULT_DISPLAY: *mut c_void = ptr::null_mut();

#[derive(PartialEq, Eq, Debug)]
pub enum EglPlatform {
    X11,
    Xcb,
    Wayland,
    Unknown,
}

pub fn egl_platform_from_ext(platform_ext: u32) -> EglPlatform {
    match platform_ext {
        egl_sys::PLATFORM_X11_EXT => EglPlatform::X11,
        egl_sys::PLATFORM_XCB_EXT => EglPlatform::Xcb,
        egl_sys::PLATFORM_WAYLAND_EXT => EglPlatform::Wayland,
        _ => EglPlatform::Unknown,
    }
}

fn egl_get_native_platform_from_env() -> Option<EglPlatform> {
    let plat_name = 'outer: {
        if let Ok(plat_name) = env::var("EGL_PLATFORM") {
            if !plat_name.is_empty() {
                break 'outer plat_name;
            }
        }
        if let Ok(plat_name) = env::var("EGL_DISPLAY") {
            if !plat_name.is_empty() {
                break 'outer plat_name;
            }
        }
        return None;
    };

    let res = match plat_name.as_str() {
        "x11" => EglPlatform::X11,
        "xcb" => EglPlatform::Xcb,
        "wayland" => EglPlatform::Xcb,
        _ => EglPlatform::Unknown,
    };
    Some(res)
}

// see https://gitlab.freedesktop.org/mesa/mesa/-/blob/44ccaca41d41e5dfa660f7c2fb6e50aa2ff03e22/src/egl/main/eglglobals.c#L142-161
unsafe fn egl_pointer_is_dereferencable(p: *mut c_void) -> bool {
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

    res >= 0
}

unsafe fn egl_native_platform_detect_native_display(
    native_display: *mut c_void,
) -> Option<EglPlatform> {
    if native_display == EGL_DEFAULT_DISPLAY {
        return None;
    }

    if egl_pointer_is_dereferencable(native_display) {
        let first = *(native_display as *mut *mut c_void);
        if first.is_null() {
            return None;
        }
        if let Some(handle) = dlopen(&[
            cstr!(b"libwayland-client.so.0\0"),
            cstr!(b"libwayland-client.so\0"),
        ]) {
            let wl_display_interface =
                libc::dlsym(handle, cstr!(b"wl_display_interface\0").as_ptr());
            if wl_display_interface == first {
                return Some(EglPlatform::Wayland);
            }
        }
    }

    None
}

#[repr(C)]
#[allow(non_camel_case_types)]
pub struct wl_egl_window_v3 {
    version: usize,
    width: c_int,
    height: c_int,
    dx: c_int,
    dy: c_int,
    attached_width: c_int,
    attached_height: c_int,
    driver_private: *mut c_void,
    resize_callback: Option<unsafe extern "C" fn(*mut wl_egl_window_v3, *mut c_void)>,
    destroy_window_callback: Option<unsafe extern "C" fn(*mut c_void)>,
    surface: *mut c_void,
}

// see https://gitlab.freedesktop.org/mesa/mesa/-/blob/c8d7e0c0235327928d9d9b12c0b603739e53f1c5/src/egl/drivers/dri2/platform_wayland.c#L417-428
pub unsafe fn wl_egl_window_get_wl_surface(window: *mut wl_egl_window_v3) -> *mut c_void {
    let window = &*window;
    if egl_pointer_is_dereferencable(window.version as _) {
        return window.version as _;
    }
    match window.version {
        3 => window.surface,
        _ => {
            log::warn!("unhandled wl_egl_window struct version {}", window.version);
            ptr::null_mut()
        }
    }
}

pub unsafe fn egl_get_native_platform(native_display: *mut c_void) -> Option<EglPlatform> {
    if let Some(plat) = egl_get_native_platform_from_env() {
        return Some(plat);
    }

    if let Some(plat) = egl_native_platform_detect_native_display(native_display) {
        return Some(plat);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ffi::c_char;
    use core::mem;

    #[allow(non_camel_case_types)]
    type PFN_wl_display_connect = unsafe extern "C" fn(*const c_char) -> *mut c_void;
    #[test]
    fn wayland() {
        unsafe {
            if let Some(handle) = dlopen(&[
                cstr!(b"libwayland-client.so.0\0"),
                cstr!(b"libwayland-client.so\0"),
            ]) {
                let wl_display_connect =
                    libc::dlsym(handle, cstr!(b"wl_display_connect\0").as_ptr());
                if wl_display_connect.is_null() {
                    return;
                }
                let wl_display_connect: PFN_wl_display_connect = mem::transmute(wl_display_connect);
                let wl_display = wl_display_connect(ptr::null_mut());
                if wl_display.is_null() {
                    return;
                }
                assert!(egl_pointer_is_dereferencable(wl_display));
                assert_eq!(
                    egl_native_platform_detect_native_display(wl_display),
                    Some(EglPlatform::Wayland)
                )
            }
        }
    }
}
