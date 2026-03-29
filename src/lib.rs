#![doc = include_str!("../README.md")]

mod codec;
mod compat;
mod container;
mod error;
mod metadata;
mod types;

pub use compat::{
    Codec, ColorGamut, ColorRange, ColorTransfer, CompressedImage, DecodedPacked, Decoder,
    EncodedStream, Encoder, ImageFormat, ImgLabel, RawImage, jpeg, mozjpeg, sys,
};
pub use error::{Error, Result};
pub use types::{
    ChromaSubsampling, ColorMetadata, DecodeOptions, DecodedGainMap, DecodedJpeg, EncodeOptions,
    GainMapEncodeOptions, UltraHdrMetadata, UltraJpegEncoder,
};
pub use ultrahdr_core::GainMapMetadata;

use codec::{decode_gain_map, decode_primary_image, encode_image};
use container::{assemble_container, parse_container};
use metadata::{build_ultra_hdr_metadata, parse_ultra_hdr_metadata};
use ultrahdr_core::{RawImage as CoreRawImage, gainmap::HdrOutputFormat};

/// Decode a JPEG or UltraHDR JPEG into a structured representation.
pub fn decode(bytes: &[u8]) -> Result<DecodedJpeg> {
    decode_with_options(bytes, DecodeOptions::default())
}

/// Decode a JPEG or UltraHDR JPEG using explicit decode options.
pub fn decode_with_options(bytes: &[u8], options: DecodeOptions) -> Result<DecodedJpeg> {
    let parsed = parse_container(bytes, &options)?;
    let mut primary_image = decode_primary_image(&parsed.primary_jpeg)?;

    if let Some(gamut) = parsed.color_metadata.gamut {
        primary_image.gamut = gamut;
    }
    if let Some(transfer) = parsed.color_metadata.transfer {
        primary_image.transfer = transfer;
    }

    let ultra_hdr = parse_ultra_hdr_metadata(parsed.xmp.as_deref(), parsed.iso.as_deref())?;
    let gain_map = match parsed.gain_map_jpeg {
        Some(gain_map_jpeg) if options.decode_gain_map => {
            let mut decoded = decode_gain_map(&gain_map_jpeg)?;
            if let Some(ultra_hdr) = ultra_hdr.as_ref() {
                decoded.metadata = ultra_hdr.gain_map_metadata.clone();
            }
            decoded.jpeg_bytes = gain_map_jpeg;
            Some(decoded)
        }
        _ => None,
    };

    Ok(DecodedJpeg {
        primary_image,
        primary_jpeg: parsed.primary_jpeg,
        color_metadata: parsed.color_metadata,
        ultra_hdr,
        gain_map,
    })
}

/// Encode a JPEG or UltraHDR JPEG with optional gain-map metadata.
pub fn encode(primary_image: &CoreRawImage, options: &EncodeOptions) -> Result<Vec<u8>> {
    UltraJpegEncoder::new(options.clone()).encode(primary_image)
}

impl UltraJpegEncoder {
    /// Create a new encoder with explicit options.
    #[must_use]
    pub fn new(options: EncodeOptions) -> Self {
        Self { options }
    }

    /// Encode a primary JPEG, optionally bundling a gain map and UltraHDR metadata.
    pub fn encode(&self, primary_image: &CoreRawImage) -> Result<Vec<u8>> {
        let primary_jpeg = encode_image(
            primary_image,
            self.options.quality,
            self.options.progressive,
            self.options.chroma_subsampling,
            &self.options.color_metadata,
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

        assemble_container(
            &primary_jpeg,
            gain_map_jpeg.as_deref(),
            &self.options.color_metadata,
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
