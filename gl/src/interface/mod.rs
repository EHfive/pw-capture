mod implementation;
mod state;
mod types;
mod wl_impl;

use implementation::*;
use state::*;
use types::*;
use wl_impl::*;

use crate::elfhack::*;

use core::ffi::c_int;

use libc::{c_char, c_void};
use pw_capture_cursor::wl_sys::*;
use pw_capture_gl_sys::prelude::*;

#[no_mangle]
pub unsafe extern "C" fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void {
    impl_dlsym(handle, symbol)
}
const _: DlsymFunc = dlsym;

#[no_mangle]
pub unsafe extern "C" fn dlvsym(
    handle: *mut c_void,
    symbol: *const c_char,
    version: *const c_char,
) -> *mut c_void {
    impl_dlvsym(handle, symbol, version)
}
const _: DlvsymFunc = dlvsym;

/// Provides actual implementation for underlying Vulkan Layer
pub use wl_impl::me_eh5_pw_capture_get_wl_cursor_manager;
pub use wl_impl::me_eh5_pw_capture_release_wl_cursor_manager;
pub use wl_impl::me_eh5_pw_capture_wl_cursor_snapshot;

#[no_mangle]
pub unsafe extern "C" fn wl_proxy_marshal_array_flags(
    proxy: *mut wl_proxy,
    opcode: u32,
    interface: *const wl_interface,
    version: u32,
    flags: u32,
    args: *mut wl_argument,
) -> *mut wl_proxy {
    impl_wl_proxy_marshal_array_flags(proxy, opcode, interface, version, flags, args)
}

#[cfg(feature = "nightly")]
pub use wl_impl::wl_proxy_marshal_flags;

#[no_mangle]
pub unsafe extern "C" fn wl_proxy_create(
    factory: *mut wl_proxy,
    interface: *const wl_interface,
) -> *mut wl_proxy {
    impl_wl_proxy_create(factory, interface)
}

#[no_mangle]
pub unsafe extern "C" fn wl_proxy_add_listener(
    proxy: *mut wl_proxy,
    implementation: *mut PFN_void,
    data: *mut c_void,
) -> c_int {
    impl_wl_proxy_add_listener(proxy, implementation, data)
}

#[no_mangle]
pub unsafe extern "C" fn wl_proxy_add_dispatcher(
    proxy: *mut wl_proxy,
    dispatcher: Option<PFN_wl_dispatcher>,
    implementation: *mut c_void,
    data: *mut c_void,
) -> c_int {
    impl_wl_proxy_add_dispatcher(proxy, dispatcher, implementation, data)
}

#[no_mangle]
pub unsafe extern "C" fn wl_proxy_get_listener(proxy: *mut wl_proxy) -> *mut c_void {
    impl_wl_proxy_get_listener(proxy)
}

#[no_mangle]
pub unsafe extern "C" fn wl_proxy_destroy(proxy: *mut wl_proxy) {
    impl_wl_proxy_destroy(proxy)
}

#[no_mangle]
pub unsafe extern "C" fn glXGetProcAddress(proc_name: *const c_char) -> *mut c_void {
    impl_glXGetProcAddress(proc_name)
}

#[no_mangle]
pub unsafe extern "C" fn glXGetProcAddressARB(proc_name: *const c_char) -> *mut c_void {
    impl_glXGetProcAddressARB(proc_name)
}

#[no_mangle]
pub unsafe extern "C" fn glXSwapBuffers(dpy: *mut glx_t::Display, drawable: glx_t::GLXDrawable) {
    impl_glXSwapBuffers(dpy, drawable)
}

#[no_mangle]
pub unsafe extern "C" fn glXSwapBuffersMscOML(
    dpy: *mut glx_t::Display,
    drawable: glx_t::GLXDrawable,
    target_msc: i64,
    divisor: i64,
    remainder: i64,
) -> i64 {
    impl_glXSwapBuffersMscOML(dpy, drawable, target_msc, divisor, remainder)
}

#[no_mangle]
pub unsafe extern "C" fn glXDestroyWindow(dpy: *mut glx_t::Display, win: glx_t::GLXWindow) {
    impl_glXDestroyWindow(dpy, win)
}

#[no_mangle]
pub unsafe extern "C" fn glXDestroyContext(dpy: *mut glx_t::Display, ctx: glx_t::GLXContext) {
    impl_glXDestroyContext(dpy, ctx)
}

#[no_mangle]
pub unsafe extern "C" fn eglGetProcAddress(proc_name: *const c_char) -> *mut c_void {
    impl_eglGetProcAddress(proc_name)
}

#[no_mangle]
pub unsafe extern "C" fn eglGetDisplay(
    native_display: egl_t::EGLNativeDisplayType,
) -> egl_t::EGLDisplay {
    impl_eglGetDisplay(native_display)
}

#[no_mangle]
pub unsafe extern "C" fn eglGetPlatformDisplay(
    platform: egl_t::EGLenum,
    native_display: *mut c_void,
    attrib_list: *const isize,
) -> egl_t::EGLDisplay {
    impl_eglGetPlatformDisplay(platform, native_display, attrib_list)
}

#[no_mangle]
pub unsafe extern "C" fn eglGetPlatformDisplayEXT(
    platform: egl_t::EGLenum,
    native_display: *mut c_void,
    attrib_list: *const i32,
) -> egl_t::EGLDisplay {
    impl_eglGetPlatformDisplayEXT(platform, native_display, attrib_list)
}

#[no_mangle]
pub unsafe extern "C" fn eglCreateWindowSurface(
    dpy: egl_t::EGLDisplay,
    config: egl_t::EGLConfig,
    win: egl_t::EGLNativeWindowType,
    attrib_list: *const i32,
) -> egl_t::EGLSurface {
    impl_eglCreateWindowSurface(dpy, config, win, attrib_list)
}

#[no_mangle]
pub unsafe extern "C" fn eglCreatePlatformWindowSurface(
    dpy: egl_t::EGLDisplay,
    config: egl_t::EGLConfig,
    native_window: *mut c_void,
    attrib_list: *const isize,
) -> egl_t::EGLSurface {
    impl_eglCreatePlatformWindowSurface(dpy, config, native_window, attrib_list)
}

#[no_mangle]
pub unsafe extern "C" fn eglCreatePlatformWindowSurfaceEXT(
    dpy: egl_t::EGLDisplay,
    config: egl_t::EGLConfig,
    native_window: *mut c_void,
    attrib_list: *const i32,
) -> egl_t::EGLSurface {
    impl_eglCreatePlatformWindowSurfaceEXT(dpy, config, native_window, attrib_list)
}

#[no_mangle]
pub unsafe extern "C" fn eglSwapBuffers(
    dpy: egl_t::EGLDisplay,
    surface: egl_t::EGLSurface,
) -> egl_t::EGLBoolean {
    impl_eglSwapBuffers(dpy, surface)
}

#[no_mangle]
pub unsafe extern "C" fn eglSwapBuffersWithDamageEXT(
    dpy: egl_t::EGLDisplay,
    surface: egl_t::EGLSurface,
    rects: *mut egl_t::EGLint,
    n_rects: egl_t::EGLint,
) -> egl_t::EGLBoolean {
    impl_eglSwapBuffersWithDamageEXT(dpy, surface, rects, n_rects)
}

#[no_mangle]
pub unsafe extern "C" fn eglSwapBuffersWithDamageKHR(
    dpy: egl_t::EGLDisplay,
    surface: egl_t::EGLSurface,
    rects: *mut egl_t::EGLint,
    n_rects: egl_t::EGLint,
) -> egl_t::EGLBoolean {
    impl_eglSwapBuffersWithDamageKHR(dpy, surface, rects, n_rects)
}

#[no_mangle]
pub unsafe extern "C" fn eglDestroySurface(
    dpy: egl_t::EGLDisplay,
    surface: egl_t::EGLSurface,
) -> egl_t::EGLBoolean {
    impl_eglDestroySurface(dpy, surface)
}

#[no_mangle]
pub unsafe extern "C" fn eglDestroyContext(
    dpy: egl_t::EGLDisplay,
    ctx: egl_t::EGLContext,
) -> egl_t::EGLBoolean {
    impl_eglDestroyContext(dpy, ctx)
}

#[no_mangle]
pub unsafe extern "C" fn eglTerminate(dpy: egl_t::EGLDisplay) -> egl_t::EGLBoolean {
    impl_eglTerminate(dpy)
}
