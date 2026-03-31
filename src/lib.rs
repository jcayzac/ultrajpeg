#![doc = include_str!("../README.md")]

mod codec;
mod compat;
mod container;
mod error;
mod gainmap;
pub mod icc;
mod metadata;
mod types;

pub use compat::{
    Codec, ColorGamut, ColorRange, ColorTransfer, CompressedImage, DecodedPacked, Decoder,
    EncodedStream, Encoder, ImageFormat, ImgLabel, RawImage, jpeg, mozjpeg, sys,
};
pub use error::{Error, Result};
pub use types::{
    ChromaSubsampling, Chromaticity, ColorMetadata, ComputeGainMapOptions, ComputedGainMap,
    DecodeOptions, DecodedGainMap, DecodedJpeg, EncodeOptions, GainMapChannels,
    GainMapEncodeOptions, GamutInfo, InspectedJpeg, UltraHdrEncodeOptions, UltraHdrMetadata,
    UltraJpegEncoder,
};
pub use ultrahdr_core::{GainMapMetadata, RawImage as CoreRawImage};

use codec::{decode_gain_map, decode_primary_image, encode_image};
use container::{assemble_container_owned, inspect_container, parse_container};
use gainmap::{compute_gain_map_impl, ultra_hdr_encode_options};
use metadata::{build_ultra_hdr_metadata, parse_ultra_hdr_metadata};
use rayon::join;
use ultrahdr_core::{
    ColorGamut as CoreColorGamut, ColorTransfer as CoreColorTransfer, gainmap::HdrOutputFormat,
};

const PARALLEL_DECODE_THRESHOLD_BYTES: usize = 256 * 1024;

/// Decode a JPEG or UltraHDR JPEG into a structured representation.
pub fn decode(bytes: &[u8]) -> Result<DecodedJpeg> {
    decode_internal(bytes, DecodeOptions::default(), true)
}

/// Decode a JPEG or UltraHDR JPEG using explicit decode options.
pub fn decode_with_options(bytes: &[u8], options: DecodeOptions) -> Result<DecodedJpeg> {
    decode_internal(bytes, options, true)
}

/// Inspect JPEG or UltraHDR container metadata without decoding image pixels.
pub fn inspect(bytes: &[u8]) -> Result<InspectedJpeg> {
    let parsed = inspect_container(bytes)?;
    Ok(InspectedJpeg {
        primary_jpeg_len: parsed.primary_jpeg_len,
        gain_map_jpeg_len: parsed.gain_map_jpeg_len,
        color_metadata: parsed.color_metadata,
        ultra_hdr: parse_ultra_hdr_metadata(parsed.xmp.as_deref(), parsed.iso.as_deref())?,
    })
}

/// Encode a JPEG or UltraHDR JPEG with optional gain-map metadata.
pub fn encode(primary_image: &CoreRawImage, options: &EncodeOptions) -> Result<Vec<u8>> {
    UltraJpegEncoder::new(options.clone()).encode(primary_image)
}

/// Compute a gain map from an HDR image and a caller-chosen SDR primary image.
///
/// This defaults to a single-channel luminance gain map unless explicitly
/// configured for multichannel computation.
pub fn compute_gain_map(
    hdr_image: &CoreRawImage,
    primary_image: &CoreRawImage,
    options: &ComputeGainMapOptions,
) -> Result<ComputedGainMap> {
    compute_gain_map_impl(hdr_image, primary_image, options)
}

/// Convenience wrapper that computes a gain map and packages an Ultra HDR JPEG.
///
/// The caller still owns the SDR primary-image preparation and color policy.
/// `options.primary.gain_map` must be `None`; the gain map is computed from the
/// provided HDR and primary images.
pub fn encode_ultra_hdr(
    hdr_image: &CoreRawImage,
    primary_image: &CoreRawImage,
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

impl UltraJpegEncoder {
    /// Create a new encoder with explicit options.
    #[must_use]
    pub fn new(options: EncodeOptions) -> Self {
        Self { options }
    }

    /// Encode a primary JPEG, optionally bundling a gain map and UltraHDR metadata.
    pub fn encode(&self, primary_image: &CoreRawImage) -> Result<Vec<u8>> {
        let color_metadata = resolved_primary_color_metadata(
            primary_image,
            &self.options.color_metadata,
            self.options.gain_map.is_some(),
        )?;
        let primary_jpeg = encode_image(
            primary_image,
            self.options.quality,
            self.options.progressive,
            self.options.chroma_subsampling,
            &color_metadata,
        )?;

        let (gain_map_jpeg, ultra_hdr_metadata) = match self.options.gain_map.as_ref() {
            Some(gain_map) => {
                gain_map.metadata.validate()?;
                let jpeg = encode_image(
                    &gain_map.image,
                    gain_map.quality,
                    gain_map.progressive,
                    ChromaSubsampling::Yuv444,
                    &ColorMetadata::default(),
                )?;
                let metadata = build_ultra_hdr_metadata(&gain_map.metadata, jpeg.len());
                (Some(jpeg), Some(metadata))
            }
            None => (None, None),
        };

        assemble_container_owned(
            primary_jpeg,
            gain_map_jpeg.as_deref(),
            &color_metadata,
            ultra_hdr_metadata.as_ref(),
        )
    }
}

impl DecodedJpeg {
    /// Apply the decoded gain map and reconstruct an HDR output image.
    pub fn reconstruct_hdr(
        &self,
        display_boost: f32,
        output_format: HdrOutputFormat,
    ) -> Result<CoreRawImage> {
        self.reconstruct_hdr_with(display_boost, output_format)
    }
}

pub(crate) fn decode_hdr_output(
    bytes: &[u8],
    output_format: HdrOutputFormat,
) -> Result<(CoreRawImage, ColorMetadata)> {
    let decoded = decode_internal(bytes, DecodeOptions::default(), false)?;
    let display_boost = decoded
        .ultra_hdr
        .as_ref()
        .and_then(|metadata| metadata.gain_map_metadata.as_ref())
        .map_or(4.0, |metadata| metadata.hdr_capacity_max.max(1.0));
    let output = decoded.reconstruct_hdr(display_boost, output_format)?;
    Ok((output, decoded.color_metadata))
}

fn decode_internal(
    bytes: &[u8],
    options: DecodeOptions,
    retain_codestreams: bool,
) -> Result<DecodedJpeg> {
    let parsed = parse_container(bytes, &options)?;
    let ultra_hdr = parse_ultra_hdr_metadata(parsed.xmp.as_deref(), parsed.iso.as_deref())?;
    let gain_map_metadata = ultra_hdr
        .as_ref()
        .and_then(|metadata| metadata.gain_map_metadata.clone());

    let (mut primary_image, gain_map) = match parsed.gain_map_jpeg {
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
            if retain_codestreams {
                decoded_gain_map.jpeg_bytes = gain_map_jpeg.to_vec();
            }
            (primary_result?, Some(decoded_gain_map))
        }
        _ => (decode_primary_image(parsed.primary_jpeg)?, None),
    };

    if let Some(gamut) = parsed.color_metadata.gamut {
        primary_image.gamut = gamut;
    }
    if let Some(transfer) = parsed.color_metadata.transfer {
        primary_image.transfer = transfer;
    }

    Ok(DecodedJpeg {
        primary_image,
        primary_jpeg: if retain_codestreams {
            parsed.primary_jpeg.to_vec()
        } else {
            Vec::new()
        },
        color_metadata: parsed.color_metadata,
        ultra_hdr,
        gain_map,
    })
}

fn should_parallel_decode(primary_jpeg: &[u8], gain_map_jpeg: &[u8]) -> bool {
    primary_jpeg.len() + gain_map_jpeg.len() >= PARALLEL_DECODE_THRESHOLD_BYTES
}

fn resolved_primary_color_metadata(
    primary_image: &CoreRawImage,
    color_metadata: &ColorMetadata,
    has_gain_map: bool,
) -> Result<ColorMetadata> {
    if !has_gain_map || color_metadata.icc_profile.is_some() {
        return Ok(color_metadata.clone());
    }

    let gamut = color_metadata.gamut.unwrap_or(primary_image.gamut);
    let transfer = color_metadata.transfer.unwrap_or(primary_image.transfer);

    if gamut == CoreColorGamut::DisplayP3 && transfer == CoreColorTransfer::Srgb {
        let mut resolved = color_metadata.clone();
        resolved.icc_profile = Some(icc::display_p3().to_vec());
        resolved.gamut = Some(CoreColorGamut::DisplayP3);
        resolved.transfer = Some(CoreColorTransfer::Srgb);
        return Ok(resolved);
    }

    Err(Error::InvalidInput(
        "gain-map JPEG primary images require an ICC profile; for Display-P3/sRGB input use EncodeOptions::ultra_hdr_defaults() or provide an explicit ICC profile".into(),
    ))
}
