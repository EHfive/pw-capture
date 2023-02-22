use crate::{CursorManager, CursorSnapshot, OwnedMem};

use core::ffi::c_void;
use core::ptr;
use core::slice;

use anyhow::{anyhow, Result};
use xcb_dl::ffi as xcb_t;
use xcb_dl::Xcb;
use xcb_dl::XcbXfixes;
use xcb_t::xcb_connection_t;

pub struct XcbWindow {
    conn: usize,
    to_close_conn: bool,
    window: u32,
    xcb: Xcb,
    xfixes: XcbXfixes,
}

pub struct XcbCursor {
    geometry: OwnedMem<xcb_t::xcb_get_geometry_reply_t>,
    translate_coordinates: OwnedMem<xcb_t::xcb_translate_coordinates_reply_t>,
    cursor_image: OwnedMem<xcb_t::xcb_xfixes_get_cursor_image_reply_t>,
    pixels: Option<ptr::NonNull<u8>>,
    focused: bool,
    serial: u64,
}

impl XcbWindow {
    unsafe fn new_internal(conn: *mut xcb_connection_t, window: u32) -> Result<Self> {
        let xcb = Xcb::load_loose()?;
        let xfixes = XcbXfixes::load_loose()?;

        let (conn, to_close_conn) = if conn.is_null() {
            let conn = xcb.xcb_connect(ptr::null(), ptr::null_mut());
            (conn, true)
        } else {
            (conn, false)
        };

        let cookie = xfixes.xcb_xfixes_query_version_unchecked(conn, 6, 0);
        let reply = xfixes.xcb_xfixes_query_version_reply(conn, cookie, ptr::null_mut());

        let _reply = OwnedMem::new(reply).ok_or(anyhow!("query xfixes version failed"))?;

        let geometry_cookie = xcb.xcb_get_geometry_unchecked(conn, window);
        let reply = xcb.xcb_get_geometry_reply(conn as _, geometry_cookie, ptr::null_mut());
        let _geometry = OwnedMem::new(reply).ok_or(anyhow!("xcb_get_geometry failed"))?;

        Ok(Self {
            conn: conn as _,
            to_close_conn,
            window,
            xcb,
            xfixes,
        })
    }

    pub unsafe fn new(conn: ptr::NonNull<c_void>, window: u32) -> Result<Self> {
        Self::new_internal(conn.as_ptr() as _, window)
    }

    pub unsafe fn new_connection(window: u32) -> Result<Self> {
        Self::new_internal(ptr::null_mut(), window)
    }
}

impl Drop for XcbWindow {
    fn drop(&mut self) {
        if self.to_close_conn {
            unsafe { self.xcb.xcb_disconnect(self.conn as _) }
        }
    }
}

impl CursorManager for XcbWindow {
    fn snapshot_cursor(&self, serial: u64) -> Result<Box<dyn CursorSnapshot>> {
        let prev_focused = (serial >> 32) != 0;
        let serial = (serial & u32::MAX as u64) as u32;
        unsafe {
            let geometry_cookie = self
                .xcb
                .xcb_get_geometry_unchecked(self.conn as _, self.window);
            let reply =
                self.xcb
                    .xcb_get_geometry_reply(self.conn as _, geometry_cookie, ptr::null_mut());
            let geometry = OwnedMem::new(reply).ok_or(anyhow!("xcb_get_geometry failed"))?;

            let focus_cookie = self.xcb.xcb_get_input_focus_unchecked(self.conn as _);

            let root = if geometry.as_ref().root != 0 {
                geometry.as_ref().root
            } else {
                self.window
            };
            let translate_cookie = self.xcb.xcb_translate_coordinates_unchecked(
                self.conn as _,
                self.window,
                root,
                0,
                0,
            );
            let cursor_cookie = self
                .xfixes
                .xcb_xfixes_get_cursor_image_unchecked(self.conn as _);

            let reply =
                self.xcb
                    .xcb_get_input_focus_reply(self.conn as _, focus_cookie, ptr::null_mut());
            let input_focus = OwnedMem::new(reply).ok_or(anyhow!("xcb_get_input_focus failed"))?;
            let focused = input_focus.as_ref().focus == self.window;

            let reply = self.xcb.xcb_translate_coordinates_reply(
                self.conn as _,
                translate_cookie,
                ptr::null_mut(),
            );
            let translate_coordinates =
                OwnedMem::new(reply).ok_or(anyhow!("xcb_translate_coordinates failed"))?;

            let reply = self.xfixes.xcb_xfixes_get_cursor_image_reply(
                self.conn as _,
                cursor_cookie,
                ptr::null_mut(),
            );
            let cursor_image =
                OwnedMem::new(reply).ok_or(anyhow!("xcb_xfixes_get_cursor_image failed"))?;

            let image = self
                .xfixes
                .xcb_xfixes_get_cursor_image_cursor_image(cursor_image.as_ptr());
            let pixels = ptr::NonNull::new(image as *mut u8);

            let curr_serial = cursor_image.as_ref().cursor_serial;
            let pixels = if !prev_focused && focused || serial != curr_serial || serial == 0 {
                pixels
            } else {
                None
            };
            let curr_serial = ((focused as u64) << 32) | (curr_serial as u64);

            Ok(Box::new(XcbCursor {
                geometry,
                translate_coordinates,
                cursor_image,
                pixels,
                focused,
                serial: curr_serial,
            }))
        }
    }
}

impl CursorSnapshot for XcbCursor {
    fn serial(&self) -> u64 {
        self.serial
    }
    fn entered(&self) -> bool {
        if !self.focused {
            return false;
        }
        unsafe {
            let (x, y) = self.position();
            let cursor_image = self.cursor_image.as_ref();
            let geometry = self.geometry.as_ref();
            let width = cursor_image.width as i32;
            let height = cursor_image.height as i32;
            let xhot = cursor_image.xhot as i32;
            let yhot = cursor_image.yhot as i32;
            let x_range = (0 - width + xhot)..(geometry.width as i32 + xhot);
            let y_range = (0 - height + yhot)..(geometry.height as i32 + yhot);
            x_range.contains(&x) && y_range.contains(&y)
        }
    }

    fn position(&self) -> (i32, i32) {
        unsafe {
            let cursor_image = self.cursor_image.as_ref();
            let translate = self.translate_coordinates.as_ref();
            (
                (cursor_image.x - translate.dst_x) as _,
                (cursor_image.y - translate.dst_y) as _,
            )
        }
    }

    fn hotspot(&self) -> (i32, i32) {
        unsafe {
            let cursor_image = self.cursor_image.as_ref();
            (cursor_image.xhot as _, cursor_image.yhot as _)
        }
    }

    fn bitmap(&self) -> Option<(u32, u32, u32, &[u8])> {
        let res = unsafe {
            let cursor_image = self.cursor_image.as_ref();
            let bitmap = slice::from_raw_parts(
                self.pixels?.as_ptr(),
                cursor_image.width as usize * cursor_image.height as usize * 4,
            );
            (cursor_image.width as _, cursor_image.height as _, 4, bitmap)
        };
        Some(res)
    }

    #[cfg(feature = "pw-capture-client")]
    fn format(&self) -> pw_capture_client::Format {
        pw_capture_client::Format::BGRA
    }
}
