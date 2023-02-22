use crate::utils::*;

use super::*;

use core::fmt::Debug;
use core::sync::atomic::AtomicU64;
use std::collections::VecDeque;
use std::sync::Mutex;

use dashmap::DashMap;
use pw_capture_client as client;
use pw_capture_cursor::CursorManager;
use pw_capture_gl_sys::prelude::*;

#[macro_export]
macro_rules! glhandle {
    ($ptr:expr) => {
        GlHandle::from_ptr($ptr)
    };
}
pub use glhandle;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlHandle(u64);

impl GlHandle {
    pub unsafe fn from_raw(val: u64) -> Self {
        Self(val)
    }
    pub unsafe fn as_raw(&self) -> u64 {
        self.0
    }
    pub unsafe fn from_ptr<T>(ptr: *const T) -> Self {
        Self(ptr as u64)
    }
    pub unsafe fn as_ptr<T>(&self) -> *const T {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeIface {
    Glx,
    Egl,
}

pub enum FenceSync {
    Gl { native: NativeIface, sync: GlHandle },
    Egl { dpy: GlHandle, sync: GlHandle },
    EglKhr { dpy: GlHandle, sync: GlHandle },
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
pub enum TextureImage {
    EglImage(GlHandle),
    Pixmap {
        glx_pixmap: GlHandle,
        x_pixmap: GlHandle,
    },
}

#[derive(Debug)]
pub struct ExportTexture {
    pub native: NativeIface,
    pub dpy: GlHandle,
    pub texture: u32,
    pub planes: Vec<client::BufferPlaneInfo>,
    pub image: TextureImage,
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

pub struct EglDisplay {
    pub platform_display: GlHandle,
    pub platform: Option<EglPlatform>,
}

pub struct LayerDisplay {
    pub egl_display: Option<EglDisplay>,
}

#[allow(unused)]
pub struct LayerSurface {
    pub native: NativeIface,
    pub platform_surface: Option<GlHandle>,
    pub display: GlHandle,
    pub surface: GlHandle,
    pub cursor_manager: Option<Box<dyn CursorManager + Sync + Send>>,
    pub capture_valid: bool,
    pub capture: Option<LayerCapture>,
}

pub struct LayerCapture {
    pub context: GlHandle,
    pub width: u32,
    pub height: u32,
    pub cursor_serial: AtomicU64,
    pub stream: client::Stream,
    pub free_textures: Mutex<VecDeque<ExportTexture>>,
    pub mapped_textures: DashMap<u32, ExportTexture>,
    pub sync_objects: DashMap<u32, FenceSync>,
}
