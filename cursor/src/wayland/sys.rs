#![allow(non_camel_case_types)]

use core::ffi::{c_char, c_int, c_void, CStr};
use core::fmt::Debug;
use core::slice;

use fixed::types::extra::U8;
use fixed::FixedI32;

pub const WL_CLOSURE_MAX_ARGS: usize = 20;

pub const WL_MARSHAL_FLAG_DESTROY: u32 = 1;

pub const WL_DISPLAY_GET_REGISTRY: u32 = 0;

pub const WL_REGISTRY_BIND: u32 = 0;

pub const WL_SEAT_GET_POINTER: u32 = 0;

pub const WL_POINTER_SET_CURSOR: u32 = 0;
pub const WL_POINTER_RELEASE: u32 = 1;

pub enum wl_proxy {}
pub enum wl_display {}

pub enum wl_object {}

#[repr(C)]
pub struct wl_message {
    pub name: *const c_char,
    pub signature: *const c_char,
    pub types: *mut *const wl_interface,
}

impl Debug for wl_message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unsafe {
            f.debug_struct("wl_message")
                .field("name", &CStr::from_ptr(self.name).to_string_lossy())
                .field("signature", &CStr::from_ptr(self.name).to_string_lossy())
                .finish_non_exhaustive()
        }
    }
}

#[repr(C)]
pub struct wl_interface {
    pub name: *const c_char,
    pub version: c_int,
    pub method_count: c_int,
    pub methods: *const wl_message,
    pub event_count: c_int,
    pub events: *const wl_message,
}

impl Debug for wl_interface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unsafe {
            let methods = slice::from_raw_parts(self.methods, self.method_count as _);
            let events = slice::from_raw_parts(self.events, self.event_count as _);
            f.debug_struct("wl_interface")
                .field("name", &CStr::from_ptr(self.name).to_string_lossy())
                .field("version", &self.version)
                .field("methods", &methods)
                .field("events", &events)
                .finish()
        }
    }
}

pub type wl_fixed_t = FixedI32<U8>;

#[repr(C)]
pub struct wl_array {
    pub size: usize,
    pub alloc: usize,
    pub data: *const c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union wl_argument {
    /// int
    pub i: i32,
    /// uint
    pub u: u32,
    // fixed
    pub f: wl_fixed_t,
    /// string
    pub s: *const c_char,
    /// object
    pub o: *mut wl_object,
    /// new_id
    pub n: u32,
    /// array
    pub a: *mut wl_array,
    /// fd
    pub h: i32,
}
impl Debug for wl_argument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("wl_argument")
            .field(&unsafe { self.s })
            .finish()
    }
}

#[repr(C)]
pub struct wl_registry_listener {
    pub global: Option<PFN_wl_registry_event_global>,
    pub global_remove: Option<PFN_wl_registry_event_global_remove>,
}

#[repr(C)]
pub struct wl_pointer_listener {
    pub enter: Option<PFN_wl_pointer_event_enter>,
    pub leave: Option<PFN_wl_pointer_event_leave>,
    pub motion: Option<PFN_wl_pointer_event_motion>,
    pub button: PFN_void,
    pub axis: PFN_void,
    pub frame: PFN_void,
    pub axis_source: PFN_void,
    pub axis_stop: PFN_void,
    pub axis_discrete: PFN_void,
    pub axis_value120: PFN_void,
}

pub type PFN_wl_registry_event_global = unsafe extern "C" fn(
    data: *mut c_void,
    wl_registry: *mut wl_proxy,
    name: u32,
    interface: *const c_char,
    version: u32,
);

pub type PFN_wl_registry_event_global_remove =
    unsafe extern "C" fn(data: *mut c_void, wl_registry: *mut wl_proxy, name: u32);

pub type PFN_wl_pointer_event_enter = unsafe extern "C" fn(
    data: *mut c_void,
    wl_pointer: *mut wl_proxy,
    serial: u32,
    surface: *mut wl_proxy,
    surface_x: wl_fixed_t,
    surface_y: wl_fixed_t,
);

pub type PFN_wl_pointer_event_leave = unsafe extern "C" fn(
    data: *mut c_void,
    wl_pointer: *mut wl_proxy,
    serial: u32,
    surface: *mut wl_proxy,
);

pub type PFN_wl_pointer_event_motion = unsafe extern "C" fn(
    data: *mut c_void,
    wl_pointer: *mut wl_proxy,
    time: u32,
    surface_x: wl_fixed_t,
    surface_y: wl_fixed_t,
);

pub type PFN_wl_proxy_marshal_array_flags = unsafe extern "C" fn(
    proxy: *mut wl_proxy,
    opcode: u32,
    interface: *const wl_interface,
    version: u32,
    flags: u32,
    args: *mut wl_argument,
) -> *mut wl_proxy;

pub type PFN_wl_proxy_create =
    unsafe extern "C" fn(factory: *mut wl_proxy, interface: *const wl_interface) -> *mut wl_proxy;

pub type PFN_wl_proxy_add_listener = unsafe extern "C" fn(
    proxy: *mut wl_proxy,
    implementation: *mut PFN_void,
    data: *mut c_void,
) -> c_int;
pub type PFN_wl_proxy_get_listener = unsafe extern "C" fn(proxy: *mut wl_proxy) -> *mut c_void;

pub type PFN_wl_dispatcher = unsafe extern "C" fn(
    implementation: *mut c_void,
    proxy: *mut wl_proxy,
    opcode: u32,
    msg: *const wl_message,
    args: *mut wl_argument,
) -> c_int;

pub type PFN_wl_proxy_add_dispatcher = unsafe extern "C" fn(
    proxy: *mut wl_proxy,
    dispatcher: Option<PFN_wl_dispatcher>,
    implementation: *mut c_void,
    data: *mut c_void,
) -> c_int;

pub type PFN_wl_proxy_get_user_data = unsafe extern "C" fn(proxy: *mut wl_proxy) -> *mut c_void;

pub type PFN_wl_proxy_destroy = unsafe extern "C" fn(proxy: *mut wl_proxy);

pub type PFN_wl_proxy_create_wrapper = unsafe extern "C" fn(proxy: *mut wl_proxy) -> *mut wl_proxy;

pub type PFN_wl_proxy_wrapper_destroy = unsafe extern "C" fn(proxy: *mut wl_proxy);

pub type PFN_void = Option<unsafe extern "C" fn()>;
