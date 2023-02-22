mod format_info;
mod logger;
mod vk_helper;

pub use format_info::*;
pub use logger::*;
pub use vk_helper::*;

use core::ffi::{c_ulong, c_void};

#[derive(Clone, Copy, Debug)]
pub enum SurfaceRawHandle {
    Xlib {
        dpy: *mut c_void,
        window: c_ulong,
    },
    Xcb {
        connection: *mut c_void,
        window: u32,
    },
    Wayland {
        display: *mut c_void,
        surface: *mut c_void,
    },
}
