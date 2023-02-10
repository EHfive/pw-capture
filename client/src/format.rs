use num_enum::{FromPrimitive, IntoPrimitive};

pub struct FormatDescriptor(pub Format, pub Transfer);

#[derive(Clone, Copy, Debug, PartialEq, Eq, IntoPrimitive, FromPrimitive)]
#[allow(non_camel_case_types)]
#[repr(u32)]
pub enum Format {
    // enum value/order must be in sync with `enum spa_video_format`
    #[num_enum(default)]
    UNKNOWN,
    ENCODED,

    I420,
    YV12,
    YUY2,
    UYVY,
    AYUV,
    RGBx,
    BGRx,
    xRGB,
    xBGR,
    RGBA,
    BGRA,
    ARGB,
    ABGR,
    RGB,
    BGR,
    Y41B,
    Y42B,
    YVYU,
    Y444,
    v210,
    v216,
    NV12,
    NV21,
    GRAY8,
    GRAY16_BE,
    GRAY16_LE,
    v308,
    RGB16,
    BGR16,
    RGB15,
    BGR15,
    UYVP,
    A420,
    RGB8P,
    YUV9,
    YVU9,
    IYU1,
    ARGB64,
    AYUV64,
    r210,
    I420_10BE,
    I420_10LE,
    I422_10BE,
    I422_10LE,
    Y444_10BE,
    Y444_10LE,
    GBR,
    GBR_10BE,
    GBR_10LE,
    NV16,
    NV24,
    NV12_64Z32,
    A420_10BE,
    A420_10LE,
    A422_10BE,
    A422_10LE,
    A444_10BE,
    A444_10LE,
    NV61,
    P010_10BE,
    P010_10LE,
    IYU2,
    VYUY,
    GBRA,
    GBRA_10BE,
    GBRA_10LE,
    GBR_12BE,
    GBR_12LE,
    GBRA_12BE,
    GBRA_12LE,
    I420_12BE,
    I420_12LE,
    I422_12BE,
    I422_12LE,
    Y444_12BE,
    Y444_12LE,

    RGBA_F16,
    RGBA_F32,

    xRGB_210LE, // 32-bit x:R:G:B 2:10:10:10 little endian
    xBGR_210LE, // 32-bit x:B:G:R 2:10:10:10 little endian
    RGBx_102LE, // 32-bit R:G:B:x 10:10:10:2 little endian
    BGRx_102LE, // 32-bit B:G:R:x 10:10:10:2 little endian
    ARGB_210LE, // 32-bit A:R:G:B 2:10:10:10 little endian
    ABGR_210LE, // 32-bit A:B:G:R 2:10:10:10 little endian
    RGBA_102LE, // 32-bit R:G:B:A 10:10:10:2 little endian
    BGRA_102LE, // 32-bit B:G:R:A 10:10:10:2 little endian
}

impl Default for Format {
    fn default() -> Self {
        Format::UNKNOWN
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transfer {
    UNKNOWN,
    SRGB,
    UNORM,
    SNORM,
    UINT,
    SINT,
    USCALED,
    SSCALED,
    UFLOAT,
    SFLOAT,
}

#[cfg(test)]
mod tests {
    use crate::Format;
    use libspa_sys::*;

    #[test]
    fn format_value() {
        assert_eq!(SPA_VIDEO_FORMAT_UNKNOWN, Format::UNKNOWN.into());
        assert_eq!(SPA_VIDEO_FORMAT_ENCODED, Format::ENCODED.into());
        assert_eq!(SPA_VIDEO_FORMAT_I420, Format::I420.into());
        assert_eq!(SPA_VIDEO_FORMAT_YV12, Format::YV12.into());
        assert_eq!(SPA_VIDEO_FORMAT_YUY2, Format::YUY2.into());
        assert_eq!(SPA_VIDEO_FORMAT_UYVY, Format::UYVY.into());
        assert_eq!(SPA_VIDEO_FORMAT_AYUV, Format::AYUV.into());
        assert_eq!(SPA_VIDEO_FORMAT_RGBx, Format::RGBx.into());
        assert_eq!(SPA_VIDEO_FORMAT_BGRx, Format::BGRx.into());
        assert_eq!(SPA_VIDEO_FORMAT_xRGB, Format::xRGB.into());
        assert_eq!(SPA_VIDEO_FORMAT_xBGR, Format::xBGR.into());
        assert_eq!(SPA_VIDEO_FORMAT_RGBA, Format::RGBA.into());
        assert_eq!(SPA_VIDEO_FORMAT_BGRA, Format::BGRA.into());
        assert_eq!(SPA_VIDEO_FORMAT_ARGB, Format::ARGB.into());
        assert_eq!(SPA_VIDEO_FORMAT_ABGR, Format::ABGR.into());
        assert_eq!(SPA_VIDEO_FORMAT_RGB, Format::RGB.into());
        assert_eq!(SPA_VIDEO_FORMAT_BGR, Format::BGR.into());
        assert_eq!(SPA_VIDEO_FORMAT_Y41B, Format::Y41B.into());
        assert_eq!(SPA_VIDEO_FORMAT_Y42B, Format::Y42B.into());
        assert_eq!(SPA_VIDEO_FORMAT_YVYU, Format::YVYU.into());
        assert_eq!(SPA_VIDEO_FORMAT_Y444, Format::Y444.into());
        assert_eq!(SPA_VIDEO_FORMAT_v210, Format::v210.into());
        assert_eq!(SPA_VIDEO_FORMAT_v216, Format::v216.into());
        assert_eq!(SPA_VIDEO_FORMAT_NV12, Format::NV12.into());
        assert_eq!(SPA_VIDEO_FORMAT_NV21, Format::NV21.into());
        assert_eq!(SPA_VIDEO_FORMAT_GRAY8, Format::GRAY8.into());
        assert_eq!(SPA_VIDEO_FORMAT_GRAY16_BE, Format::GRAY16_BE.into());
        assert_eq!(SPA_VIDEO_FORMAT_GRAY16_LE, Format::GRAY16_LE.into());
        assert_eq!(SPA_VIDEO_FORMAT_v308, Format::v308.into());
        assert_eq!(SPA_VIDEO_FORMAT_RGB16, Format::RGB16.into());
        assert_eq!(SPA_VIDEO_FORMAT_BGR16, Format::BGR16.into());
        assert_eq!(SPA_VIDEO_FORMAT_RGB15, Format::RGB15.into());
        assert_eq!(SPA_VIDEO_FORMAT_BGR15, Format::BGR15.into());
        assert_eq!(SPA_VIDEO_FORMAT_UYVP, Format::UYVP.into());
        assert_eq!(SPA_VIDEO_FORMAT_A420, Format::A420.into());
        assert_eq!(SPA_VIDEO_FORMAT_RGB8P, Format::RGB8P.into());
        assert_eq!(SPA_VIDEO_FORMAT_YUV9, Format::YUV9.into());
        assert_eq!(SPA_VIDEO_FORMAT_YVU9, Format::YVU9.into());
        assert_eq!(SPA_VIDEO_FORMAT_IYU1, Format::IYU1.into());
        assert_eq!(SPA_VIDEO_FORMAT_ARGB64, Format::ARGB64.into());
        assert_eq!(SPA_VIDEO_FORMAT_AYUV64, Format::AYUV64.into());
        assert_eq!(SPA_VIDEO_FORMAT_r210, Format::r210.into());
        assert_eq!(SPA_VIDEO_FORMAT_I420_10BE, Format::I420_10BE.into());
        assert_eq!(SPA_VIDEO_FORMAT_I420_10LE, Format::I420_10LE.into());
        assert_eq!(SPA_VIDEO_FORMAT_I422_10BE, Format::I422_10BE.into());
        assert_eq!(SPA_VIDEO_FORMAT_I422_10LE, Format::I422_10LE.into());
        assert_eq!(SPA_VIDEO_FORMAT_Y444_10BE, Format::Y444_10BE.into());
        assert_eq!(SPA_VIDEO_FORMAT_Y444_10LE, Format::Y444_10LE.into());
        assert_eq!(SPA_VIDEO_FORMAT_GBR, Format::GBR.into());
        assert_eq!(SPA_VIDEO_FORMAT_GBR_10BE, Format::GBR_10BE.into());
        assert_eq!(SPA_VIDEO_FORMAT_GBR_10LE, Format::GBR_10LE.into());
        assert_eq!(SPA_VIDEO_FORMAT_NV16, Format::NV16.into());
        assert_eq!(SPA_VIDEO_FORMAT_NV24, Format::NV24.into());
        assert_eq!(SPA_VIDEO_FORMAT_NV12_64Z32, Format::NV12_64Z32.into());
        assert_eq!(SPA_VIDEO_FORMAT_A420_10BE, Format::A420_10BE.into());
        assert_eq!(SPA_VIDEO_FORMAT_A420_10LE, Format::A420_10LE.into());
        assert_eq!(SPA_VIDEO_FORMAT_A422_10BE, Format::A422_10BE.into());
        assert_eq!(SPA_VIDEO_FORMAT_A422_10LE, Format::A422_10LE.into());
        assert_eq!(SPA_VIDEO_FORMAT_A444_10BE, Format::A444_10BE.into());
        assert_eq!(SPA_VIDEO_FORMAT_A444_10LE, Format::A444_10LE.into());
        assert_eq!(SPA_VIDEO_FORMAT_NV61, Format::NV61.into());
        assert_eq!(SPA_VIDEO_FORMAT_P010_10BE, Format::P010_10BE.into());
        assert_eq!(SPA_VIDEO_FORMAT_P010_10LE, Format::P010_10LE.into());
        assert_eq!(SPA_VIDEO_FORMAT_IYU2, Format::IYU2.into());
        assert_eq!(SPA_VIDEO_FORMAT_VYUY, Format::VYUY.into());
        assert_eq!(SPA_VIDEO_FORMAT_GBRA, Format::GBRA.into());
        assert_eq!(SPA_VIDEO_FORMAT_GBRA_10BE, Format::GBRA_10BE.into());
        assert_eq!(SPA_VIDEO_FORMAT_GBRA_10LE, Format::GBRA_10LE.into());
        assert_eq!(SPA_VIDEO_FORMAT_GBR_12BE, Format::GBR_12BE.into());
        assert_eq!(SPA_VIDEO_FORMAT_GBR_12LE, Format::GBR_12LE.into());
        assert_eq!(SPA_VIDEO_FORMAT_GBRA_12BE, Format::GBRA_12BE.into());
        assert_eq!(SPA_VIDEO_FORMAT_GBRA_12LE, Format::GBRA_12LE.into());
        assert_eq!(SPA_VIDEO_FORMAT_I420_12BE, Format::I420_12BE.into());
        assert_eq!(SPA_VIDEO_FORMAT_I420_12LE, Format::I420_12LE.into());
        assert_eq!(SPA_VIDEO_FORMAT_I422_12BE, Format::I422_12BE.into());
        assert_eq!(SPA_VIDEO_FORMAT_I422_12LE, Format::I422_12LE.into());
        assert_eq!(SPA_VIDEO_FORMAT_Y444_12BE, Format::Y444_12BE.into());
        assert_eq!(SPA_VIDEO_FORMAT_Y444_12LE, Format::Y444_12LE.into());
        assert_eq!(SPA_VIDEO_FORMAT_RGBA_F16, Format::RGBA_F16.into());
        assert_eq!(SPA_VIDEO_FORMAT_RGBA_F32, Format::RGBA_F32.into());
        assert_eq!(SPA_VIDEO_FORMAT_xRGB_210LE, Format::xRGB_210LE.into());
        assert_eq!(SPA_VIDEO_FORMAT_xBGR_210LE, Format::xBGR_210LE.into());
        assert_eq!(SPA_VIDEO_FORMAT_RGBx_102LE, Format::RGBx_102LE.into());
        assert_eq!(SPA_VIDEO_FORMAT_BGRx_102LE, Format::BGRx_102LE.into());
        assert_eq!(SPA_VIDEO_FORMAT_ARGB_210LE, Format::ARGB_210LE.into());
        assert_eq!(SPA_VIDEO_FORMAT_ABGR_210LE, Format::ABGR_210LE.into());
        assert_eq!(SPA_VIDEO_FORMAT_RGBA_102LE, Format::RGBA_102LE.into());
        assert_eq!(SPA_VIDEO_FORMAT_BGRA_102LE, Format::BGRA_102LE.into());
    }
}
