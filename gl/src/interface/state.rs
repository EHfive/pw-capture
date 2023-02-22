use crate::elfhack::*;
use crate::utils::*;

use super::*;

use core::ffi::CStr;
use core::mem;
use core::ptr;
use std::ffi::CString;

use dashmap::DashMap;
use libc::RTLD_NEXT;
use libc::{c_char, c_void};
use once_cell::sync::Lazy;
use pw_capture_client as client;

pub static GLOBAL_INIT: Lazy<()> = Lazy::new(|| init_logger());

pub static DLSYMS: Lazy<(DlsymFunc, Option<DlvsymFunc>)> = Lazy::new(|| {
    Lazy::force(&GLOBAL_INIT);
    let (dlsym, dlvsym) = load_dlsym_dlvsym();
    (dlsym.expect("this layer requires dlsym to work"), dlvsym)
});

#[inline]
pub unsafe fn real_dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void {
    (DLSYMS.0)(handle, symbol)
}

#[allow(non_camel_case_types)]
type PFN_GetProcAddress = unsafe extern "C" fn(proc_name: *const c_char) -> *mut c_void;

unsafe fn load_gpa(filenames: &[&CStr], symbol: &CStr) -> Option<PFN_GetProcAddress> {
    Lazy::force(&GLOBAL_INIT);
    let res = loop {
        let gpa = real_dlsym(RTLD_NEXT, symbol.as_ptr());
        if !gpa.is_null() {
            break gpa;
        }
        let handle = dlopen(filenames)?;
        let gpa = real_dlsym(handle, symbol.as_ptr());
        if !gpa.is_null() {
            break gpa;
        }

        log::warn!("failed to load {}", symbol.to_string_lossy());
        return None;
    };
    mem::transmute(res)
}

pub static GL_EGL: Lazy<Option<(Gl, Egl)>> = Lazy::new(|| unsafe {
    Lazy::force(&GLOBAL_INIT);
    let gpa = load_gpa(
        &[cstr!(b"libEGL.so.1\0"), cstr!(b"libEGL.so\0")],
        cstr!(b"eglGetProcAddress\0"),
    )?;
    let gl = Gl::load_with(|name| {
        let name = CString::new(name).expect("invalid string");
        gpa(name.as_ptr())
    });
    let egl = Egl::load_with(|name| {
        let name = CString::new(name).expect("invalid string");
        gpa(name.as_ptr())
    });
    Some((gl, egl))
});

pub static GL_GLX: Lazy<Option<(Gl, Glx)>> = Lazy::new(|| unsafe {
    Lazy::force(&GLOBAL_INIT);
    let glx_files = &[cstr!(b"libGLX.so.0\0"), cstr!(b"libGLX.so\0")];
    let gl_files = &[cstr!(b"libGL.so.1\0"), cstr!(b"libGL.so\0")];
    let gpa = load_gpa(glx_files, cstr!(b"glXGetProcAddress\0"))
        .or_else(|| load_gpa(gl_files, cstr!(b"glXGetProcAddress\0")))
        .or_else(|| load_gpa(glx_files, cstr!(b"glXGetProcAddressARB\0")))
        .or_else(|| load_gpa(gl_files, cstr!(b"glXGetProcAddressARB\0")))?;
    let gl = Gl::load_with(|name| {
        let name = CString::new(name).expect("invalid string");
        gpa(name.as_ptr())
    });
    let glx = Glx::load_with(|name| {
        let name = CString::new(name).expect("invalid string");
        gpa(name.as_ptr())
    });
    Some((gl, glx))
});

pub static X11_LIB: Lazy<Option<X11Lib>> = Lazy::new(|| unsafe {
    X11Lib::load_with(|handle, symbol| real_dlsym(handle, symbol.as_ptr()))
});

pub static CLIENT: Lazy<Option<client::Client>> = Lazy::new(|| {
    Lazy::force(&GLOBAL_INIT);
    client::Client::new()
        .map_err(|e| error!(target:"client init", "failed to create client: {e:?}"))
        .ok()
});

pub static DISPLAY_MAP: Lazy<DashMap<GlHandle, LayerDisplay>> = Lazy::new(|| DashMap::new());
pub static SURFACE_MAP: Lazy<DashMap<GlHandle, LayerSurface>> = Lazy::new(|| DashMap::new());

#[inline]
pub fn glx() -> &'static Glx {
    &GL_GLX.as_ref().unwrap().1
}

#[inline]
pub fn egl() -> &'static Egl {
    &GL_EGL.as_ref().unwrap().1
}

#[inline]
pub fn gl(native: NativeIface) -> &'static Gl {
    match native {
        NativeIface::Egl => &GL_EGL.as_ref().unwrap().0,
        NativeIface::Glx => &GL_GLX.as_ref().unwrap().0,
    }
}

impl FenceSync {
    pub unsafe fn new(native: NativeIface, dpy: *const c_void) -> Option<Self> {
        let gl = gl(native);
        let sync = if gl.FenceSync.is_loaded() {
            let sync = gl.FenceSync(gl_sys::SYNC_GPU_COMMANDS_COMPLETE, 0);
            Self::Gl {
                native,
                sync: glhandle!(sync),
            }
        } else if let NativeIface::Egl = native {
            let egl = egl();
            if egl.CreateSync.is_loaded() {
                let sync = egl.CreateSync(dpy, egl_sys::SYNC_FENCE, ptr::null());
                Self::Egl {
                    dpy: glhandle!(dpy),
                    sync: glhandle!(sync),
                }
            } else if egl.CreateSyncKHR.is_loaded() {
                let sync = egl.CreateSyncKHR(dpy, egl_sys::SYNC_FENCE, ptr::null());
                Self::EglKhr {
                    dpy: glhandle!(dpy),
                    sync: glhandle!(sync),
                }
            } else {
                return None;
            }
        } else {
            return None;
        };
        Some(sync)
    }

    pub unsafe fn wait(&self) {
        match self {
            Self::Gl { native, sync } => {
                gl(*native).ClientWaitSync(sync.as_ptr(), 0, u64::MAX);
            }
            Self::Egl { dpy, sync } => {
                egl().ClientWaitSync(dpy.as_ptr(), sync.as_ptr(), 0, egl_sys::FOREVER);
            }
            Self::EglKhr { dpy, sync } => {
                egl().ClientWaitSyncKHR(dpy.as_ptr(), sync.as_ptr(), 0, egl_sys::FOREVER);
            }
        }
    }
}
