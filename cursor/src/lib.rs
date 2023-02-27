mod utils;
mod wayland;
mod xcb;

pub use wayland::*;
pub use xcb::*;

use core::ptr;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use anyhow::Result;
#[cfg(feature = "pw-capture-client")]
use pw_capture_client as client;

pub trait CursorManager: Send + Sync {
    fn snapshot_cursor(&self, serial: u64) -> Result<Box<dyn CursorSnapshot>>;
}

pub trait CursorSnapshot {
    fn serial(&self) -> u64;
    fn entered(&self) -> bool;
    /// returns (x, y) relative to window coordinate
    fn position(&self) -> (i32, i32);
    /// returns (x, y) relative to bitmap coordinate
    fn hotspot(&self) -> (i32, i32);
    /// returns (width, height, bytes_per_pixel, pixels_data)
    fn bitmap(&self) -> Option<(u32, u32, u32, &[u8])>;
    #[cfg(feature = "pw-capture-client")]
    fn format(&self) -> client::Format;
    #[cfg(feature = "pw-capture-client")]
    fn as_cursor_info(&self, serial: bool) -> Option<client::BufferCursorInfo> {
        if !self.entered() {
            return None;
        }
        let bitmap = self
            .bitmap()
            .map(|(width, height, _bpp, pixels)| client::BufferBitmap {
                width,
                height,
                format: self.format(),
                pixels,
            });

        let cursor = client::BufferCursorInfo {
            serial,
            position: {
                let (x, y) = self.position();
                client::Point { x, y }
            },
            hotspot: {
                let (x, y) = self.hotspot();
                client::Point { x, y }
            },
            bitmap,
        };
        Some(cursor)
    }
}

struct OwnedMem<T> {
    mem: ptr::NonNull<T>,
    _phantom: PhantomData<T>,
}

impl<T> OwnedMem<T> {
    unsafe fn new(ptr: *mut T) -> Option<Self> {
        ptr::NonNull::new(ptr).map(|mem| OwnedMem {
            mem,
            _phantom: Default::default(),
        })
    }
}

impl<T> Deref for OwnedMem<T> {
    type Target = ptr::NonNull<T>;

    fn deref(&self) -> &Self::Target {
        &self.mem
    }
}
impl<T> DerefMut for OwnedMem<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.mem
    }
}

impl<T> Drop for OwnedMem<T> {
    fn drop(&mut self) {
        unsafe { libc::free(self.mem.as_ptr() as _) }
    }
}
