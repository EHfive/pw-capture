use super::*;
use crate::utils::*;

use core::ffi::{c_char, c_void, CStr};
use core::fmt::Debug;
use core::mem;
use libffi::middle;
use std::ffi::CString;

#[allow(non_snake_case)]
pub struct WlLib {
    pub wl_display_interface: WlHandle,
    pub wl_proxy_marshal_array_flags: PFN_wl_proxy_marshal_array_flags,
    pub wl_proxy_create: PFN_wl_proxy_create,
    pub wl_proxy_add_listener: PFN_wl_proxy_add_listener,
    pub wl_proxy_add_dispatcher: PFN_wl_proxy_add_dispatcher,
    pub wl_proxy_get_user_data: PFN_wl_proxy_get_user_data,
    pub wl_proxy_get_listener: PFN_wl_proxy_get_listener,
    pub wl_proxy_destroy: PFN_wl_proxy_destroy,
}

impl WlLib {
    pub unsafe fn load_with<F>(mut dlsym: F) -> Option<Self>
    where
        F: FnMut(*mut c_void, &CStr) -> *const c_void,
    {
        macro_rules! dlsym {
            ($h: expr, $cstr: expr) => {{
                let res = dlsym($h, $cstr);
                if res.is_null() {
                    return None;
                } else {
                    mem::transmute(res)
                }
            }};
        }
        macro_rules! construct {
            ($(  $sym:ident : $h:expr , )*) => {
                Self {
                    $( $sym : dlsym!($h, CString::new(stringify!($sym)).unwrap().as_c_str()) , )*
                }
            };
        }

        let wl_client = dlopen(&[
            cstr!(b"libwayland-client.so.0\0"),
            cstr!(b"libwayland-client.so\0"),
        ])?;

        Some(construct!(
            wl_display_interface: wl_client,
            wl_proxy_marshal_array_flags: wl_client,
            wl_proxy_create: wl_client,
            wl_proxy_add_listener: wl_client,
            wl_proxy_add_dispatcher: wl_client,
            wl_proxy_get_user_data: wl_client,
            wl_proxy_get_listener: wl_client,
            wl_proxy_destroy: wl_client,
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WlSig {
    Int,
    UInt,
    Fixed,
    String,
    Object,
    NewId,
    Array,
    Fd,
}

pub struct WlSignatureIter {
    signature: *const c_char,
}

impl WlSignatureIter {
    pub unsafe fn new(signature: *const c_char) -> Self {
        Self { signature }
    }
}

impl Iterator for WlSignatureIter {
    type Item = WlSig;
    fn next(&mut self) -> Option<Self::Item> {
        let arg = loop {
            unsafe {
                let ch = *self.signature as u8;
                use WlSig::*;
                let arg = match ch {
                    b'i' => Int,
                    b'u' => UInt,
                    b'f' => Fixed,
                    b's' => String,
                    b'o' => Object,
                    b'n' => NewId,
                    b'a' => Array,
                    b'h' => Fd,
                    b'\0' => return None,
                    _ => {
                        // unknown signature
                        self.signature = self.signature.offset(1);
                        continue;
                    }
                };
                self.signature = self.signature.offset(1);
                break arg;
            };
        };
        Some(arg)
    }
}

impl From<WlSig> for middle::Type {
    fn from(value: WlSig) -> Self {
        use middle::Type;
        match value {
            WlSig::Int => Type::i32(),
            WlSig::UInt => Type::u32(),
            WlSig::Fixed => Type::i32(),
            WlSig::String => Type::pointer(),
            WlSig::Object => Type::pointer(),
            // pointer on client side, u32 on server side
            // see wayland/src/connection.c:convert_arguments_to_ffi
            WlSig::NewId => Type::pointer(),
            WlSig::Array => Type::pointer(),
            WlSig::Fd => Type::i32(),
        }
    }
}

pub struct WlArg<'a>(pub WlSig, pub &'a wl_argument);

impl<'a> From<WlArg<'a>> for middle::Arg {
    fn from(value: WlArg<'a>) -> Self {
        use middle::arg;
        let WlArg(sig, value) = value;
        unsafe {
            match sig {
                WlSig::Int => arg(&value.i),
                WlSig::UInt => arg(&value.u),
                WlSig::Fixed => arg(&value.f),
                WlSig::String => arg(&value.s),
                WlSig::Object => arg(&value.o),
                // pointer on client side, u32 on server side
                // see wayland/src/connection.c:convert_arguments_to_ffi
                WlSig::NewId => arg(&value.o),
                WlSig::Array => arg(&value.a),
                WlSig::Fd => arg(&value.h),
            }
        }
    }
}

macro_rules! wlhandle {
    ($ptr:expr) => {
        WlHandle::from_ptr($ptr)
    };
}
pub(crate) use wlhandle;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct WlHandle(usize);

impl WlHandle {
    pub unsafe fn from_raw(addr: usize) -> Self {
        Self(addr)
    }
    pub unsafe fn from_ptr<T>(ptr: *mut T) -> Self {
        Self(ptr as usize)
    }
    pub unsafe fn as_ptr<T>(&self) -> *mut T {
        self.0 as _
    }
}

impl Debug for WlHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("GlHandle")
            .field(unsafe { &self.as_ptr::<c_void>() })
            .finish()
    }
}
