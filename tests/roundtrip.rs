use img_parts::{
    Bytes,
    jpeg::{Jpeg, markers},
};
use ultrahdr_core::{
    ColorGamut, ColorTransfer, GainMapMetadata, PixelFormat, RawImage, gainmap::HdrOutputFormat,
    metadata::find_jpeg_boundaries,
};
use ultrajpeg::{
    ColorMetadata, ComputeGainMapOptions, DecodeOptions, EncodeOptions, GainMapChannels,
    GainMapEncodeOptions, UltraHdrEncodeOptions, UltraJpegEncoder, compute_gain_map, decode,
    decode_with_options, encode_ultra_hdr, icc, inspect,
};
use ultrajpeg::{
    CompressedImage, Decoder as CompatDecoder, Encoder as CompatEncoder, ImgLabel,
    RawImage as CompatRawImage, jpeg, sys,
};

const XMP_NAMESPACE: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const ISO_NAMESPACE: &[u8] = b"urn:iso:std:iso:ts:21496:-1\0";

fn sample_primary() -> RawImage {
    let data = vec![
        255, 0, 0, 255, 128, 0, 255, 255, 0, 255, 255, 255, //
        0, 255, 0, 0, 128, 255, 0, 255, 255, 255, 0, 255, //
        0, 0, 255, 32, 64, 255, 96, 160, 255, 200, 240, 255, //
        16, 16, 16, 64, 64, 64, 160, 160, 160, 240, 240, 240, //
    ];
    RawImage::from_data(
        4,
        4,
        PixelFormat::Rgb8,
        ColorGamut::DisplayP3,
        ColorTransfer::Srgb,
        data,
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

fn sample_multichannel_gain_map() -> RawImage {
    RawImage::from_data(
        4,
        4,
        PixelFormat::Rgb8,
        ColorGamut::Bt709,
        ColorTransfer::Linear,
        vec![
            0, 32, 64, 32, 64, 96, 64, 96, 128, 96, 128, 160, //
            16, 48, 80, 48, 80, 112, 80, 112, 144, 112, 144, 176, //
            32, 64, 96, 64, 96, 128, 96, 128, 160, 128, 160, 192, //
            48, 80, 112, 80, 112, 144, 112, 144, 176, 144, 176, 208, //
        ],
    )
    .expect("sample multichannel gain map image")
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

fn sample_multichannel_gain_map_metadata() -> GainMapMetadata {
    GainMapMetadata {
        max_content_boost: [4.0, 8.0, 2.0],
        min_content_boost: [1.0, 1.0, 1.0],
        gamma: [1.0, 1.2, 1.4],
        offset_sdr: [1.0 / 64.0; 3],
        offset_hdr: [1.0 / 64.0; 3],
        hdr_capacity_min: 1.0,
        hdr_capacity_max: 8.0,
        use_base_color_space: true,
    }
}

fn sample_bt709_primary() -> RawImage {
    let mut primary = sample_primary();
    primary.gamut = ColorGamut::Bt709;
    primary.transfer = ColorTransfer::Srgb;
    primary
}

fn sample_hdr() -> RawImage {
    let pixels = [
        [2.0f32, 0.0, 0.0, 1.0],
        [1.5, 0.6, 0.0, 1.0],
        [0.4, 1.8, 0.0, 1.0],
        [0.3, 0.4, 1.6, 1.0],
        [0.0, 1.5, 0.0, 1.0],
        [0.0, 0.7, 1.6, 1.0],
        [0.0, 1.8, 1.4, 1.0],
        [1.8, 0.0, 1.5, 1.0],
        [0.0, 0.0, 1.7, 1.0],
        [0.6, 0.8, 1.7, 1.0],
        [0.9, 1.4, 2.0, 1.0],
        [1.7, 1.9, 1.2, 1.0],
        [0.1, 0.1, 0.1, 1.0],
        [0.2, 0.2, 0.2, 1.0],
        [0.8, 0.8, 0.8, 1.0],
        [1.2, 1.2, 1.2, 1.0],
    ]
    .into_iter()
    .flat_map(|rgba| rgba.into_iter().flat_map(f32::to_le_bytes))
    .collect::<Vec<_>>();

    RawImage::from_data(
        4,
        4,
        PixelFormat::Rgba32F,
        ColorGamut::DisplayP3,
        ColorTransfer::Linear,
        pixels,
    )
    .expect("sample hdr image")
}

fn split_embedded_jpegs(bytes: &[u8]) -> Vec<&[u8]> {
    find_jpeg_boundaries(bytes)
        .into_iter()
        .map(|(start, end)| &bytes[start..end])
        .collect()
}

fn xmp_payload(bytes: &[u8]) -> Option<String> {
    let jpeg = Jpeg::from_bytes(Bytes::copy_from_slice(bytes)).unwrap();
    jpeg.segments().iter().find_map(|segment| {
        let contents = segment.contents();
        (segment.marker() == markers::APP1 && contents.starts_with(XMP_NAMESPACE))
            .then(|| String::from_utf8(contents[XMP_NAMESPACE.len()..].to_vec()).unwrap())
    })
}

fn iso_payload(bytes: &[u8]) -> Option<Vec<u8>> {
    let jpeg = Jpeg::from_bytes(Bytes::copy_from_slice(bytes)).unwrap();
    jpeg.segments().iter().find_map(|segment| {
        let contents = segment.contents();
        (segment.marker() == markers::APP2 && contents.starts_with(ISO_NAMESPACE))
            .then(|| contents[ISO_NAMESPACE.len()..].to_vec())
    })
}

#[test]
fn encodes_and_decodes_ultrahdr_roundtrip() {
    let options = EncodeOptions {
        quality: 87,
        progressive: true,
        color_metadata: ColorMetadata {
            icc_profile: Some(b"fake-icc-profile".to_vec()),
            exif: Some(b"II*\0\x08\0\0\0\0\0".to_vec()),
            gamut: Some(ColorGamut::DisplayP3),
            transfer: Some(ColorTransfer::Srgb),
        },
        gain_map: Some(GainMapEncodeOptions {
            image: sample_gain_map(),
            metadata: sample_gain_map_metadata(),
            quality: 80,
            progressive: false,
        }),
        ..EncodeOptions::default()
    };

    let encoded = UltraJpegEncoder::new(options)
        .encode(&sample_primary())
        .unwrap();
    let decoded = decode(&encoded).unwrap();

    assert_eq!(decoded.primary_image.width, 4);
    assert_eq!(decoded.primary_image.height, 4);
    assert_eq!(decoded.primary_image.format, PixelFormat::Rgb8);
    assert_eq!(
        decoded.color_metadata.icc_profile.as_deref(),
        Some(b"fake-icc-profile".as_slice())
    );
    assert_eq!(decoded.color_metadata.gamut, Some(ColorGamut::DisplayP3));
    assert_eq!(decoded.color_metadata.transfer, Some(ColorTransfer::Srgb));

    let ultra_hdr = decoded.ultra_hdr.as_ref().expect("ultra hdr metadata");
    assert!(ultra_hdr.xmp.as_deref().unwrap().contains("hdrgm:Version"));
    assert!(ultra_hdr.iso_21496_1.is_some());

    let gain_map = decoded.gain_map.as_ref().expect("decoded gain map");
    assert_eq!(gain_map.image.width, 4);
    assert_eq!(gain_map.image.height, 4);
    assert_eq!(gain_map.image.format, PixelFormat::Gray8);
    assert!(!gain_map.jpeg_bytes.is_empty());

    let metadata = gain_map.metadata.as_ref().expect("gain map metadata");
    assert!((metadata.max_content_boost[0] - 4.0).abs() < 0.01);
    assert!((metadata.hdr_capacity_max - 4.0).abs() < 0.01);

    let hdr = decoded
        .reconstruct_hdr(4.0, HdrOutputFormat::LinearFloat)
        .unwrap();
    assert_eq!(hdr.width, 4);
    assert_eq!(hdr.height, 4);
}

#[test]
fn encoder_splits_primary_container_xmp_and_gain_map_metadata_xmp() {
    let options = EncodeOptions {
        gain_map: Some(GainMapEncodeOptions {
            image: sample_gain_map(),
            metadata: sample_gain_map_metadata(),
            quality: 80,
            progressive: false,
        }),
        ..EncodeOptions::ultra_hdr_defaults()
    };

    let encoded = UltraJpegEncoder::new(options)
        .encode(&sample_primary())
        .unwrap();
    let codestreams = split_embedded_jpegs(&encoded);

    assert_eq!(codestreams.len(), 2);

    let primary_xmp = xmp_payload(codestreams[0]).expect("primary xmp");
    assert!(primary_xmp.contains("Item:Semantic=\"Primary\""));
    assert!(primary_xmp.contains("Item:Semantic=\"GainMap\""));
    assert!(!primary_xmp.contains("hdrgm:GainMapMax"));
    assert!(iso_payload(codestreams[0]).is_none());

    let gain_map_xmp = xmp_payload(codestreams[1]).expect("gain map xmp");
    assert!(gain_map_xmp.contains("hdrgm:GainMapMax"));
    assert!(gain_map_xmp.contains("hdrgm:HDRCapacityMax"));
    assert!(!gain_map_xmp.contains("Item:Semantic=\"GainMap\""));
    assert!(iso_payload(codestreams[1]).is_some());
}

#[test]
fn decodes_plain_jpeg_without_gain_map() {
    let encoded = UltraJpegEncoder::new(EncodeOptions::default())
        .encode(&sample_primary())
        .unwrap();
    let decoded = decode(&encoded).unwrap();

    assert!(decoded.gain_map.is_none());
    assert!(decoded.ultra_hdr.is_none());
    assert_eq!(decoded.primary_image.width, 4);
}

#[test]
fn decode_options_can_skip_gain_map_decoding() {
    let options = EncodeOptions {
        gain_map: Some(GainMapEncodeOptions {
            image: sample_gain_map(),
            metadata: sample_gain_map_metadata(),
            quality: 75,
            progressive: false,
        }),
        ..EncodeOptions::default()
    };

    let encoded = UltraJpegEncoder::new(options)
        .encode(&sample_primary())
        .unwrap();
    let decoded = decode_with_options(
        &encoded,
        DecodeOptions {
            decode_gain_map: false,
        },
    )
    .unwrap();

    assert!(decoded.gain_map.is_none());
    assert!(decoded.ultra_hdr.is_some());
}

#[test]
fn high_level_gain_map_encode_auto_injects_display_p3_icc_for_display_p3_primary() {
    let options = EncodeOptions {
        gain_map: Some(GainMapEncodeOptions {
            image: sample_gain_map(),
            metadata: sample_gain_map_metadata(),
            quality: 75,
            progressive: false,
        }),
        ..EncodeOptions::default()
    };

    let encoded = UltraJpegEncoder::new(options)
        .encode(&sample_primary())
        .unwrap();
    let inspected = inspect(&encoded).unwrap();

    assert_eq!(
        inspected.color_metadata.icc_profile.as_deref(),
        Some(icc::display_p3())
    );
    assert_eq!(inspected.color_metadata.gamut, Some(ColorGamut::DisplayP3));
    assert_eq!(inspected.color_metadata.transfer, Some(ColorTransfer::Srgb));
}

#[test]
fn high_level_gain_map_encode_requires_explicit_icc_for_non_display_p3_primary() {
    let options = EncodeOptions {
        gain_map: Some(GainMapEncodeOptions {
            image: sample_gain_map(),
            metadata: sample_gain_map_metadata(),
            quality: 75,
            progressive: false,
        }),
        ..EncodeOptions::default()
    };

    let error = UltraJpegEncoder::new(options)
        .encode(&sample_bt709_primary())
        .unwrap_err();

    assert!(error.to_string().contains("require an ICC profile"));
}

#[test]
fn compute_gain_map_defaults_to_single_channel() {
    let computed = compute_gain_map(
        &sample_hdr(),
        &sample_primary(),
        &ComputeGainMapOptions::default(),
    )
    .unwrap();

    assert_eq!(computed.image.format, PixelFormat::Gray8);
    assert_eq!(computed.image.width, 1);
    assert_eq!(computed.image.height, 1);
    assert_eq!(computed.metadata.gamma, [1.0; 3]);
}

#[test]
fn compute_gain_map_multichannel_requires_explicit_opt_in() {
    let computed = compute_gain_map(
        &sample_hdr(),
        &sample_primary(),
        &ComputeGainMapOptions {
            channels: GainMapChannels::Multi,
        },
    )
    .unwrap();

    assert_eq!(computed.image.format, PixelFormat::Rgb8);
    assert_eq!(computed.image.width, 1);
    assert_eq!(computed.image.height, 1);
    assert_eq!(computed.image.data.len(), 3);
}

#[test]
fn computed_gain_map_composes_with_structured_encode() {
    let computed = compute_gain_map(
        &sample_hdr(),
        &sample_primary(),
        &ComputeGainMapOptions::default(),
    )
    .unwrap();
    let options = EncodeOptions {
        gain_map: Some(computed.into_encode_options(83, false)),
        ..EncodeOptions::ultra_hdr_defaults()
    };

    let encoded = UltraJpegEncoder::new(options)
        .encode(&sample_primary())
        .unwrap();
    let decoded = decode(&encoded).unwrap();

    assert!(decoded.gain_map.is_some());
    assert_eq!(decoded.gain_map.as_ref().unwrap().gain_map.channels, 1);
}

#[test]
fn decode_uses_xmp_metadata_when_iso_is_absent() {
    let options = EncodeOptions {
        gain_map: Some(GainMapEncodeOptions {
            image: sample_gain_map(),
            metadata: sample_gain_map_metadata(),
            quality: 80,
            progressive: false,
        }),
        ..EncodeOptions::ultra_hdr_defaults()
    };
    let mut encoded = UltraJpegEncoder::new(options)
        .encode(&sample_primary())
        .unwrap();

    replace_once(
        &mut encoded,
        b"urn:iso:std:iso:ts:21496:-1\0",
        b"urn:xso:std:iso:ts:21496:-1\0",
    );

    let decoded = decode(&encoded).unwrap();
    let ultra_hdr = decoded.ultra_hdr.as_ref().unwrap();

    assert!(ultra_hdr.xmp.is_some());
    assert!(ultra_hdr.iso_21496_1.is_none());
    assert!(
        (ultra_hdr
            .gain_map_metadata
            .as_ref()
            .unwrap()
            .hdr_capacity_max
            - 4.0)
            .abs()
            < 0.01
    );
}

#[test]
fn decode_uses_iso_metadata_when_xmp_is_absent() {
    let options = EncodeOptions {
        gain_map: Some(GainMapEncodeOptions {
            image: sample_gain_map(),
            metadata: sample_gain_map_metadata(),
            quality: 80,
            progressive: false,
        }),
        ..EncodeOptions::ultra_hdr_defaults()
    };
    let mut encoded = UltraJpegEncoder::new(options)
        .encode(&sample_primary())
        .unwrap();

    replace_all(
        &mut encoded,
        b"http://ns.adobe.com/xap/1.0/\0",
        b"http://ns.adobe.com/xaq/1.0/\0",
    );

    let decoded = decode(&encoded).unwrap();
    let ultra_hdr = decoded.ultra_hdr.as_ref().unwrap();

    assert!(ultra_hdr.xmp.is_none());
    assert!(ultra_hdr.iso_21496_1.is_some());
    assert!(
        (ultra_hdr
            .gain_map_metadata
            .as_ref()
            .unwrap()
            .hdr_capacity_max
            - 4.0)
            .abs()
            < 0.01
    );
}

#[test]
fn decode_prefers_iso_metadata_over_xmp_when_both_are_present() {
    let options = EncodeOptions {
        gain_map: Some(GainMapEncodeOptions {
            image: sample_gain_map(),
            metadata: sample_gain_map_metadata(),
            quality: 80,
            progressive: false,
        }),
        ..EncodeOptions::ultra_hdr_defaults()
    };
    let mut encoded = UltraJpegEncoder::new(options)
        .encode(&sample_primary())
        .unwrap();

    replace_once(
        &mut encoded,
        b"hdrgm:HDRCapacityMax=\"2.000000\"",
        b"hdrgm:HDRCapacityMax=\"0.000000\"",
    );

    let decoded = decode(&encoded).unwrap();
    let metadata = decoded
        .ultra_hdr
        .as_ref()
        .and_then(|ultra_hdr| ultra_hdr.gain_map_metadata.as_ref())
        .unwrap();

    assert!((metadata.hdr_capacity_max - 4.0).abs() < 0.01);
}

#[test]
fn decode_rejects_xmp_fallback_when_base_rendition_is_hdr_true() {
    let options = EncodeOptions {
        gain_map: Some(GainMapEncodeOptions {
            image: sample_gain_map(),
            metadata: sample_gain_map_metadata(),
            quality: 80,
            progressive: false,
        }),
        ..EncodeOptions::ultra_hdr_defaults()
    };
    let mut encoded = UltraJpegEncoder::new(options)
        .encode(&sample_primary())
        .unwrap();

    replace_once(
        &mut encoded,
        b"urn:iso:std:iso:ts:21496:-1\0",
        b"urn:xso:std:iso:ts:21496:-1\0",
    );
    replace_once(
        &mut encoded,
        b"hdrgm:BaseRenditionIsHDR=\"False\"",
        b"hdrgm:BaseRenditionIsHDR=\"True \"",
    );

    let decoded = decode(&encoded).unwrap();
    let ultra_hdr = decoded.ultra_hdr.as_ref().unwrap();

    assert!(ultra_hdr.xmp.is_some());
    assert!(ultra_hdr.iso_21496_1.is_none());
    assert!(ultra_hdr.gain_map_metadata.is_none());
}

#[test]
fn decode_rejects_xmp_fallback_when_required_fields_are_missing() {
    let options = EncodeOptions {
        gain_map: Some(GainMapEncodeOptions {
            image: sample_gain_map(),
            metadata: sample_gain_map_metadata(),
            quality: 80,
            progressive: false,
        }),
        ..EncodeOptions::ultra_hdr_defaults()
    };
    let mut encoded = UltraJpegEncoder::new(options)
        .encode(&sample_primary())
        .unwrap();

    replace_once(
        &mut encoded,
        b"urn:iso:std:iso:ts:21496:-1\0",
        b"urn:xso:std:iso:ts:21496:-1\0",
    );
    replace_once(
        &mut encoded,
        b"hdrgm:HDRCapacityMax",
        b"hdrgm:HDRCapacityMaz",
    );

    let decoded = decode(&encoded).unwrap();
    let ultra_hdr = decoded.ultra_hdr.as_ref().unwrap();

    assert!(ultra_hdr.xmp.is_some());
    assert!(ultra_hdr.iso_21496_1.is_none());
    assert!(ultra_hdr.gain_map_metadata.is_none());
}

#[test]
fn encode_ultra_hdr_convenience_wrapper_packages_image() {
    let encoded = encode_ultra_hdr(
        &sample_hdr(),
        &sample_primary(),
        &UltraHdrEncodeOptions::default(),
    )
    .unwrap();
    let inspected = inspect(&encoded).unwrap();

    assert!(inspected.gain_map_jpeg_len.is_some());
    assert_eq!(
        inspected.color_metadata.icc_profile.as_deref(),
        Some(icc::display_p3())
    );
}

#[test]
fn encode_ultra_hdr_rejects_prepopulated_primary_gain_map() {
    let mut options = UltraHdrEncodeOptions::default();
    options.primary.gain_map = Some(GainMapEncodeOptions {
        image: sample_gain_map(),
        metadata: sample_gain_map_metadata(),
        quality: 80,
        progressive: false,
    });

    let error = encode_ultra_hdr(&sample_hdr(), &sample_primary(), &options).unwrap_err();
    assert!(error.to_string().contains("primary.gain_map"));
}

#[test]
fn encodes_and_decodes_multichannel_gain_map_roundtrip() {
    let options = EncodeOptions {
        gain_map: Some(GainMapEncodeOptions {
            image: sample_multichannel_gain_map(),
            metadata: sample_multichannel_gain_map_metadata(),
            quality: 82,
            progressive: false,
        }),
        ..EncodeOptions::ultra_hdr_defaults()
    };

    let encoded = UltraJpegEncoder::new(options)
        .encode(&sample_primary())
        .unwrap();
    let decoded = decode(&encoded).unwrap();

    let gain_map = decoded.gain_map.as_ref().expect("decoded gain map");
    assert_eq!(gain_map.image.format, PixelFormat::Rgb8);
    assert_eq!(gain_map.gain_map.channels, 3);
    assert_eq!(gain_map.gain_map.data.len(), 4 * 4 * 3);

    let metadata = decoded
        .ultra_hdr
        .as_ref()
        .and_then(|ultra_hdr| ultra_hdr.gain_map_metadata.as_ref())
        .unwrap();
    assert_eq!(metadata.max_content_boost, [4.0, 8.0, 2.0]);
    assert_eq!(metadata.gamma, [1.0, 1.2, 1.4]);

    let hdr = decoded
        .reconstruct_hdr(4.0, HdrOutputFormat::LinearFloat)
        .unwrap();
    assert_eq!(hdr.width, 4);
    assert_eq!(hdr.height, 4);
}

#[test]
fn display_p3_helpers_embed_the_built_in_profile() {
    let color_metadata = ColorMetadata::display_p3();
    assert_eq!(
        color_metadata.icc_profile.as_deref(),
        Some(icc::display_p3())
    );
    assert_eq!(color_metadata.gamut, Some(ColorGamut::DisplayP3));
    assert_eq!(color_metadata.transfer, Some(ColorTransfer::Srgb));

    let options = EncodeOptions {
        gain_map: Some(GainMapEncodeOptions {
            image: sample_gain_map(),
            metadata: sample_gain_map_metadata(),
            quality: 80,
            progressive: false,
        }),
        ..EncodeOptions::ultra_hdr_defaults()
    };

    let encoded = UltraJpegEncoder::new(options)
        .encode(&sample_primary())
        .unwrap();
    let inspected = inspect(&encoded).unwrap();

    assert_eq!(
        inspected.color_metadata.icc_profile.as_deref(),
        Some(icc::display_p3())
    );
    assert_eq!(inspected.color_metadata.gamut, Some(ColorGamut::DisplayP3));
    assert_eq!(inspected.color_metadata.transfer, Some(ColorTransfer::Srgb));
    assert!(inspected.gain_map_jpeg_len.is_some());
}

#[test]
fn ultra_hdr_defaults_preserve_regular_jpeg_defaults() {
    let options = EncodeOptions::ultra_hdr_defaults();

    assert_eq!(options.quality, EncodeOptions::default().quality);
    assert_eq!(options.progressive, EncodeOptions::default().progressive);
    assert_eq!(
        options.chroma_subsampling,
        EncodeOptions::default().chroma_subsampling
    );
    assert_eq!(
        options.color_metadata.icc_profile.as_deref(),
        Some(icc::display_p3())
    );
    assert!(options.gain_map.is_none());
}

#[test]
fn compat_ultrahdr_api_matches_wrapper_flow() {
    let base = jpeg::Encoder::new(jpeg::Preset::ProgressiveSmallest)
        .quality(90)
        .encode_rgb(sample_primary().data.as_slice(), 4, 4)
        .unwrap();

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
    let mut base_bytes = base.clone();
    let mut hdr_raw = CompatRawImage::packed(
        sys::uhdr_img_fmt::UHDR_IMG_FMT_32bppRGBA1010102,
        4,
        4,
        &mut hdr_bytes,
        sys::uhdr_color_gamut::UHDR_CG_DISPLAY_P3,
        sys::uhdr_color_transfer::UHDR_CT_PQ,
        sys::uhdr_color_range::UHDR_CR_FULL_RANGE,
    )
    .unwrap();
    let mut base_compressed = CompressedImage::from_bytes(
        &mut base_bytes,
        sys::uhdr_color_gamut::UHDR_CG_BT_709,
        sys::uhdr_color_transfer::UHDR_CT_SRGB,
        sys::uhdr_color_range::UHDR_CR_FULL_RANGE,
    );

    let mut encoder = CompatEncoder::new().unwrap();
    encoder
        .set_raw_image(&mut hdr_raw, ImgLabel::UHDR_HDR_IMG)
        .unwrap();
    encoder
        .set_compressed_image(&mut base_compressed, ImgLabel::UHDR_SDR_IMG)
        .unwrap();
    encoder.set_quality(90, ImgLabel::UHDR_BASE_IMG).unwrap();
    encoder
        .set_quality(90, ImgLabel::UHDR_GAIN_MAP_IMG)
        .unwrap();
    encoder
        .set_output_format(sys::uhdr_codec::UHDR_CODEC_JPG)
        .unwrap();
    encoder.encode().unwrap();
    let mut encoded = encoder.encoded_stream().unwrap().bytes().unwrap().to_vec();

    let mut compressed = CompressedImage::from_bytes(
        encoded.as_mut_slice(),
        sys::uhdr_color_gamut::UHDR_CG_UNSPECIFIED,
        sys::uhdr_color_transfer::UHDR_CT_UNSPECIFIED,
        sys::uhdr_color_range::UHDR_CR_UNSPECIFIED,
    );
    let mut decoder = CompatDecoder::new().unwrap();
    decoder.set_image(&mut compressed).unwrap();

    assert!(decoder.gainmap_metadata().unwrap().is_some());

    let decoded = decoder
        .decode_packed_view(
            sys::uhdr_img_fmt::UHDR_IMG_FMT_32bppRGBA1010102,
            sys::uhdr_color_transfer::UHDR_CT_PQ,
        )
        .unwrap()
        .to_owned()
        .unwrap();

    assert_eq!(decoded.width, 4);
    assert_eq!(decoded.height, 4);
    let (gamut, transfer, range) = decoded.meta();
    assert_eq!(gamut, sys::uhdr_color_gamut::UHDR_CG_DISPLAY_P3);
    assert_eq!(transfer, sys::uhdr_color_transfer::UHDR_CT_PQ);
    assert_eq!(range, sys::uhdr_color_range::UHDR_CR_FULL_RANGE);

    let inspected = inspect(&encoded).unwrap();
    assert_eq!(
        inspected.color_metadata.icc_profile.as_deref(),
        Some(icc::display_p3())
    );
}

#[test]
fn compat_owned_buffer_constructors_work() {
    let base = jpeg::Encoder::new(jpeg::Preset::ProgressiveSmallest)
        .quality(90)
        .encode_rgb(sample_primary().data.as_slice(), 4, 4)
        .unwrap();

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

    let hdr_raw = CompatRawImage::packed_owned(
        sys::uhdr_img_fmt::UHDR_IMG_FMT_32bppRGBA1010102,
        4,
        4,
        hdr_pixels,
        sys::uhdr_color_gamut::UHDR_CG_DISPLAY_P3,
        sys::uhdr_color_transfer::UHDR_CT_PQ,
        sys::uhdr_color_range::UHDR_CR_FULL_RANGE,
    )
    .unwrap();
    let base_compressed = CompressedImage::from_vec(
        base,
        sys::uhdr_color_gamut::UHDR_CG_BT_709,
        sys::uhdr_color_transfer::UHDR_CT_SRGB,
        sys::uhdr_color_range::UHDR_CR_FULL_RANGE,
    );

    let mut encoder = CompatEncoder::new().unwrap();
    encoder
        .set_raw_image_owned(hdr_raw, ImgLabel::UHDR_HDR_IMG)
        .unwrap();
    encoder
        .set_compressed_image_owned(base_compressed, ImgLabel::UHDR_SDR_IMG)
        .unwrap();
    encoder.encode().unwrap();

    let encoded = encoder.encoded_stream().unwrap().bytes().unwrap();
    assert!(!encoded.is_empty());
}

#[test]
fn compat_encoder_preserves_existing_primary_icc_profile() {
    let base = jpeg::Encoder::new(jpeg::Preset::ProgressiveSmallest)
        .quality(90)
        .icc_profile(b"custom-base-icc".to_vec())
        .encode_rgb(sample_primary().data.as_slice(), 4, 4)
        .unwrap();

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

    let hdr_raw = CompatRawImage::packed_owned(
        sys::uhdr_img_fmt::UHDR_IMG_FMT_32bppRGBA1010102,
        4,
        4,
        hdr_pixels,
        sys::uhdr_color_gamut::UHDR_CG_DISPLAY_P3,
        sys::uhdr_color_transfer::UHDR_CT_PQ,
        sys::uhdr_color_range::UHDR_CR_FULL_RANGE,
    )
    .unwrap();
    let base_compressed = CompressedImage::from_vec(
        base,
        sys::uhdr_color_gamut::UHDR_CG_BT_709,
        sys::uhdr_color_transfer::UHDR_CT_SRGB,
        sys::uhdr_color_range::UHDR_CR_FULL_RANGE,
    );

    let mut encoder = CompatEncoder::new().unwrap();
    encoder
        .set_raw_image_owned(hdr_raw, ImgLabel::UHDR_HDR_IMG)
        .unwrap();
    encoder
        .set_compressed_image_owned(base_compressed, ImgLabel::UHDR_SDR_IMG)
        .unwrap();
    encoder.encode().unwrap();

    let encoded = encoder.encoded_stream().unwrap().bytes().unwrap();
    let inspected = inspect(encoded).unwrap();
    assert_eq!(
        inspected.color_metadata.icc_profile.as_deref(),
        Some(b"custom-base-icc".as_slice())
    );
}

#[test]
fn compat_decoder_accepts_borrowed_slice_api() {
    let options = EncodeOptions {
        gain_map: Some(GainMapEncodeOptions {
            image: sample_gain_map(),
            metadata: sample_gain_map_metadata(),
            quality: 80,
            progressive: false,
        }),
        ..EncodeOptions::default()
    };
    let encoded = UltraJpegEncoder::new(options)
        .encode(&sample_primary())
        .unwrap();

    let mut decoder = CompatDecoder::new().unwrap();
    decoder
        .set_image_slice(
            encoded.as_slice(),
            sys::uhdr_color_gamut::UHDR_CG_UNSPECIFIED,
            sys::uhdr_color_transfer::UHDR_CT_UNSPECIFIED,
            sys::uhdr_color_range::UHDR_CR_UNSPECIFIED,
        )
        .unwrap();

    assert!(decoder.gainmap_metadata().unwrap().is_some());

    let decoded = decoder
        .decode_packed_view(
            sys::uhdr_img_fmt::UHDR_IMG_FMT_32bppRGBA1010102,
            sys::uhdr_color_transfer::UHDR_CT_PQ,
        )
        .unwrap();

    assert_eq!(decoded.width, 4);
    assert_eq!(decoded.height, 4);
    assert_eq!(decoded.data.len(), 4 * 4 * 4);
}

fn replace_once(bytes: &mut [u8], needle: &[u8], replacement: &[u8]) {
    assert_eq!(
        needle.len(),
        replacement.len(),
        "replacement must preserve byte length"
    );

    let offset = bytes
        .windows(needle.len())
        .position(|window| window == needle)
        .expect("needle not found");
    bytes[offset..offset + needle.len()].copy_from_slice(replacement);
}

fn replace_all(bytes: &mut [u8], needle: &[u8], replacement: &[u8]) {
    assert_eq!(
        needle.len(),
        replacement.len(),
        "replacement must preserve byte length"
    );

    let mut replaced = 0;
    for offset in (0..=bytes.len().saturating_sub(needle.len()))
        .filter(|&offset| &bytes[offset..offset + needle.len()] == needle)
        .collect::<Vec<_>>()
    {
        bytes[offset..offset + needle.len()].copy_from_slice(replacement);
        replaced += 1;
    }

    assert!(replaced > 0, "needle not found");
}
