use std::{fs, path::PathBuf};

use img_parts::{ImageICC, jpeg::Jpeg};
use ultrahdr_core::{
    ColorGamut, ColorTransfer, GainMapMetadata, PixelFormat, RawImage, gainmap::HdrOutputFormat,
};
use ultrajpeg::{
    ColorMetadata, CompressedImage, DecodeOptions, EncodeOptions, Encoder as CompatEncoder,
    GainMapEncodeOptions, ImgLabel, RawImage as CompatRawImage, UltraJpegEncoder, decode,
    decode_with_options, jpeg, sys,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    fs::create_dir_all(&root)?;

    let primary = sample_primary();
    let gain_map = sample_gain_map();

    let plain_options = EncodeOptions {
        quality: 88,
        progressive: true,
        color_metadata: ColorMetadata {
            icc_profile: Some(b"fixture-icc-profile".to_vec()),
            exif: Some(b"II*\0\x08\0\0\0\0\0".to_vec()),
            gamut: Some(ColorGamut::DisplayP3),
            transfer: Some(ColorTransfer::Srgb),
        },
        ..EncodeOptions::default()
    };
    let plain = UltraJpegEncoder::new(plain_options).encode(&primary)?;
    fs::write(root.join("plain-sdr.jpg"), &plain)?;

    let plain_compat = jpeg::Encoder::new(jpeg::Preset::ProgressiveSmallest)
        .quality(88)
        .icc_profile(b"fixture-icc-profile".to_vec())
        .encode_rgb(&primary.data, primary.width, primary.height)?;
    fs::write(root.join("plain-sdr-compat.jpg"), &plain_compat)?;

    let ultrahdr_options = EncodeOptions {
        quality: 87,
        progressive: true,
        color_metadata: ColorMetadata {
            icc_profile: Some(b"fixture-icc-profile".to_vec()),
            exif: Some(b"II*\0\x08\0\0\0\0\0".to_vec()),
            gamut: Some(ColorGamut::DisplayP3),
            transfer: Some(ColorTransfer::Pq),
        },
        gain_map: Some(GainMapEncodeOptions {
            image: gain_map,
            metadata: sample_gain_map_metadata(),
            quality: 80,
            progressive: false,
        }),
        ..EncodeOptions::default()
    };
    let ultrahdr = UltraJpegEncoder::new(ultrahdr_options).encode(&primary)?;
    fs::write(root.join("sample-ultrahdr.jpg"), &ultrahdr)?;

    let ultrahdr_compat = compat_ultrahdr_fixture(&primary)?;
    fs::write(root.join("sample-ultrahdr-compat.jpg"), &ultrahdr_compat)?;

    let decoded_plain = decode(&plain)?;
    let decoded_ultrahdr = decode(&ultrahdr)?;
    let reconstructed = decoded_ultrahdr.reconstruct_hdr(4.0, HdrOutputFormat::Pq1010102)?;
    let decoded_plain_compat = decode(&plain_compat)?;
    let decoded_ultrahdr_compat = decode(&ultrahdr_compat)?;
    fs::write(
        root.join("README.md"),
        fixture_readme(
            &decoded_plain,
            &decoded_plain_compat,
            &decoded_ultrahdr,
            &decoded_ultrahdr_compat,
            &reconstructed,
        ),
    )?;

    let skipped = decode_with_options(
        &ultrahdr,
        DecodeOptions {
            decode_gain_map: false,
        },
    )?;
    assert!(decoded_plain.gain_map.is_none());
    assert!(decoded_ultrahdr.gain_map.is_some());
    assert!(decoded_plain_compat.gain_map.is_none());
    assert!(decoded_ultrahdr_compat.gain_map.is_some());
    assert!(skipped.gain_map.is_none());

    let compat_jpeg = Jpeg::from_bytes(img_parts::Bytes::from(plain_compat))?;
    assert!(compat_jpeg.icc_profile().is_some());

    Ok(())
}

fn sample_primary() -> RawImage {
    RawImage::from_data(
        4,
        4,
        PixelFormat::Rgb8,
        ColorGamut::DisplayP3,
        ColorTransfer::Srgb,
        vec![
            255, 0, 0, 255, 128, 0, 255, 255, 0, 255, 255, 255, //
            0, 255, 0, 0, 128, 255, 0, 255, 255, 255, 0, 255, //
            0, 0, 255, 32, 64, 255, 96, 160, 255, 200, 240, 255, //
            16, 16, 16, 64, 64, 64, 160, 160, 160, 240, 240, 240, //
        ],
    )
    .expect("sample primary image")
}

fn sample_gain_map() -> RawImage {
    RawImage::from_data(
        4,
        4,
        PixelFormat::Gray8,
        ColorGamut::Bt709,
        ColorTransfer::Linear,
        vec![
            0, 32, 64, 96, //
            32, 64, 96, 128, //
            64, 96, 128, 160, //
            96, 128, 160, 192, //
        ],
    )
    .expect("sample gain map image")
}

fn sample_gain_map_metadata() -> GainMapMetadata {
    GainMapMetadata {
        max_content_boost: [4.0; 3],
        min_content_boost: [1.0; 3],
        gamma: [1.0; 3],
        offset_sdr: [1.0 / 64.0; 3],
        offset_hdr: [1.0 / 64.0; 3],
        hdr_capacity_min: 1.0,
        hdr_capacity_max: 4.0,
        use_base_color_space: true,
    }
}

fn compat_ultrahdr_fixture(primary: &RawImage) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let hdr_pixels = [
        0x000000c0u32,
        0x100080c0,
        0x200100c0,
        0x300180c0,
        0x080100c0,
        0x180180c0,
        0x280200c0,
        0x380280c0,
        0x100200c0,
        0x200280c0,
        0x300300c0,
        0x3ff380c0,
        0x180300c0,
        0x280380c0,
        0x3803c0c0,
        0x3ff3ffc0,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect::<Vec<_>>();

    let mut hdr_bytes = hdr_pixels;
    let mut base_bytes = jpeg::Encoder::new(jpeg::Preset::ProgressiveSmallest)
        .quality(90)
        .encode_rgb(&primary.data, primary.width, primary.height)?;
    let mut hdr_raw = CompatRawImage::packed(
        sys::uhdr_img_fmt::UHDR_IMG_FMT_32bppRGBA1010102,
        primary.width,
        primary.height,
        &mut hdr_bytes,
        sys::uhdr_color_gamut::UHDR_CG_DISPLAY_P3,
        sys::uhdr_color_transfer::UHDR_CT_PQ,
        sys::uhdr_color_range::UHDR_CR_FULL_RANGE,
    )?;
    let mut base = CompressedImage::from_bytes(
        &mut base_bytes,
        sys::uhdr_color_gamut::UHDR_CG_BT_709,
        sys::uhdr_color_transfer::UHDR_CT_SRGB,
        sys::uhdr_color_range::UHDR_CR_FULL_RANGE,
    );

    let mut encoder = CompatEncoder::new()?;
    encoder.set_raw_image(&mut hdr_raw, ImgLabel::UHDR_HDR_IMG)?;
    encoder.set_compressed_image(&mut base, ImgLabel::UHDR_SDR_IMG)?;
    encoder.set_quality(90, ImgLabel::UHDR_BASE_IMG)?;
    encoder.set_quality(90, ImgLabel::UHDR_GAIN_MAP_IMG)?;
    encoder.set_output_format(sys::uhdr_codec::UHDR_CODEC_JPG)?;
    encoder.encode()?;
    Ok(encoder
        .encoded_stream()
        .ok_or("missing encoded stream")?
        .bytes()?
        .to_vec())
}

fn fixture_readme(
    plain: &ultrajpeg::DecodedJpeg,
    plain_compat: &ultrajpeg::DecodedJpeg,
    ultrahdr: &ultrajpeg::DecodedJpeg,
    ultrahdr_compat: &ultrajpeg::DecodedJpeg,
    reconstructed: &RawImage,
) -> String {
    format!(
        "# Fixture Vectors\n\n\
         `plain-sdr.jpg`\n\
         - primary dimensions: {}x{}\n\
         - gain map: {}\n\
         - icc bytes: {}\n\n\
         `plain-sdr-compat.jpg`\n\
         - primary dimensions: {}x{}\n\
         - gain map: {}\n\
         - icc bytes: {}\n\n\
         `sample-ultrahdr.jpg`\n\
         - primary dimensions: {}x{}\n\
         - gain map: {}\n\
         - xmp: {}\n\
         - iso21496-1: {}\n\n\
         `sample-ultrahdr-compat.jpg`\n\
         - primary dimensions: {}x{}\n\
         - gain map: {}\n\
         - xmp: {}\n\
         - iso21496-1: {}\n\n\
         Reconstruction check\n\
         - reconstructed format: {:?}\n",
        plain.primary_image.width,
        plain.primary_image.height,
        plain.gain_map.is_some(),
        plain
            .color_metadata
            .icc_profile
            .as_ref()
            .map_or(0, Vec::len),
        plain_compat.primary_image.width,
        plain_compat.primary_image.height,
        plain_compat.gain_map.is_some(),
        plain_compat
            .color_metadata
            .icc_profile
            .as_ref()
            .map_or(0, Vec::len),
        ultrahdr.primary_image.width,
        ultrahdr.primary_image.height,
        ultrahdr.gain_map.is_some(),
        ultrahdr
            .ultra_hdr
            .as_ref()
            .and_then(|metadata| metadata.xmp.as_ref())
            .is_some(),
        ultrahdr
            .ultra_hdr
            .as_ref()
            .and_then(|metadata| metadata.iso_21496_1.as_ref())
            .is_some(),
        ultrahdr_compat.primary_image.width,
        ultrahdr_compat.primary_image.height,
        ultrahdr_compat.gain_map.is_some(),
        ultrahdr_compat
            .ultra_hdr
            .as_ref()
            .and_then(|metadata| metadata.xmp.as_ref())
            .is_some(),
        ultrahdr_compat
            .ultra_hdr
            .as_ref()
            .and_then(|metadata| metadata.iso_21496_1.as_ref())
            .is_some(),
        reconstructed.format
    )
}
