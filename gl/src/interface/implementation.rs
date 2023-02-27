use super::*;

use crate::elfhack::*;
use crate::utils::*;

use core::ffi::CStr;
use core::ptr;
use core::slice;
use core::sync::atomic::{self, AtomicU64};
use std::collections::VecDeque;
use std::result::Result::Ok;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use client::BufferPlaneInfo;
use dashmap::DashMap;
use function_name::named;
use libc::{c_char, c_void};
use pw_capture_client as client;
use pw_capture_cursor as local_cursor;
use pw_capture_cursor::CursorManager;
use pw_capture_gl_sys::prelude::*;
use sentinel::{Null, SSlice};

const MAX_BUFFERS: u32 = 32;

#[named]
#[inline(never)]
pub unsafe extern "C" fn impl_dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void {
    let name = CStr::from_ptr(symbol);
    let res = if let Some(res) = do_intercept(name) {
        res
    } else {
        real_dlsym(handle, symbol)
    };
    trace!(
        "{:?}: address {:?}, symbol {}",
        handle,
        res,
        name.to_string_lossy(),
    );
    res
}
const _: DlsymFunc = impl_dlsym;

#[inline(never)]
pub unsafe extern "C" fn impl_dlvsym(
    handle: *mut c_void,
    symbol: *const c_char,
    version: *const c_char,
) -> *mut c_void {
    let name = CStr::from_ptr(symbol);

    let res = if let Some(res) = do_intercept(name) {
        res
    } else {
        let real_dlvsym = DLSYMS.1.unwrap();
        real_dlvsym(handle, symbol, version)
    };
    res
}
const _: DlvsymFunc = impl_dlvsym;

#[named]
unsafe fn do_intercept(name: &CStr) -> Option<*mut c_void> {
    trace!("proc: {}", name.to_string_lossy());
    let pfn: *mut c_void = match name.to_bytes() {
        b"dlsym" => impl_dlsym as _,
        b"dlvsym" => impl_dlvsym as _,
        b"wl_proxy_marshal_array_flags" => impl_wl_proxy_marshal_array_flags as _,
        #[cfg(feature = "nightly")]
        b"wl_proxy_marshal_flags" => impl_wl_proxy_marshal_flags as _,
        b"wl_proxy_create" => impl_wl_proxy_create as _,
        b"wl_proxy_add_listener" => impl_wl_proxy_add_listener as _,
        b"wl_proxy_add_dispatcher" => impl_wl_proxy_add_dispatcher as _,
        b"wl_proxy_get_listener" => impl_wl_proxy_get_listener as _,
        b"wl_proxy_destroy" => impl_wl_proxy_destroy as _,
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
        b"glXGetProcAddress" => impl_glXGetProcAddress as _,
        b"glXGetProcAddressARB" => impl_glXGetProcAddressARB as _,
        b"glXSwapBuffers" => impl_glXSwapBuffers as _,
        b"glXSwapBuffersMscOML" => impl_glXSwapBuffersMscOML as _,
        b"glXDestroyWindow" => impl_glXDestroyWindow as _,
        b"glXDestroyContext" => impl_glXDestroyContext as _,
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
        b"eglGetProcAddress" => impl_eglGetProcAddress as _,
        b"eglGetDisplay" => impl_eglGetDisplay as _,
        b"eglGetPlatformDisplay" => impl_eglGetPlatformDisplay as _,
        b"eglGetPlatformDisplayEXT" => impl_eglGetPlatformDisplayEXT as _,
        b"eglCreateWindowSurface" => impl_eglCreateWindowSurface as _,
        b"eglCreatePlatformWindowSurface" => impl_eglCreatePlatformWindowSurface as _,
        b"eglCreatePlatformWindowSurfaceEXT" => impl_eglCreatePlatformWindowSurfaceEXT as _,
        b"eglSwapBuffers" => impl_eglSwapBuffers as _,
        b"eglSwapBuffersWithDamageEXT" => impl_eglSwapBuffersWithDamageEXT as _,
        b"eglSwapBuffersWithDamageKHR" => impl_eglSwapBuffersWithDamageKHR as _,
        b"eglDestroySurface" => impl_eglDestroySurface as _,
        b"eglDestroyContext" => impl_eglDestroyContext as _,
        b"eglTerminate" => impl_eglTerminate as _,
        _ => return None,
    };
    debug!("address: {:?} proc: {}", pfn, name.to_string_lossy());
    return Some(pfn);
}

#[allow(non_snake_case)]
#[inline(never)]
pub unsafe extern "C" fn impl_glXGetProcAddress(proc_name: *const c_char) -> *mut c_void {
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

#[allow(non_snake_case)]
#[inline(never)]
pub unsafe extern "C" fn impl_glXGetProcAddressARB(proc_name: *const c_char) -> *mut c_void {
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

#[allow(non_snake_case)]
#[inline(never)]
pub unsafe extern "C" fn impl_glXSwapBuffers(
    dpy: *mut glx_t::Display,
    drawable: glx_t::GLXDrawable,
) {
    let glx = glx();

    try_capture(NativeIface::Glx, dpy as _, drawable as _);

    let mut val: u32 = 2;
    glx.QueryDrawable(dpy, drawable, glx_sys::TEXTURE_FORMAT_EXT as _, &mut val);

    glx.SwapBuffers(dpy, drawable)
}

#[allow(non_snake_case)]
#[inline(never)]
pub unsafe extern "C" fn impl_glXSwapBuffersMscOML(
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

#[allow(non_snake_case)]
#[inline(never)]
pub unsafe extern "C" fn impl_glXDestroyWindow(dpy: *mut glx_t::Display, win: glx_t::GLXWindow) {
    let glx = glx();

    destroy_surface(dpy as _, win as _);

    glx.DestroyWindow(dpy, win)
}

#[allow(non_snake_case)]
#[inline(never)]
pub unsafe extern "C" fn impl_glXDestroyContext(dpy: *mut glx_t::Display, ctx: glx_t::GLXContext) {
    let glx = glx();

    destroy_context(dpy as _, ctx);

    glx.DestroyContext(dpy, ctx)
}

#[allow(non_snake_case)]
#[inline(never)]
pub unsafe extern "C" fn impl_eglGetProcAddress(proc_name: *const c_char) -> *mut c_void {
    let egl = if let Some((_, egl)) = GL_EGL.as_ref() {
        egl
    } else {
        return ptr::null_mut();
    };
    let orig = egl.GetProcAddress(proc_name as _);
    if orig.is_null() {
        return ptr::null_mut();
    }
    if let Some(v) = do_intercept_egl(CStr::from_ptr(proc_name)) {
        return v;
    }
    orig as _
}

unsafe fn add_egl_display(
    dpy: egl_t::EGLDisplay,
    platform: Option<EglPlatform>,
    native_display: *const c_void,
) {
    if dpy == egl_sys::NO_DISPLAY {
        return;
    }
    let ly_display = LayerDisplay {
        egl_display: Some(EglDisplay {
            platform_display: glhandle!(native_display),
            platform,
        }),
    };
    DISPLAY_MAP.insert(glhandle!(dpy), ly_display);
}

#[allow(non_snake_case)]
#[named]
#[inline(never)]
pub unsafe extern "C" fn impl_eglGetDisplay(
    native_display: egl_t::EGLNativeDisplayType,
) -> egl_t::EGLDisplay {
    let egl = egl();

    let egl_plat = egl_get_native_platform(native_display as _);

    let dpy = egl.GetDisplay(native_display);
    debug!(
        "native_display:{:?} gl_display:{:?} platform:{:?}",
        native_display, dpy, egl_plat
    );
    add_egl_display(dpy, egl_plat, native_display);
    dpy
}

#[allow(non_snake_case)]
#[inline(never)]
pub unsafe extern "C" fn impl_eglGetPlatformDisplay(
    platform: egl_t::EGLenum,
    native_display: *mut c_void,
    attrib_list: *const isize,
) -> egl_t::EGLDisplay {
    let egl = egl();

    let egl_plat = egl_platform_from_ext(platform);

    let dpy = egl.GetPlatformDisplay(platform, native_display, attrib_list);
    add_egl_display(dpy, Some(egl_plat), native_display);
    dpy
}

#[allow(non_snake_case)]
#[inline(never)]
pub unsafe extern "C" fn impl_eglGetPlatformDisplayEXT(
    platform: egl_t::EGLenum,
    native_display: *mut c_void,
    attrib_list: *const i32,
) -> egl_t::EGLDisplay {
    let egl = egl();

    let egl_plat = egl_platform_from_ext(platform);

    let dpy = egl.GetPlatformDisplayEXT(platform, native_display, attrib_list);
    add_egl_display(dpy, Some(egl_plat), native_display);
    dpy
}

#[allow(non_snake_case)]
#[inline(never)]
pub unsafe extern "C" fn impl_eglCreateWindowSurface(
    dpy: egl_t::EGLDisplay,
    config: egl_t::EGLConfig,
    win: egl_t::EGLNativeWindowType,
    attrib_list: *const i32,
) -> egl_t::EGLSurface {
    let egl = egl();

    let win = *(win as *const *const c_void);

    let surface = egl.CreateWindowSurface(dpy, config, win, attrib_list);
    let _ = try_init_surface(NativeIface::Egl, dpy, surface, Some(win as _));
    surface
}

#[allow(non_snake_case)]
#[inline(never)]
pub unsafe extern "C" fn impl_eglCreatePlatformWindowSurface(
    dpy: egl_t::EGLDisplay,
    config: egl_t::EGLConfig,
    native_window: *mut c_void,
    attrib_list: *const isize,
) -> egl_t::EGLSurface {
    let egl = egl();

    let surface = egl.CreatePlatformWindowSurface(dpy, config, native_window, attrib_list);
    let _ = try_init_surface(NativeIface::Egl, dpy, surface, Some(native_window as _));
    surface
}

#[allow(non_snake_case)]
#[inline(never)]
pub unsafe extern "C" fn impl_eglCreatePlatformWindowSurfaceEXT(
    dpy: egl_t::EGLDisplay,
    config: egl_t::EGLConfig,
    native_window: *mut c_void,
    attrib_list: *const i32,
) -> egl_t::EGLSurface {
    let egl = egl();

    let surface = egl.CreatePlatformWindowSurfaceEXT(dpy, config, native_window, attrib_list);
    let _ = try_init_surface(NativeIface::Egl, dpy, surface, Some(native_window as _));
    surface
}

unsafe fn egl_swap_buffer(egl: &Egl, dpy: egl_t::EGLDisplay, surface: egl_t::EGLSurface) {
    let api = egl.QueryAPI();
    if api == egl_sys::OPENGL_API || api == egl_sys::OPENGL_ES_API {
        try_capture(NativeIface::Egl, dpy, surface);
    }
}

#[allow(non_snake_case)]
#[inline(never)]
pub unsafe extern "C" fn impl_eglSwapBuffers(
    dpy: egl_t::EGLDisplay,
    surface: egl_t::EGLSurface,
) -> egl_t::EGLBoolean {
    let egl = egl();
    egl_swap_buffer(egl, dpy, surface);
    egl.SwapBuffers(dpy, surface)
}

#[allow(non_snake_case)]
#[inline(never)]
pub unsafe extern "C" fn impl_eglSwapBuffersWithDamageEXT(
    dpy: egl_t::EGLDisplay,
    surface: egl_t::EGLSurface,
    rects: *mut egl_t::EGLint,
    n_rects: egl_t::EGLint,
) -> egl_t::EGLBoolean {
    let egl = egl();
    egl_swap_buffer(egl, dpy, surface);
    egl.SwapBuffersWithDamageEXT(dpy, surface, rects, n_rects)
}

#[allow(non_snake_case)]
#[inline(never)]
pub unsafe extern "C" fn impl_eglSwapBuffersWithDamageKHR(
    dpy: egl_t::EGLDisplay,
    surface: egl_t::EGLSurface,
    rects: *mut egl_t::EGLint,
    n_rects: egl_t::EGLint,
) -> egl_t::EGLBoolean {
    let egl = egl();
    egl_swap_buffer(egl, dpy, surface);
    egl.SwapBuffersWithDamageKHR(dpy, surface, rects, n_rects)
}

#[allow(non_snake_case)]
#[inline(never)]
pub unsafe extern "C" fn impl_eglDestroySurface(
    dpy: egl_t::EGLDisplay,
    surface: egl_t::EGLSurface,
) -> egl_t::EGLBoolean {
    let egl = egl();

    destroy_surface(dpy, surface);

    egl.DestroySurface(dpy, surface)
}

#[allow(non_snake_case)]
#[inline(never)]
pub unsafe extern "C" fn impl_eglDestroyContext(
    dpy: egl_t::EGLDisplay,
    ctx: egl_t::EGLContext,
) -> egl_t::EGLBoolean {
    let egl = egl();

    destroy_context(dpy, ctx);

    egl.DestroyContext(dpy, ctx)
}

#[allow(non_snake_case)]
#[inline(never)]
pub unsafe extern "C" fn impl_eglTerminate(dpy: egl_t::EGLDisplay) -> egl_t::EGLBoolean {
    let egl = egl();

    DISPLAY_MAP.remove(&glhandle!(dpy));

    egl.Terminate(dpy)
}

unsafe fn capture(
    native: NativeIface,
    dpy: *const c_void,
    ly_capture: &LayerCapture,
) -> Result<()> {
    let gl = gl(native);

    let stream = ly_capture.stream.proxy();

    let (buffer, user_handle) = if let Some(v) = stream.try_dequeue_buffer()?? {
        v
    } else {
        return Ok(());
    };
    let width = ly_capture.width;
    let height = ly_capture.height;

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
            let buffers = &[gl_sys::COLOR_ATTACHMENT0];
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
                gl_sys::NEAREST,
            );
        } else {
            unimplemented!()
        }

        if let Some(sync) = FenceSync::new(native, dpy) {
            ly_capture.sync_objects.insert(texture, sync);
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
    let surface_handle = glhandle!(surface);
    if let Some(ly_display) = SURFACE_MAP.get(&surface_handle) {
        if !ly_display.capture_valid {
            return;
        }
    } else {
        try_init_surface(native, dpy, surface, None);
    };

    match try_init_capture(native, dpy, surface) {
        Ok(()) => (),
        Err(e) => {
            if let Some(mut ly_surface) = SURFACE_MAP.get_mut(&surface_handle) {
                ly_surface.capture_valid = false;
                let capture = ly_surface.capture.take();
                drop(ly_surface);
                drop(capture);
            }
            warn!("failed to init capture context: {e:?}");
            return;
        }
    }
    if let Some(ly_surface) = SURFACE_MAP.get(&surface_handle) {
        if let Err(e) = capture(native, dpy, ly_surface.capture.as_ref().unwrap()) {
            warn!("capture error: {e:?}");
        }
    } else {
        error!("surface data not exist")
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
unsafe fn create_xcb_cursor_manager(
    _dpy: Option<*const c_void>,
    _dpy_is_xcb: bool,
    window: u32,
) -> Option<Box<dyn CursorManager + Send + Sync>> {
    // create a new connection as we will use the connection in another thread,
    // the drawback is it only connects to the default display or `DISPLAY`
    // so it might not connect to actually server of window
    match local_cursor::XcbWindow::new_connection(window) {
        Ok(m) => Some(Box::new(m)),
        Err(e) => {
            warn!("failed to create xcb cursor manager: {e:?}");
            None
        }
    }
}

#[named]
unsafe fn try_init_surface(
    native: NativeIface,
    dpy: *const c_void,
    surface: *const c_void,
    platform_surface: Option<*const c_void>,
) {
    let dpy_handle = glhandle!(dpy);
    let surface_handle = glhandle!(surface);
    if let Some(_ly_surface) = SURFACE_MAP.get(&surface_handle) {
        return;
    }

    debug!(
        "native:{:?}, display:{:?}, surface:{:?}, platform surface:{:?}",
        native, dpy, surface, platform_surface
    );

    let platform_surface = match native {
        NativeIface::Glx => Some(glhandle!(surface)),
        _ => platform_surface.map(|v| glhandle!(v)),
    };

    let cursor_manager: Option<Box<dyn CursorManager + Send + Sync>> = loop {
        let platform_surface = if let Some(v) = platform_surface {
            v
        } else {
            break None;
        };
        match native {
            NativeIface::Egl => {
                let ly_display = if let Some(v) = DISPLAY_MAP.get(&dpy_handle) {
                    v
                } else {
                    break None;
                };
                let EglDisplay {
                    platform_display,
                    platform,
                } = if let Some(v) = &ly_display.egl_display {
                    v
                } else {
                    break None;
                };

                if let Some(platform) = platform {
                    match *platform {
                        EglPlatform::X11 => {
                            break create_xcb_cursor_manager(
                                Some(platform_display.as_ptr()),
                                false,
                                platform_surface.as_raw() as _,
                            );
                        }
                        EglPlatform::Xcb => {
                            break create_xcb_cursor_manager(
                                Some(platform_display.as_ptr()),
                                true,
                                platform_surface.as_raw() as _,
                            );
                        }
                        EglPlatform::Wayland => {
                            let wl_surface = wl_egl_window_get_wl_surface(
                                platform_surface.as_ptr::<c_void>() as _,
                            );
                            debug!(
                                "wl_egl_window:{:?} wl_surface:{:?}",
                                platform_surface, wl_surface
                            );
                            if wl_surface.is_null() {
                                break None;
                            }
                            let m = WL_INTERCEPT.as_ref().and_then(|intercept| {
                                intercept.get_cursor_manager(
                                    platform_display.as_ptr::<wl_display>() as _,
                                    wl_surface as _,
                                )
                            });
                            if let Some(m) = m {
                                break Some(Box::new(m));
                            }
                        }
                        _ => (),
                    }
                } else {
                    // fallback to X11/XCB platform,
                    // returns None if window does not exists in default connection
                    break create_xcb_cursor_manager(None, false, platform_surface.as_raw() as _);
                }
            }
            NativeIface::Glx => {
                break create_xcb_cursor_manager(Some(dpy), false, platform_surface.as_raw() as _)
            }
        }
        break None;
    };

    let ly_surface = LayerSurface {
        native,
        platform_surface,
        display: glhandle!(dpy),
        surface: surface_handle,
        cursor_manager,
        capture_valid: true,
        capture: None,
    };
    SURFACE_MAP.insert(surface_handle, ly_surface);
}

#[named]
unsafe fn try_init_capture(
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
        if let Some(ly_capture) = ly_surface.capture.as_ref() {
            if ly_capture.context != context {
                return Err(anyhow!("context switching not supported"));
            }
            if ly_capture.width == width && ly_capture.height == height {
                return Ok(());
            }
        }
        if let Some(ly_capture) = ly_surface.capture.take() {
            // IMPORTANT: drop the write lock first as try_terminate()
            // would call into callbacks that requires read lock to surface item
            drop(ly_surface);
            let _ = ly_capture.stream.proxy().try_terminate();
        }
    } else {
        return Err(anyhow!("surface not exist"));
    }

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
    )?;

    let ly_capture = LayerCapture {
        context,
        cursor_serial: AtomicU64::new(0),
        width,
        height,
        stream,
        free_textures: Mutex::new(textures),
        mapped_textures: DashMap::new(),
        sync_objects: DashMap::new(),
    };

    if let Some(mut ly_surface) = SURFACE_MAP.get_mut(&handle) {
        ly_surface.capture_valid = true;
        ly_surface.capture = Some(ly_capture);
    } else {
        return Err(anyhow!("surface not exist"));
    }

    Ok(())
}

#[named]
unsafe fn destroy_surface(dpy: *const c_void, surface: *const c_void) {
    debug!("{:?} {:?}", dpy, surface);
    let handle = glhandle!(surface);
    loop {
        if let Some(ly_surface) = SURFACE_MAP.get(&handle) {
            if ly_surface.capture.is_none() {
                break;
            }
            if let Some(context) = get_current_context(ly_surface.native) {
                if let Some(ly_capture) = &ly_surface.capture {
                    if ly_capture.context == context {
                        break;
                    }
                    warn!("context changed: {:?} -> {:?}", ly_capture.context, context);
                }
            }
        }
        return;
    }
    if let Some((_, _ly_surface)) = SURFACE_MAP.remove(&glhandle!(surface)) {
        // extra destroy work
    }
}

#[named]
unsafe fn destroy_context(dpy: *const c_void, ctx: *const c_void) {
    debug!("destroying context {:?}", ctx);
    let ctx = glhandle!(ctx);
    let to_destroy = SURFACE_MAP
        .iter()
        .filter_map(|ly_surface| {
            if let Some(ly_capture) = &ly_surface.capture {
                if ly_capture.context == ctx {
                    return Some(*ly_surface.key());
                }
            }
            None
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
        let modifier = reply.modifier;

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

        libc::free(reply as *mut _ as _);
        drop(reply);

        // XXX: why it's BGR instead of RGB?
        return Ok((client::Format::BGRA, modifier, image, planes));
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
    let ly_capture = ly_surface
        .capture
        .as_ref()
        .ok_or(anyhow!("no capture data"))?;

    let export_texture = ly_capture
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

    ly_capture.mapped_textures.insert(texture, export_texture);

    Ok(res)
}

#[named]
fn on_remove_buffer(surface: GlHandle, user_handle: client::BufferUserHandle) -> Result<()> {
    debug!("unmapping buffer {:?}", user_handle);
    let ly_surface = SURFACE_MAP
        .get(&surface)
        .ok_or(anyhow!("surface removed"))?;
    let ly_capture = ly_surface
        .capture
        .as_ref()
        .ok_or(anyhow!("no capture data"))?;

    let texture = match user_handle {
        client::BufferUserHandle::Texture(v) => v,
        _ => unreachable!(),
    };
    let (_, export_texture) = ly_capture
        .mapped_textures
        .remove(&texture)
        .ok_or(anyhow!("texture already unmapped"))?;

    ly_capture
        .free_textures
        .lock()
        .unwrap()
        .push_back(export_texture);

    Ok(())
}

#[named]
fn on_process_buffer(
    surface: GlHandle,
    user_handle: client::BufferUserHandle,
    add_meta_cbs: client::AddBufferMetaCbs,
) -> Result<()> {
    let ly_surface = SURFACE_MAP
        .get(&surface)
        .ok_or(anyhow!("surface removed"))?;
    let ly_capture = ly_surface
        .capture
        .as_ref()
        .ok_or(anyhow!("no capture data"))?;

    let texture = match user_handle {
        client::BufferUserHandle::Texture(v) => v,
        _ => unreachable!(),
    };

    if let Some(add_cursor) = add_meta_cbs.add_cursor {
        let old_serial = ly_capture.cursor_serial.load(atomic::Ordering::Acquire);
        if let Some(cursor_manager) = ly_surface.cursor_manager.as_ref() {
            if let Ok(snap) = cursor_manager.snapshot_cursor(old_serial) {
                let _ = ly_capture.cursor_serial.compare_exchange(
                    old_serial,
                    snap.serial(),
                    atomic::Ordering::AcqRel,
                    atomic::Ordering::Acquire,
                );

                snap.as_cursor_info(old_serial != snap.serial())
                    .map(|info| add_cursor(info));
            }
        }
    }

    if let Some((_, sync)) = ly_capture.sync_objects.remove(&texture) {
        drop(ly_surface);
        unsafe { sync.wait() };
    };

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
        process_buffer: Box::new(move |user_handle, add_meta_cbs| {
            let _ = on_process_buffer(surface, user_handle, add_meta_cbs);
        }),
    };
    CLIENT
        .as_ref()
        .ok_or(anyhow!("failed to get client"))?
        .proxy()
        .try_create_stream(stream_info)??
}
