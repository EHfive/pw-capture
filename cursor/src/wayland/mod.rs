mod sys;
mod wl_lib;
pub mod wl_sys {
    pub use super::sys::*;
}

use sys::*;
use wl_lib::*;

use crate::utils::*;
use crate::{CursorManager, CursorSnapshot};

use core::ffi::{c_int, c_void, CStr};
use core::ptr;
use core::slice;
use std::alloc::{alloc, dealloc, Layout};
use std::os::fd::RawFd;
use std::sync::RwLock;

use anyhow::{anyhow, Result};
use dashmap::DashMap;
use log::{debug, trace, warn};

pub use wl_lib::WlHandle;

struct RegistryState {
    #[allow(unused)]
    display: WlHandle,
}

struct GlobalState {
    registry: WlHandle,
    #[allow(unused)]
    name: String,
}

struct SurfaceBuffer {
    buffer: WlHandle,
    scale: i32,
}

struct SurfacePointer {
    #[allow(unused)]
    pointer: WlHandle,
    serial: u32,
    cursor_surface: Option<WlHandle>,
    hotspot_x: i32,
    hotspot_y: i32,
    motion_time: u32,
    surface_x: wl_fixed_t,
    surface_y: wl_fixed_t,
}

struct SurfaceState {
    g_compositor: WlHandle,
    active_buffer: RwLock<Option<SurfaceBuffer>>,
    pending_buffer: RwLock<Option<SurfaceBuffer>>,
    entered_pointer: RwLock<Option<SurfacePointer>>,
}

struct ShmPoolState {
    #[allow(unused)]
    g_shm: WlHandle,
    fd: RawFd,
    size: i32,
}

struct ShmBufferState {
    shm_pool: WlHandle,
    offset: i32,
    width: i32,
    height: i32,
    stride: i32,
    format: u32,
}

struct PointerState {
    #[allow(unused)]
    g_seat: WlHandle,
    current_surface: Option<WlHandle>,
}

#[repr(C)]
pub struct WlIntercept {
    wl: WlLib,
    registry_map: DashMap<WlHandle, RegistryState>,
    global_map: DashMap<WlHandle, GlobalState>,
    surface_map: DashMap<WlHandle, SurfaceState>,
    shm_pool_map: DashMap<WlHandle, ShmPoolState>,
    shm_buffer_map: DashMap<WlHandle, ShmBufferState>,
    pointer_map: DashMap<WlHandle, PointerState>,
}

pub struct WlCursorManager {
    intercept: &'static WlIntercept,
    surface: WlHandle,
}

struct BitmapInfo {
    width: u32,
    height: u32,
    bpp: u32,
    format: u32,
    data: Vec<u8>,
}

pub struct WlCursorSnapshot {
    serial: u64,
    entered: bool,
    position: (i32, i32),
    hotspot: (i32, i32),
    bitmap: Option<BitmapInfo>,
}

impl WlIntercept {
    pub unsafe fn new_load_with<F>(dlsym: F) -> Result<Self>
    where
        F: FnMut(*mut c_void, &CStr) -> *const c_void,
    {
        let wl = WlLib::load_with(dlsym).ok_or(anyhow!("failed to load libwayland"))?;

        Ok(Self {
            wl,
            registry_map: DashMap::new(),
            global_map: DashMap::new(),
            surface_map: DashMap::new(),
            shm_pool_map: DashMap::new(),
            shm_buffer_map: DashMap::new(),
            pointer_map: DashMap::new(),
        })
    }
}

impl CursorManager for WlCursorManager {
    fn snapshot_cursor(&self, serial: u64) -> Result<Box<dyn CursorSnapshot>> {
        let snap = self
            .intercept
            .snapshot_cursor(serial, self.surface)
            .ok_or(anyhow!("failed to snapshot cursor"))?;
        Ok(Box::new(snap))
    }
}

impl CursorSnapshot for WlCursorSnapshot {
    fn serial(&self) -> u64 {
        self.serial
    }
    fn entered(&self) -> bool {
        self.entered
    }
    fn position(&self) -> (i32, i32) {
        self.position
    }
    fn hotspot(&self) -> (i32, i32) {
        self.hotspot
    }
    fn bitmap(&self) -> Option<(u32, u32, u32, &[u8])> {
        let info = self.bitmap.as_ref()?;
        Some((info.width, info.height, info.bpp, &info.data))
    }
    #[cfg(feature = "pw-capture-client")]
    fn format(&self) -> pw_capture_client::Format {
        use pw_capture_client::Format::*;
        let format = if let Some(info) = &self.bitmap {
            info.format
        } else {
            return UNKNOWN;
        };
        match format {
            0 => BGRA,
            1 => BGRx,
            0x34324152 => RGBA,
            0x34325852 => RGBx,
            _ => UNKNOWN,
        }
    }
}

impl WlIntercept {
    pub unsafe fn get_cursor_manager(
        &'static self,
        display: *mut wl_display,
        surface: *mut wl_proxy,
    ) -> Option<WlCursorManager> {
        if !pointer_is_dereferencable(display as _) {
            return None;
        }
        let interface = *(display as *mut *const wl_interface);
        if self.wl.wl_display_interface.as_ptr() as *const _ != interface {
            warn!("Application not using shared libwayland-client library, cannot intercept");
            return None;
        }

        let handle = wlhandle!(surface);
        let surface = self.surface_map.get(&handle)?;
        let compositor = self.global_map.get(&surface.g_compositor)?;
        let _registry = self.registry_map.get(&compositor.registry)?;

        Some(WlCursorManager {
            intercept: self,
            surface: handle,
        })
    }

    unsafe fn copy_surface_buffer(&self, buffer: &ShmBufferState) -> Option<BitmapInfo> {
        let pool = self.shm_pool_map.get(&buffer.shm_pool)?;

        let bpp: u32 = match buffer.format {
            0 | 1 | 0x34324152 | 0x34325852 => 4,
            _ => {
                warn!("cursor format {:#x} not handled", buffer.format);
                return None;
            }
        };

        if buffer.stride as u32 != bpp * buffer.width as u32 {
            return None;
        }

        let buffer_size = buffer.width as usize * buffer.height as usize * bpp as usize;
        let map_size = buffer.offset as usize + buffer_size;
        if map_size > pool.size as usize {
            return None;
        }

        let mem = libc::mmap(
            ptr::null_mut(),
            map_size,
            libc::PROT_READ,
            libc::MAP_SHARED,
            pool.fd,
            0,
        );
        if mem.is_null() {
            return None;
        }

        let data = (mem as *mut u8).offset(buffer.offset as _);
        let data = slice::from_raw_parts(data, buffer_size).to_vec();
        libc::munmap(mem, map_size);

        Some(BitmapInfo {
            width: buffer.width as _,
            height: buffer.height as _,
            bpp,
            format: buffer.format,
            data,
        })
    }

    pub fn snapshot_cursor(&self, serial: u64, surface: WlHandle) -> Option<WlCursorSnapshot> {
        let surface = self.surface_map.get(&surface)?;
        let pointer = surface.entered_pointer.read().unwrap();
        let pointer = pointer.as_ref()?;
        let surface = self.surface_map.get(&pointer.cursor_surface?)?;
        let surface_buffer = surface.active_buffer.read().unwrap();
        let surface_buffer = surface_buffer.as_ref()?;
        let buffer = self.shm_buffer_map.get(&surface_buffer.buffer)?;

        let bitmap = if serial != pointer.serial as u64 || serial == 0 {
            unsafe { self.copy_surface_buffer(&buffer) }
        } else {
            None
        };

        let scale = surface_buffer.scale;
        let x: i32 = pointer.surface_x.checked_mul_int(scale)?.round().to_num();
        let y: i32 = pointer.surface_y.checked_mul_int(scale)?.round().to_num();
        Some(WlCursorSnapshot {
            serial: pointer.serial as _,
            entered: true,
            position: (x, y),
            hotspot: (pointer.hotspot_x, pointer.hotspot_y),
            bitmap,
        })
    }
}

impl WlIntercept {
    fn m_display_get_registry(&self, display: WlHandle, registry: WlHandle) {
        self.registry_map
            .insert(registry, RegistryState { display });
    }

    fn m_registry_bind(&self, registry: WlHandle, name: String, global: WlHandle) {
        self.global_map
            .insert(global, GlobalState { name, registry });
    }

    fn m_compositor_create_surface(&self, g_compositor: WlHandle, surface: WlHandle) {
        debug!("created surface: {:?}", surface);
        self.surface_map.insert(
            surface,
            SurfaceState {
                g_compositor,
                active_buffer: RwLock::new(None),
                pending_buffer: RwLock::new(None),
                entered_pointer: RwLock::new(None),
            },
        );
    }

    fn m_shm_create_pool(&self, g_shm: WlHandle, fd: RawFd, size: i32, shm_pool: WlHandle) {
        self.shm_pool_map
            .insert(shm_pool, ShmPoolState { g_shm, fd, size });
    }

    fn m_shm_pool_create_buffer(
        &self,
        shm_pool: WlHandle,
        offset: i32,
        width: i32,
        height: i32,
        stride: i32,
        format: u32,
        shm_buffer: WlHandle,
    ) {
        self.shm_buffer_map.insert(
            shm_buffer,
            ShmBufferState {
                shm_pool,
                offset,
                width,
                height,
                stride,
                format,
            },
        );
    }

    fn m_shm_pool_resize(&self, shm_pool: WlHandle, size: i32) -> Option<()> {
        let mut shm_pool = self.shm_pool_map.get_mut(&shm_pool)?;
        shm_pool.size = size;
        Some(())
    }

    fn m_surface_attach(&self, surface: WlHandle, buffer: WlHandle) -> Option<()> {
        let surface = self.surface_map.get(&surface)?;
        let mut pending_buffer = surface.pending_buffer.write().unwrap();
        if let Some(state) = pending_buffer.as_mut() {
            state.buffer = buffer;
        } else {
            *pending_buffer = Some(SurfaceBuffer {
                buffer: unsafe { WlHandle::from_raw(0) },
                scale: 1,
            })
        }
        Some(())
    }

    fn m_surface_set_buffer_scale(&self, surface: WlHandle, scale: i32) -> Option<()> {
        let surface = self.surface_map.get(&surface)?;
        let mut pending_buffer = surface.pending_buffer.write().unwrap();
        if let Some(state) = pending_buffer.as_mut() {
            state.scale = scale;
        } else {
            *pending_buffer = Some(SurfaceBuffer {
                buffer: unsafe { WlHandle::from_raw(0) },
                scale,
            })
        }
        Some(())
    }

    fn m_surface_commit(&self, surface: WlHandle) -> Option<()> {
        let surface = self.surface_map.get(&surface)?;
        let mut pending_buffer = surface.pending_buffer.write().unwrap();
        let mut active_buffer = surface.active_buffer.write().unwrap();
        *active_buffer = pending_buffer.take();
        Some(())
    }

    fn m_seat_get_pointer(&self, g_seat: WlHandle, pointer: WlHandle) {
        self.pointer_map.insert(
            pointer,
            PointerState {
                g_seat,
                current_surface: None,
            },
        );
    }

    fn m_pointer_set_cursor(
        &self,
        pointer: WlHandle,
        serial: u32,
        cursor_surface: WlHandle,
        hotspot_x: i32,
        hotspot_y: i32,
    ) -> Option<()> {
        let pointer = self.pointer_map.get_mut(&pointer)?;
        let current_surface = self.surface_map.get(pointer.current_surface.as_ref()?)?;
        let mut pointer = current_surface.entered_pointer.write().unwrap();
        let pointer = pointer.as_mut()?;
        #[cfg(debug_assertions)]
        if pointer.serial != serial {
            debug!(
                "set_cursor serial {} does not match latest serial {}",
                serial, pointer.serial
            );
        }
        pointer.cursor_surface = Some(cursor_surface);
        pointer.hotspot_x = hotspot_x;
        pointer.hotspot_y = hotspot_y;
        Some(())
    }

    fn m_pointer_release(&self, pointer: WlHandle) {
        self.pointer_map.remove(&pointer);
    }

    fn e_pointer_enter(
        &self,
        pointer: WlHandle,
        serial: u32,
        surface: WlHandle,
        surface_x: wl_fixed_t,
        surface_y: wl_fixed_t,
    ) -> Option<()> {
        let surface_state = self.surface_map.get(&surface)?;
        let mut entered_pointer = surface_state.entered_pointer.write().unwrap();
        *entered_pointer = Some(SurfacePointer {
            pointer,
            serial,
            cursor_surface: None,
            hotspot_x: 0,
            hotspot_y: 0,
            motion_time: 0,
            surface_x,
            surface_y,
        });

        let mut pointer_state = self.pointer_map.get_mut(&pointer)?;
        pointer_state.current_surface = Some(surface);
        Some(())
    }

    fn e_pointer_leave(&self, pointer: WlHandle, _serial: u32, surface: WlHandle) -> Option<()> {
        let surface = self.surface_map.get(&surface)?;
        let mut entered_pointer = surface.entered_pointer.write().unwrap();
        *entered_pointer = None;

        let mut pointer = self.pointer_map.get_mut(&pointer)?;
        pointer.current_surface = None;
        Some(())
    }

    fn e_pointer_motion(
        &self,
        pointer: WlHandle,
        time: u32,
        surface_x: wl_fixed_t,
        surface_y: wl_fixed_t,
    ) -> Option<()> {
        let pointer = self.pointer_map.get_mut(&pointer)?;
        let current_surface = self.surface_map.get(pointer.current_surface.as_ref()?)?;
        let mut pointer = current_surface.entered_pointer.write().unwrap();
        let pointer = pointer.as_mut()?;
        pointer.motion_time = time;
        pointer.surface_x = surface_x;
        pointer.surface_y = surface_y;
        Some(())
    }

    unsafe fn collect_method(
        &self,
        proxy: *mut wl_proxy,
        interface: &wl_interface,
        _opcode: u32,
        method: &wl_message,
        args: &[wl_argument],
    ) {
        let interface_name = CStr::from_ptr(interface.name).to_string_lossy();
        let method_name = CStr::from_ptr(method.name).to_string_lossy();

        let proxy = wlhandle!(proxy);

        match (interface_name.as_ref(), method_name.as_ref()) {
            ("wl_display", "get_registry") => {
                let new_proxy = args[0].o;
                self.m_display_get_registry(proxy, wlhandle!(new_proxy as _));
            }
            ("wl_registry", "bind") => {
                let name = CStr::from_ptr(args[1].s);
                let new_proxy = args[3].o;
                self.m_registry_bind(
                    proxy,
                    name.to_string_lossy().to_string(),
                    wlhandle!(new_proxy as _),
                );
            }
            ("wl_compositor", "create_surface") => {
                let new_proxy = args[0].o;
                self.m_compositor_create_surface(proxy, wlhandle!(new_proxy as _));
            }
            ("wl_shm", "create_pool") => {
                let new_proxy = args[0].o;
                let fd = args[1].h;
                let size = args[2].i;
                self.m_shm_create_pool(proxy, fd, size, wlhandle!(new_proxy as _));
            }
            ("wl_shm_pool", "create_buffer") => {
                let new_proxy = args[0].o;
                let offset = args[1].i;
                let width = args[2].i;
                let height = args[3].i;
                let stride = args[4].i;
                let format = args[5].u;
                self.m_shm_pool_create_buffer(
                    proxy,
                    offset,
                    width,
                    height,
                    stride,
                    format,
                    wlhandle!(new_proxy as _),
                );
            }
            ("wl_shm_pool", "resize") => {
                let size = args[0].i;
                self.m_shm_pool_resize(proxy, size);
            }
            ("wl_shm_pool", "destroy") => {
                self.shm_pool_map.remove(&proxy);
            }
            ("wl_buffer", "destroy") => {
                self.shm_buffer_map.remove(&proxy);
            }
            ("wl_surface", "attach") => {
                let buffer = args[0].o;
                self.m_surface_attach(proxy, wlhandle!(buffer as _));
            }
            ("wl_surface", "set_buffer_scale") => {
                let scale = args[0].i;
                self.m_surface_set_buffer_scale(proxy, scale);
            }
            ("wl_surface", "commit") => {
                self.m_surface_commit(proxy);
            }
            ("wl_surface", "destroy") => {
                self.surface_map.remove(&proxy);
            }
            ("wl_seat", "get_pointer") => {
                let new_proxy = args[0].o;
                self.m_seat_get_pointer(proxy, wlhandle!(new_proxy as _));
            }
            ("wl_pointer", "set_cursor") => {
                let serial = args[0].u;
                let surface = args[1].o;
                let hot_x = args[2].i;
                let hot_y = args[3].i;
                self.m_pointer_set_cursor(proxy, serial, wlhandle!(surface as _), hot_x, hot_y);
            }
            ("wl_pointer", "release") => {
                self.m_pointer_release(proxy);
            }
            _ => (),
        }

        #[cfg(debug_assertions)]
        if false {
            let signatures = WlSignatureIter::new(method.signature).collect::<Vec<_>>();
            for (i, &sig) in signatures.iter().enumerate() {
                if sig == WlSig::NewId {
                    let new_proxy = args[i].o;
                    let new_interface = &**(new_proxy as *mut *const wl_interface);
                    let new_name = CStr::from_ptr(new_interface.name);
                    debug!(
                        "{:?} {}:{} new {}: {:?} {:?}",
                        proxy,
                        interface_name,
                        method_name,
                        new_name.to_string_lossy(),
                        new_proxy,
                        &args[..signatures.len()]
                    );
                    break;
                }
            }
        }
    }

    unsafe fn collect_event_filter(&self, interface: &wl_interface) -> bool {
        let interface_name = CStr::from_ptr(interface.name).to_string_lossy();
        interface_name.as_ref() == "wl_pointer"
    }

    unsafe fn collect_event(
        &self,
        proxy: *mut wl_proxy,
        interface: &wl_interface,
        _opcode: u32,
        event: &wl_message,
        args: &[wl_argument],
    ) {
        let interface_name = CStr::from_ptr(interface.name).to_string_lossy();
        let event_name = CStr::from_ptr(event.name).to_string_lossy();

        let proxy = wlhandle!(proxy);

        match (interface_name.as_ref(), event_name.as_ref()) {
            ("wl_pointer", "enter") => {
                let serial = args[0].u;
                let surface = args[1].o;
                let x = args[2].f;
                let y = args[3].f;
                self.e_pointer_enter(proxy, serial, wlhandle!(surface as _), x, y);
            }
            ("wl_pointer", "motion") => {
                let time = args[0].u;
                let x = args[1].f;
                let y = args[2].f;
                self.e_pointer_motion(proxy, time, x, y);
            }
            ("wl_pointer", "leave") => {
                let serial = args[0].u;
                let surface = args[1].o;
                self.e_pointer_leave(proxy, serial, wlhandle!(surface as _));
            }
            _ => (),
        }
    }
}

struct DispatcherData {
    wl_intercept: &'static WlIntercept,
    raw_dispatcher: Option<PFN_wl_dispatcher>,
    raw_implementation: *mut c_void,
    collect: bool,
}

impl WlIntercept {
    pub unsafe fn intercept_wl_proxy_marshal_array_flags(
        &'static self,
        proxy: *mut wl_proxy,
        opcode: u32,
        interface: *const wl_interface,
        version: u32,
        flags: u32,
        args: *mut wl_argument,
    ) -> *mut wl_proxy {
        let proxy_interface = &**(proxy as *mut *const wl_interface);
        let method = &*proxy_interface.methods.offset(opcode as _);
        let args_slice = slice::from_raw_parts(args, WL_CLOSURE_MAX_ARGS);

        let implementation = (self.wl.wl_proxy_get_listener)(proxy);

        let res =
            (self.wl.wl_proxy_marshal_array_flags)(proxy, opcode, interface, version, flags, args);

        // proxy could be destroyed at this point if flags contains WL_MARSHAL_FLAG_DESTROY,
        // so do not dereference proxy pointer from here

        self.collect_method(proxy, proxy_interface, opcode, method, args_slice);
        if (flags & WL_MARSHAL_FLAG_DESTROY) != 0 && !implementation.is_null() {
            let layout = Layout::new::<DispatcherData>();
            dealloc(implementation as _, layout);
        }
        res
    }

    pub unsafe fn intercept_wl_proxy_create(
        &'static self,
        factory: *mut wl_proxy,
        interface: *const wl_interface,
    ) -> *mut wl_proxy {
        (self.wl.wl_proxy_create)(factory, interface)
    }

    pub unsafe fn intercept_wl_proxy_add_listener(
        &'static self,
        proxy: *mut wl_proxy,
        implementation: *mut PFN_void,
        user_data: *mut c_void,
    ) -> c_int {
        self.intercept_wl_proxy_add_dispatcher(proxy, None, implementation as _, user_data)
    }

    pub unsafe fn intercept_wl_proxy_add_dispatcher(
        &'static self,
        proxy: *mut wl_proxy,
        dispatcher: Option<PFN_wl_dispatcher>,
        implementation: *mut c_void,
        user_data: *mut c_void,
    ) -> c_int {
        if dispatcher.is_none() && implementation.is_null() {
            return 0;
        }
        let interface = &**(proxy as *mut *const wl_interface);

        let layout = Layout::new::<DispatcherData>();
        let impl_data_ptr = alloc(layout);
        let impl_data = &mut *(impl_data_ptr as *mut DispatcherData);
        impl_data.wl_intercept = self;
        impl_data.raw_dispatcher = dispatcher;
        impl_data.raw_implementation = implementation as _;
        impl_data.collect = self.collect_event_filter(interface);

        let res = (self.wl.wl_proxy_add_dispatcher)(
            proxy,
            Some(impl_dispatcher),
            impl_data_ptr as _,
            user_data,
        );
        if res != 0 {
            dealloc(impl_data_ptr, layout);
        }
        res
    }

    pub unsafe fn intercept_wl_proxy_get_listener(
        &'static self,
        proxy: *mut wl_proxy,
    ) -> *mut c_void {
        let implementation = (self.wl.wl_proxy_get_listener)(proxy) as *mut DispatcherData;
        if implementation.is_null() {
            return ptr::null_mut();
        }
        (&*implementation).raw_implementation
    }

    pub unsafe fn intercept_wl_proxy_destroy(&'static self, proxy: *mut wl_proxy) {
        trace!("destroying {:?}", proxy);
        let implementation = (self.wl.wl_proxy_get_listener)(proxy);

        (self.wl.wl_proxy_destroy)(proxy);
        if !implementation.is_null() {
            let layout = Layout::new::<DispatcherData>();
            dealloc(implementation as _, layout);
        }
    }
}

unsafe extern "C" fn impl_dispatcher(
    implementation: *mut c_void,
    proxy: *mut wl_proxy,
    opcode: u32,
    msg: *const wl_message,
    args: *mut wl_argument,
) -> c_int {
    let data = &*(implementation as *mut DispatcherData);
    let self_ = data.wl_intercept;
    let proxy_interface = &**(proxy as *mut *const wl_interface);
    let msg = &*msg;
    let args_slice = slice::from_raw_parts_mut(args, WL_CLOSURE_MAX_ARGS);

    if data.collect {
        self_.collect_event(proxy, proxy_interface, opcode, msg, args_slice);
    }

    let user_data = (self_.wl.wl_proxy_get_user_data)(proxy);
    dispatch_event(
        data.raw_dispatcher,
        data.raw_implementation,
        user_data,
        proxy,
        opcode,
        msg,
        args,
    )
}

unsafe fn dispatch_event(
    raw_dispatcher: Option<PFN_wl_dispatcher>,
    raw_implementation: *mut c_void,
    user_data: *mut c_void,
    proxy: *mut wl_proxy,
    opcode: u32,
    msg: *const wl_message,
    args: *mut wl_argument,
) -> c_int {
    use libffi::middle::{arg, Cif, CodePtr, Type};
    let msg = &*msg;

    if let Some(raw_dispatcher) = raw_dispatcher {
        return raw_dispatcher(raw_implementation, proxy, opcode, msg, args);
    }

    // reimplements libwayland internal libffi dispatching

    let raw_implementation = raw_implementation as *mut PFN_void;
    let callback = (*raw_implementation.offset(opcode as _)).unwrap();

    let p_userdata = &*user_data;
    let p_proxy = &*proxy;
    let mut ffi_types = vec![Type::pointer(), Type::pointer()];
    let mut ffi_args = vec![arg(&p_userdata), arg(&p_proxy)];

    for (i, sig) in WlSignatureIter::new(msg.signature).enumerate() {
        ffi_types.push(sig.into());
        let wl_arg = &*args.offset(i as _);
        ffi_args.push(WlArg(sig, wl_arg).into());
    }

    let cif = Cif::new(ffi_types, Type::void());
    cif.call::<()>(CodePtr(callback as _), &ffi_args);
    return 0;
}
