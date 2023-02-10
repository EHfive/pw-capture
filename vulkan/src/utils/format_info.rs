use std::fmt::Debug;

use ash::vk;
use concat_idents::concat_idents;
use pw_capture_client::{Format, Transfer};

#[derive(Clone, Copy, Debug)]
pub struct VkFormatInfo {
    pub format: Format,
    pub transfer: Transfer,
    pub vk_format: vk::Format,
}

macro_rules! finfo {
    ($fmt:ident, $vk_fmt:ident, $ts:ident $(, $pack:ident)?) => {
        concat_idents!(full_fmt = $vk_fmt, _, $ts $(, $pack)? {
            VkFormatInfo {
                format: Format::$fmt,
                transfer: Transfer::$ts,
                vk_format: vk::Format::full_fmt,
            }
        })
    };
}

pub const VK_FORMAT_INFO_TABLE: &[VkFormatInfo] = &[
    // BGRA
    finfo!(BGRA, B8G8R8A8, SINT),
    finfo!(BGRA, B8G8R8A8, SNORM),
    finfo!(BGRA, B8G8R8A8, SRGB),
    finfo!(BGRA, B8G8R8A8, SSCALED),
    finfo!(BGRA, B8G8R8A8, UINT),
    finfo!(BGRA, B8G8R8A8, UNORM),
    finfo!(BGRA, B8G8R8A8, USCALED),
    // RGBA
    finfo!(RGBA, R8G8B8A8, SINT),
    finfo!(RGBA, R8G8B8A8, SNORM),
    finfo!(RGBA, R8G8B8A8, SRGB),
    finfo!(RGBA, R8G8B8A8, SSCALED),
    finfo!(RGBA, R8G8B8A8, UINT),
    finfo!(RGBA, R8G8B8A8, UNORM),
    finfo!(RGBA, R8G8B8A8, USCALED),
    // BGR
    finfo!(BGR, B8G8R8, SINT),
    finfo!(BGR, B8G8R8, SNORM),
    finfo!(BGR, B8G8R8, SRGB),
    finfo!(BGR, B8G8R8, SSCALED),
    finfo!(BGR, B8G8R8, UINT),
    finfo!(BGR, B8G8R8, UNORM),
    finfo!(BGR, B8G8R8, USCALED),
    // RGB
    finfo!(RGB, R8G8B8, SINT),
    finfo!(RGB, R8G8B8, SNORM),
    finfo!(RGB, R8G8B8, SRGB),
    finfo!(RGB, R8G8B8, SSCALED),
    finfo!(RGB, R8G8B8, UINT),
    finfo!(RGB, R8G8B8, UNORM),
    finfo!(RGB, R8G8B8, USCALED),
    // RGBA_102LE
    finfo!(RGBA_102LE, A2B10G10R10, SINT, _PACK32),
    finfo!(RGBA_102LE, A2B10G10R10, SNORM, _PACK32),
    finfo!(RGBA_102LE, A2B10G10R10, SSCALED, _PACK32),
    finfo!(RGBA_102LE, A2B10G10R10, UINT, _PACK32),
    finfo!(RGBA_102LE, A2B10G10R10, UNORM, _PACK32),
    finfo!(RGBA_102LE, A2B10G10R10, USCALED, _PACK32),
    // BGRA_102LE
    finfo!(RGBA_102LE, A2R10G10B10, SINT, _PACK32),
    finfo!(RGBA_102LE, A2R10G10B10, SNORM, _PACK32),
    finfo!(RGBA_102LE, A2R10G10B10, SSCALED, _PACK32),
    finfo!(RGBA_102LE, A2R10G10B10, UINT, _PACK32),
    finfo!(RGBA_102LE, A2R10G10B10, UNORM, _PACK32),
    finfo!(RGBA_102LE, A2R10G10B10, USCALED, _PACK32),
    // GRAY16
    finfo!(GRAY16_LE, R16, SINT),
    finfo!(GRAY16_LE, R16, SNORM),
    finfo!(GRAY16_LE, R16, SSCALED),
    finfo!(GRAY16_LE, R16, UINT),
    finfo!(GRAY16_LE, R16, UNORM),
    finfo!(GRAY16_LE, R16, USCALED),
    finfo!(GRAY16_BE, R16, SINT),
    finfo!(GRAY16_BE, R16, SNORM),
    finfo!(GRAY16_BE, R16, SSCALED),
    finfo!(GRAY16_BE, R16, UINT),
    finfo!(GRAY16_BE, R16, UNORM),
    finfo!(GRAY16_BE, R16, USCALED),
    // GRAY8
    finfo!(GRAY8, R8, SINT),
    finfo!(GRAY8, R8, SNORM),
    finfo!(GRAY8, R8, SRGB),
    finfo!(GRAY8, R8, SSCALED),
    finfo!(GRAY8, R8, UINT),
    finfo!(GRAY8, R8, UNORM),
    finfo!(GRAY8, R8, USCALED),
];

pub fn vk_format_get_transfer(vk_format: vk::Format) -> Transfer {
    let format_name = format!("{:?}", vk_format);
    if format_name.contains("_SRGB") {
        return Transfer::SRGB;
    }
    if format_name.contains("_UNORM") {
        return Transfer::UNORM;
    }
    if format_name.contains("_UINT") {
        return Transfer::UINT;
    }
    if format_name.contains("_USCALED") {
        return Transfer::USCALED;
    }
    if format_name.contains("_SNORM") {
        return Transfer::SNORM;
    }
    if format_name.contains("_SINT") {
        return Transfer::SINT;
    }
    if format_name.contains("_SSCALED") {
        return Transfer::SSCALED;
    }
    Transfer::UNKNOWN
}

pub fn vk_format_get_info(vk_format: vk::Format) -> VkFormatInfo {
    for info in VK_FORMAT_INFO_TABLE {
        if info.vk_format == vk_format {
            return *info;
        }
    }
    VkFormatInfo {
        format: Format::UNKNOWN,
        transfer: vk_format_get_transfer(vk_format),
        vk_format,
    }
}

pub fn client_format_get_info(format: Format, transfer: Transfer) -> VkFormatInfo {
    for info in VK_FORMAT_INFO_TABLE {
        if info.format == format && info.transfer == transfer {
            return *info;
        }
    }
    VkFormatInfo {
        format,
        transfer,
        vk_format: vk::Format::UNDEFINED,
    }
}

#[cfg(test)]
mod tests {
    use crate::utils::*;
    use ash::vk;
    use pw_capture_client::Transfer;

    #[test]
    fn get_transfer() {
        let transfer = vk_format_get_transfer(vk::Format::B8G8R8A8_SRGB);
        assert_eq!(Transfer::SRGB, transfer);
        let transfer = vk_format_get_transfer(vk::Format::B8G8R8A8_UNORM);
        assert_eq!(Transfer::UNORM, transfer);
        let transfer = vk_format_get_transfer(vk::Format::B8G8R8A8_UINT);
        assert_eq!(Transfer::UINT, transfer);
        let transfer = vk_format_get_transfer(vk::Format::B8G8R8A8_USCALED);
        assert_eq!(Transfer::USCALED, transfer);
        let transfer = vk_format_get_transfer(vk::Format::B8G8R8A8_SNORM);
        assert_eq!(Transfer::SNORM, transfer);
        let transfer = vk_format_get_transfer(vk::Format::B8G8R8A8_SINT);
        assert_eq!(Transfer::SINT, transfer);
        let transfer = vk_format_get_transfer(vk::Format::B8G8R8A8_SSCALED);
        assert_eq!(Transfer::SSCALED, transfer);
        let transfer =
            vk_format_get_transfer(vk::Format::G12X4_B12X4_R12X4_3PLANE_420_UNORM_3PACK16);
        assert_eq!(Transfer::UNORM, transfer);
    }
}
