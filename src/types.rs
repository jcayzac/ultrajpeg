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
    /// ICC profile bytes to embed in the primary JPEG.
    pub icc_profile: Option<Vec<u8>>,
    /// EXIF payload to embed in the primary JPEG.
    pub exif: Option<Vec<u8>>,
    /// Explicit primary-image gamut metadata tracked alongside the JPEG bytes.
    pub gamut: Option<ColorGamut>,
    /// Explicit primary-image transfer metadata tracked alongside the JPEG bytes.
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
    /// Gain-map image pixels to encode as the secondary JPEG.
    pub image: RawImage,
    /// Gain-map metadata to serialize as Ultra HDR XMP and ISO 21496-1 payloads.
    pub metadata: GainMapMetadata,
    /// JPEG quality for the secondary gain-map codestream.
    pub quality: u8,
    /// Whether to emit the secondary gain-map JPEG as progressive.
    pub progressive: bool,
}

/// Gain-map channel layout for computed Ultra HDR metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GainMapChannels {
    /// Compute a single-channel luminance gain map.
    #[default]
    Single,
    /// Compute a multichannel RGB gain map.
    Multi,
}

/// Options for gain-map computation from HDR and SDR primary images.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ComputeGainMapOptions {
    /// Whether the computed gain map should be single-channel or multichannel.
    ///
    /// The default is [`GainMapChannels::Single`].
    pub channels: GainMapChannels,
}

/// Result of computing an Ultra HDR gain map from HDR and SDR images.
#[derive(Debug, Clone)]
pub struct ComputedGainMap {
    /// Gain-map image pixels ready to be JPEG-encoded and bundled.
    pub image: RawImage,
    /// Gain-map metadata ready to be serialized as Ultra HDR XMP and ISO 21496-1.
    pub metadata: GainMapMetadata,
}

/// Encode configuration for the primary image and optional bundled gain map.
#[derive(Debug, Clone)]
pub struct EncodeOptions {
    /// JPEG quality for the primary image.
    pub quality: u8,
    /// Whether to emit the primary JPEG as progressive.
    pub progressive: bool,
    /// Chroma subsampling for the primary image.
    pub chroma_subsampling: ChromaSubsampling,
    /// Color-related metadata to embed in the primary JPEG.
    pub color_metadata: ColorMetadata,
    /// Optional gain-map image and metadata to bundle into an Ultra HDR container.
    pub gain_map: Option<GainMapEncodeOptions>,
}

/// High-level convenience options for direct Ultra HDR packaging from HDR and SDR inputs.
#[derive(Debug, Clone)]
pub struct UltraHdrEncodeOptions {
    /// Primary JPEG encoding options. `gain_map` must be `None`.
    pub primary: EncodeOptions,
    /// Gain-map computation policy.
    pub gain_map: ComputeGainMapOptions,
    /// JPEG quality for the computed secondary gain-map codestream.
    pub gain_map_quality: u8,
    /// Whether to emit the computed secondary gain-map JPEG as progressive.
    pub gain_map_progressive: bool,
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

impl Default for UltraHdrEncodeOptions {
    fn default() -> Self {
        Self {
            primary: EncodeOptions::ultra_hdr_defaults(),
            gain_map: ComputeGainMapOptions::default(),
            gain_map_quality: 90,
            gain_map_progressive: false,
        }
    }
}

impl ColorMetadata {
    /// Build color metadata for a Display-P3 primary JPEG.
    ///
    /// The returned metadata:
    ///
    /// - embeds the crate's built-in Display-P3 ICC profile
    /// - sets [`ColorMetadata::gamut`] to [`ColorGamut::DisplayP3`]
    /// - sets [`ColorMetadata::transfer`] to [`ColorTransfer::Srgb`]
    #[must_use]
    pub fn display_p3() -> Self {
        Self {
            icc_profile: Some(crate::icc::display_p3().to_vec()),
            exif: None,
            gamut: Some(ColorGamut::DisplayP3),
            transfer: Some(ColorTransfer::Srgb),
        }
    }
}

impl EncodeOptions {
    /// Build default encoder options for an Ultra HDR primary image.
    ///
    /// This keeps the crate's regular JPEG defaults and preconfigures
    /// [`EncodeOptions::color_metadata`] with [`ColorMetadata::display_p3()`].
    ///
    /// In other words, the returned options already include:
    ///
    /// - the built-in Display-P3 ICC profile
    /// - [`ColorGamut::DisplayP3`] as the explicit primary-image gamut
    /// - [`ColorTransfer::Srgb`] as the explicit primary-image transfer
    ///
    /// Use it as a struct update base when bundling a gain map:
    ///
    /// ```rust
    /// # use ultrahdr_core::{GainMapMetadata, PixelFormat, RawImage};
    /// # use ultrajpeg::{EncodeOptions, GainMapEncodeOptions};
    /// let gain_map = RawImage::new(8, 8, PixelFormat::Gray8)?;
    /// let options = EncodeOptions {
    ///     gain_map: Some(GainMapEncodeOptions {
    ///         image: gain_map,
    ///         metadata: GainMapMetadata::new(),
    ///         quality: 85,
    ///         progressive: false,
    ///     }),
    ///     ..EncodeOptions::ultra_hdr_defaults()
    /// };
    /// # let _ = options;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn ultra_hdr_defaults() -> Self {
        Self {
            color_metadata: ColorMetadata::display_p3(),
            ..Self::default()
        }
    }
}

impl ComputedGainMap {
    /// Convert a computed gain map into bundling options for [`EncodeOptions::gain_map`].
    #[must_use]
    pub fn into_encode_options(self, quality: u8, progressive: bool) -> GainMapEncodeOptions {
        GainMapEncodeOptions {
            image: self.image,
            metadata: self.metadata,
            quality,
            progressive,
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
