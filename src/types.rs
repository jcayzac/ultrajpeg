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

/// An xy chromaticity coordinate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chromaticity {
    /// Horizontal chromaticity coordinate.
    pub x: f32,
    /// Vertical chromaticity coordinate.
    pub y: f32,
}

/// Structured gamut information derived from explicit signaling or an ICC
/// profile.
///
/// This type always carries explicit chromaticity coordinates when gamut data
/// could be recovered.
///
/// [`GamutInfo::standard`] is only a convenience classification:
///
/// - `Some(...)` means the recovered primaries and white point match one of the
///   crate's named RGB standards within tolerance
/// - `None` means gamut coordinates were recovered successfully, but they do
///   not match one of the crate's named RGB standards
#[derive(Debug, Clone, PartialEq)]
pub struct GamutInfo {
    /// Matching named gamut standard, if the primaries and white point match
    /// one of the crate's known RGB standards within tolerance.
    pub standard: Option<ColorGamut>,
    /// Red primary xy chromaticity.
    pub red: Chromaticity,
    /// Green primary xy chromaticity.
    pub green: Chromaticity,
    /// Blue primary xy chromaticity.
    pub blue: Chromaticity,
    /// White-point xy chromaticity.
    pub white: Chromaticity,
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
    /// Effective XMP payload used for Ultra HDR metadata parsing.
    ///
    /// For spec-shaped files this may come from the gain-map JPEG rather than
    /// from the primary JPEG's container XMP.
    pub xmp: Option<String>,
    /// Effective ISO 21496-1 payload used for Ultra HDR metadata parsing.
    pub iso_21496_1: Option<Vec<u8>>,
    /// Parsed gain-map metadata after the crate's source-selection rules.
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
    /// Gain-map metadata to serialize into the secondary gain-map JPEG's
    /// `hdrgm:*` XMP and ISO 21496-1 payloads.
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
    /// Gain-map metadata ready to be serialized into the secondary gain-map
    /// JPEG's `hdrgm:*` XMP and ISO 21496-1 payloads.
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

    /// Resolve structured gamut information from this metadata, if available.
    ///
    /// Resolution order is:
    ///
    /// - the explicit [`ColorMetadata::gamut`] field, if present
    /// - otherwise the embedded ICC profile, if it contains usable RGB primaries
    ///   and white-point data
    ///
    /// This method returns:
    ///
    /// - `None` when no trustworthy gamut information could be recovered
    /// - `Some(GamutInfo { standard: Some(...), .. })` when the recovered gamut
    ///   matches a named standard such as [`ColorGamut::DisplayP3`]
    /// - `Some(GamutInfo { standard: None, .. })` when the recovered gamut is
    ///   available structurally but does not match one of the named standards
    ///
    /// This is the preferred API when the caller needs more than the crate's
    /// small [`ColorGamut`] enum can express.
    #[must_use]
    pub fn gamut_info(&self) -> Option<GamutInfo> {
        self.gamut.map(GamutInfo::from_standard).or_else(|| {
            self.icc_profile
                .as_deref()
                .and_then(crate::icc::gamut_info_from_profile)
        })
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

impl GamutInfo {
    #[must_use]
    pub(crate) fn from_standard(standard: ColorGamut) -> Self {
        let (red, green, blue, white) = match standard {
            ColorGamut::Bt709 => (
                Chromaticity { x: 0.64, y: 0.33 },
                Chromaticity { x: 0.30, y: 0.60 },
                Chromaticity { x: 0.15, y: 0.06 },
                Chromaticity {
                    x: 0.3127,
                    y: 0.3290,
                },
            ),
            ColorGamut::DisplayP3 => (
                Chromaticity { x: 0.68, y: 0.32 },
                Chromaticity { x: 0.265, y: 0.69 },
                Chromaticity { x: 0.15, y: 0.06 },
                Chromaticity {
                    x: 0.3127,
                    y: 0.3290,
                },
            ),
            ColorGamut::Bt2100 => (
                Chromaticity { x: 0.708, y: 0.292 },
                Chromaticity { x: 0.170, y: 0.797 },
                Chromaticity { x: 0.131, y: 0.046 },
                Chromaticity {
                    x: 0.3127,
                    y: 0.3290,
                },
            ),
        };

        Self {
            standard: Some(standard),
            red,
            green,
            blue,
            white,
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
