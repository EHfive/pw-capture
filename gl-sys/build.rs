use gl_generator::{Api, Fallbacks, Profile, Registry, StructGenerator};
use std::env;
use std::fs::File;
use std::path::PathBuf;

fn main() {
    let target = env::var("TARGET").unwrap();
    if !(target.contains("linux")
        || target.contains("dragonfly")
        || target.contains("freebsd")
        || target.contains("netbsd")
        || target.contains("openbsd"))
    {
        return;
    }

    let dest = PathBuf::from(&env::var("OUT_DIR").unwrap());

    let mut file = File::create(dest.join("gl_bindings.rs")).unwrap();
    Registry::new(
        Api::Gl,
        (3, 0),
        Profile::Core,
        Fallbacks::All,
        ["GL_ARB_sync"],
    )
    .write_bindings(StructGenerator, &mut file)
    .unwrap();

    let mut file = File::create(dest.join("egl_bindings.rs")).unwrap();
    Registry::new(
        Api::Egl,
        (1, 5),
        Profile::Core,
        Fallbacks::All,
        [
            // "EGL_ANDROID_native_fence_sync",
            // "EGL_EXT_buffer_age",
            // "EGL_EXT_create_context_robustness",
            // "EGL_EXT_device_base",
            // "EGL_EXT_device_drm",
            // "EGL_EXT_device_drm_render_node",
            // "EGL_EXT_device_enumeration",
            // "EGL_EXT_device_query",
            // "EGL_EXT_device_query_name",
            // "EGL_EXT_pixel_format_float",
            "EGL_EXT_platform_base",
            // "EGL_EXT_platform_device",
            "EGL_EXT_platform_wayland",
            "EGL_EXT_platform_x11",
            "EGL_EXT_swap_buffers_with_damage",
            // "EGL_KHR_create_context",
            // "EGL_KHR_create_context_no_error",
            "EGL_KHR_fence_sync",
            // "EGL_KHR_platform_android",
            // "EGL_KHR_platform_gbm",
            "EGL_KHR_platform_wayland",
            "EGL_KHR_platform_x11",
            "EGL_KHR_swap_buffers_with_damage",
            "EGL_KHR_image_base",
            // "EGL_MESA_platform_gbm",
            "EGL_MESA_image_dma_buf_export",
        ],
    )
    .write_bindings(StructGenerator, &mut file)
    .unwrap();

    let mut file = File::create(dest.join("glx_bindings.rs")).unwrap();
    Registry::new(
        Api::Glx,
        (1, 4),
        Profile::Core,
        Fallbacks::All,
        [
            // "GLX_ARB_context_flush_control",
            // "GLX_ARB_create_context",
            // "GLX_ARB_create_context_no_error",
            // "GLX_ARB_create_context_profile",
            // "GLX_ARB_create_context_robustness",
            // "GLX_ARB_fbconfig_float",
            "GLX_ARB_framebuffer_sRGB",
            // "GLX_ARB_multisample",
            "GLX_ARB_get_proc_address",
            // "GLX_EXT_buffer_age",
            // "GLX_EXT_create_context_es2_profile",
            "GLX_EXT_framebuffer_sRGB",
            "GLX_EXT_swap_control",
            "GLX_EXT_texture_from_pixmap",
            // "GLX_MESA_swap_control",
            // "GLX_SGI_swap_control",
            "GLX_OML_sync_control",
        ],
    )
    .write_bindings(StructGenerator, &mut file)
    .unwrap();
}
