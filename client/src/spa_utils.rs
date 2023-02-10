use crate::Format;

pub use libspa as spa;
pub use libspa_sys as spa_sys;
pub use spa::pod::*;
pub use spa::utils::*;

use core::mem;
use core::ptr;
use core::slice;
use std::io::Cursor;

use anyhow::{anyhow, Result};

#[derive(Clone, Debug, Default)]
pub(crate) struct VideoRawInfo {
    pub format: Format,
    pub dont_fixate_modifier: bool,
    pub modifiers: Vec<u64>,
}

pub(crate) unsafe fn spa_buffer_find_meta_data<T>(
    buffer: *mut libspa_sys::spa_buffer,
    type_: u32,
) -> *mut T {
    let buffer = &*buffer;
    let metas = slice::from_raw_parts_mut(buffer.metas, buffer.n_metas as _);
    for meta in metas {
        if meta.type_ == type_ {
            if meta.size >= mem::size_of::<T>() as _ {
                return meta.data as _;
            }
            break;
        }
    }
    ptr::null_mut()
}

pub(crate) fn spa_pod_serialize<P: serialize::PodSerialize + ?Sized>(value: &P) -> Result<Vec<u8>> {
    let res = serialize::PodSerializer::serialize(Cursor::new(Vec::new()), value)?
        .0
        .into_inner();
    Ok(res)
}

pub(crate) fn choice_collect<T>(choice: ChoiceEnum<T>) -> Vec<T>
where
    T: CanonicalFixedSizedPod,
{
    match choice {
        ChoiceEnum::Enum {
            default,
            mut alternatives,
        } => {
            alternatives.insert(0, default);
            alternatives
        }
        ChoiceEnum::Flags { default, mut flags } => {
            flags.insert(0, default);
            flags
        }
        ChoiceEnum::Step { default, .. } => vec![default],
        ChoiceEnum::Range { default, .. } => vec![default],
        ChoiceEnum::None(value) => vec![value],
    }
}

pub(crate) fn value_collect_id(value: Value) -> Vec<u32> {
    let ids = match value {
        Value::ValueArray(ValueArray::Id(v)) => v,
        Value::Choice(ChoiceValue::Id(choice)) => choice_collect(choice.1),
        Value::Id(v) => vec![v],
        _ => vec![],
    };
    ids.into_iter().map(|id| id.0).collect()
}

pub(crate) fn value_collect_long(value: Value) -> Vec<i64> {
    match value {
        Value::ValueArray(ValueArray::Long(value)) => value,
        Value::Choice(ChoiceValue::Long(choice)) => choice_collect(choice.1),
        Value::Long(value) => vec![value],
        _ => vec![],
    }
}

impl TryFrom<Value> for VideoRawInfo {
    type Error = anyhow::Error;
    fn try_from(value: Value) -> Result<Self> {
        let obj = if let Value::Object(obj) = value {
            obj
        } else {
            return Err(anyhow!("{:?} is not a object", value));
        };
        if obj.type_ != spa_sys::SPA_TYPE_OBJECT_Format {
            return Err(anyhow!("{:?} is not a format", obj));
        }
        let mut info = VideoRawInfo::default();
        for Property { key, flags, value } in obj.properties {
            match key {
                spa_sys::SPA_FORMAT_VIDEO_format => {
                    info.format = Format::from(
                        *value_collect_id(value)
                            .first()
                            .ok_or(anyhow!("no format"))?,
                    )
                }
                spa_sys::SPA_FORMAT_VIDEO_modifier => {
                    info.dont_fixate_modifier = flags.contains(PropertyFlags::DONT_FIXATE);
                    info.modifiers = value_collect_long(value)
                        .into_iter()
                        .map(|v| v as _)
                        .collect();
                }
                _ => continue,
            }
        }
        Ok(info)
    }
}
