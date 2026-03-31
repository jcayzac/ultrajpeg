use std::borrow::Cow;

use crate::{
    codec::{decode_primary_image, encode_image},
    container::assemble_container,
    decode_hdr_output,
    error::{Error, Result},
    icc, inspect,
    metadata::build_ultra_hdr_metadata,
    types::{ChromaSubsampling, ColorMetadata},
};
use ultrahdr_core::{
    ColorGamut as CoreColorGamut, ColorTransfer as CoreColorTransfer, GainMapConfig,
    GainMapMetadata, PixelFormat, RawImage as CoreRawImage, Unstoppable,
    gainmap::{HdrOutputFormat, compute_gainmap},
};

pub type ColorGamut = sys::uhdr_color_gamut::Type;
pub type ColorTransfer = sys::uhdr_color_transfer::Type;
pub type ColorRange = sys::uhdr_color_range::Type;
pub type ImageFormat = sys::uhdr_img_fmt::Type;
pub type Codec = sys::uhdr_codec::Type;

#[allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
pub mod sys {
    pub mod uhdr_img_fmt {
        pub type Type = i32;
        pub const UHDR_IMG_FMT_32bppRGBA1010102: Type = 0;
    }

    pub mod uhdr_color_gamut {
        pub type Type = i32;
        pub const UHDR_CG_UNSPECIFIED: Type = 0;
        pub const UHDR_CG_BT_709: Type = 1;
        pub const UHDR_CG_DISPLAY_P3: Type = 2;
        pub const UHDR_CG_BT_2100: Type = 3;
    }

    pub mod uhdr_color_transfer {
        pub type Type = i32;
        pub const UHDR_CT_UNSPECIFIED: Type = 0;
        pub const UHDR_CT_LINEAR: Type = 1;
        pub const UHDR_CT_HLG: Type = 2;
        pub const UHDR_CT_PQ: Type = 3;
        pub const UHDR_CT_SRGB: Type = 4;
    }

    pub mod uhdr_color_range {
        pub type Type = i32;
        pub const UHDR_CR_UNSPECIFIED: Type = 0;
        pub const UHDR_CR_FULL_RANGE: Type = 1;
    }

    pub mod uhdr_codec {
        pub type Type = i32;
        pub const UHDR_CODEC_JPG: Type = 0;
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CompressedImage<'a> {
    bytes: Cow<'a, [u8]>,
    color_gamut: ColorGamut,
    color_transfer: ColorTransfer,
    color_range: ColorRange,
}

impl<'a> CompressedImage<'a> {
    /// Borrow an immutable JPEG buffer without copying it.
    #[must_use]
    pub fn from_slice(
        bytes: &'a [u8],
        color_gamut: ColorGamut,
        color_transfer: ColorTransfer,
        color_range: ColorRange,
    ) -> Self {
        Self {
            bytes: Cow::Borrowed(bytes),
            color_gamut,
            color_transfer,
            color_range,
        }
    }

    #[must_use]
    pub fn from_bytes(
        bytes: &'a mut [u8],
        color_gamut: ColorGamut,
        color_transfer: ColorTransfer,
        color_range: ColorRange,
    ) -> Self {
        Self::from_slice(bytes, color_gamut, color_transfer, color_range)
    }

    /// Build an owned compressed image without borrowing the caller's buffer.
    #[must_use]
    pub fn from_vec(
        bytes: Vec<u8>,
        color_gamut: ColorGamut,
        color_transfer: ColorTransfer,
        color_range: ColorRange,
    ) -> Self {
        Self {
            bytes: Cow::Owned(bytes),
            color_gamut,
            color_transfer,
            color_range,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RawImage<'a> {
    format: ImageFormat,
    width: u32,
    height: u32,
    data: Cow<'a, [u8]>,
    color_gamut: ColorGamut,
    color_transfer: ColorTransfer,
    color_range: ColorRange,
}

impl<'a> RawImage<'a> {
    pub fn packed(
        format: ImageFormat,
        width: u32,
        height: u32,
        bytes: &'a mut [u8],
        color_gamut: ColorGamut,
        color_transfer: ColorTransfer,
        color_range: ColorRange,
    ) -> Result<Self> {
        if format != sys::uhdr_img_fmt::UHDR_IMG_FMT_32bppRGBA1010102 {
            return Err(Error::UnsupportedFormat(
                "only packed RGBA1010102 raw images are supported",
            ));
        }

        let expected_len = width
            .checked_mul(height)
            .and_then(|pixel_count| pixel_count.checked_mul(4))
            .ok_or_else(|| Error::InvalidInput("raw image dimensions overflow".into()))?
            as usize;
        if bytes.len() != expected_len {
            return Err(Error::InvalidInput(format!(
                "raw image byte length mismatch: expected {expected_len}, got {}",
                bytes.len()
            )));
        }

        Ok(Self {
            format,
            width,
            height,
            data: Cow::Borrowed(bytes),
            color_gamut,
            color_transfer,
            color_range,
        })
    }

    /// Build an owned packed image without borrowing the caller's buffer.
    pub fn packed_owned(
        format: ImageFormat,
        width: u32,
        height: u32,
        bytes: Vec<u8>,
        color_gamut: ColorGamut,
        color_transfer: ColorTransfer,
        color_range: ColorRange,
    ) -> Result<Self> {
        let expected_len = width
            .checked_mul(height)
            .and_then(|pixel_count| pixel_count.checked_mul(4))
            .ok_or_else(|| Error::InvalidInput("raw image dimensions overflow".into()))?
            as usize;
        if bytes.len() != expected_len {
            return Err(Error::InvalidInput(format!(
                "raw image byte length mismatch: expected {expected_len}, got {}",
                bytes.len()
            )));
        }
        if format != sys::uhdr_img_fmt::UHDR_IMG_FMT_32bppRGBA1010102 {
            return Err(Error::UnsupportedFormat(
                "only packed RGBA1010102 raw images are supported",
            ));
        }

        Ok(Self {
            format,
            width,
            height,
            data: Cow::Owned(bytes),
            color_gamut,
            color_transfer,
            color_range,
        })
    }
}

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImgLabel {
    UHDR_HDR_IMG,
    UHDR_SDR_IMG,
    UHDR_BASE_IMG,
    UHDR_GAIN_MAP_IMG,
}

#[derive(Debug, Clone)]
pub struct EncodedStream {
    bytes: Vec<u8>,
}

impl EncodedStream {
    pub fn bytes(&self) -> Result<&[u8]> {
        Ok(&self.bytes)
    }
}

#[derive(Debug, Clone)]
pub struct DecodedPacked {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    color_gamut: ColorGamut,
    color_transfer: ColorTransfer,
    color_range: ColorRange,
}

impl DecodedPacked {
    pub fn to_owned(&self) -> Result<Self> {
        Ok(self.clone())
    }

    #[must_use]
    pub fn meta(&self) -> (ColorGamut, ColorTransfer, ColorRange) {
        (self.color_gamut, self.color_transfer, self.color_range)
    }
}

#[derive(Debug, Default, Clone)]
pub struct Encoder<'a> {
    hdr_raw: Option<RawImage<'a>>,
    sdr_image: Option<CompressedImage<'a>>,
    base_quality: u8,
    gain_map_quality: u8,
    output_format: Codec,
    encoded: Option<EncodedStream>,
}

impl<'a> Encoder<'a> {
    pub fn new() -> Result<Self> {
        Ok(Self {
            base_quality: 90,
            gain_map_quality: 90,
            output_format: sys::uhdr_codec::UHDR_CODEC_JPG,
            ..Self::default()
        })
    }

    pub fn set_raw_image(
        &mut self,
        image: &mut RawImage<'a>,
        label: ImgLabel,
    ) -> Result<&mut Self> {
        self.set_raw_image_owned(image.clone(), label)
    }

    /// Move a packed HDR image into the encoder without cloning its buffer.
    pub fn set_raw_image_owned(
        &mut self,
        image: RawImage<'a>,
        label: ImgLabel,
    ) -> Result<&mut Self> {
        if label != ImgLabel::UHDR_HDR_IMG {
            return Err(Error::InvalidInput(format!(
                "unsupported raw image label {label:?}"
            )));
        }
        self.hdr_raw = Some(image);
        Ok(self)
    }

    pub fn set_compressed_image(
        &mut self,
        image: &mut CompressedImage<'a>,
        label: ImgLabel,
    ) -> Result<&mut Self> {
        self.set_compressed_image_owned(image.clone(), label)
    }

    /// Move a compressed SDR image into the encoder without cloning its buffer.
    pub fn set_compressed_image_owned(
        &mut self,
        image: CompressedImage<'a>,
        label: ImgLabel,
    ) -> Result<&mut Self> {
        if label != ImgLabel::UHDR_SDR_IMG {
            return Err(Error::InvalidInput(format!(
                "unsupported compressed image label {label:?}"
            )));
        }
        self.sdr_image = Some(image);
        Ok(self)
    }

    pub fn set_quality(&mut self, quality: i32, label: ImgLabel) -> Result<&mut Self> {
        let quality = quality.clamp(1, 100) as u8;
        match label {
            ImgLabel::UHDR_BASE_IMG => self.base_quality = quality,
            ImgLabel::UHDR_GAIN_MAP_IMG => self.gain_map_quality = quality,
            other => {
                return Err(Error::InvalidInput(format!(
                    "quality cannot be assigned to {other:?}"
                )));
            }
        }
        Ok(self)
    }

    pub fn set_output_format(&mut self, output_format: Codec) -> Result<&mut Self> {
        if output_format != sys::uhdr_codec::UHDR_CODEC_JPG {
            return Err(Error::UnsupportedFormat(
                "only UltraHDR JPEG output is supported",
            ));
        }
        self.output_format = output_format;
        Ok(self)
    }

    /// Encode an Ultra HDR JPEG from the configured SDR base JPEG and HDR input.
    ///
    /// ICC handling for the primary JPEG is explicit:
    ///
    /// - if the SDR base JPEG already contains an ICC profile, it is preserved
    /// - if the SDR base JPEG has no ICC profile and the HDR input gamut is
    ///   [`sys::uhdr_color_gamut::UHDR_CG_DISPLAY_P3`], the crate injects its
    ///   built-in Display-P3 ICC profile automatically
    /// - for other HDR input gamuts, no ICC profile is auto-injected
    pub fn encode(&mut self) -> Result<()> {
        if self.output_format != sys::uhdr_codec::UHDR_CODEC_JPG {
            return Err(Error::UnsupportedFormat(
                "only UltraHDR JPEG output is supported",
            ));
        }

        let hdr_raw = self
            .hdr_raw
            .as_ref()
            .ok_or_else(|| Error::InvalidInput("missing HDR raw image".into()))?;
        let sdr = self
            .sdr_image
            .as_ref()
            .ok_or_else(|| Error::InvalidInput("missing SDR compressed image".into()))?;

        let hdr_core = compat_raw_to_core(hdr_raw)?;
        let sdr_core = decode_primary_image(sdr.bytes.as_ref())?;
        let (gain_map, metadata) =
            compute_gainmap(&hdr_core, &sdr_core, &GainMapConfig::default(), Unstoppable)?;
        let gain_map_core = CoreRawImage::from_data(
            gain_map.width,
            gain_map.height,
            PixelFormat::Gray8,
            CoreColorGamut::Bt709,
            CoreColorTransfer::Linear,
            gain_map.data.clone(),
        )?;
        let gain_map_jpeg = encode_image(
            &gain_map_core,
            self.gain_map_quality,
            false,
            ChromaSubsampling::Yuv444,
            &ColorMetadata::default(),
        )?;
        let ultra_hdr_metadata = build_ultra_hdr_metadata(&metadata, gain_map_jpeg.len());
        let primary_icc_profile = inspect(sdr.bytes.as_ref())?
            .color_metadata
            .icc_profile
            .or_else(|| {
                (hdr_raw.color_gamut == sys::uhdr_color_gamut::UHDR_CG_DISPLAY_P3)
                    .then(|| icc::display_p3().to_vec())
            });
        let bytes = assemble_container(
            sdr.bytes.as_ref(),
            Some(&gain_map_jpeg),
            &ColorMetadata {
                icc_profile: primary_icc_profile,
                exif: None,
                gamut: Some(core_gamut_from_compat(hdr_raw.color_gamut)?),
                transfer: Some(core_transfer_from_compat(hdr_raw.color_transfer)?),
            },
            Some(&ultra_hdr_metadata),
        )?;
        self.encoded = Some(EncodedStream { bytes });
        Ok(())
    }

    pub fn encoded_stream(&self) -> Option<&EncodedStream> {
        self.encoded.as_ref()
    }
}

#[derive(Debug, Default, Clone)]
pub struct Decoder<'a> {
    image: Option<CompressedImage<'a>>,
}

impl<'a> Decoder<'a> {
    pub fn new() -> Result<Self> {
        Ok(Self::default())
    }

    pub fn set_image(&mut self, image: &mut CompressedImage<'a>) -> Result<&mut Self> {
        self.set_image_owned(image.clone())
    }

    /// Move a compressed image into the decoder without cloning its buffer.
    pub fn set_image_owned(&mut self, image: CompressedImage<'a>) -> Result<&mut Self> {
        self.image = Some(image);
        Ok(self)
    }

    /// Borrow an immutable JPEG buffer directly for compatibility decoding.
    pub fn set_image_slice(
        &mut self,
        bytes: &'a [u8],
        color_gamut: ColorGamut,
        color_transfer: ColorTransfer,
        color_range: ColorRange,
    ) -> Result<&mut Self> {
        self.image = Some(CompressedImage::from_slice(
            bytes,
            color_gamut,
            color_transfer,
            color_range,
        ));
        Ok(self)
    }

    pub fn gainmap_metadata(&mut self) -> Result<Option<GainMapMetadata>> {
        let image = self
            .image
            .as_ref()
            .ok_or_else(|| Error::InvalidInput("missing compressed image".into()))?;
        let inspected = inspect(image.bytes.as_ref())?;
        Ok(inspected
            .ultra_hdr
            .and_then(|metadata| metadata.gain_map_metadata))
    }

    pub fn decode_packed_view(
        &mut self,
        format: ImageFormat,
        transfer: ColorTransfer,
    ) -> Result<DecodedPacked> {
        if format != sys::uhdr_img_fmt::UHDR_IMG_FMT_32bppRGBA1010102 {
            return Err(Error::UnsupportedFormat(
                "only packed RGBA1010102 output is supported",
            ));
        }

        let image = self
            .image
            .as_ref()
            .ok_or_else(|| Error::InvalidInput("missing compressed image".into()))?;
        let output = match transfer {
            sys::uhdr_color_transfer::UHDR_CT_PQ => {
                decode_hdr_output(image.bytes.as_ref(), HdrOutputFormat::Pq1010102)?
            }
            _ => {
                return Err(Error::UnsupportedFormat(
                    "only PQ packed decode output is supported",
                ));
            }
        };
        let (output, color_metadata) = output;

        Ok(DecodedPacked {
            data: output.data,
            width: output.width,
            height: output.height,
            color_gamut: color_metadata
                .gamut
                .map(compat_gamut_from_core)
                .unwrap_or(image.color_gamut),
            color_transfer: compat_transfer_from_core(output.transfer),
            color_range: sys::uhdr_color_range::UHDR_CR_FULL_RANGE,
        })
    }
}

pub mod jpeg {
    use crate::{
        Result,
        codec::encode_image,
        types::{ChromaSubsampling, ColorMetadata},
    };
    use ultrahdr_core::{ColorGamut, ColorTransfer, PixelFormat, RawImage};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Preset {
        ProgressiveSmallest,
        ProgressiveBalanced,
        BaselineBalanced,
        BaselineFastest,
    }

    #[derive(Debug, Clone)]
    pub struct Encoder {
        preset: Preset,
        quality: u8,
        icc_profile: Option<Vec<u8>>,
    }

    impl Encoder {
        #[must_use]
        pub fn new(preset: Preset) -> Self {
            Self {
                preset,
                quality: 75,
                icc_profile: None,
            }
        }

        #[must_use]
        pub fn quality(mut self, quality: u8) -> Self {
            self.quality = quality;
            self
        }

        #[must_use]
        pub fn icc_profile(mut self, icc_profile: Vec<u8>) -> Self {
            self.icc_profile = if icc_profile.is_empty() {
                None
            } else {
                Some(icc_profile)
            };
            self
        }

        pub fn encode_rgb(&self, bytes: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
            let image = RawImage::from_data(
                width,
                height,
                PixelFormat::Rgb8,
                ColorGamut::Bt709,
                ColorTransfer::Srgb,
                bytes.to_vec(),
            )?;
            encode_image(
                &image,
                self.quality,
                self.preset != Preset::BaselineBalanced && self.preset != Preset::BaselineFastest,
                ChromaSubsampling::Yuv420,
                &ColorMetadata {
                    icc_profile: self.icc_profile.clone(),
                    ..ColorMetadata::default()
                },
            )
        }
    }
}

pub mod mozjpeg {
    pub use super::jpeg::{Encoder, Preset};
}

fn compat_raw_to_core(raw: &RawImage<'_>) -> Result<CoreRawImage> {
    Ok(CoreRawImage::from_data(
        raw.width,
        raw.height,
        match raw.format {
            sys::uhdr_img_fmt::UHDR_IMG_FMT_32bppRGBA1010102 => match raw.color_transfer {
                sys::uhdr_color_transfer::UHDR_CT_PQ => PixelFormat::Rgba1010102Pq,
                sys::uhdr_color_transfer::UHDR_CT_HLG => PixelFormat::Rgba1010102Hlg,
                _ => {
                    return Err(Error::UnsupportedFormat(
                        "unsupported transfer for packed RGBA1010102 input",
                    ));
                }
            },
            _ => {
                return Err(Error::UnsupportedFormat("unsupported raw image format"));
            }
        },
        core_gamut_from_compat(raw.color_gamut)?,
        core_transfer_from_compat(raw.color_transfer)?,
        raw.data.as_ref().to_vec(),
    )?)
}

fn core_gamut_from_compat(gamut: ColorGamut) -> Result<CoreColorGamut> {
    match gamut {
        sys::uhdr_color_gamut::UHDR_CG_UNSPECIFIED | sys::uhdr_color_gamut::UHDR_CG_BT_709 => {
            Ok(CoreColorGamut::Bt709)
        }
        sys::uhdr_color_gamut::UHDR_CG_DISPLAY_P3 => Ok(CoreColorGamut::DisplayP3),
        sys::uhdr_color_gamut::UHDR_CG_BT_2100 => Ok(CoreColorGamut::Bt2100),
        _ => Err(Error::InvalidInput(format!("unknown color gamut {gamut}"))),
    }
}

fn core_transfer_from_compat(transfer: ColorTransfer) -> Result<CoreColorTransfer> {
    match transfer {
        sys::uhdr_color_transfer::UHDR_CT_UNSPECIFIED | sys::uhdr_color_transfer::UHDR_CT_SRGB => {
            Ok(CoreColorTransfer::Srgb)
        }
        sys::uhdr_color_transfer::UHDR_CT_LINEAR => Ok(CoreColorTransfer::Linear),
        sys::uhdr_color_transfer::UHDR_CT_HLG => Ok(CoreColorTransfer::Hlg),
        sys::uhdr_color_transfer::UHDR_CT_PQ => Ok(CoreColorTransfer::Pq),
        _ => Err(Error::InvalidInput(format!(
            "unknown color transfer {transfer}"
        ))),
    }
}

fn compat_gamut_from_core(gamut: CoreColorGamut) -> ColorGamut {
    match gamut {
        CoreColorGamut::Bt709 => sys::uhdr_color_gamut::UHDR_CG_BT_709,
        CoreColorGamut::DisplayP3 => sys::uhdr_color_gamut::UHDR_CG_DISPLAY_P3,
        CoreColorGamut::Bt2100 => sys::uhdr_color_gamut::UHDR_CG_BT_2100,
    }
}

fn compat_transfer_from_core(transfer: CoreColorTransfer) -> ColorTransfer {
    match transfer {
        CoreColorTransfer::Srgb => sys::uhdr_color_transfer::UHDR_CT_SRGB,
        CoreColorTransfer::Linear => sys::uhdr_color_transfer::UHDR_CT_LINEAR,
        CoreColorTransfer::Pq => sys::uhdr_color_transfer::UHDR_CT_PQ,
        CoreColorTransfer::Hlg => sys::uhdr_color_transfer::UHDR_CT_HLG,
    }
}
