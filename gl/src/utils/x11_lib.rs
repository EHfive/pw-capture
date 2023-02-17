use super::{cstr, dlopen};

use core::ffi::{c_int, c_uint, c_ulong, c_void, CStr};
use core::mem;
use std::ffi::CString;

#[repr(C)]
#[allow(non_camel_case_types)]
pub struct xcb_dri3_buffers_from_pixmap_cookie_t {
    pub sequence: c_uint,
}
#[allow(non_camel_case_types)]
pub type xcb_pixmap_t = u32;

#[repr(C)]
#[derive(Debug)]
#[allow(non_camel_case_types)]
pub struct xcb_dri3_buffers_from_pixmap_reply_t {
    pub response_type: u8,
    pub nfd: u8,
    pub sequence: u16,
    pub length: u32,
    pub width: u16,
    pub height: u16,
    pub _pad0: [u8; 4],
    pub modifier: u64,
    pub depth: u8,
    pub bpp: u8,
    pub _pad1: [u8; 6],
}

#[allow(non_snake_case)]
pub struct X11Lib {
    pub XDefaultRootWindow: unsafe extern "C" fn(dpy: *mut c_void) -> c_ulong,
    pub XDefaultScreen: unsafe extern "C" fn(dpy: *mut c_void) -> c_int,
    pub XCreatePixmap: unsafe extern "C" fn(
        dpy: *mut c_void,
        drawable: c_ulong,
        width: c_uint,
        height: c_uint,
        depth: c_uint,
    ) -> c_ulong,
    pub XFreePixmap: unsafe extern "C" fn(dpy: *mut c_void, pixmap: c_ulong) -> c_int,
    pub XFree: unsafe extern "C" fn(m: *mut c_void) -> c_int,
    pub XGetXCBConnection: unsafe extern "C" fn(dpy: *mut c_void) -> *mut c_void,
    pub xcb_dri3_buffers_from_pixmap: unsafe extern "C" fn(
        xcb_conn: *mut c_void,
        pixmap: xcb_pixmap_t,
    )
        -> xcb_dri3_buffers_from_pixmap_cookie_t,
    pub xcb_dri3_buffers_from_pixmap_reply:
        unsafe extern "C" fn(
            xcb_conn: *mut c_void,
            cookie: xcb_dri3_buffers_from_pixmap_cookie_t,
            *mut *mut c_void,
        ) -> *mut xcb_dri3_buffers_from_pixmap_reply_t,
    pub xcb_dri3_buffers_from_pixmap_reply_fds: unsafe extern "C" fn(
        xcb_conn: *mut c_void,
        reply: *const xcb_dri3_buffers_from_pixmap_reply_t,
    ) -> *mut c_int,
    pub xcb_dri3_buffers_from_pixmap_strides:
        unsafe extern "C" fn(reply: *const xcb_dri3_buffers_from_pixmap_reply_t) -> *mut u32,

    pub xcb_dri3_buffers_from_pixmap_offsets:
        unsafe extern "C" fn(reply: *const xcb_dri3_buffers_from_pixmap_reply_t) -> *mut u32,
}

impl X11Lib {
    pub unsafe fn load_with<F>(mut dlsym: F) -> Option<Self>
    where
        F: FnMut(*mut c_void, &CStr) -> *const c_void,
    {
        macro_rules! dlsym {
            ($h: expr, $cstr: expr) => {{
                let res = dlsym($h, $cstr);
                if res.is_null() {
                    return None;
                } else {
                    mem::transmute(res)
                }
            }};
        }
        macro_rules! construct {
            ($(  $sym:ident : $h:expr , )*) => {
                Self {
                    $( $sym : dlsym!($h, CString::new(stringify!($sym)).unwrap().as_c_str()) , )*
                }
            };
        }

        let xlib = dlopen(&[cstr!(b"libX11.so.6\0"), cstr!(b"libX11.so\0")])?;
        let xlib_xcb = dlopen(&[cstr!(b"libX11-xcb.so.1\0"), cstr!(b"libX11-xcb.so\0")])?;
        let dri3 = dlopen(&[cstr!(b"libxcb-dri3.so.0\0"), cstr!(b"libxcb-dri3.so\0")])?;

        Some(construct!(
            XDefaultRootWindow: xlib,
            XDefaultScreen: xlib,
            XCreatePixmap: xlib,
            XFreePixmap: xlib,
            XFree: xlib,
            XGetXCBConnection: xlib_xcb,
            xcb_dri3_buffers_from_pixmap: dri3,
            xcb_dri3_buffers_from_pixmap_reply: dri3,
            xcb_dri3_buffers_from_pixmap_reply_fds: dri3,
            xcb_dri3_buffers_from_pixmap_strides: dri3,
            xcb_dri3_buffers_from_pixmap_offsets: dri3,
        ))
    }
}
