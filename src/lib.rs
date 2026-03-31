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
/// Color-related metadata attached to the primary JPEG image.
pub use types::ColorMetadata;
/// Options for gain-map computation from HDR and SDR inputs.
pub use types::ComputeGainMapOptions;
/// Result of computing an Ultra HDR gain map from HDR and SDR inputs.
pub use types::ComputedGainMap;
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
use container::{assemble_container_owned, inspect_container, parse_container};
use gainmap::{compute_gain_map_impl, ultra_hdr_encode_options};
use metadata::{build_ultra_hdr_metadata, parse_ultra_hdr_metadata};
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
pub fn compute_gain_map(
    hdr_image: &Image,
    primary_image: &Image,
    options: &ComputeGainMapOptions,
) -> Result<ComputedGainMap> {
    compute_gain_map_impl(hdr_image, primary_image, options)
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
                    ChromaSubsampling::Yuv444,
                    &PrimaryMetadata::default(),
                )?;
                let metadata = build_ultra_hdr_metadata(&gain_map.metadata, jpeg.len());
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
    /// - effective parsed gain-map metadata
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
