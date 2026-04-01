use crate::{Error, Result};
use ultrahdr_core::{
    ColorGamut, ColorTransfer, GainMap, GainMapMetadata, RawImage, Unstoppable,
    gainmap::{HdrOutputFormat, apply_gainmap},
};

/// Chroma subsampling modes exposed by the public API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChromaSubsampling {
    /// 4:2:0 chroma subsampling.
    #[default]
    Yuv420,
    /// 4:2:2 chroma subsampling.
    Yuv422,
    /// 4:4:4 chroma subsampling.
    Yuv444,
    /// 4:4:0 chroma subsampling.
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

/// Color-related metadata attached to the primary JPEG image.
///
/// This struct is used on both encode and decode paths.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ColorMetadata {
    /// ICC profile bytes embedded in the primary JPEG, if present.
    ///
    /// On encode, setting this field requests that the given profile be written
    /// into the primary JPEG.
    pub icc_profile: Option<Vec<u8>>,
    /// Explicit primary-image named gamut tracked by the crate alongside the
    /// JPEG.
    ///
    /// This field is a convenience classification, not the authoritative gamut
    /// model for the stable API.
    pub gamut: Option<ColorGamut>,
    /// Structured gamut information recovered from explicit signaling or the
    /// embedded ICC profile.
    ///
    /// This is the authoritative stable gamut representation.
    ///
    /// The stable API distinguishes:
    ///
    /// - `None` when no trustworthy gamut data could be recovered
    /// - `Some(GamutInfo { standard: None, .. })` when gamut coordinates were
    ///   recovered but do not match a named standard
    /// - `Some(GamutInfo { standard: Some(...), .. })` when gamut coordinates
    ///   were recovered and also match a named standard
    pub gamut_info: Option<GamutInfo>,
    /// Explicit primary-image transfer tracked by the crate alongside the JPEG.
    pub transfer: Option<ColorTransfer>,
}

/// Primary-JPEG metadata handled by the crate.
///
/// This type covers metadata physically attached to the primary JPEG itself.
/// Ultra HDR gain-map metadata is represented separately by
/// [`UltraHdrMetadata`] on decode and by [`GainMapBundle`] on encode.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PrimaryMetadata {
    /// Primary-image color signaling and ICC payload.
    pub color: ColorMetadata,
    /// EXIF payload to embed in or extract from the primary JPEG.
    pub exif: Option<Vec<u8>>,
}

/// Location from which Ultra HDR metadata was resolved.
///
/// For spec-shaped files this is usually:
///
/// - container or directory metadata on the primary JPEG
/// - gain-map metadata on the secondary JPEG
///
/// For malformed but recoverable files, `ultrajpeg` may recover effective
/// metadata from whichever location remains valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataLocation {
    /// Metadata came from the primary JPEG.
    Primary,
    /// Metadata came from the embedded gain-map JPEG.
    GainMap,
}

/// Representation from which effective gain-map metadata was parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GainMapMetadataSource {
    /// Effective metadata was parsed from ISO 21496-1.
    Iso21496_1,
    /// Effective metadata was parsed from Ultra HDR XMP.
    Xmp,
}

/// Structured effective Ultra HDR metadata resolved by the crate.
///
/// This struct exposes both the effective raw payloads that the crate used and
/// where they came from.
///
/// When both ISO 21496-1 and XMP are present and valid, the crate prefers
/// ISO 21496-1 for [`UltraHdrMetadata::gain_map_metadata`].
#[derive(Debug, Clone, Default)]
pub struct UltraHdrMetadata {
    /// Effective XMP payload used for Ultra HDR metadata parsing.
    ///
    /// For spec-shaped files this may come from the gain-map JPEG rather than
    /// from the primary JPEG's container XMP.
    pub xmp: Option<String>,
    /// Location from which [`UltraHdrMetadata::xmp`] was resolved.
    pub xmp_location: Option<MetadataLocation>,
    /// Effective ISO 21496-1 payload used for Ultra HDR metadata parsing.
    pub iso_21496_1: Option<Vec<u8>>,
    /// Location from which [`UltraHdrMetadata::iso_21496_1`] was resolved.
    pub iso_21496_1_location: Option<MetadataLocation>,
    /// Parsed gain-map metadata after the crate's source-selection rules.
    pub gain_map_metadata: Option<GainMapMetadata>,
    /// Representation from which [`UltraHdrMetadata::gain_map_metadata`] was
    /// parsed.
    pub gain_map_metadata_source: Option<GainMapMetadataSource>,
}

/// Decoded gain-map payload and associated metadata.
#[derive(Debug, Clone)]
pub struct DecodedGainMap {
    /// Decoded secondary JPEG pixels.
    pub image: RawImage,
    /// Gain-map representation derived from [`DecodedGainMap::image`].
    pub gain_map: GainMap,
    /// Effective parsed gain-map metadata used for HDR reconstruction, if it
    /// could be resolved.
    ///
    /// This is the effective metadata selected by the crate's decode-time
    /// precedence and recovery rules, not necessarily a payload parsed only
    /// from the secondary JPEG itself.
    pub metadata: Option<GainMapMetadata>,
    /// Raw gain-map JPEG bytes, retained only when requested via
    /// [`DecodeOptions::retain_gain_map_jpeg`].
    pub jpeg_bytes: Option<Vec<u8>>,
}

/// Fully decoded JPEG/UltraHDR image.
#[derive(Debug, Clone)]
pub struct DecodedImage {
    /// Decoded primary-image pixels.
    ///
    /// When parsed primary-image color metadata is available, `ultrajpeg`
    /// applies the resolved gamut and transfer to this image value rather than
    /// leaving the decoder defaults in place.
    pub image: RawImage,
    /// Raw primary JPEG bytes, retained only when requested via
    /// [`DecodeOptions::retain_primary_jpeg`].
    pub primary_jpeg: Option<Vec<u8>>,
    /// Primary-JPEG metadata exposed by the crate.
    pub primary_metadata: PrimaryMetadata,
    /// Effective Ultra HDR metadata, if the image is gain-map based or
    /// recoverable as such.
    pub ultra_hdr: Option<UltraHdrMetadata>,
    /// Decoded gain-map JPEG payload, if present and enabled by
    /// [`DecodeOptions::decode_gain_map`].
    pub gain_map: Option<DecodedGainMap>,
}

/// Metadata-only JPEG or Ultra HDR inspection result.
///
/// This type never contains decoded pixel buffers.
#[derive(Debug, Clone)]
pub struct Inspection {
    /// Length in bytes of the primary JPEG codestream.
    pub primary_jpeg_len: usize,
    /// Length in bytes of the embedded gain-map JPEG codestream, if present.
    pub gain_map_jpeg_len: Option<usize>,
    /// Primary-JPEG metadata exposed by the crate.
    pub primary_metadata: PrimaryMetadata,
    /// Effective Ultra HDR metadata, if present.
    pub ultra_hdr: Option<UltraHdrMetadata>,
}

/// Parsed `hdrgm:*` XMP payload.
///
/// This type exposes the gain-map metadata carried by a raw XMP payload and
/// the optional bundled gain-map JPEG length signaled by the container
/// directory.
#[derive(Debug, Clone)]
pub struct ParsedGainMapXmp {
    /// Parsed gain-map metadata.
    pub metadata: GainMapMetadata,
    /// Gain-map JPEG length recovered from container-directory XMP, if present.
    pub gain_map_jpeg_len: Option<usize>,
}

/// Structural classification of a JPEG container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerKind {
    /// A single JPEG codestream with no additional embedded JPEG payloads.
    Jpeg,
    /// An MPF-bundled multi-image JPEG.
    Mpf,
    /// Multiple concatenated JPEG codestreams were found, but no MPF directory
    /// could be parsed.
    ConcatenatedJpegs,
}

/// Byte range of one embedded JPEG codestream inside an input buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodestreamLayout {
    /// Byte offset of the codestream start in the original input.
    pub offset: usize,
    /// Byte length of the codestream.
    pub len: usize,
}

/// Structural layout of a JPEG or multi-image JPEG container.
///
/// This type exposes codestream boundaries and the indices that `ultrajpeg`
/// treats as the primary and gain-map JPEG payloads.
///
/// The layout is structural only. It does not imply that all embedded
/// codestreams are semantically valid Ultra HDR payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerLayout {
    /// Structural classification of the input container.
    pub kind: ContainerKind,
    /// Embedded JPEG codestreams in container order.
    pub codestreams: Vec<CodestreamLayout>,
    /// Index of the primary JPEG codestream in [`ContainerLayout::codestreams`].
    pub primary_index: usize,
    /// Index of the gain-map JPEG codestream in
    /// [`ContainerLayout::codestreams`], if one was identified.
    pub gain_map_index: Option<usize>,
}

/// Decode configuration.
///
/// The default configuration decodes the primary image and any gain-map JPEG,
/// but retains no raw JPEG codestream bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeOptions {
    /// Whether to decode the embedded gain-map JPEG when present.
    ///
    /// This only affects gain-map pixel decode. Ultra HDR metadata inspection
    /// and recovery still run.
    pub decode_gain_map: bool,
    /// Whether to retain the primary JPEG codestream in
    /// [`DecodedImage::primary_jpeg`].
    pub retain_primary_jpeg: bool,
    /// Whether to retain the gain-map JPEG codestream in
    /// [`DecodedGainMap::jpeg_bytes`].
    ///
    /// This takes effect only when [`DecodeOptions::decode_gain_map`] is `true`
    /// and a gain-map JPEG was decoded successfully.
    pub retain_gain_map_jpeg: bool,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self {
            decode_gain_map: true,
            retain_primary_jpeg: false,
            retain_gain_map_jpeg: false,
        }
    }
}

/// Gain-map payload and metadata to bundle into the final output.
#[derive(Debug, Clone)]
pub struct GainMapBundle {
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

/// Options for deriving an SDR primary image from source pixels.
///
/// The prepared primary image is always:
///
/// - `Rgb8`
/// - tagged as [`ColorTransfer::Srgb`]
/// - tagged for the requested [`PreparePrimaryOptions::target_gamut`]
/// - brightness-floored so the default [`crate::compute_gain_map`] path stays
///   within the crate's default gain-map boost envelope
///
/// `source_peak_nits` controls how source luminance is interpreted:
///
/// - for PQ input, `None` defaults to `10000`
/// - for HLG input, `None` defaults to `1000`
/// - for linear input, `None` defaults to `1000`
/// - for sRGB input, `None` defaults to `203`
#[derive(Debug, Clone, PartialEq)]
pub struct PreparePrimaryOptions {
    /// Target gamut for the SDR primary image.
    ///
    /// The current high-level helper supports [`ColorGamut::Bt709`] and
    /// [`ColorGamut::DisplayP3`].
    pub target_gamut: ColorGamut,
    /// Source peak luminance in nits.
    ///
    /// When set to `None`, `ultrajpeg` picks a transfer-specific default as
    /// described on [`PreparePrimaryOptions`].
    pub source_peak_nits: Option<f32>,
    /// Target SDR peak luminance in nits.
    pub target_peak_nits: f32,
}

/// Prepared SDR primary image and matching primary-JPEG metadata.
///
/// This type is produced by [`crate::prepare_sdr_primary`] for workflows where
/// the caller manages geometry or pixel edits before computing a gain map and
/// packaging the final JPEG.
///
/// [`PreparedPrimary::image`] and [`PreparedPrimary::metadata`] are intended to
/// be used together on subsequent encode calls.
#[derive(Debug, Clone)]
pub struct PreparedPrimary {
    /// Prepared SDR primary image pixels.
    ///
    /// The image is always `Rgb8` with [`ColorTransfer::Srgb`].
    pub image: RawImage,
    /// Primary metadata that matches [`PreparedPrimary::image`].
    ///
    /// For Display-P3 output this includes the crate's bundled Display-P3 ICC
    /// profile. For BT.709 output, no ICC profile is attached automatically.
    pub metadata: PrimaryMetadata,
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
    /// Primary-JPEG metadata to embed in the output.
    pub primary_metadata: PrimaryMetadata,
    /// Optional gain-map image and metadata to bundle into an Ultra HDR
    /// container.
    pub gain_map: Option<GainMapBundle>,
}

/// High-level convenience options for direct Ultra HDR packaging from HDR and
/// SDR inputs.
#[derive(Debug, Clone)]
pub struct UltraHdrEncodeOptions {
    /// Primary JPEG encoding options.
    ///
    /// [`EncodeOptions::gain_map`] must be `None`; the gain map is computed
    /// from the `hdr_image` and `primary_image` inputs supplied to
    /// [`crate::encode_ultra_hdr`].
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
            primary_metadata: PrimaryMetadata::default(),
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

impl Default for PreparePrimaryOptions {
    fn default() -> Self {
        Self {
            target_gamut: ColorGamut::Bt709,
            source_peak_nits: None,
            target_peak_nits: 203.0,
        }
    }
}

impl ColorMetadata {
    /// Build Display-P3 primary-image metadata using the crate's bundled ICC
    /// profile.
    ///
    /// The returned metadata:
    ///
    /// - embeds the crate's built-in Display-P3 ICC profile
    /// - sets [`ColorMetadata::gamut`] to [`ColorGamut::DisplayP3`]
    /// - sets [`ColorMetadata::gamut_info`] to matching Display-P3 structural
    ///   coordinates
    /// - sets [`ColorMetadata::transfer`] to [`ColorTransfer::Srgb`]
    #[must_use]
    pub fn display_p3() -> Self {
        Self {
            icc_profile: Some(crate::icc::display_p3().to_vec()),
            gamut: Some(ColorGamut::DisplayP3),
            gamut_info: Some(GamutInfo::from_standard(ColorGamut::DisplayP3)),
            transfer: Some(ColorTransfer::Srgb),
        }
    }

    #[must_use]
    pub(crate) fn bt709_srgb() -> Self {
        Self {
            icc_profile: None,
            gamut: Some(ColorGamut::Bt709),
            gamut_info: Some(GamutInfo::from_standard(ColorGamut::Bt709)),
            transfer: Some(ColorTransfer::Srgb),
        }
    }
}

impl EncodeOptions {
    /// Build default encoder options for an Ultra HDR primary image.
    ///
    /// This keeps the crate's regular JPEG defaults and preconfigures
    /// [`EncodeOptions::primary_metadata`] with a Display-P3 color
    /// configuration built from [`ColorMetadata::display_p3()`].
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
    /// # use ultrajpeg::{EncodeOptions, GainMapBundle};
    /// let gain_map = RawImage::new(8, 8, PixelFormat::Gray8)?;
    /// let options = EncodeOptions {
    ///     gain_map: Some(GainMapBundle {
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
            primary_metadata: PrimaryMetadata {
                color: ColorMetadata::display_p3(),
                exif: None,
            },
            ..Self::default()
        }
    }
}

impl PreparePrimaryOptions {
    /// Build SDR-primary preparation defaults for Ultra HDR packaging.
    ///
    /// The returned options target a Display-P3 primary image with the usual
    /// SDR reference peak of `203` nits.
    ///
    /// This is a high-level policy default, not a guarantee of source-image
    /// conformance checking.
    #[must_use]
    pub fn ultra_hdr_defaults() -> Self {
        Self {
            target_gamut: ColorGamut::DisplayP3,
            ..Self::default()
        }
    }
}

impl ComputedGainMap {
    /// Convert a computed gain map into bundling options for
    /// [`EncodeOptions::gain_map`].
    ///
    /// The gain-map JPEG is always encoded as a secondary JPEG payload inside
    /// the final container.
    #[must_use]
    pub fn into_bundle(self, quality: u8, progressive: bool) -> GainMapBundle {
        GainMapBundle {
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

/// Reusable stateful encoder.
///
/// This type exists for callers that want to reuse one configuration across
/// many images without repeatedly passing the same [`EncodeOptions`] value.
#[derive(Debug, Clone)]
pub struct Encoder {
    pub(crate) options: EncodeOptions,
}

impl DecodedImage {
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
            &self.image,
            &gain_map.gain_map,
            metadata,
            display_boost,
            output_format,
            Unstoppable,
        )
        .map_err(Into::into)
    }
}
