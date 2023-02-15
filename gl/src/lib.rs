mod elfhack;
mod utils;

use elfhack::*;
use utils::*;

use core::ffi::CStr;
use core::fmt::Debug;
use core::mem;
use core::ptr;
use core::slice;
use std::collections::VecDeque;
use std::ffi::CString;
use std::result::Result::Ok;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use client::BufferPlaneInfo;
use dashmap::DashMap;
use function_name::named;
use libc::RTLD_NEXT;
use libc::{c_char, c_void};
use once_cell::sync::Lazy;
use pw_capture_client as client;
use pw_capture_gl_sys::prelude::*;
use sentinel::{Null, SSlice};

const MAX_BUFFERS: u32 = 32;

macro_rules! glhandle {
    ($ptr:expr) => {
        GlHandle::from_ptr($ptr)
    };
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlHandle(u64);

impl GlHandle {
    unsafe fn from_raw(val: u64) -> Self {
        Self(val)
    }
    unsafe fn as_raw(&self) -> u64 {
        self.0
    }
    unsafe fn from_ptr<T>(ptr: *const T) -> Self {
        Self(ptr as u64)
    }
    unsafe fn as_ptr<T>(&self) -> *const T {
        self.0 as _
    }
}

impl Debug for GlHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("GlHandle")
            .field(unsafe { &self.as_ptr::<()>() })
            .finish()
    }
}

enum FenceSync {
    Gl { native: NativeIface, sync: GlHandle },
    Egl { dpy: GlHandle, sync: GlHandle },
    EglKhr { dpy: GlHandle, sync: GlHandle },
}

impl FenceSync {
    unsafe fn new(native: NativeIface, dpy: *const c_void) -> Option<Self> {
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

    unsafe fn wait(&self) {
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

impl Drop for FenceSync {
    fn drop(&mut self) {
        unsafe {
            match self {
                Self::Gl { native, sync } => {
                    gl(*native).DeleteSync(sync.as_ptr());
                }
                Self::Egl { dpy, sync } => {
                    egl().DestroySync(dpy.as_ptr(), sync.as_ptr());
                }
                Self::EglKhr { dpy, sync } => {
                    egl().DestroySyncKHR(dpy.as_ptr(), sync.as_ptr());
                }
            }
        }
    }
}

#[derive(Debug)]
enum TextureImage {
    EglImage(GlHandle),
    Pixmap {
        glx_pixmap: GlHandle,
        x_pixmap: GlHandle,
    },
}

#[derive(Debug)]
struct ExportTexture {
    native: NativeIface,
    dpy: GlHandle,
    texture: u32,
    planes: Vec<client::BufferPlaneInfo>,
    image: TextureImage,
}

impl Drop for ExportTexture {
    fn drop(&mut self) {
        unsafe {
            for plane in &self.planes {
                libc::close(plane.fd as _);
            }

            let dpy = self.dpy.as_ptr();

            match self.image {
                TextureImage::EglImage(image) => {
                    egl().DestroyImage(dpy, image.as_ptr());
                }
                TextureImage::Pixmap {
                    glx_pixmap,
                    x_pixmap,
                } => {
                    let glx = glx();
                    let x11 = X11_LIB.as_ref().unwrap();
                    glx.ReleaseTexImageEXT(
                        dpy as _,
                        glx_pixmap.as_raw() as _,
                        glx_sys::FRONT_LEFT_EXT as _,
                    );
                    glx.DestroyPixmap(dpy as _, glx_pixmap.as_ptr::<c_void>() as _);
                    (x11.XFreePixmap)(dpy as _, x_pixmap.as_ptr::<c_void>() as _);
                }
            }

            gl(self.native).DeleteTextures(1, &self.texture);
        }
    }
}

struct LayerDisplay {
    surface_valid_map: DashMap<GlHandle, bool>,
}

#[allow(unused)]
struct LayerSurface {
    native: NativeIface,
    context: GlHandle,
    width: u32,
    height: u32,
    stream: Option<client::Stream>,
    free_textures: Mutex<VecDeque<ExportTexture>>,
    mapped_textures: DashMap<u32, ExportTexture>,
    sync_objects: DashMap<u32, FenceSync>,
}

static GLOBAL_INIT: Lazy<()> = Lazy::new(|| init_logger());

static DLSYMS: Lazy<(DlsymFunc, Option<DlvsymFunc>)> = Lazy::new(|| {
    Lazy::force(&GLOBAL_INIT);
    let (dlsym, dlvsym) = load_dlsym_dlvsym();
    (dlsym.expect("this layer requires dlsym to work"), dlvsym)
});

#[inline]
unsafe fn real_dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void {
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

static GL_EGL: Lazy<Option<(Gl, Egl)>> = Lazy::new(|| unsafe {
    Lazy::force(&GLOBAL_INIT);
    let gpa = load_gpa(
        &[cstr!(b"libEGL.so.1\0"), cstr!(b"libEGL.so")],
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

static GL_GLX: Lazy<Option<(Gl, Glx)>> = Lazy::new(|| unsafe {
    Lazy::force(&GLOBAL_INIT);
    let glx_files = &[cstr!(b"libGLX.so.0\0"), cstr!(b"libGLX.so")];
    let gl_files = &[cstr!(b"libGL.so.1\0"), cstr!(b"libGL.so")];
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

static X11_LIB: Lazy<Option<X11Lib>> = Lazy::new(|| unsafe {
    X11Lib::load_with(|handle, symbol| real_dlsym(handle, symbol.as_ptr()))
});

#[derive(Clone, Copy, Debug)]
enum NativeIface {
    Glx,
    Egl,
}

static CLIENT: Lazy<Option<client::Client>> = Lazy::new(|| {
    Lazy::force(&GLOBAL_INIT);
    client::Client::new()
        .map_err(|e| error!(target:"client init", "failed to create client: {e:?}"))
        .ok()
});

static DISPLAY_MAP: Lazy<DashMap<GlHandle, LayerDisplay>> = Lazy::new(|| DashMap::new());
static SURFACE_MAP: Lazy<DashMap<GlHandle, LayerSurface>> = Lazy::new(|| DashMap::new());

#[inline]
fn glx() -> &'static Glx {
    &GL_GLX.as_ref().unwrap().1
}

#[inline]
fn egl() -> &'static Egl {
    &GL_EGL.as_ref().unwrap().1
}

#[inline]
fn gl(native: NativeIface) -> &'static Gl {
    match native {
        NativeIface::Egl => &GL_EGL.as_ref().unwrap().0,
        NativeIface::Glx => &GL_GLX.as_ref().unwrap().0,
    }
}

#[no_mangle]
#[named]
pub unsafe extern "C" fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void {
    let name = CStr::from_ptr(symbol);
    let real = real_dlsym(handle, symbol);
    let res = if let Some(res) = do_intercept(name) {
        res
    } else {
        real
    };
    trace!(
        "{:?}: address {:?}, symbol {}",
        handle,
        res,
        name.to_string_lossy(),
    );
    res
}
const _: DlsymFunc = dlsym;

#[no_mangle]
pub unsafe extern "C" fn dlvsym(
    handle: *mut c_void,
    symbol: *const c_char,
    version: *const c_char,
) -> *mut c_void {
    let name = CStr::from_ptr(symbol);
    let real_dlvsym = DLSYMS.1.unwrap();
    let real = real_dlvsym(handle, symbol, version);

    let res = if let Some(res) = do_intercept(name) {
        res
    } else {
        real
    };
    res
}
const _: DlvsymFunc = dlvsym;

#[named]
unsafe fn do_intercept(name: &CStr) -> Option<*mut c_void> {
    trace!("proc: {}", name.to_string_lossy());
    let pfn: *mut c_void = match name.to_bytes() {
        b"dlsym" => dlsym as _,
        b"dlvsym" => dlvsym as _,
        _ => do_intercept_egl(name).or_else(|| do_intercept_glx(name))?,
    };
    return Some(pfn);
}

#[named]
unsafe fn do_intercept_glx(name: &CStr) -> Option<*mut c_void> {
    if name.to_string_lossy().starts_with("glX") {
        trace!("proc: {}", name.to_string_lossy());
    }
    let pfn: *mut c_void = match name.to_bytes() {
        b"glXGetProcAddress" => glXGetProcAddress as _,
        b"glXGetProcAddressARB" => glXGetProcAddressARB as _,
        b"glXSwapBuffers" => glXSwapBuffers as _,
        b"glXSwapBuffersMscOML" => glXSwapBuffersMscOML as _,
        b"glXDestroyWindow" => glXDestroyWindow as _,
        b"glXDestroyContext" => glXDestroyContext as _,
        _ => return None,
    };
    debug!("address: {:?} proc: {}", pfn, name.to_string_lossy());
    return Some(pfn);
}

#[named]
unsafe fn do_intercept_egl(name: &CStr) -> Option<*mut c_void> {
    if name.to_string_lossy().starts_with("egl") {
        trace!("proc: {}", name.to_string_lossy());
    }
    let pfn: *mut c_void = match name.to_bytes() {
        b"eglGetProcAddress" => eglGetProcAddress as _,
        b"eglCreateWindowSurface" => eglCreateWindowSurface as _,
        b"eglSwapBuffers" => eglSwapBuffers as _,
        b"eglSwapBuffersWithDamageEXT" => eglSwapBuffersWithDamageEXT as _,
        b"eglSwapBuffersWithDamageKHR" => eglSwapBuffersWithDamageKHR as _,
        b"eglDestroySurface" => eglDestroySurface as _,
        b"eglDestroyContext" => eglDestroyContext as _,
        _ => return None,
    };
    debug!("address: {:?} proc: {}", pfn, name.to_string_lossy());
    return Some(pfn);
}

#[no_mangle]
pub unsafe extern "C" fn glXGetProcAddress(proc_name: *const c_char) -> *mut c_void {
    let glx = if let Some((_, glx)) = GL_GLX.as_ref() {
        glx
    } else {
        return ptr::null_mut();
    };
    let orig = glx.GetProcAddress(proc_name as _);
    if orig.is_null() {
        return ptr::null_mut();
    }
    if let Some(v) = do_intercept_glx(CStr::from_ptr(proc_name)) {
        return v;
    }
    orig as _
}

#[no_mangle]
pub unsafe extern "C" fn glXGetProcAddressARB(proc_name: *const c_char) -> *mut c_void {
    let glx = if let Some((_, glx)) = GL_GLX.as_ref() {
        glx
    } else {
        return ptr::null_mut();
    };
    let orig = glx.GetProcAddressARB(proc_name as _);
    if orig.is_null() {
        return ptr::null_mut();
    }
    if let Some(v) = do_intercept_glx(CStr::from_ptr(proc_name)) {
        return v;
    }
    orig as _
}

#[no_mangle]
pub unsafe extern "C" fn glXSwapBuffers(dpy: *mut glx_t::Display, drawable: glx_t::GLXDrawable) {
    let glx = glx();

    try_capture(NativeIface::Glx, dpy as _, drawable as _);

    let mut val: u32 = 2;
    glx.QueryDrawable(dpy, drawable, glx_sys::TEXTURE_FORMAT_EXT as _, &mut val);

    glx.SwapBuffers(dpy, drawable)
}

#[no_mangle]
pub unsafe extern "C" fn glXSwapBuffersMscOML(
    dpy: *mut glx_t::Display,
    drawable: glx_t::GLXDrawable,
    target_msc: i64,
    divisor: i64,
    remainder: i64,
) -> i64 {
    let glx = glx();

    try_capture(NativeIface::Glx, dpy as _, drawable as _);

    glx.SwapBuffersMscOML(dpy, drawable, target_msc, divisor, remainder)
}

#[no_mangle]
pub unsafe extern "C" fn glXDestroyWindow(dpy: *mut glx_t::Display, win: glx_t::GLXWindow) {
    let glx = glx();

    destroy_surface(dpy as _, win as _);

    glx.DestroyWindow(dpy, win)
}

#[no_mangle]
pub unsafe extern "C" fn glXDestroyContext(dpy: *mut glx_t::Display, ctx: glx_t::GLXContext) {
    let glx = glx();

    destroy_context(dpy as _, ctx);

    glx.DestroyContext(dpy, ctx)
}

#[no_mangle]
pub unsafe extern "C" fn eglGetProcAddress(proc_name: *const c_char) -> *mut c_void {
    let egl = if let Some((_, egl)) = GL_EGL.as_ref() {
        egl
    } else {
        return ptr::null_mut();
    };
    if let Some(v) = do_intercept_egl(CStr::from_ptr(proc_name)) {
        return v;
    }
    egl.GetProcAddress(proc_name) as _
}

#[no_mangle]
#[named]
pub unsafe extern "C" fn eglCreateWindowSurface(
    dpy: egl_t::EGLDisplay,
    config: egl_t::EGLConfig,
    win: egl_t::EGLNativeWindowType,
    attrib_list: *const i32,
) -> egl_t::EGLSurface {
    let egl = egl();

    debug!("win: {:?}", win);

    egl.CreateWindowSurface(dpy, config, win, attrib_list)
}

unsafe fn egl_swap_buffer(egl: &Egl, dpy: egl_t::EGLDisplay, surface: egl_t::EGLSurface) {
    let api = egl.QueryAPI();
    if api == egl_sys::OPENGL_API || api == egl_sys::OPENGL_ES_API {
        try_capture(NativeIface::Egl, dpy, surface);
    }
}

#[no_mangle]
pub unsafe extern "C" fn eglSwapBuffers(
    dpy: egl_t::EGLDisplay,
    surface: egl_t::EGLSurface,
) -> egl_t::EGLBoolean {
    let egl = egl();
    egl_swap_buffer(egl, dpy, surface);
    egl.SwapBuffers(dpy, surface)
}

#[no_mangle]
pub unsafe extern "C" fn eglSwapBuffersWithDamageEXT(
    dpy: egl_t::EGLDisplay,
    surface: egl_t::EGLSurface,
    rects: *mut egl_t::EGLint,
    n_rects: egl_t::EGLint,
) -> egl_t::EGLBoolean {
    let egl = egl();
    egl_swap_buffer(egl, dpy, surface);
    egl.SwapBuffersWithDamageEXT(dpy, surface, rects, n_rects)
}

#[no_mangle]
pub unsafe extern "C" fn eglSwapBuffersWithDamageKHR(
    dpy: egl_t::EGLDisplay,
    surface: egl_t::EGLSurface,
    rects: *mut egl_t::EGLint,
    n_rects: egl_t::EGLint,
) -> egl_t::EGLBoolean {
    let egl = egl();
    egl_swap_buffer(egl, dpy, surface);
    egl.SwapBuffersWithDamageKHR(dpy, surface, rects, n_rects)
}

#[no_mangle]
pub unsafe extern "C" fn eglDestroySurface(
    dpy: egl_t::EGLDisplay,
    surface: egl_t::EGLSurface,
) -> egl_t::EGLBoolean {
    let egl = egl();

    destroy_surface(dpy, surface);

    egl.DestroySurface(dpy, surface)
}

#[no_mangle]
pub unsafe extern "C" fn eglDestroyContext(
    dpy: egl_t::EGLDisplay,
    ctx: egl_t::EGLContext,
) -> egl_t::EGLBoolean {
    let egl = egl();

    destroy_context(dpy, ctx);

    egl.DestroyContext(dpy, ctx)
}

unsafe fn capture(
    native: NativeIface,
    dpy: *const c_void,
    ly_surface: &LayerSurface,
) -> Result<()> {
    let gl = gl(native);

    let stream = ly_surface
        .stream
        .as_ref()
        .ok_or(anyhow!("no stream"))?
        .proxy();

    let (buffer, user_handle) = if let Some(v) = stream.try_dequeue_buffer()?? {
        v
    } else {
        return Ok(());
    };
    let width = ly_surface.width;
    let height = ly_surface.height;

    let texture = match user_handle {
        client::BufferUserHandle::Texture(v) => v,
        _ => unreachable!(),
    };

    let mut prev_read_fbo: i32 = 0;
    let mut prev_draw_fbo: i32 = 0;
    let mut prev_texture: i32 = 0;
    let prev_srgb: u8 = gl.IsEnabled(gl_sys::FRAMEBUFFER_SRGB);
    gl.GetIntegerv(gl_sys::READ_FRAMEBUFFER_BINDING, &mut prev_read_fbo);
    gl.GetIntegerv(gl_sys::DRAW_FRAMEBUFFER_BINDING, &mut prev_draw_fbo);
    gl.GetIntegerv(gl_sys::TEXTURE_BINDING_2D, &mut prev_texture);
    {
        if prev_srgb != 0 {
            gl.Disable(gl_sys::FRAMEBUFFER_SRGB);
        }
        let mut fbo: u32 = 0;
        gl.GenFramebuffers(1, &mut fbo);

        gl.BindFramebuffer(gl_sys::READ_FRAMEBUFFER, 0);
        gl.BindFramebuffer(gl_sys::DRAW_FRAMEBUFFER, fbo);
        gl.BindTexture(gl_sys::TEXTURE_2D, texture);
        gl.FramebufferTexture2D(
            gl_sys::DRAW_FRAMEBUFFER,
            gl_sys::COLOR_ATTACHMENT0,
            gl_sys::TEXTURE_2D,
            texture,
            0,
        );

        if gl.ReadBuffer.is_loaded() {
            gl.ReadBuffer(gl_sys::BACK);
        } else {
            unimplemented!()
        }

        if gl.DrawBuffers.is_loaded() {
            let buffers = &[1];
            gl.DrawBuffers(1, buffers.as_ptr());
        } else if gl.DrawBuffer.is_loaded() {
            gl.DrawBuffer(gl_sys::COLOR_ATTACHMENT0);
        } else {
            unimplemented!()
        }

        if gl.BlitFramebuffer.is_loaded() {
            gl.BlitFramebuffer(
                0,
                0,
                width as _,
                height as _,
                0,
                height as _,
                width as _,
                0 as _,
                gl_sys::COLOR_BUFFER_BIT,
                gl_sys::LINEAR,
            );
        } else {
            unimplemented!()
        }

        if let Some(sync) = FenceSync::new(native, dpy) {
            ly_surface.sync_objects.insert(texture, sync);
        } else {
            gl.Finish();
        }

        gl.DeleteFramebuffers(1, &mut fbo);
    }
    gl.BindFramebuffer(gl_sys::READ_FRAMEBUFFER, prev_read_fbo as _);
    gl.BindFramebuffer(gl_sys::DRAW_FRAMEBUFFER, prev_draw_fbo as _);
    gl.BindTexture(gl_sys::TEXTURE_2D, prev_texture as _);
    if prev_srgb != 0 {
        gl.Enable(gl_sys::FRAMEBUFFER_SRGB);
    } else {
        gl.Disable(gl_sys::FRAMEBUFFER_SRGB);
    }

    stream.try_queue_buffer_process(buffer)??
}

unsafe fn query_surface_extent(
    native: NativeIface,
    dpy: *const c_void,
    surface: *const c_void,
) -> (u32, u32) {
    match native {
        NativeIface::Egl => {
            let egl = egl();
            let mut width: i32 = 0;
            let mut height: i32 = 0;
            egl.QuerySurface(dpy, surface, egl_sys::WIDTH as _, &mut width);
            egl.QuerySurface(dpy, surface, egl_sys::HEIGHT as _, &mut height);
            (width as _, height as _)
        }
        NativeIface::Glx => {
            let glx = glx();
            let mut width: u32 = 0;
            let mut height: u32 = 0;
            glx.QueryDrawable(dpy as _, surface as _, glx_sys::WIDTH as _, &mut width);
            glx.QueryDrawable(dpy as _, surface as _, glx_sys::HEIGHT as _, &mut height);
            (width, height)
        }
    }
}

#[named]
unsafe fn try_capture(native: NativeIface, dpy: *const c_void, surface: *const c_void) {
    let dpy_handle = glhandle!(dpy);
    let surface_handle = glhandle!(surface);
    if let Some(ly_display) = DISPLAY_MAP.get(&dpy_handle) {
        if let Some(valid) = ly_display.surface_valid_map.get(&surface_handle) {
            if !*valid {
                return;
            }
        } else {
            ly_display.surface_valid_map.insert(surface_handle, true);
        }
    } else {
        DISPLAY_MAP.insert(
            dpy_handle,
            LayerDisplay {
                surface_valid_map: DashMap::new(),
            },
        );
    };
    match try_init_surface(native, dpy, surface) {
        Ok(()) => (),
        Err(e) => {
            if let Some(ly_display) = DISPLAY_MAP.get(&dpy_handle) {
                ly_display.surface_valid_map.insert(surface_handle, false);
            }
            warn!("failed to init capture context: {e:?}");
            return;
        }
    }
    let handle = glhandle!(surface);
    let ly_surface = SURFACE_MAP.get(&handle).unwrap();
    if let Err(e) = capture(native, dpy, &ly_surface) {
        warn!("capture error: {e:?}");
    }
}

unsafe fn get_current_context(native: NativeIface) -> Option<GlHandle> {
    let ptr = match native {
        NativeIface::Egl => {
            let egl = egl();
            egl.GetCurrentContext()
        }
        NativeIface::Glx => {
            let glx = glx();
            glx.GetCurrentContext()
        }
    };
    if ptr.is_null() {
        return None;
    }
    Some(glhandle!(ptr))
}

#[named]
unsafe fn try_init_surface(
    native: NativeIface,
    dpy: *const c_void,
    surface: *const c_void,
) -> Result<()> {
    let handle = glhandle!(surface);
    let context = get_current_context(native).ok_or(anyhow!("no context"))?;
    let (width, height) = query_surface_extent(native, dpy, surface);
    let gl = gl(native);

    if !(gl.BlitFramebuffer.is_loaded()
        && gl.ReadBuffer.is_loaded()
        && (gl.DrawBuffer.is_loaded() || gl.DrawBuffers.is_loaded()))
    {
        return Err(anyhow!("missing required GL methods"));
    }

    if let Some(mut ly_surface) = SURFACE_MAP.get_mut(&handle) {
        if ly_surface.context != context {
            return Err(anyhow!("context switching not supported"));
        }
        if ly_surface.width == width && ly_surface.height == height {
            return Ok(());
        }
        let stream = ly_surface.stream.take();
        // IMPORTANT: drop the write lock first as try_terminate()
        // would call into callbacks that requires read lock to surface item
        drop(ly_surface);
        stream.map(|s| s.proxy().try_terminate());
    }
    SURFACE_MAP.remove(&handle);

    info!("{:?}: {}x{}", native, width, height);

    let (format, modifier, num_planes, textures) =
        create_target_textures(native, dpy, width, height, MAX_BUFFERS)?;

    let stream = create_stream(
        handle,
        format,
        modifier,
        num_planes as _,
        textures.len() as _,
        width as _,
        height as _,
    )
    .ok();

    let ly_surface = LayerSurface {
        native,
        context,
        width,
        height,
        stream,
        free_textures: Mutex::new(textures),
        mapped_textures: DashMap::new(),
        sync_objects: DashMap::new(),
    };
    SURFACE_MAP.insert(handle, ly_surface);

    Ok(())
}

#[named]
unsafe fn destroy_surface(dpy: *const c_void, surface: *const c_void) {
    debug!("{:?} {:?}", dpy, surface);
    let handle = glhandle!(surface);
    loop {
        if let Some(ly_surface) = SURFACE_MAP.get(&handle) {
            if let Some(context) = get_current_context(ly_surface.native) {
                if ly_surface.context == context {
                    break;
                }
                warn!("context changed: {:?} -> {:?}", ly_surface.context, context);
            }
        }
        return;
    }
    if let Some((_, _ly_surface)) = SURFACE_MAP.remove(&glhandle!(surface)) {
        // extra destroy work
    }
    if let Some(ly_display) = DISPLAY_MAP.get(&glhandle!(dpy)) {
        debug!("destroyed");
        ly_display.surface_valid_map.remove(&glhandle!(surface));
    }
}

#[named]
unsafe fn destroy_context(dpy: *const c_void, ctx: *const c_void) {
    debug!("destroying context {:?}", ctx);
    let ctx = glhandle!(ctx);
    let to_destroy = SURFACE_MAP
        .iter()
        .filter_map(|ly_surface| {
            if ly_surface.context == ctx {
                Some(*ly_surface.key())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    for surface in to_destroy {
        destroy_surface(dpy, surface.as_ptr())
    }
}

const DRM_FORMAT_R8: i32 = 0x20203852;
const DRM_FORMAT_ABGR8888: i32 = 0x34324241;
const DRM_FORMAT_XBGR8888: i32 = 0x34324258;
const DRM_FORMAT_ABGR2101010: i32 = 0x30334241;
const DRM_FORMAT_XBGR2101010: i32 = 0x30334258;

unsafe fn egl_export_dmabuf(
    dpy: *const c_void,
    width: u32,
    height: u32,
    texture: u32,
) -> Result<(client::Format, u64, TextureImage, Vec<BufferPlaneInfo>)> {
    let (gl, egl) = GL_EGL.as_ref().unwrap();
    if !(egl.ExportDMABUFImageQueryMESA.is_loaded() && egl.ExportDMABUFImageMESA.is_loaded()) {
        return Err(anyhow!("require EGL_MESA_image_dma_buf_export"));
    }

    gl.BindTexture(gl_sys::TEXTURE_2D, texture);
    // TODO: allows other compatible formats,
    // how to determine format of current render buffers?
    // glGetRenderbufferParameteriv of GL_RENDERBUFFER_INTERNAL_FORMAT
    // glxQueryDrawable of GLX_TEXTURE_FORMAT_EXT
    // eglQuerySurface of EGL_TEXTURE_FORMAT
    // all returns receiver variable unmodified
    //
    // Possible solution: hook to all functions setting formats then recoding
    gl.TexImage2D(
        gl_sys::TEXTURE_2D,
        0,
        gl_sys::RGBA as _,
        width as _,
        height as _,
        0,
        gl_sys::RGBA,
        gl_sys::UNSIGNED_BYTE,
        ptr::null(),
    );
    let image = if egl.CreateImage.is_loaded() {
        egl.CreateImageKHR(
            dpy,
            egl.GetCurrentContext(),
            egl_sys::GL_TEXTURE_2D,
            texture as _,
            ptr::null(),
        )
    } else if egl.CreateImageKHR.is_loaded() {
        egl.CreateImage(
            dpy,
            egl.GetCurrentContext(),
            egl_sys::GL_TEXTURE_2D,
            texture as _,
            ptr::null(),
        )
    } else {
        return Err(anyhow!("require EGL 1.5 or EGL_KHR_image_base"));
    };

    let err = loop {
        if image.is_null() {
            break anyhow!("failed to create EGLImage");
        }
        let mut fourcc: i32 = 0;
        let mut num_planes: i32 = 0;
        let mut modifier: u64 = 0;
        let res =
            egl.ExportDMABUFImageQueryMESA(dpy, image, &mut fourcc, &mut num_planes, &mut modifier);
        if res != 1 {
            break anyhow!("failed to query export dmabuf info");
        }
        let mut fds = vec![0i32; num_planes as usize];
        let mut strides = vec![0i32; num_planes as usize];
        let mut offsets = vec![0i32; num_planes as usize];
        let res = egl.ExportDMABUFImageMESA(
            dpy,
            image,
            fds.as_mut_ptr(),
            strides.as_mut_ptr(),
            offsets.as_mut_ptr(),
        );
        if res != 1 {
            break anyhow!("failed to export dmabuf");
        }
        let image = TextureImage::EglImage(glhandle!(image));

        let planes = (0..num_planes as usize)
            .into_iter()
            .map(|i| client::BufferPlaneInfo {
                fd: fds[i] as _,
                offset: offsets[i] as _,
                size: height * strides[i] as u32,
                stride: strides[i] as _,
            })
            .collect();
        let format = match fourcc {
            DRM_FORMAT_R8 => client::Format::GRAY8,
            DRM_FORMAT_ABGR8888 => client::Format::RGBA,
            DRM_FORMAT_XBGR8888 => client::Format::RGBx,
            DRM_FORMAT_ABGR2101010 => client::Format::ABGR_210LE,
            DRM_FORMAT_XBGR2101010 => client::Format::xBGR_210LE,
            _ => return Err(anyhow!("unhandled DRM format {:#x}", fourcc)),
        };
        return Ok((format, modifier, image, planes));
    };
    if !image.is_null() {
        egl.DestroyImage(dpy, image);
    }
    Err(err)
}

unsafe fn glx_export_dmabuf(
    dpy: *const c_void,
    width: u32,
    height: u32,
    texture: u32,
) -> Result<(client::Format, u64, TextureImage, Vec<BufferPlaneInfo>)> {
    let (gl, glx) = GL_GLX.as_ref().unwrap();
    let x11 = X11_LIB
        .as_ref()
        .ok_or(anyhow!("failed to load x11 libraries"))?;
    if !glx.BindTexImageEXT.is_loaded() {
        return Err(anyhow!("require GLX_EXT_texture_from_pixmap"));
    }
    let root = (x11.XDefaultRootWindow)(dpy as _);
    let screen = (x11.XDefaultScreen)(dpy as _);
    let xcb_conn = (x11.XGetXCBConnection)(dpy as _);
    if xcb_conn.is_null() {
        return Err(anyhow!("no xcb connection"));
    }

    let attrib_list = SSlice::<_, Null>::from_slice(&[
        glx_sys::BIND_TO_TEXTURE_RGBA_EXT,
        1,
        glx_sys::DRAWABLE_TYPE,
        glx_sys::PIXMAP_BIT,
        glx_sys::BIND_TO_TEXTURE_TARGETS_EXT,
        glx_sys::TEXTURE_2D_BIT_EXT,
        glx_sys::DONT_CARE,
        1,
        glx_sys::DOUBLEBUFFER,
        0,
        glx_sys::RED_SIZE,
        8,
        glx_sys::GREEN_SIZE,
        8,
        glx_sys::BLUE_SIZE,
        8,
        glx_sys::ALPHA_SIZE,
        8,
        0,
    ])
    .unwrap();
    let mut num = 0;
    let fb_configs = glx.ChooseFBConfig(dpy as _, screen, attrib_list.as_ptr() as _, &mut num);
    if num <= 0 || fb_configs.is_null() {
        return Err(anyhow!("no available framebuffer config"));
    }

    let attrib_list = SSlice::<_, Null>::from_slice(&[
        glx_sys::TEXTURE_TARGET_EXT,
        glx_sys::TEXTURE_2D_EXT,
        glx_sys::TEXTURE_FORMAT_EXT,
        glx_sys::TEXTURE_FORMAT_RGBA_EXT,
        glx_sys::MIPMAP_TEXTURE_EXT,
        0,
        0,
    ])
    .unwrap();

    let x_pixmap = (x11.XCreatePixmap)(dpy as _, root, width, height, 32);
    let glx_pixmap = glx.CreatePixmap(dpy as _, *fb_configs, x_pixmap, attrib_list.as_ptr() as _);
    (x11.XFree)(fb_configs as _);

    let err = loop {
        gl.BindTexture(gl_sys::TEXTURE_2D, texture);
        glx.BindTexImageEXT(
            dpy as _,
            glx_pixmap,
            glx_sys::FRONT_LEFT_EXT as _,
            ptr::null(),
        );

        let cookie = (x11.xcb_dri3_buffers_from_pixmap)(xcb_conn, x_pixmap as _);
        let reply = (x11.xcb_dri3_buffers_from_pixmap_reply)(xcb_conn, cookie, ptr::null_mut());
        if reply.is_null() {
            break anyhow!("failed to get DRI3 buffer from pixmap");
        }
        let reply = &mut *reply;

        let fds = (x11.xcb_dri3_buffers_from_pixmap_reply_fds)(xcb_conn, reply);
        let strides = (x11.xcb_dri3_buffers_from_pixmap_strides)(reply);
        let offsets = (x11.xcb_dri3_buffers_from_pixmap_offsets)(reply);
        let fds = slice::from_raw_parts(fds, reply.nfd as _);
        let strides = slice::from_raw_parts(strides, reply.nfd as _);
        let offsets = slice::from_raw_parts(offsets, reply.nfd as _);

        let image = TextureImage::Pixmap {
            glx_pixmap: GlHandle::from_raw(glx_pixmap as _),
            x_pixmap: GlHandle::from_raw(x_pixmap as _),
        };

        let planes = fds
            .iter()
            .enumerate()
            .map(|(i, &fd)| client::BufferPlaneInfo {
                fd: fd as _,
                offset: offsets[i],
                size: height * strides[i],
                stride: strides[i],
            })
            .collect();

        // XXX: why it's BGR instead of RGB?
        return Ok((client::Format::BGRA, reply.modifier, image, planes));
    };

    glx.DestroyPixmap(dpy as _, glx_pixmap);
    (x11.XFreePixmap)(dpy as _, x_pixmap);

    Err(err)
}

#[named]
unsafe fn create_target_textures(
    native: NativeIface,
    dpy: *const c_void,
    width: u32,
    height: u32,
    num: u32,
) -> Result<(client::Format, u64, usize, VecDeque<ExportTexture>)> {
    let gl = gl(native);
    let mut textures: Vec<u32> = vec![0; num as usize];

    let mut prev_texture: i32 = 0;
    gl.GetIntegerv(gl_sys::TEXTURE_BINDING_2D, &mut prev_texture);

    gl.GenTextures(num as _, textures.as_mut_ptr());

    let mut export_format: Option<(client::Format, u64, usize)> = None;
    let res = textures
        .iter()
        .map(|&texture| -> Result<_> {
            gl.BindTexture(gl_sys::TEXTURE_2D, texture);
            gl.TexParameteri(
                gl_sys::TEXTURE_2D,
                gl_sys::TEXTURE_MIN_FILTER,
                gl_sys::NEAREST as _,
            );
            gl.TexParameteri(
                gl_sys::TEXTURE_2D,
                gl_sys::TEXTURE_MAG_FILTER,
                gl_sys::NEAREST as _,
            );

            let (client_format, modifier, image, planes) = match native {
                NativeIface::Egl => egl_export_dmabuf(dpy, width, height, texture)?,
                NativeIface::Glx => glx_export_dmabuf(dpy, width, height, texture)?,
            };
            if let Some(format) = export_format {
                assert_eq!(client_format, format.0);
                assert_eq!(modifier, format.1);
                assert_eq!(planes.len(), format.2);
            } else {
                export_format = Some((client_format, modifier, planes.len()));
            }

            let res = ExportTexture {
                native,
                dpy: glhandle!(dpy),
                texture,
                planes,
                image,
            };
            debug!(
                "{:?} format:{:?} modifier:{:#x}",
                res, client_format, modifier
            );
            Ok(res)
        })
        .collect::<Result<VecDeque<_>>>();

    let res = res.map_err(|e| {
        gl.DeleteTextures(num as _, textures.as_mut_ptr());
        e
    })?;

    gl.BindTexture(gl_sys::TEXTURE_2D, prev_texture as _);

    let (client_format, modifier, num_planes) =
        export_format.ok_or(anyhow!("no image exported"))?;

    Ok((client_format, modifier, num_planes, res))
}

#[named]
fn on_add_buffer(surface: GlHandle) -> Result<client::BufferInfo> {
    debug!("mapping buffer");
    let ly_surface = SURFACE_MAP
        .get(&surface)
        .ok_or(anyhow!("surface removed"))?;

    let export_texture = ly_surface
        .free_textures
        .lock()
        .map_err(|e| anyhow!("{e:?}"))?
        .pop_front()
        .ok_or(anyhow!("no free texture available"))?;
    let texture = export_texture.texture;

    let res = client::BufferInfo {
        is_dma_buf: true,
        planes: export_texture.planes.clone(),
        user_handle: client::BufferUserHandle::Texture(texture),
    };

    ly_surface.mapped_textures.insert(texture, export_texture);

    Ok(res)
}

#[named]
fn on_remove_buffer(surface: GlHandle, user_handle: client::BufferUserHandle) -> Result<()> {
    debug!("unmapping buffer {:?}", user_handle);
    let ly_surface = SURFACE_MAP
        .get(&surface)
        .ok_or(anyhow!("surface removed"))?;
    debug!("acquired");

    let texture = match user_handle {
        client::BufferUserHandle::Texture(v) => v,
        _ => unreachable!(),
    };
    let (_, export_texture) = ly_surface
        .mapped_textures
        .remove(&texture)
        .ok_or(anyhow!("texture already unmapped"))?;

    ly_surface
        .free_textures
        .lock()
        .unwrap()
        .push_back(export_texture);

    Ok(())
}

#[named]
fn on_process_buffer(surface: GlHandle, user_handle: client::BufferUserHandle) -> Result<()> {
    trace!("process buffer {:?}", user_handle);
    let ly_surface = SURFACE_MAP
        .get(&surface)
        .ok_or(anyhow!("surface removed"))?;

    let texture = match user_handle {
        client::BufferUserHandle::Texture(v) => v,
        _ => unreachable!(),
    };

    let sync = if let Some((_, sync)) = ly_surface.sync_objects.remove(&texture) {
        sync
    } else {
        return Ok(());
    };

    unsafe { sync.wait() };

    trace!("processed");

    Ok(())
}

#[named]
fn create_stream(
    surface: GlHandle,
    format: client::Format,
    modifier: u64,
    num_planes: u32,
    max_buffers: u32,
    width: u32,
    height: u32,
) -> Result<client::Stream> {
    let stream_info = client::StreamInfo {
        width,
        height,
        enum_formats: vec![client::EnumFormatInfo {
            formats: vec![format],
            modifiers: vec![modifier],
        }],
        max_buffers,
        fixate_format: Box::new(move |enum_format| {
            info!("fixate format: {:?}", enum_format);
            let fixate_format = *enum_format.formats.first()?;
            let fixate_modifier = *enum_format.modifiers.first()?;
            if fixate_format != format || fixate_modifier != modifier {
                return None;
            }
            Some(client::FixateFormat {
                modifier: Some(modifier),
                num_planes,
            })
        }),
        add_buffer: Box::new(move || on_add_buffer(surface).ok()),
        remove_buffer: Box::new(move |user_handle| {
            let _ = on_remove_buffer(surface, user_handle);
        }),
        process_buffer: Box::new(move |user_handle| {
            let _ = on_process_buffer(surface, user_handle);
        }),
    };
    CLIENT
        .as_ref()
        .ok_or(anyhow!("failed to get client"))?
        .proxy()
        .try_create_stream(stream_info)??
}
