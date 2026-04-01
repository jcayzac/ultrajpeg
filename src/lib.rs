#![doc = include_str!("../docs/user/overview.md")]
#![doc = include_str!("../docs/user/scenarios.md")]
#![doc = include_str!("../docs/user/color.md")]
#![doc = include_str!("../docs/user/ultra-hdr.md")]
#![doc = include_str!("../docs/user/limitations.md")]
#![doc = include_str!("../docs/user/migration-0.5.md")]
#![deny(missing_docs)]

mod codec;
mod container;
mod error;
mod gainmap;
pub mod icc;
mod metadata;
mod types;

/// Public error type for codec, container, and metadata failures.
pub use error::{Error, Result};
/// JPEG chroma-subsampling modes supported when encoding primary images.
pub use types::ChromaSubsampling;
/// An xy chromaticity coordinate.
pub use types::Chromaticity;
/// Structural layout of one embedded JPEG codestream.
pub use types::CodestreamLayout;
/// Color-related metadata attached to the primary JPEG image.
pub use types::ColorMetadata;
/// JPEG compression effort used during encoding.
pub use types::CompressionEffort;
/// Options for gain-map computation from HDR and SDR inputs.
pub use types::ComputeGainMapOptions;
/// Result of computing an Ultra HDR gain map from HDR and SDR inputs.
pub use types::ComputedGainMap;
/// Structural classification of a JPEG container.
pub use types::ContainerKind;
/// Structural layout of a JPEG or multi-image JPEG container.
pub use types::ContainerLayout;
/// Decode configuration.
pub use types::DecodeOptions;
/// Decoded gain-map JPEG payload and associated metadata.
pub use types::DecodedGainMap;
/// Fully decoded JPEG or Ultra HDR JPEG.
pub use types::DecodedImage;
/// Encode configuration for the primary JPEG and optional gain-map bundle.
pub use types::EncodeOptions;
/// Reusable stateful encoder.
pub use types::Encoder;
/// Gain-map payload and metadata to bundle into an Ultra HDR JPEG.
pub use types::GainMapBundle;
/// Channel layout used when computing a gain map.
pub use types::GainMapChannels;
/// Representation from which effective gain-map metadata was parsed.
pub use types::GainMapMetadataSource;
/// Structured gamut information recovered from explicit metadata or ICC data.
pub use types::GamutInfo;
/// Metadata-only inspection result.
pub use types::Inspection;
/// Location from which Ultra HDR metadata was resolved.
pub use types::MetadataLocation;
/// Parsed raw `hdrgm:*` XMP payload.
pub use types::ParsedGainMapXmp;
/// Options for deriving an SDR primary image from source pixels.
pub use types::PreparePrimaryOptions;
/// Prepared SDR primary image and matching primary-JPEG metadata.
pub use types::PreparedPrimary;
/// Primary-JPEG metadata handled by the crate.
pub use types::PrimaryMetadata;
/// Convenience options for one-shot Ultra HDR packaging.
pub use types::UltraHdrEncodeOptions;
/// Structured effective Ultra HDR metadata resolved by the crate.
pub use types::UltraHdrMetadata;

/// Named color gamut classification used by decoded images and metadata.
pub use ultrahdr_core::ColorGamut;
/// Color transfer function used by decoded images and metadata.
pub use ultrahdr_core::ColorTransfer;
/// Gain-map image representation produced by decode and used for HDR reconstruction.
pub use ultrahdr_core::GainMap;
/// Structured Ultra HDR gain-map metadata.
pub use ultrahdr_core::GainMapMetadata;
/// Stable pixel-format type used by [`Image`].
pub use ultrahdr_core::PixelFormat;
/// Stable image type used by `ultrajpeg` for decoded pixels, encoder input, and gain-map computation.
pub use ultrahdr_core::RawImage as Image;
/// HDR reconstruction output formats supported by the crate.
pub use ultrahdr_core::gainmap::HdrOutputFormat;

use codec::{decode_gain_map, decode_primary_image, encode_image};
use container::{
    assemble_container_owned, inspect_container, inspect_container_layout as inspect_layout_impl,
    parse_container,
};
use gainmap::{compute_gain_map_impl, prepare_primary_impl, ultra_hdr_encode_options};
use metadata::{
    build_ultra_hdr_metadata, parse_gain_map_xmp_raw, parse_iso_21496_1_raw,
    parse_ultra_hdr_metadata,
};
use rayon::join;
use ultrahdr_core::{ColorGamut as CoreColorGamut, ColorTransfer as CoreColorTransfer};

const PARALLEL_DECODE_THRESHOLD_BYTES: usize = 256 * 1024;

/// Decode a JPEG or Ultra HDR JPEG using the default decode configuration.
///
/// The default behavior is:
///
/// - decode the primary image
/// - decode the embedded gain-map JPEG when present
/// - retain no raw JPEG codestream bytes
/// - apply parsed primary-image color metadata to the returned [`DecodedImage::image`]
pub fn decode(bytes: &[u8]) -> Result<DecodedImage> {
    decode_internal(bytes, DecodeOptions::default())
}

/// Decode a JPEG or Ultra HDR JPEG using explicit decode options.
///
/// Use this when you need to:
///
/// - skip gain-map decode
/// - retain the primary JPEG codestream
/// - retain the gain-map JPEG codestream
///
/// Even when gain-map pixel decode is disabled, this function still inspects
/// bundled metadata and may return [`DecodedImage::ultra_hdr`] when effective
/// Ultra HDR metadata could be resolved.
pub fn decode_with_options(bytes: &[u8], options: DecodeOptions) -> Result<DecodedImage> {
    decode_internal(bytes, options)
}

/// Inspect JPEG or Ultra HDR container metadata without decoding image pixels.
///
/// This function:
///
/// - parses primary-JPEG metadata
/// - detects MPF-bundled gain-map JPEG payloads
/// - resolves effective Ultra HDR metadata with provenance
/// - may recover effective Ultra HDR metadata from the gain-map JPEG when the
///   primary JPEG is incomplete but the bundled structure is still usable
/// - does not decode primary or gain-map pixels
pub fn inspect(bytes: &[u8]) -> Result<Inspection> {
    let parsed = inspect_container(bytes)?;
    Ok(Inspection {
        primary_jpeg_len: parsed.primary_jpeg_len,
        gain_map_jpeg_len: parsed.gain_map_jpeg_len,
        primary_metadata: parsed.primary_metadata,
        ultra_hdr: parse_ultra_hdr_metadata(
            parsed.xmp.as_deref(),
            parsed.xmp_location,
            parsed.iso.as_deref(),
            parsed.iso_location,
        )?,
    })
}

/// Inspect JPEG codestream boundaries and bundled-container structure.
///
/// This function:
///
/// - does not decode pixels
/// - exposes embedded JPEG codestream offsets and lengths
/// - reports whether the container was identified as MPF or as concatenated
///   JPEG codestreams without MPF directory metadata
/// - identifies which codestreams `ultrajpeg` treats as the primary and
///   gain-map JPEG payloads
///
/// This API is inspection-only. It does not provide generic public MPF rewrite
/// or JPEG surgery primitives.
///
/// For non-MPF multi-JPEG inputs, `ultrajpeg` treats the second codestream as
/// the gain-map candidate structurally; semantic validity is determined
/// separately by metadata parsing on decode or inspect.
///
/// The offsets and lengths in [`ContainerLayout::codestreams`] are byte ranges
/// into the original input buffer, so callers can slice the original bytes as
/// `&bytes[offset..offset + len]` when they need direct codestream access.
pub fn inspect_container_layout(bytes: &[u8]) -> Result<ContainerLayout> {
    inspect_layout_impl(bytes)
}

/// Encode a primary JPEG, optionally bundling a gain map and Ultra HDR metadata.
///
/// When [`EncodeOptions::gain_map`] is `Some`, this function emits:
///
/// - an MPF-bundled primary JPEG
/// - container or directory XMP on the primary JPEG
/// - `hdrgm:*` XMP on the gain-map JPEG
/// - ISO 21496-1 metadata on the gain-map JPEG
///
/// Primary-image ICC handling is explicit:
///
/// - if [`EncodeOptions::primary_metadata`] already includes an ICC profile, it is embedded as-is
/// - if no ICC profile is present and the resolved primary gamut and transfer are Display-P3 plus sRGB, the bundled Display-P3 ICC profile is embedded automatically
/// - otherwise gain-map packaging fails with [`Error::InvalidInput`]
pub fn encode(image: &Image, options: &EncodeOptions) -> Result<Vec<u8>> {
    Encoder::new(options.clone()).encode(image)
}

/// Compute a gain map from an HDR image and a caller-chosen SDR primary image.
///
/// This function:
///
/// - does not encode JPEG bytes
/// - does not choose the SDR primary image for the caller
/// - does not apply output policy such as SDR fallback
///
/// The default configuration computes a single-channel luminance gain map
/// unless explicitly configured for multichannel computation.
///
/// This function assumes the caller already chose an SDR primary image with the
/// desired tone-mapping and color policy. Use [`prepare_sdr_primary`] when the
/// caller needs a supported high-level path for deriving that SDR primary from
/// HDR source pixels.
pub fn compute_gain_map(
    hdr_image: &Image,
    primary_image: &Image,
    options: &ComputeGainMapOptions,
) -> Result<ComputedGainMap> {
    compute_gain_map_impl(hdr_image, primary_image, options)
}

/// Parse a raw `hdrgm:*` XMP payload into structured gain-map metadata.
///
/// This function is intentionally raw:
///
/// - it does not apply `ultrajpeg`'s decode-time precedence rules
/// - it does not apply the crate's defensive recovery filters
/// - it is intended for callers that need to validate or compare raw payloads
///   explicitly
///
/// Container-only XMP that does not actually carry `hdrgm:*` metadata does not
/// parse successfully here.
///
/// In other words:
///
/// - [`inspect`] and [`decode`] expose the crate's effective metadata view
/// - [`parse_gain_map_xmp`] parses exactly one raw XMP payload that the caller
///   already has
pub fn parse_gain_map_xmp(xmp: &str) -> Result<ParsedGainMapXmp> {
    parse_gain_map_xmp_raw(xmp)
}

/// Parse a raw ISO 21496-1 gain-map payload into structured metadata.
///
/// This function is intentionally raw:
///
/// - it does not apply `ultrajpeg`'s decode-time precedence rules
/// - it does not compare the result against any XMP payload
/// - it is intended for callers that need to validate or compare raw payloads
///   explicitly
/// - it accepts both the canonical Ultra HDR payload layout and the older
///   legacy layout emitted by earlier `ultrahdr-core` releases
///
/// The primary JPEG in an Ultra HDR bundle may also carry a four-byte
/// version-only ISO APP2 block. That structural block is not gain-map
/// metadata; passing it here returns an error.
///
/// In other words:
///
/// - [`inspect`] and [`decode`] expose the crate's effective metadata view
/// - [`parse_iso_21496_1`] parses exactly one raw ISO payload that the caller
///   already has
pub fn parse_iso_21496_1(iso_21496_1: &[u8]) -> Result<GainMapMetadata> {
    parse_iso_21496_1_raw(iso_21496_1)
}

/// Prepare an SDR primary image from source pixels for gain-map workflows.
///
/// The returned [`PreparedPrimary`] contains:
///
/// - an `Rgb8` primary image tagged as sRGB
/// - matching [`PrimaryMetadata`] for the requested target gamut
///
/// This is the supported high-level path for callers that:
///
/// - transform HDR pixels first
/// - then need an SDR primary image for [`compute_gain_map`]
/// - and later package the result with [`encode`] or [`encode_ultra_hdr`]
///
/// The current helper supports:
///
/// - `Rgb8`
/// - `Rgba8`
/// - `Rgba16F`
/// - `Rgba32F`
/// - `Rgba1010102Pq`
/// - `Rgba1010102Hlg`
///
/// The current output-gamut policy supports [`ColorGamut::Bt709`] and
/// [`ColorGamut::DisplayP3`].
///
/// To keep the default [`compute_gain_map`] workflow composable, this helper
/// also floors the derived SDR primary brightness so that the prepared image
/// stays within the crate's default gain-map boost envelope.
///
/// This helper is meant to provide a supported default policy, not to replace
/// all caller-specific SDR rendering intent. Callers that already have a
/// bespoke SDR primary image should keep using that image directly with
/// [`compute_gain_map`] and [`encode`].
pub fn prepare_sdr_primary(
    image: &Image,
    options: &PreparePrimaryOptions,
) -> Result<PreparedPrimary> {
    prepare_primary_impl(image, options)
}

/// Convenience wrapper that computes a gain map and packages an Ultra HDR JPEG.
///
/// The caller still owns:
///
/// - SDR primary-image preparation
/// - primary-image color policy
/// - EXIF policy
/// - any SDR fallback behavior
///
/// Use [`prepare_sdr_primary`] when the caller wants `ultrajpeg` to derive a
/// supported SDR primary image and matching metadata before this packaging step.
///
/// `options.primary.gain_map` must be `None`; the gain map is computed from the
/// provided HDR and primary images and then bundled into the final JPEG.
pub fn encode_ultra_hdr(
    hdr_image: &Image,
    primary_image: &Image,
    options: &UltraHdrEncodeOptions,
) -> Result<Vec<u8>> {
    if options.primary.gain_map.is_some() {
        return Err(Error::InvalidInput(
            "UltraHdrEncodeOptions::primary.gain_map must be None".into(),
        ));
    }

    let computed = compute_gain_map(hdr_image, primary_image, &options.gain_map)?;
    let encode_options = ultra_hdr_encode_options(&options.primary, computed, options);
    encode(primary_image, &encode_options)
}

impl Encoder {
    /// Create a new encoder with explicit options.
    #[must_use]
    pub fn new(options: EncodeOptions) -> Self {
        Self { options }
    }

    /// Encode a primary JPEG, optionally bundling a gain map and Ultra HDR metadata.
    ///
    /// This applies the same ICC rules as [`encode`].
    pub fn encode(&self, image: &Image) -> Result<Vec<u8>> {
        let primary_metadata = resolved_primary_metadata(
            image,
            &self.options.primary_metadata,
            self.options.gain_map.is_some(),
        )?;
        let primary_jpeg = encode_image(
            image,
            self.options.quality,
            self.options.progressive,
            self.options.compression,
            self.options.chroma_subsampling,
            &primary_metadata,
        )?;

        let (gain_map_jpeg, ultra_hdr_metadata) = match self.options.gain_map.as_ref() {
            Some(gain_map) => {
                gain_map.metadata.validate()?;
                let jpeg = encode_image(
                    &gain_map.image,
                    gain_map.quality,
                    gain_map.progressive,
                    gain_map.compression,
                    ChromaSubsampling::Yuv444,
                    &PrimaryMetadata::default(),
                )?;
                let metadata = build_ultra_hdr_metadata(&gain_map.metadata)?;
                (Some(jpeg), Some(metadata))
            }
            None => (None, None),
        };

        assemble_container_owned(
            primary_jpeg,
            gain_map_jpeg.as_deref(),
            &primary_metadata,
            ultra_hdr_metadata.as_ref(),
        )
    }
}

impl DecodedImage {
    /// Reconstruct an HDR output image from the decoded primary image and gain map.
    ///
    /// This method requires:
    ///
    /// - a decoded gain map
    /// - effective parsed gain-map metadata, taken from
    ///   [`DecodedGainMap::metadata`] or, if that is absent,
    ///   [`DecodedImage::ultra_hdr`]
    ///
    /// It returns:
    ///
    /// - [`Error::MissingGainMap`] if no gain map was decoded
    /// - [`Error::MissingGainMapMetadata`] if no effective gain-map metadata is available
    /// - or a metadata or codec error if HDR reconstruction fails
    pub fn reconstruct_hdr(
        &self,
        display_boost: f32,
        output_format: HdrOutputFormat,
    ) -> Result<Image> {
        self.reconstruct_hdr_with(display_boost, output_format)
    }
}

fn decode_internal(bytes: &[u8], options: DecodeOptions) -> Result<DecodedImage> {
    let parsed = parse_container(bytes, &options)?;
    let ultra_hdr = parse_ultra_hdr_metadata(
        parsed.xmp.as_deref(),
        parsed.xmp_location,
        parsed.iso.as_deref(),
        parsed.iso_location,
    )?;
    let gain_map_metadata = ultra_hdr
        .as_ref()
        .and_then(|metadata| metadata.gain_map_metadata.clone());

    let (mut image, gain_map) = match parsed.gain_map_jpeg {
        Some(gain_map_jpeg) if options.decode_gain_map => {
            let decode_gain_map_fn = || decode_gain_map(gain_map_jpeg, gain_map_metadata.as_ref());
            let decode_primary_fn = || decode_primary_image(parsed.primary_jpeg);
            let (primary_result, gain_map_result) =
                if should_parallel_decode(parsed.primary_jpeg, gain_map_jpeg) {
                    join(decode_primary_fn, decode_gain_map_fn)
                } else {
                    (decode_primary_fn(), decode_gain_map_fn())
                };

            let mut decoded_gain_map = gain_map_result?;
            decoded_gain_map.metadata = gain_map_metadata;
            if options.retain_gain_map_jpeg {
                decoded_gain_map.jpeg_bytes = Some(gain_map_jpeg.to_vec());
            }
            (primary_result?, Some(decoded_gain_map))
        }
        _ => (decode_primary_image(parsed.primary_jpeg)?, None),
    };

    if let Some(gamut) = named_gamut(&parsed.primary_metadata.color) {
        image.gamut = gamut;
    }
    if let Some(transfer) = parsed.primary_metadata.color.transfer {
        image.transfer = transfer;
    }

    Ok(DecodedImage {
        image,
        primary_jpeg: options
            .retain_primary_jpeg
            .then(|| parsed.primary_jpeg.to_vec()),
        primary_metadata: parsed.primary_metadata,
        ultra_hdr,
        gain_map,
    })
}

fn should_parallel_decode(primary_jpeg: &[u8], gain_map_jpeg: &[u8]) -> bool {
    primary_jpeg.len() + gain_map_jpeg.len() >= PARALLEL_DECODE_THRESHOLD_BYTES
}

fn resolved_primary_metadata(
    image: &Image,
    primary_metadata: &PrimaryMetadata,
    has_gain_map: bool,
) -> Result<PrimaryMetadata> {
    if !has_gain_map || primary_metadata.color.icc_profile.is_some() {
        return Ok(primary_metadata.clone());
    }

    let gamut = named_gamut(&primary_metadata.color).unwrap_or(image.gamut);
    let transfer = primary_metadata.color.transfer.unwrap_or(image.transfer);

    if gamut == CoreColorGamut::DisplayP3 && transfer == CoreColorTransfer::Srgb {
        let mut resolved = primary_metadata.clone();
        resolved.color.icc_profile = Some(icc::display_p3().to_vec());
        resolved.color.gamut = Some(CoreColorGamut::DisplayP3);
        resolved.color.gamut_info = Some(GamutInfo::from_standard(CoreColorGamut::DisplayP3));
        resolved.color.transfer = Some(CoreColorTransfer::Srgb);
        return Ok(resolved);
    }

    Err(Error::InvalidInput(
        "gain-map JPEG primary images require an ICC profile; for Display-P3/sRGB input use EncodeOptions::ultra_hdr_defaults() or provide an explicit ICC profile".into(),
    ))
}

fn named_gamut(color_metadata: &ColorMetadata) -> Option<ColorGamut> {
    color_metadata.gamut.or_else(|| {
        color_metadata
            .gamut_info
            .as_ref()
            .and_then(|info| info.standard)
    })
}
