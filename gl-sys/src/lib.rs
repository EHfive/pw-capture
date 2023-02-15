mod egl_sys;
mod glx_sys;

pub use egl_sys::*;
pub use glx_sys::*;

pub mod gl {
    #![allow(clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/gl_bindings.rs"));

    unsafe impl Sync for Gl {}
}

pub mod prelude {
    pub use crate::egl::{self as egl_sys, types as egl_t, Egl};
    pub use crate::gl::{self as gl_sys, types as gl_t, Gl};
    pub use crate::glx::{self as glx_sys, types as glx_t, Glx};
}
