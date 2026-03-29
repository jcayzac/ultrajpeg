use crate::{Error, Result};
use ultrahdr_core::{
    ColorGamut, ColorTransfer, GainMap, GainMapMetadata, RawImage, Unstoppable,
    gainmap::{HdrOutputFormat, apply_gainmap},
};

/// Chroma subsampling modes exposed by the public API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChromaSubsampling {
    #[default]
    Yuv420,
    Yuv422,
    Yuv444,
    Yuv440,
}

/// JPEG color-related metadata handled by the crate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColorMetadata {
    pub icc_profile: Option<Vec<u8>>,
    pub exif: Option<Vec<u8>>,
    pub gamut: Option<ColorGamut>,
    pub transfer: Option<ColorTransfer>,
}

/// Structured UltraHDR metadata extracted from the container.
#[derive(Debug, Clone, Default)]
pub struct UltraHdrMetadata {
    pub xmp: Option<String>,
    pub iso_21496_1: Option<Vec<u8>>,
    pub gain_map_metadata: Option<GainMapMetadata>,
}

/// Decoded gain-map payload and associated metadata.
#[derive(Debug, Clone)]
pub struct DecodedGainMap {
    pub image: RawImage,
    pub gain_map: GainMap,
    pub jpeg_bytes: Vec<u8>,
    pub metadata: Option<GainMapMetadata>,
}

/// Fully decoded JPEG/UltraHDR image.
#[derive(Debug, Clone)]
pub struct DecodedJpeg {
    pub primary_image: RawImage,
    pub primary_jpeg: Vec<u8>,
    pub color_metadata: ColorMetadata,
    pub ultra_hdr: Option<UltraHdrMetadata>,
    pub gain_map: Option<DecodedGainMap>,
}

/// Metadata-only JPEG/UltraHDR inspection result.
#[derive(Debug, Clone)]
pub struct InspectedJpeg {
    pub primary_jpeg_len: usize,
    pub gain_map_jpeg_len: Option<usize>,
    pub color_metadata: ColorMetadata,
    pub ultra_hdr: Option<UltraHdrMetadata>,
}

/// Decode configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeOptions {
    pub decode_gain_map: bool,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self {
            decode_gain_map: true,
        }
    }
}

/// Gain-map specific encoding configuration.
#[derive(Debug, Clone)]
pub struct GainMapEncodeOptions {
    pub image: RawImage,
    pub metadata: GainMapMetadata,
    pub quality: u8,
    pub progressive: bool,
}

/// Encode configuration for the primary image and optional bundled gain map.
#[derive(Debug, Clone)]
pub struct EncodeOptions {
    pub quality: u8,
    pub progressive: bool,
    pub chroma_subsampling: ChromaSubsampling,
    pub color_metadata: ColorMetadata,
    pub gain_map: Option<GainMapEncodeOptions>,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            quality: 90,
            progressive: true,
            chroma_subsampling: ChromaSubsampling::Yuv420,
            color_metadata: ColorMetadata::default(),
            gain_map: None,
        }
    }
}

/// Stateful encoder wrapper.
#[derive(Debug, Clone)]
pub struct UltraJpegEncoder {
    pub(crate) options: EncodeOptions,
}

impl DecodedJpeg {
    pub(crate) fn reconstruct_hdr_with(
        &self,
        display_boost: f32,
        output_format: HdrOutputFormat,
    ) -> Result<RawImage> {
        let gain_map = self.gain_map.as_ref().ok_or(Error::MissingGainMap)?;
        let metadata = gain_map
            .metadata
            .as_ref()
            .or_else(|| {
                self.ultra_hdr
                    .as_ref()
                    .and_then(|metadata| metadata.gain_map_metadata.as_ref())
            })
            .ok_or(Error::MissingGainMapMetadata)?;

        apply_gainmap(
            &self.primary_image,
            &gain_map.gain_map,
            metadata,
            display_boost,
            output_format,
            Unstoppable,
        )
        .map_err(Into::into)
    }
}
