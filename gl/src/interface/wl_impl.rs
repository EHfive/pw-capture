use super::*;

use core::ffi::c_int;

use pw_capture_cursor::wl_sys::*;
use pw_capture_cursor::{CursorManager, CursorSnapshot, WlCursorManager};

use dashmap::DashMap;
use once_cell::sync::Lazy;

static CURSOR_MANAGER_MAP: Lazy<DashMap<usize, Box<WlCursorManager>>> =
    Lazy::new(DashMap::new);

#[no_mangle]
pub unsafe fn me_eh5_pw_capture_get_wl_cursor_manager(
    display: *mut c_void,
    surface: *mut c_void,
) -> usize {
    if display.is_null() || surface.is_null() {
        return 0;
    }
    let manager = WL_INTERCEPT
        .as_ref()
        .and_then(|intercept| intercept.get_cursor_manager(display as _, surface as _))
        .map(Box::new);
    if let Some(m) = manager {
        let handle = m.as_ref() as *const _ as usize;
        CURSOR_MANAGER_MAP.insert(handle, m);
        handle
    } else {
        0
    }
}

#[no_mangle]
pub unsafe fn me_eh5_pw_capture_release_wl_cursor_manager(cursor_manager: usize) -> bool {
    if cursor_manager == 0 {
        return false;
    }
    CURSOR_MANAGER_MAP.remove(&cursor_manager).is_some()
}

// FIXME: design a proper C interface
#[no_mangle]
pub unsafe fn me_eh5_pw_capture_wl_cursor_snapshot(
    cursor_manager: usize,
    serial: u64,
) -> Option<Box<dyn CursorSnapshot>> {
    if cursor_manager == 0 {
        return None;
    }
    let manager = CURSOR_MANAGER_MAP.get(&cursor_manager)?;
    let snap = manager.snapshot_cursor(serial).ok()?;
    Some(snap)
}

#[inline(never)]
pub unsafe extern "C" fn impl_wl_proxy_marshal_array_flags(
    proxy: *mut wl_proxy,
    opcode: u32,
    interface: *const wl_interface,
    version: u32,
    flags: u32,
    args: *mut wl_argument,
) -> *mut wl_proxy {
    let wl_intercept = WL_INTERCEPT.as_ref().unwrap();
    wl_intercept
        .intercept_wl_proxy_marshal_array_flags(proxy, opcode, interface, version, flags, args)
}

#[inline(never)]
pub unsafe extern "C" fn impl_wl_proxy_create(
    factory: *mut wl_proxy,
    interface: *const wl_interface,
) -> *mut wl_proxy {
    let wl_intercept = WL_INTERCEPT.as_ref().unwrap();
    wl_intercept.intercept_wl_proxy_create(factory, interface)
}

#[inline(never)]
pub unsafe extern "C" fn impl_wl_proxy_add_listener(
    proxy: *mut wl_proxy,
    implementation: *mut PFN_void,
    data: *mut c_void,
) -> c_int {
    let wl_intercept = WL_INTERCEPT.as_ref().unwrap();
    wl_intercept.intercept_wl_proxy_add_listener(proxy, implementation, data)
}

#[inline(never)]
pub unsafe extern "C" fn impl_wl_proxy_add_dispatcher(
    proxy: *mut wl_proxy,
    dispatcher: Option<PFN_wl_dispatcher>,
    implementation: *mut c_void,
    data: *mut c_void,
) -> c_int {
    let wl_intercept = WL_INTERCEPT.as_ref().unwrap();
    wl_intercept.intercept_wl_proxy_add_dispatcher(proxy, dispatcher, implementation, data)
}

#[inline(never)]
pub unsafe extern "C" fn impl_wl_proxy_get_listener(proxy: *mut wl_proxy) -> *mut c_void {
    let wl_intercept = WL_INTERCEPT.as_ref().unwrap();
    wl_intercept.intercept_wl_proxy_get_listener(proxy)
}

#[inline(never)]
pub unsafe extern "C" fn impl_wl_proxy_destroy(proxy: *mut wl_proxy) {
    let wl_intercept = WL_INTERCEPT.as_ref().unwrap();
    wl_intercept.intercept_wl_proxy_destroy(proxy)
}

#[cfg(feature = "nightly")]
#[no_mangle]
pub unsafe extern "C" fn wl_proxy_marshal_flags(
    proxy: *mut wl_proxy,
    opcode: u32,
    interface: *const wl_interface,
    version: u32,
    flags: u32,
    args: ...
) -> *mut wl_proxy {
    let proxy_interface = &**(proxy as *mut *const wl_interface);
    let method = &*proxy_interface.methods.offset(opcode as _);
    let mut args = VaListToWlArgumentIter::new(method.signature, args).collect::<Vec<_>>();

    impl_wl_proxy_marshal_array_flags(proxy, opcode, interface, version, flags, args.as_mut_ptr())
}

#[cfg(feature = "nightly")]
#[inline(never)]
pub unsafe extern "C" fn impl_wl_proxy_marshal_flags(
    proxy: *mut wl_proxy,
    opcode: u32,
    interface: *const wl_interface,
    version: u32,
    flags: u32,
    args: ...
) -> *mut wl_proxy {
    let proxy_interface = &**(proxy as *mut *const wl_interface);
    let method = &*proxy_interface.methods.offset(opcode as _);
    let mut args = VaListToWlArgumentIter::new(method.signature, args).collect::<Vec<_>>();

    impl_wl_proxy_marshal_array_flags(proxy, opcode, interface, version, flags, args.as_mut_ptr())
}
