# Stable API Contract Draft

## Status

This draft was implemented in `0.5.0-rc1`.

Later additive work also filled the public gaps identified in issue `#4`:

- raw Ultra HDR payload parsing
- structural container-layout inspection
- high-level SDR-primary preparation for caller-managed HDR workflows

It remains useful as design history and for explaining why the public surface
looks the way it does, but it is no longer just a proposal.

For current maintainer-facing guidance and migration material, see:

- `docs/maintainer/api-guide.md`
- `docs/user/migration-0.5.md`

This document defines the proposed stable public API for `ultrajpeg`.

It is a review artifact, not an implementation file. The goal is to make the
intended `1.0` surface concrete enough that a consumer can evaluate:

- naming,
- layering,
- ergonomics,
- ownership behavior,
- migration cost,
- and capability coverage.

The design constraints for this draft are:

- preserve all practical workflows currently supported by the crate,
- remove wrapper-era public API surface,
- make ownership and allocation behavior explicit,
- follow Rust API Guidelines and Ed Page's style expectations,
- and present one coherent JPEG API with optional Ultra HDR support.

## Public Surface Summary

The proposed stable API is intentionally small:

- root functions:
  - `inspect`
  - `decode`
  - `decode_with_options`
  - `encode`
  - `compute_gain_map`
  - `encode_ultra_hdr`
- root types:
  - `Image`
  - `PixelFormat`
  - `ColorGamut`
  - `ColorTransfer`
  - `Chromaticity`
  - `GamutInfo`
  - `GainMap`
  - `GainMapMetadata`
  - `HdrOutputFormat`
  - `Error`
  - `Result`
  - `ChromaSubsampling`
  - `ColorMetadata`
  - `PrimaryMetadata`
  - `UltraHdrMetadata`
  - `MetadataLocation`
  - `GainMapMetadataSource`
  - `DecodedGainMap`
  - `DecodedImage`
  - `Inspection`
  - `DecodeOptions`
  - `GainMapChannels`
  - `ComputeGainMapOptions`
  - `ComputedGainMap`
  - `GainMapBundle`
  - `EncodeOptions`
  - `UltraHdrEncodeOptions`
  - `Encoder`
- public module:
  - `icc`

Everything else should be private.

## Draft `lib.rs` Contract

```rust
#![doc = include_str!("../README.md")]

/// Stable image type used by `ultrajpeg` for decoded pixels, encoder input, and
/// gain-map computation.
///
/// This is a stable re-export of `ultrahdr_core::RawImage`.
pub use ultrahdr_core::RawImage as Image;

/// Stable pixel-format type used by [`Image`].
pub use ultrahdr_core::PixelFormat;

/// Stable color-gamut type used by the crate's color metadata.
pub use ultrahdr_core::ColorGamut;

/// Stable color-transfer type used by the crate's color metadata.
pub use ultrahdr_core::ColorTransfer;

/// Stable gain-map image representation produced by decode and used for HDR
/// reconstruction.
pub use ultrahdr_core::GainMap;

/// Stable structured Ultra HDR gain-map metadata type.
pub use ultrahdr_core::GainMapMetadata;

/// HDR reconstruction output formats supported by the crate.
pub use ultrahdr_core::gainmap::HdrOutputFormat;

pub mod icc;

/// Result type used by all fallible `ultrajpeg` APIs.
pub type Result<T> = std::result::Result<T, Error>;

/// Public error type for codec, container, and Ultra HDR metadata failures.
///
/// This type is intentionally non-exhaustive so that future versions can add
/// more precise failure variants without breaking callers.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The caller requested an unsupported pixel format, JPEG coding mode, or
    /// reconstruction output.
    #[error("unsupported format: {0}")]
    UnsupportedFormat(&'static str),

    /// The caller provided invalid input, such as mismatched image dimensions,
    /// missing required metadata, or an invalid option combination.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// JPEG encoding or decoding failed.
    #[error("codec error: {0}")]
    Codec(String),

    /// JPEG marker parsing, MPF processing, or container assembly failed.
    #[error("container error: {0}")]
    Container(String),

    /// Ultra HDR metadata parsing, validation, or synthesis failed.
    #[error("metadata error: {0}")]
    Metadata(String),

    /// HDR reconstruction was requested on a decoded image that did not include
    /// a gain map.
    #[error("missing gain map image")]
    MissingGainMap,

    /// HDR reconstruction was requested on a decoded image whose gain-map
    /// metadata could not be resolved.
    #[error("missing gain map metadata")]
    MissingGainMapMetadata,

    /// I/O failed while reading or writing external data.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// JPEG chroma-subsampling modes supported when encoding the primary image.
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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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
    /// The stable API should distinguish:
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

impl ColorMetadata {
    /// Build Display-P3 primary-image color metadata using the crate's bundled
    /// ICC profile.
    ///
    /// The returned metadata:
    ///
    /// - embeds the built-in Display-P3 ICC profile,
    /// - sets [`ColorMetadata::gamut`] to [`ColorGamut::DisplayP3`],
    /// - sets [`ColorMetadata::gamut_info`] to the matching Display-P3
    ///   structural coordinates,
    /// - sets [`ColorMetadata::transfer`] to [`ColorTransfer::Srgb`].
    ///
    /// This is the most direct helper for callers that want a spec-friendly
    /// Display-P3 primary image when packaging a gain map.
    #[must_use]
    pub fn display_p3() -> Self;
}

/// Primary-JPEG metadata handled by the crate.
///
/// This type covers metadata physically attached to the primary JPEG itself.
/// Ultra HDR gain-map metadata is represented separately by
/// [`UltraHdrMetadata`] on decode and by [`GainMapBundle`] on encode.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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
/// - container/directory metadata on the primary JPEG,
/// - or gain-map metadata on the secondary JPEG.
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

/// Structured Ultra HDR metadata resolved by the crate.
///
/// This struct exposes both the effective raw payloads that the crate used and
/// where they came from.
///
/// When both ISO 21496-1 and XMP are present and valid, the crate prefers
/// ISO 21496-1 for [`UltraHdrMetadata::gain_map_metadata`].
#[derive(Debug, Clone, Default)]
pub struct UltraHdrMetadata {
    /// Effective XMP payload used by the crate after fallback and recovery
    /// logic.
    pub xmp: Option<String>,

    /// Location from which [`UltraHdrMetadata::xmp`] was resolved.
    pub xmp_location: Option<MetadataLocation>,

    /// Effective ISO 21496-1 payload used by the crate after fallback and
    /// recovery logic.
    pub iso_21496_1: Option<Vec<u8>>,

    /// Location from which [`UltraHdrMetadata::iso_21496_1`] was resolved.
    pub iso_21496_1_location: Option<MetadataLocation>,

    /// Effective parsed gain-map metadata after precedence rules have been
    /// applied.
    pub gain_map_metadata: Option<GainMapMetadata>,

    /// Representation from which [`UltraHdrMetadata::gain_map_metadata`] was
    /// parsed.
    pub gain_map_metadata_source: Option<GainMapMetadataSource>,
}

/// Decoded gain-map JPEG payload and its structured metadata.
#[derive(Debug, Clone)]
pub struct DecodedGainMap {
    /// Decoded secondary JPEG pixels.
    pub image: Image,

    /// Gain-map representation derived from [`DecodedGainMap::image`].
    pub gain_map: GainMap,

    /// Effective parsed gain-map metadata used for HDR reconstruction, if it
    /// could be resolved.
    pub metadata: Option<GainMapMetadata>,

    /// Raw gain-map JPEG bytes, retained only when requested via
    /// [`DecodeOptions::retain_gain_map_jpeg`].
    pub jpeg_bytes: Option<Vec<u8>>,
}

/// Result of metadata-only inspection.
///
/// This type never contains decoded pixel buffers.
///
/// It still exposes the same effective primary-image color semantics as the
/// full decode path, including structured gamut data recovered from ICC when
/// available.
#[derive(Debug, Clone)]
pub struct Inspection {
    /// Length in bytes of the primary JPEG codestream.
    pub primary_jpeg_len: usize,

    /// Length in bytes of the embedded gain-map JPEG codestream, if present.
    pub gain_map_jpeg_len: Option<usize>,

    /// Primary-JPEG metadata exposed by the crate.
    pub primary_metadata: PrimaryMetadata,

    /// Effective Ultra HDR metadata, if the image is gain-map based or
    /// recoverable as such.
    pub ultra_hdr: Option<UltraHdrMetadata>,
}

/// Fully decoded JPEG or Ultra HDR JPEG.
#[derive(Debug, Clone)]
pub struct DecodedImage {
    /// Decoded primary-image pixels.
    pub image: Image,

    /// Raw primary JPEG bytes, retained only when requested via
    /// [`DecodeOptions::retain_primary_jpeg`].
    pub primary_jpeg: Option<Vec<u8>>,

    /// Primary-JPEG metadata exposed by the crate.
    pub primary_metadata: PrimaryMetadata,

    /// Effective Ultra HDR metadata, if present.
    pub ultra_hdr: Option<UltraHdrMetadata>,

    /// Decoded gain-map JPEG payload, if present and enabled by
    /// [`DecodeOptions::decode_gain_map`].
    pub gain_map: Option<DecodedGainMap>,
}

impl DecodedImage {
    /// Reconstruct an HDR output image using the decoded primary image, decoded
    /// gain map, and effective gain-map metadata.
    ///
    /// This method requires:
    ///
    /// - a decoded gain map,
    /// - and effective parsed gain-map metadata.
    ///
    /// It returns:
    ///
    /// - [`Error::MissingGainMap`] if no gain map was decoded,
    /// - [`Error::MissingGainMapMetadata`] if no effective gain-map metadata is
    ///   available,
    /// - or a metadata/codec error if HDR reconstruction fails.
    pub fn reconstruct_hdr(
        &self,
        display_boost: f32,
        output_format: HdrOutputFormat,
    ) -> Result<Image>;
}

/// Decode configuration.
///
/// The default configuration decodes the primary image and any gain-map JPEG,
/// but retains no raw JPEG codestream bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeOptions {
    /// Whether to decode the embedded gain-map JPEG when present.
    pub decode_gain_map: bool,

    /// Whether to retain the primary JPEG codestream in
    /// [`DecodedImage::primary_jpeg`].
    pub retain_primary_jpeg: bool,

    /// Whether to retain the gain-map JPEG codestream in
    /// [`DecodedGainMap::jpeg_bytes`].
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

/// Channel layout used when computing a gain map from HDR and SDR inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GainMapChannels {
    /// Compute a single-channel luminance gain map.
    #[default]
    Single,
    /// Compute a multichannel RGB gain map.
    Multi,
}

/// Options for gain-map computation from HDR and SDR images.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ComputeGainMapOptions {
    /// Channel layout to compute.
    ///
    /// The default is [`GainMapChannels::Single`].
    pub channels: GainMapChannels,
}

/// Result of computing a gain map from HDR and SDR inputs.
#[derive(Debug, Clone)]
pub struct ComputedGainMap {
    /// Gain-map image pixels ready to be JPEG-encoded and bundled.
    pub image: Image,

    /// Gain-map metadata ready to be serialized.
    pub metadata: GainMapMetadata,
}

impl ComputedGainMap {
    /// Convert a computed gain map into a [`GainMapBundle`] for
    /// [`EncodeOptions::gain_map`].
    ///
    /// The gain-map JPEG is always encoded as a secondary JPEG payload inside
    /// the final container.
    #[must_use]
    pub fn into_bundle(self, quality: u8, progressive: bool) -> GainMapBundle;
}

/// Gain-map JPEG payload and metadata to bundle into the final output.
#[derive(Debug, Clone)]
pub struct GainMapBundle {
    /// Gain-map image pixels to encode as the secondary JPEG.
    pub image: Image,

    /// Gain-map metadata to serialize into Ultra HDR XMP and ISO 21496-1.
    pub metadata: GainMapMetadata,

    /// JPEG quality for the secondary gain-map codestream.
    pub quality: u8,

    /// Whether to emit the secondary gain-map JPEG as progressive.
    pub progressive: bool,
}

/// Encode configuration for the primary JPEG and optional bundled gain map.
#[derive(Debug, Clone)]
pub struct EncodeOptions {
    /// JPEG quality for the primary image.
    pub quality: u8,

    /// Whether to emit the primary JPEG as progressive.
    pub progressive: bool,

    /// Chroma subsampling used for the primary image.
    pub chroma_subsampling: ChromaSubsampling,

    /// Primary-JPEG metadata to embed in the output.
    pub primary_metadata: PrimaryMetadata,

    /// Optional gain-map payload to bundle into an Ultra HDR JPEG.
    ///
    /// When this field is `Some`, the output is an MPF-bundled gain-map JPEG.
    pub gain_map: Option<GainMapBundle>,
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

impl EncodeOptions {
    /// Build default options for authoring an Ultra HDR primary image.
    ///
    /// This keeps the crate's regular JPEG defaults and sets
    /// [`EncodeOptions::primary_metadata`] to a Display-P3 primary configuration
    /// with the bundled ICC profile.
    ///
    /// The returned options:
    ///
    /// - keep the normal primary JPEG defaults,
    /// - set [`PrimaryMetadata::color`] to [`ColorMetadata::display_p3()`],
    /// - leave [`EncodeOptions::gain_map`] as `None`.
    #[must_use]
    pub fn ultra_hdr_defaults() -> Self;
}

/// High-level convenience options for computing a gain map and packaging the
/// final Ultra HDR JPEG in one step.
#[derive(Debug, Clone)]
pub struct UltraHdrEncodeOptions {
    /// Primary-image encoding options.
    ///
    /// [`EncodeOptions::gain_map`] must be `None`; the gain map is computed from
    /// the `hdr_image` and `primary_image` inputs supplied to
    /// [`encode_ultra_hdr`].
    pub primary: EncodeOptions,

    /// Gain-map computation policy.
    pub gain_map: ComputeGainMapOptions,

    /// JPEG quality for the computed secondary gain-map codestream.
    pub gain_map_quality: u8,

    /// Whether to emit the computed secondary gain-map JPEG as progressive.
    pub gain_map_progressive: bool,
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

/// Reusable stateful encoder.
///
/// This type exists for callers that want to reuse one configuration across
/// many images without repeatedly passing the same [`EncodeOptions`] value.
#[derive(Debug, Clone)]
pub struct Encoder {
    options: EncodeOptions,
}

impl Encoder {
    /// Create a new encoder with the given options.
    #[must_use]
    pub fn new(options: EncodeOptions) -> Self;

    /// Encode a primary JPEG, optionally bundling a gain map and Ultra HDR
    /// metadata.
    ///
    /// If [`EncodeOptions::gain_map`] is `Some`, the encoder applies the same
    /// rules as [`encode`] for primary-image ICC handling:
    ///
    /// - if an ICC profile is already present in
    ///   [`EncodeOptions::primary_metadata`], it is embedded as-is,
    /// - if no ICC profile is present and the resolved primary gamut/transfer is
    ///   Display-P3 + sRGB, the built-in Display-P3 profile is embedded
    ///   automatically,
    /// - otherwise encoding fails with [`Error::InvalidInput`].
    pub fn encode(&self, image: &Image) -> Result<Vec<u8>>;
}

/// Inspect JPEG container metadata without decoding image pixels.
///
/// This function:
///
/// - parses the primary JPEG container,
/// - detects MPF-bundled gain-map JPEG payloads,
/// - extracts ICC and EXIF payloads,
/// - resolves effective Ultra HDR metadata,
/// - does not decode primary or gain-map pixels.
///
/// The returned [`Inspection::ultra_hdr`] may be recovered from either the
/// primary JPEG or the gain-map JPEG, depending on file layout and recovery
/// rules.
pub fn inspect(bytes: &[u8]) -> Result<Inspection>;

/// Decode a JPEG or Ultra HDR JPEG using the default decode configuration.
///
/// The default behavior is:
///
/// - decode the primary image,
/// - decode the gain-map JPEG when present,
/// - retain no raw JPEG codestream bytes.
pub fn decode(bytes: &[u8]) -> Result<DecodedImage>;

/// Decode a JPEG or Ultra HDR JPEG using explicit decode options.
///
/// Use this when the caller needs to:
///
/// - skip gain-map decode,
/// - retain the primary JPEG bytes,
/// - retain the gain-map JPEG bytes.
pub fn decode_with_options(bytes: &[u8], options: DecodeOptions) -> Result<DecodedImage>;

/// Encode a primary JPEG, optionally bundling a gain map and Ultra HDR
/// metadata.
///
/// This is the main structured encode entry point.
///
/// When [`EncodeOptions::gain_map`] is `Some`, this function emits:
///
/// - an MPF-bundled primary JPEG,
/// - Ultra HDR container/directory XMP on the primary JPEG,
/// - `hdrgm:*` XMP on the gain-map JPEG,
/// - ISO 21496-1 on the gain-map JPEG.
///
/// Primary-image ICC handling is explicit:
///
/// - if [`EncodeOptions::primary_metadata`] already includes an ICC profile, it
///   is embedded as-is,
/// - if no ICC profile is present and the resolved primary gamut/transfer is
///   Display-P3 + sRGB, the built-in Display-P3 profile is embedded
///   automatically,
/// - otherwise gain-map packaging fails with [`Error::InvalidInput`].
///
/// This behavior is intended to keep compliant Display-P3 Ultra HDR packaging
/// easy without hiding the rule from the caller.
pub fn encode(image: &Image, options: &EncodeOptions) -> Result<Vec<u8>>;

/// Compute a gain map from an HDR image and a caller-chosen SDR primary image.
///
/// This function:
///
/// - does not encode JPEG bytes,
/// - does not choose the SDR primary image for the caller,
/// - does not apply output-policy decisions such as SDR fallback.
///
/// The default configuration computes a single-channel gain map.
pub fn compute_gain_map(
    hdr_image: &Image,
    primary_image: &Image,
    options: &ComputeGainMapOptions,
) -> Result<ComputedGainMap>;

/// Convenience wrapper that computes a gain map and packages an Ultra HDR JPEG.
///
/// The caller still owns:
///
/// - SDR primary-image preparation,
/// - primary-image color policy,
/// - EXIF policy,
/// - and any fallback-to-SDR behavior.
///
/// This function is equivalent to:
///
/// 1. [`compute_gain_map`]
/// 2. convert the result into a [`GainMapBundle`]
/// 3. [`encode`]
///
/// It returns [`Error::InvalidInput`] if `options.primary.gain_map` is already
/// populated.
pub fn encode_ultra_hdr(
    hdr_image: &Image,
    primary_image: &Image,
    options: &UltraHdrEncodeOptions,
) -> Result<Vec<u8>>;
```

## Draft `icc` Module Contract

```rust
//! Bundled ICC profiles and related helpers.
//!
//! The stable public API intentionally keeps ICC helpers small and explicit.

/// Raw Display-P3 ICC profile bytes.
///
/// This payload is bundled with the crate so that callers can build compliant
/// or spec-friendly Display-P3 primary JPEGs without carrying a separate ICC
/// asset in their application.
#[must_use]
pub fn display_p3() -> &'static [u8];
```

## Behavioral Contracts

These are the important semantic commitments behind the signatures above.

### Decode

- `inspect` never decodes pixels.
- `decode` decodes pixels and returns no retained JPEG codestreams by default.
- `decode_with_options` is the explicit escape hatch for callers that want to
  retain primary or gain-map JPEG bytes.
- large Ultra HDR decodes may use internal parallelism, but the public API
  remains synchronous.
- effective primary-image color semantics are available from both inspect and
  decode, including ICC-backed structured gamut recovery
- enum-level gamut access is a convenience layer over the richer structural
  gamut model

### Effective Ultra HDR metadata

- `UltraHdrMetadata` describes the effective metadata used by the crate.
- XMP and ISO payloads may come from the primary JPEG or the gain-map JPEG.
- `gain_map_metadata` prefers ISO 21496-1 over XMP when both are present and
  valid.
- `xmp_location`, `iso_21496_1_location`, and `gain_map_metadata_source` exist
  so provenance is not hidden from the caller.

### Encode

- `encode` is the primary structured encode API.
- `Encoder` is only a reusable wrapper around `EncodeOptions`; it does not
  represent a different capability tier.
- `compute_gain_map` and `encode_ultra_hdr` are convenience layers around the
  same structured encode flow.
- gain-map packaging still emits both Ultra HDR XMP and ISO 21496-1.

### Color semantics

- `GamutInfo` is the stable public gamut model.
- `ColorGamut` remains useful as a convenience classification for common named
  standards such as BT.709, Display-P3, and BT.2100.
- `ColorMetadata::gamut_info` is authoritative when present.
- `ColorMetadata::gamut` is only the best matching named standard view.
- The stable API must distinguish:
  - no gamut data available,
  - gamut data available but not matching a named standard,
  - gamut data available and matching a named standard.
- ICC-backed gamut recovery is part of the documented decode contract, not an
  accidental implementation detail.

### ICC behavior for gain-map packaging

- the primary image must have an ICC profile when packaging a gain map,
- `ColorMetadata::display_p3()` and `EncodeOptions::ultra_hdr_defaults()` are
  the explicit convenience path,
- if the caller omits the ICC profile but the resolved primary image is
  Display-P3 + sRGB, the built-in Display-P3 profile is auto-injected,
- otherwise the encode fails.

This keeps compliant packaging easy while still making the rule visible and
testable.

## Intentionally Not In The Stable Public API

The following current public items do not belong in the proposed stable API:

- `compat`
- `jpeg`
- `mozjpeg`
- `sys`
- `CompressedImage`
- `RawImage` as a separate wrapper type
- wrapper-era `Encoder` / `Decoder` types
- `DecodedPacked`
- `ImgLabel`
- `Codec`
- `ImageFormat`
- `ColorRange`
- `UltraJpegEncoder`
- `CoreRawImage`

If implementation code for some of these survives internally, it should remain
private.

## Migration Hotspots

This is not the full migration guide, but these are the major mappings implied
by the draft.

- `ultrahdr_core::RawImage` used at the `ultrajpeg` boundary becomes
  `ultrajpeg::Image`.
- `DecodedJpeg` becomes `DecodedImage`.
- `InspectedJpeg` becomes `Inspection`.
- `ColorMetadata` loses `exif`; EXIF moves to `PrimaryMetadata`.
- `ColorMetadata` grows first-class structured gamut semantics instead of being
  limited to `Option<ColorGamut>`.
- `EncodeOptions::color_metadata` becomes `EncodeOptions::primary_metadata`.
- `GainMapEncodeOptions` becomes `GainMapBundle`.
- `ComputedGainMap::into_encode_options(...)` becomes
  `ComputedGainMap::into_bundle(...)`.
- `UltraJpegEncoder` becomes `Encoder`.
- retained JPEG codestreams move from always-owned fields to explicit
  `Option<Vec<u8>>` fields controlled by `DecodeOptions`.
- legacy wrapper APIs disappear from the stable public surface.

## Open Questions For Consumer Review

These are the most important questions to validate before implementation:

1. Is `Image` the right stable public name, or should the crate keep
   `RawImage`?
2. Is `PrimaryMetadata` the right way to separate EXIF from color metadata?
3. Is `DecodedImage` preferable to `DecodedJpeg`?
4. Is `Inspection` preferable to `InspectedJpeg`?
5. Is `GainMapBundle` the right encode-side name?
6. Is `ColorMetadata { gamut, gamut_info, ... }` the right stable shape, or
   should the named-gamut convenience view move behind accessors instead of
   being a stored public field?
7. Are the explicit metadata provenance fields useful enough to keep?
8. Is the `Encoder` wrapper worth keeping, or should the stable API only expose
   the free functions?
