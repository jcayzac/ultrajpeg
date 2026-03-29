use crate::{
    error::{Error, Result},
    types::{ChromaSubsampling, ColorMetadata, DecodedGainMap},
};
use mozjpeg_rs::{Encoder, Preset, Subsampling};
use ultrahdr_core::{ColorGamut, ColorTransfer, GainMap, PixelFormat, RawImage};
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
    color_metadata: &ColorMetadata,
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

    if let Some(exif) = color_metadata.exif.as_ref() {
        encoder = encoder.exif_data(exif.clone());
    }
    if let Some(icc_profile) = color_metadata.icc_profile.as_ref() {
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

pub(crate) fn decode_gain_map(bytes: &[u8]) -> Result<DecodedGainMap> {
    let image = decode_image(bytes, ColorSpace::Luma, PixelFormat::Gray8)?;
    let gain_map = GainMap {
        width: image.width,
        height: image.height,
        channels: 1,
        data: image.data.clone(),
    };

    Ok(DecodedGainMap {
        image,
        gain_map,
        jpeg_bytes: Vec::new(),
        metadata: None,
    })
}

fn decode_image(
    bytes: &[u8],
    colorspace: ColorSpace,
    pixel_format: PixelFormat,
) -> Result<RawImage> {
    let options = DecoderOptions::default().jpeg_set_out_colorspace(colorspace);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes.to_vec()), options);
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
