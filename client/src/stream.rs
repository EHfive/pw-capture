use crate::*;

use core::mem;
use core::ptr;
use core::slice;
use std::alloc::{alloc, dealloc, handle_alloc_error, Layout};
use std::sync::Arc;
use std::{cell::RefCell, fmt::Debug};

use anyhow::{anyhow, Result};
#[cfg(feature = "ash")]
use ash::vk;
use crossbeam_channel::{bounded, Sender};
use educe::Educe;
use log::{debug, error, info, trace, warn};
use pipewire as pw;
use pw::properties;
use pw::stream::ListenerBuilderT;
use trait_enumizer::{crossbeam_class, enumizer};

// allows 4 frames latency of buffer processing
const MAX_PROCESS_BUFFERS: usize = 4;
const MAX_CURSOR_WIDTH: usize = 64;
const MAX_CURSOR_BPP: usize = 4;
const MAX_CURSOR_BITMAP_SIZE: usize = MAX_CURSOR_WIDTH * MAX_CURSOR_WIDTH * MAX_CURSOR_BPP;

#[enumizer(
    name=StreamMessage,
    pub,
    returnval=crossbeam_class,
    call_fn(name=try_call_mut,ref_mut),
    proxy(Fn, name=StreamMethodsProxy),
    enum_attr[derive(Debug)],
)]
pub trait StreamMethods {
    fn terminate(&self) -> Result<()>;
    fn dequeue_buffer(&self) -> Option<(BufferHandle, BufferUserHandle)>;
    fn queue_buffer_process(&self, buffer: BufferHandle) -> Result<()>;
}

#[derive(Clone, Debug)]
pub struct EnumFormatInfo {
    pub formats: Vec<Format>,
    pub modifiers: Vec<u64>,
}

#[derive(Clone, Copy, Debug)]
pub struct FixateFormat {
    pub modifier: Option<u64>,
    pub num_planes: u32,
}

pub struct AddBufferMetaCbs<'a> {
    pub add_cursor: Option<Box<dyn FnOnce(BufferCursorInfo) + 'a>>,
}

type ProcessBufferCb = Box<dyn Fn(BufferUserHandle, AddBufferMetaCbs) + Send>;

#[derive(Educe)]
#[educe(Debug)]
pub struct StreamInfo {
    pub width: u32,
    pub height: u32,
    pub enum_formats: Vec<EnumFormatInfo>,
    pub max_buffers: u32,
    #[educe(Debug(ignore))]
    pub fixate_format: Box<dyn Fn(EnumFormatInfo) -> Option<FixateFormat> + Send>,
    #[educe(Debug(ignore))]
    pub add_buffer: Box<dyn Fn() -> Option<BufferInfo> + Send>,
    #[educe(Debug(ignore))]
    pub remove_buffer: Box<dyn Fn(BufferUserHandle) + Send>,
    #[educe(Debug(ignore))]
    pub process_buffer: ProcessBufferCb,
}

mod buffer_handle {
    use super::*;
    use core::num::NonZeroUsize;

    #[derive(Clone, Copy, Hash, Debug)]
    pub struct BufferHandle(NonZeroUsize);

    impl From<ptr::NonNull<pw::sys::pw_buffer>> for BufferHandle {
        fn from(value: ptr::NonNull<pw::sys::pw_buffer>) -> Self {
            let non_zero = unsafe { NonZeroUsize::new_unchecked(value.as_ptr() as usize) };
            Self(non_zero)
        }
    }

    impl From<BufferHandle> for ptr::NonNull<pw::sys::pw_buffer> {
        fn from(value: BufferHandle) -> Self {
            unsafe { ptr::NonNull::new_unchecked(value.0.get() as *mut pw::sys::pw_buffer) }
        }
    }
}
pub use buffer_handle::*;

#[derive(Clone, Copy, Debug)]
pub struct BufferPlaneInfo {
    pub fd: i64,
    pub offset: u32,
    pub size: u32,
    pub stride: u32,
}

#[derive(Clone, Debug)]
pub struct BufferInfo {
    pub is_dma_buf: bool,
    pub planes: Vec<BufferPlaneInfo>,
    pub user_handle: BufferUserHandle,
}

#[non_exhaustive]
#[derive(Clone, Copy, Hash, Debug)]
pub enum BufferUserHandle {
    #[cfg(feature = "ash")]
    VkImage(vk::Image),
    #[cfg(feature = "frontend_gl")]
    Texture(u32),
}

#[derive(Clone, Debug)]
pub struct BufferBitmap<'a> {
    pub width: u32,
    pub height: u32,
    pub format: Format,
    pub pixels: &'a [u8],
}

#[derive(Clone, Debug)]
pub struct BufferCursorInfo<'a> {
    /// change the internal cursor id if `true`
    pub serial: bool,
    pub position: Point,
    pub hotspot: Point,
    pub bitmap: Option<BufferBitmap<'a>>,
}

#[derive(Default)]
struct StreamData {
    seq: u64,
    cursor_id: u32,
}

struct StreamImplInner {
    stream: pw::stream::Stream<StreamData>,
    #[allow(unused)]
    listener: Option<pw::stream::StreamListener<StreamData>>,
    enum_formats: Vec<EnumFormatInfo>,
    max_buffers: u32,
    buffer_sender: Sender<BufferHandle>,
    on_terminate: Option<Box<dyn FnOnce()>>,
}

#[derive(Clone)]
pub(crate) struct StreamImpl {
    inner: Arc<RefCell<StreamImplInner>>,
}

pub(crate) fn build_stream_params(max_buffers: u32, blocks: u32, is_dma_buf: bool) -> Vec<Vec<u8>> {
    let data_type_flag = if is_dma_buf {
        1 << spa_sys::SPA_DATA_DmaBuf
    } else {
        1 << spa_sys::SPA_DATA_MemFd
    };
    let buffers = Value::Object(Object {
        type_: spa_sys::SPA_TYPE_OBJECT_ParamBuffers,
        id: spa_sys::SPA_PARAM_Buffers,
        properties: vec![
            Property {
                key: spa_sys::SPA_PARAM_BUFFERS_buffers,
                flags: PropertyFlags::empty(),
                value: Value::Choice(ChoiceValue::Int(Choice(
                    ChoiceFlags::empty(),
                    ChoiceEnum::Range {
                        default: 8,
                        min: 1,
                        max: max_buffers as _,
                    },
                ))),
            },
            Property {
                key: spa_sys::SPA_PARAM_BUFFERS_blocks,
                flags: PropertyFlags::empty(),
                value: Value::Int(blocks.max(1) as _),
            },
            Property {
                key: spa_sys::SPA_PARAM_BUFFERS_dataType,
                flags: PropertyFlags::empty(),
                value: Value::Choice(ChoiceValue::Int(Choice(
                    ChoiceFlags::empty(),
                    ChoiceEnum::Flags {
                        default: data_type_flag,
                        flags: vec![],
                    },
                ))),
            },
        ],
    });

    let meta_header = Value::Object(Object {
        type_: spa_sys::SPA_TYPE_OBJECT_ParamMeta,
        id: spa_sys::SPA_PARAM_Meta,
        properties: vec![
            Property {
                key: spa_sys::SPA_PARAM_META_type,
                flags: PropertyFlags::empty(),
                value: Value::Id(Id(spa_sys::SPA_META_Header)),
            },
            Property {
                key: spa_sys::SPA_PARAM_META_size,
                flags: PropertyFlags::empty(),
                value: Value::Int(mem::size_of::<spa_sys::spa_meta_header>() as _),
            },
        ],
    });

    let cursor_meta_size = mem::size_of::<spa_sys::spa_meta_cursor>()
        + mem::size_of::<spa_sys::spa_meta_bitmap>()
        + MAX_CURSOR_BITMAP_SIZE;

    let meta_cursor = Value::Object(Object {
        type_: spa_sys::SPA_TYPE_OBJECT_ParamMeta,
        id: spa_sys::SPA_PARAM_Meta,
        properties: vec![
            Property {
                key: spa_sys::SPA_PARAM_META_type,
                flags: PropertyFlags::empty(),
                value: Value::Id(Id(spa_sys::SPA_META_Cursor)),
            },
            Property {
                key: spa_sys::SPA_PARAM_META_size,
                flags: PropertyFlags::empty(),
                value: Value::Int(cursor_meta_size as _),
            },
        ],
    });

    let params = &[buffers, meta_header, meta_cursor];
    params
        .iter()
        .map(|value| -> Result<Vec<u8>> { spa_pod_serialize(value) })
        .collect::<Result<Vec<_>>>()
        .unwrap_or_default()
}

pub(crate) fn build_format(
    width: u32,
    height: u32,
    formats: &[Format],
    modifiers: &[u64],
    fixate: bool,
) -> Result<Vec<u8>> {
    assert!(!formats.is_empty());

    let format_value = if formats.len() > 1 {
        Value::Choice(ChoiceValue::Id(Choice(
            ChoiceFlags::empty(),
            ChoiceEnum::Enum {
                default: Id(formats[0].into()),
                alternatives: formats.iter().map(|&f| Id(f.into())).collect(),
            },
        )))
    } else {
        Value::Id(Id(formats[0].into()))
    };

    let mut properties = vec![
        Property {
            key: spa_sys::SPA_FORMAT_mediaType,
            flags: PropertyFlags::empty(),
            value: Value::Id(Id(spa_sys::SPA_MEDIA_TYPE_video)),
        },
        Property {
            key: spa_sys::SPA_FORMAT_mediaSubtype,
            flags: PropertyFlags::empty(),
            value: Value::Id(Id(spa_sys::SPA_MEDIA_SUBTYPE_raw)),
        },
        Property {
            key: spa_sys::SPA_FORMAT_VIDEO_format,
            flags: PropertyFlags::empty(),
            value: format_value,
        },
        Property {
            key: spa_sys::SPA_FORMAT_VIDEO_size,
            flags: PropertyFlags::empty(),
            value: Value::Rectangle(Rectangle { width, height }),
        },
        Property {
            key: spa_sys::SPA_FORMAT_VIDEO_framerate,
            flags: PropertyFlags::empty(),
            value: Value::Fraction(Fraction { num: 0, denom: 1 }),
        },
    ];

    if modifiers.len() > 0 {
        let prop = if fixate {
            Property {
                key: spa_sys::SPA_FORMAT_VIDEO_modifier,
                flags: PropertyFlags::MANDATORY,
                value: Value::Long(modifiers[0] as _),
            }
        } else {
            Property {
                key: spa_sys::SPA_FORMAT_VIDEO_modifier,
                flags: PropertyFlags::MANDATORY | PropertyFlags::DONT_FIXATE,
                value: Value::Choice(ChoiceValue::Long(Choice(
                    ChoiceFlags::empty(),
                    ChoiceEnum::Enum {
                        default: modifiers[0] as _,
                        alternatives: modifiers.iter().map(|&m| m as _).collect(),
                    },
                ))),
            }
        };
        properties.push(prop);
    }

    let param = Value::Object(Object {
        type_: spa_sys::SPA_TYPE_OBJECT_Format,
        id: spa_sys::SPA_PARAM_EnumFormat,
        properties,
    });
    spa_pod_serialize(&param)
}

impl StreamMethods for StreamImpl {
    fn terminate(&self) -> Result<()> {
        debug!("terminate stream");
        let _ = self.inner.borrow().stream.disconnect();
        self.inner.borrow_mut().on_terminate.take().map(|f| f());
        Ok(())
    }

    fn dequeue_buffer(&self) -> Option<(BufferHandle, BufferUserHandle)> {
        let inner = self.inner.borrow();
        let stream = &inner.stream;
        match inner.stream.state() {
            pw::stream::StreamState::Streaming => (),
            _ => return None,
        }
        if !inner.stream.is_driving() {
            return None;
        }
        unsafe {
            let buffer = ptr::NonNull::new(stream.dequeue_raw_buffer());
            let buffer = if let Some(v) = buffer {
                v
            } else {
                trace!("out of buffer");
                return None;
            };
            let pw_buffer = buffer.as_ref();
            let user_data = pw_buffer.user_data as *mut BufferUserHandle;
            if user_data.is_null() {
                error!("buffer broken no user data");
                stream.queue_raw_buffer(buffer.as_ptr());
                return None;
            };
            Some((buffer.into(), *user_data))
        }
    }

    fn queue_buffer_process(&self, buffer: BufferHandle) -> Result<()> {
        if self.inner.borrow().stream.is_driving() {
            self.inner
                .borrow()
                .buffer_sender
                .send(buffer)
                .map_err(|e| anyhow!("{e:?}"))?;

            self.inner.borrow().stream.trigger_process()?;
        }
        Ok(())
    }
}

unsafe fn on_param_changed(
    inner: &StreamImplInner,
    id: u32,
    param: *const spa_sys::spa_pod,
    width: u32,
    height: u32,
    fixate_format: &Box<dyn Fn(EnumFormatInfo) -> Option<FixateFormat> + Send>,
) {
    debug!("param changed: id {}, param: {:?}", id, param);
    if param.is_null() || id != spa_sys::SPA_PARAM_Format {
        return;
    }
    let pod = deserialize::PodDeserializer::deserialize_ptr::<Value>(ptr::NonNull::new_unchecked(
        param as _,
    ));
    let pod = match pod {
        Ok(v) => v,
        Err(e) => {
            debug!("error parsing pod {:?} {:?}", param, e);
            return;
        }
    };
    debug!("{pod:?}");
    let raw_info: VideoRawInfo = match pod.clone().try_into() {
        Ok(v) => v,
        Err(e) => {
            error!("error parsing format info {:?} {:?}", param, e);
            return;
        }
    };
    debug!("{raw_info:?}");

    debug!("fixating");
    let fixate_info = fixate_format(EnumFormatInfo {
        formats: vec![raw_info.format],
        modifiers: raw_info.modifiers.clone(),
    });
    let fixate_info = if let Some(v) = fixate_info {
        v
    } else {
        error!("no compatible format");
        // XXX: re-update params?
        return;
    };
    debug!("fixate to {:?}", fixate_info);

    let stream = &inner.stream;

    if raw_info.modifiers.len() > 0 {
        debug!("has modifier");
        let fixate_modifier = fixate_info.modifier.unwrap();
        if raw_info.dont_fixate_modifier {
            let mut params =
                vec![
                    build_format(width, height, &[raw_info.format], &[fixate_modifier], true)
                        .unwrap(),
                ];
            for enum_format in &inner.enum_formats {
                params.push(
                    build_format(
                        width,
                        height,
                        &enum_format.formats,
                        &enum_format.modifiers,
                        false,
                    )
                    .unwrap(),
                )
            }
            let mut params = params
                .iter()
                .map(|p| p.as_ptr() as *const spa_sys::spa_pod)
                .collect::<Vec<_>>();

            let _ = stream.update_params(&mut params);
            return;
        }
    } else {
        debug!("no modifier");
    }

    let params = build_stream_params(
        inner.max_buffers,
        fixate_info.num_planes,
        fixate_info.modifier.is_some(),
    );
    let mut params = params
        .iter()
        .map(|p| p.as_ptr() as *const spa_sys::spa_pod)
        .collect::<Vec<_>>();

    let _ = stream.update_params(&mut params);
}

unsafe fn on_add_buffer(
    buffer: *mut pw::sys::pw_buffer,
    add_buffer: &Box<dyn Fn() -> Option<BufferInfo> + Send>,
) {
    debug!("add buffer");
    let mut buffer = ptr::NonNull::new(buffer).unwrap();
    let pw_buffer = buffer.as_mut();
    let spa_buffer = &mut *pw_buffer.buffer;
    pw_buffer.user_data = ptr::null_mut();

    let datas = slice::from_raw_parts_mut(spa_buffer.datas, spa_buffer.n_datas as _);
    // let metas = slice::from_raw_parts_mut(spa_buffer.metas, spa_buffer.n_metas as _);

    let info = add_buffer();
    let info = if let Some(info) = info {
        info
    } else {
        error!("failed to add buffer, mark invalid");
        for data in datas {
            data.fd = -1;
            data.data = ptr::null_mut();
            data.type_ = libspa_sys::SPA_DATA_Invalid;
        }
        return;
    };

    let data_type = if info.is_dma_buf {
        libspa_sys::SPA_DATA_DmaBuf
    } else {
        libspa_sys::SPA_DATA_MemFd
    };

    assert_eq!(spa_buffer.n_datas, info.planes.len() as _);
    for (data, plane) in datas.iter_mut().zip(&info.planes) {
        let chunk = &mut *data.chunk;
        data.fd = plane.fd as _;
        data.data = ptr::null_mut();
        data.mapoffset = plane.offset as _;
        data.maxsize = plane.size as _;
        data.type_ = data_type;
        chunk.offset = plane.offset as _;
        chunk.size = plane.size as _;
        chunk.stride = plane.stride as _;
        debug!("{:?}", plane);
    }

    let layout = Layout::new::<BufferUserHandle>();
    let user_data = alloc(layout);
    if user_data.is_null() {
        handle_alloc_error(layout);
    }
    *(user_data as *mut BufferUserHandle) = info.user_handle;
    pw_buffer.user_data = user_data as _;

    debug!("added buffer");
}

unsafe fn on_remove_buffer(
    buffer: *mut pw::sys::pw_buffer,
    remove_buffer: &Box<dyn Fn(BufferUserHandle) + Send>,
) {
    debug!("remove buffer");
    let mut buffer = ptr::NonNull::new(buffer).unwrap();

    let pw_buffer = buffer.as_mut();
    let user_data = pw_buffer.user_data as *mut BufferUserHandle;
    if user_data.is_null() {
        return;
    }
    remove_buffer(*user_data);
    dealloc(user_data as _, Layout::new::<BufferUserHandle>());
}

#[inline]
fn get_pts_nanos() -> i64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
    }
    ts.tv_sec as i64 * 1_000_000_000 + ts.tv_nsec as i64
}

unsafe fn fill_cursor_meta(
    id: &mut u32,
    cursor_ptr: *mut libspa_sys::spa_meta_cursor,
    cursor_info: Option<BufferCursorInfo>,
) {
    debug_assert!(!cursor_ptr.is_null());
    let cursor = &mut *cursor_ptr;
    if let Some(info) = cursor_info {
        if info.serial {
            *id += 1;
        }
        cursor.id = *id;
        cursor.flags = 0;
        cursor.position = info.position.into();
        cursor.hotspot = info.hotspot.into();

        if let Some(b_info) = info.bitmap {
            let offset = mem::size_of::<spa_sys::spa_meta_cursor>();
            cursor.bitmap_offset = offset as _;
            let bitmap_ptr = cursor_ptr
                .cast::<u8>()
                .offset(offset as _)
                .cast::<spa_sys::spa_meta_bitmap>();
            let bitmap = &mut *bitmap_ptr;
            bitmap.format = b_info.format.into();
            bitmap.size.width = b_info.width;
            bitmap.size.height = b_info.height;
            bitmap.stride = b_info.width as _;
            let offset = mem::size_of::<spa_sys::spa_meta_bitmap>();
            bitmap.offset = offset as _;
            let bitmap_data = bitmap_ptr.cast::<u8>().offset(offset as _);

            if b_info.pixels.len() <= MAX_CURSOR_BITMAP_SIZE {
                let bitmap_data = slice::from_raw_parts_mut(bitmap_data, b_info.pixels.len());
                bitmap_data.copy_from_slice(&b_info.pixels);
            } else {
                warn!(
                    "cursor bitmap size {} exceed max size {}, discarded",
                    b_info.pixels.len(),
                    MAX_CURSOR_BITMAP_SIZE
                );
                cursor.bitmap_offset = 0;
            }
        } else {
            cursor.bitmap_offset = 0;
        }
    } else {
        cursor.id = 0;
    }
}

unsafe fn on_process_buffer(
    stream: &pw::stream::Stream<StreamData>,
    data: &mut StreamData,
    buffer: BufferHandle,
    user_process: &ProcessBufferCb,
) {
    let pw_buffer = ptr::NonNull::from(buffer).as_mut();

    let header = spa_buffer_find_meta_data::<libspa_sys::spa_meta_header>(
        pw_buffer.buffer,
        libspa_sys::SPA_META_Header,
    );

    let cursor = spa_buffer_find_meta_data::<libspa_sys::spa_meta_cursor>(
        pw_buffer.buffer,
        libspa_sys::SPA_META_Cursor,
    );

    let user_data = pw_buffer.user_data as *mut BufferUserHandle;
    if user_data.is_null() {
        error!("buffer broken no user data");
        return;
    };

    let mut cursor_meta_filled = false;
    user_process(
        *user_data,
        AddBufferMetaCbs {
            add_cursor: if cursor.is_null() {
                None
            } else {
                Some(Box::new(|info| {
                    fill_cursor_meta(&mut data.cursor_id, cursor, Some(info));
                    cursor_meta_filled = true;
                }))
            },
        },
    );

    if !header.is_null() {
        let header = &mut *header;
        header.flags = 0;
        header.pts = get_pts_nanos();
        // header.pts = -1;
        header.offset = 0;
        header.seq = data.seq;
        header.dts_offset = 0;
    }
    data.seq += 1;

    if !cursor.is_null() && !cursor_meta_filled {
        fill_cursor_meta(&mut data.cursor_id, cursor, None);
    }

    pw_buffer.size = 1;

    stream.queue_raw_buffer(pw_buffer);
}

impl StreamImpl {
    pub(crate) fn new(
        core: &pw::Core,
        info: StreamInfo,
        on_terminate: Box<dyn FnOnce()>,
    ) -> Result<Self> {
        let name = format!("{} (pw-capture)", get_app_name());
        let stream = pw::stream::Stream::<StreamData>::new(
            core,
            name.as_str(),
            properties! {
                *pw::keys::MEDIA_TYPE => "Video",
                *pw::keys::MEDIA_CATEGORY => "Capture",
                *pw::keys::MEDIA_ROLE => "Screen",
                *pw::keys::MEDIA_CLASS => "Video/Source",
                *pw::keys::MEDIA_SOFTWARE => "pw-capture",
                *pw::keys::NODE_WANT_DRIVER => "false",
                *pw::keys::NODE_DESCRIPTION => name.as_str(),
            },
        )?;

        let (buffer_sender, buffer_receiver) = bounded::<BufferHandle>(MAX_PROCESS_BUFFERS);

        let inner = StreamImplInner {
            stream,
            listener: None,
            enum_formats: info.enum_formats,
            max_buffers: info.max_buffers,
            buffer_sender,
            on_terminate: Some(on_terminate),
        };
        let stream_impl = StreamImpl {
            inner: Arc::new(RefCell::new(inner)),
        };

        let listener = stream_impl
            .inner
            .borrow_mut()
            .stream
            .add_local_listener_with_user_data(StreamData {
                seq: 0,
                cursor_id: 1,
            })
            .state_changed({
                let stream_impl = stream_impl.clone();
                let buffer_receiver = buffer_receiver.clone();
                move |old, new| {
                    info!("stream state changed: {:?} -> {:?}", old, new);
                    match new {
                        pw::stream::StreamState::Paused => {
                            let _ = stream_impl.inner.borrow().stream.flush(false);
                            for _ in buffer_receiver.try_iter() {
                                // drain buffer channel, in case buffer was not processed
                            }
                        }
                        pw::stream::StreamState::Error(e) => error!("stream error: {}", e),
                        _ => (),
                    }
                }
            })
            .param_changed({
                let stream_impl = stream_impl.clone();
                move |id, _data, param| unsafe {
                    on_param_changed(
                        &stream_impl.inner.borrow(),
                        id,
                        param,
                        info.width,
                        info.height,
                        &info.fixate_format,
                    )
                }
            })
            .add_buffer(move |buffer| unsafe { on_add_buffer(buffer, &info.add_buffer) })
            .remove_buffer(move |buffer| unsafe { on_remove_buffer(buffer, &info.remove_buffer) })
            .process(move |stream, data| unsafe {
                if let Ok(buffer) = buffer_receiver.try_recv() {
                    on_process_buffer(stream, data, buffer, &info.process_buffer);
                } else {
                    warn!("unscheduled process call");
                }
            })
            .register()?;

        let mut params = vec![];
        for enum_format in &stream_impl.inner.borrow().enum_formats {
            params.push(
                build_format(
                    info.width,
                    info.height,
                    &enum_format.formats,
                    &enum_format.modifiers,
                    false,
                )
                .unwrap(),
            )
        }
        let mut params = params
            .iter()
            .map(|p| p.as_ptr() as *const spa_sys::spa_pod)
            .collect::<Vec<_>>();

        stream_impl.inner.borrow().stream.connect(
            spa::Direction::Output,
            None,
            pw::stream::StreamFlags::DRIVER
                | pw::stream::StreamFlags::ALLOC_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS
                | pw::stream::StreamFlags::TRIGGER,
            &mut params,
        )?;

        stream_impl.inner.borrow_mut().listener = Some(listener);

        Ok(stream_impl)
    }

    pub(crate) fn attach<'a>(
        &self,
        loop_: &'a pw::LoopRef,
        pw_receiver: pw::channel::Receiver<StreamMessage>,
    ) -> pw::channel::AttachedReceiver<'a, StreamMessage> {
        let inner_weak = Arc::downgrade(&self.inner);
        let receiver = pw_receiver.attach(loop_, move |msg| {
            trace!("[msg] receive {:?}", msg);
            if let Some(inner) = inner_weak.upgrade() {
                let _ = msg.try_call_mut(&mut StreamImpl { inner });
                trace!("[msg] handled");
            } else {
                debug!("stream impl dropped");
            }
        });
        receiver
    }
}
