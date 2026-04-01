use crate::{
    error::{Error, Result},
    types::{ChromaSubsampling, DecodedGainMap, PrimaryMetadata},
};
use mozjpeg_rs::{Encoder, Preset, Subsampling};
use ultrahdr_core::{ColorGamut, ColorTransfer, GainMap, GainMapMetadata, PixelFormat, RawImage};
use zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};
use zune_jpeg::JpegDecoder;

fn to_subsampling(subsampling: ChromaSubsampling) -> Subsampling {
    match subsampling {
        ChromaSubsampling::Yuv420 => Subsampling::S420,
        ChromaSubsampling::Yuv422 => Subsampling::S422,
        ChromaSubsampling::Yuv444 => Subsampling::S444,
        ChromaSubsampling::Yuv440 => Subsampling::S440,
    }
}

pub(crate) fn encode_image(
    image: &RawImage,
    quality: u8,
    progressive: bool,
    chroma_subsampling: ChromaSubsampling,
    primary_metadata: &PrimaryMetadata,
) -> Result<Vec<u8>> {
    let preset = if progressive {
        Preset::ProgressiveBalanced
    } else {
        Preset::BaselineBalanced
    };

    let mut encoder = Encoder::new(preset)
        .quality(quality)
        .progressive(progressive)
        .subsampling(match image.format {
            PixelFormat::Gray8 => Subsampling::Gray,
            _ => to_subsampling(chroma_subsampling),
        });

    if let Some(exif) = primary_metadata.exif.as_ref() {
        encoder = encoder.exif_data(exif.clone());
    }
    if let Some(icc_profile) = primary_metadata.color.icc_profile.as_ref() {
        encoder = encoder.icc_profile(icc_profile.clone());
    }

    match image.format {
        PixelFormat::Rgb8 => encoder.encode_rgb(&image.data, image.width, image.height),
        PixelFormat::Rgba8 => {
            let rgb = rgba_to_rgb(&image.data)?;
            encoder.encode_rgb(&rgb, image.width, image.height)
        }
        PixelFormat::Gray8 => encoder.encode_gray(&image.data, image.width, image.height),
        _ => Err(mozjpeg_rs::Error::UnsupportedColorSpace),
    }
    .map_err(Into::into)
}

pub(crate) fn decode_primary_image(bytes: &[u8]) -> Result<RawImage> {
    decode_image(bytes, ColorSpace::RGB, PixelFormat::Rgb8)
}

pub(crate) fn decode_gain_map(
    bytes: &[u8],
    metadata_hint: Option<&GainMapMetadata>,
) -> Result<DecodedGainMap> {
    let components = gain_map_component_count(bytes, metadata_hint)?;
    let (image, gain_map) = match components {
        1 => {
            let image = decode_image(bytes, ColorSpace::Luma, PixelFormat::Gray8)?;
            let gain_map = GainMap {
                width: image.width,
                height: image.height,
                channels: 1,
                data: image.data.clone(),
            };
            (image, gain_map)
        }
        3 => {
            let image = decode_image(bytes, ColorSpace::RGB, PixelFormat::Rgb8)?;
            let gain_map = GainMap {
                width: image.width,
                height: image.height,
                channels: 3,
                data: image.data.clone(),
            };
            (image, gain_map)
        }
        other => {
            return Err(Error::InvalidInput(format!(
                "unsupported gain-map JPEG component count {other}"
            )));
        }
    };

    Ok(DecodedGainMap {
        image,
        gain_map,
        metadata: None,
        jpeg_bytes: None,
    })
}

fn decode_image(
    bytes: &[u8],
    colorspace: ColorSpace,
    pixel_format: PixelFormat,
) -> Result<RawImage> {
    let options = DecoderOptions::default().jpeg_set_out_colorspace(colorspace);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    decoder.decode_headers()?;
    let (width, height) = decoder
        .dimensions()
        .ok_or_else(|| Error::Codec("decoder did not expose image dimensions".into()))?;
    let pixels = decoder.decode()?;
    let mut image = RawImage::from_data(
        width as u32,
        height as u32,
        pixel_format,
        ColorGamut::Bt709,
        ColorTransfer::Srgb,
        pixels,
    )?;
    image.gamut = ColorGamut::Bt709;
    image.transfer = ColorTransfer::Srgb;
    Ok(image)
}

fn rgba_to_rgb(rgba: &[u8]) -> Result<Vec<u8>> {
    if !rgba.len().is_multiple_of(4) {
        return Err(Error::InvalidInput(
            "RGBA input length must be divisible by 4".into(),
        ));
    }

    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    for chunk in rgba.chunks_exact(4) {
        rgb.extend_from_slice(&chunk[..3]);
    }
    Ok(rgb)
}

fn gain_map_component_count(bytes: &[u8], metadata_hint: Option<&GainMapMetadata>) -> Result<u8> {
    if let Some(metadata) = metadata_hint {
        return Ok(if metadata.is_single_channel() { 1 } else { 3 });
    }

    jpeg_component_count(bytes)
}

fn jpeg_component_count(bytes: &[u8]) -> Result<u8> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return Err(Error::Container("invalid JPEG signature".into()));
    }

    let mut offset = 2usize;

    while offset + 1 < bytes.len() {
        if bytes[offset] != 0xFF {
            return Err(Error::Container(format!(
                "invalid JPEG marker prefix at byte offset {offset}"
            )));
        }

        while offset < bytes.len() && bytes[offset] == 0xFF {
            offset += 1;
        }
        if offset >= bytes.len() {
            return Err(Error::Container("truncated JPEG marker stream".into()));
        }

        let marker = bytes[offset];
        offset += 1;

        if marker == 0xD9 {
            break;
        }
        if !marker_has_length(marker) {
            continue;
        }
        if offset + 2 > bytes.len() {
            return Err(Error::Container("truncated JPEG marker length".into()));
        }

        let segment_len = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
        if segment_len < 2 {
            return Err(Error::Container("invalid JPEG marker length".into()));
        }
        let contents_start = offset + 2;
        let contents_end = offset + segment_len;
        if contents_end > bytes.len() {
            return Err(Error::Container("truncated JPEG segment".into()));
        }

        if is_start_of_frame(marker) {
            if contents_start + 6 > contents_end {
                return Err(Error::Container("truncated JPEG SOF segment".into()));
            }
            return Ok(bytes[contents_start + 5]);
        }

        offset = contents_end;
        if marker == 0xDA {
            break;
        }
    }

    Err(Error::Container(
        "could not determine JPEG component count".into(),
    ))
}

fn marker_has_length(marker: u8) -> bool {
    matches!(marker, 0xC0..=0xFE if !matches!(marker, 0xD0..=0xD9))
}

fn is_start_of_frame(marker: u8) -> bool {
    matches!(
        marker,
        0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF
    )
}
