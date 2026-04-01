use img_parts::{
    Bytes,
    jpeg::{Jpeg, markers},
};
use ultrahdr_core::{
    ColorGamut, ColorTransfer, GainMapMetadata, PixelFormat, RawImage, gainmap::HdrOutputFormat,
    metadata::find_jpeg_boundaries,
};
use ultrajpeg::{
    ColorMetadata, ComputeGainMapOptions, DecodeOptions, EncodeOptions, Encoder, GainMapBundle,
    GainMapChannels, GainMapMetadataSource, MetadataLocation, PrimaryMetadata,
    UltraHdrEncodeOptions, compute_gain_map, decode, decode_with_options, encode_ultra_hdr, icc,
    inspect,
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
    .unwrap()
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
    .unwrap()
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
    .unwrap()
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
    .unwrap()
}

fn sample_primary_metadata() -> PrimaryMetadata {
    let display_p3 = ColorMetadata::display_p3();
    PrimaryMetadata {
        color: ColorMetadata {
            icc_profile: Some(b"fake-icc-profile".to_vec()),
            gamut: Some(ColorGamut::DisplayP3),
            gamut_info: display_p3.gamut_info,
            transfer: Some(ColorTransfer::Srgb),
        },
        exif: Some(b"II*\0\x08\0\0\0\0\0".to_vec()),
    }
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
        primary_metadata: sample_primary_metadata(),
        gain_map: Some(GainMapBundle {
            image: sample_gain_map(),
            metadata: sample_gain_map_metadata(),
            quality: 80,
            progressive: false,
        }),
        ..EncodeOptions::default()
    };

    let encoded = Encoder::new(options).encode(&sample_primary()).unwrap();
    let decoded = decode_with_options(
        &encoded,
        DecodeOptions {
            retain_primary_jpeg: true,
            retain_gain_map_jpeg: true,
            ..DecodeOptions::default()
        },
    )
    .unwrap();

    assert_eq!(decoded.image.width, 4);
    assert_eq!(decoded.image.height, 4);
    assert_eq!(decoded.image.format, PixelFormat::Rgb8);
    assert_eq!(
        decoded.primary_metadata.color.icc_profile.as_deref(),
        Some(b"fake-icc-profile".as_slice())
    );
    assert_eq!(
        decoded.primary_metadata.color.gamut,
        Some(ColorGamut::DisplayP3)
    );
    assert_eq!(
        decoded.primary_metadata.color.transfer,
        Some(ColorTransfer::Srgb)
    );
    assert!(decoded.primary_metadata.exif.is_some());
    assert!(
        decoded
            .primary_jpeg
            .as_ref()
            .is_some_and(|jpeg| !jpeg.is_empty())
    );

    let ultra_hdr = decoded.ultra_hdr.as_ref().unwrap();
    assert!(ultra_hdr.xmp.as_deref().unwrap().contains("hdrgm:Version"));
    assert!(ultra_hdr.iso_21496_1.is_some());
    assert_eq!(ultra_hdr.xmp_location, Some(MetadataLocation::GainMap));
    assert_eq!(
        ultra_hdr.iso_21496_1_location,
        Some(MetadataLocation::GainMap)
    );
    assert_eq!(
        ultra_hdr.gain_map_metadata_source,
        Some(GainMapMetadataSource::Iso21496_1)
    );

    let gain_map = decoded.gain_map.as_ref().unwrap();
    assert_eq!(gain_map.image.width, 4);
    assert_eq!(gain_map.image.height, 4);
    assert_eq!(gain_map.image.format, PixelFormat::Gray8);
    assert!(
        gain_map
            .jpeg_bytes
            .as_ref()
            .is_some_and(|jpeg| !jpeg.is_empty())
    );

    let metadata = gain_map.metadata.as_ref().unwrap();
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
        gain_map: Some(GainMapBundle {
            image: sample_gain_map(),
            metadata: sample_gain_map_metadata(),
            quality: 80,
            progressive: false,
        }),
        ..EncodeOptions::ultra_hdr_defaults()
    };

    let encoded = Encoder::new(options).encode(&sample_primary()).unwrap();
    let codestreams = split_embedded_jpegs(&encoded);

    assert_eq!(codestreams.len(), 2);

    let primary_xmp = xmp_payload(codestreams[0]).unwrap();
    assert!(primary_xmp.contains("Item:Semantic=\"Primary\""));
    assert!(primary_xmp.contains("Item:Semantic=\"GainMap\""));
    assert!(!primary_xmp.contains("hdrgm:GainMapMax"));
    assert!(iso_payload(codestreams[0]).is_none());

    let gain_map_xmp = xmp_payload(codestreams[1]).unwrap();
    assert!(gain_map_xmp.contains("hdrgm:GainMapMax"));
    assert!(gain_map_xmp.contains("hdrgm:HDRCapacityMax"));
    assert!(!gain_map_xmp.contains("Item:Semantic=\"GainMap\""));
    assert!(iso_payload(codestreams[1]).is_some());
}

#[test]
fn decodes_plain_jpeg_without_gain_map() {
    let encoded = Encoder::new(EncodeOptions::default())
        .encode(&sample_primary())
        .unwrap();
    let decoded = decode(&encoded).unwrap();

    assert!(decoded.gain_map.is_none());
    assert!(decoded.ultra_hdr.is_none());
    assert_eq!(decoded.image.width, 4);
    assert!(decoded.primary_jpeg.is_none());
}

#[test]
fn decode_options_control_gain_map_and_codestream_retention() {
    let encoded = Encoder::new(EncodeOptions {
        gain_map: Some(GainMapBundle {
            image: sample_gain_map(),
            metadata: sample_gain_map_metadata(),
            quality: 75,
            progressive: false,
        }),
        ..EncodeOptions::ultra_hdr_defaults()
    })
    .encode(&sample_primary())
    .unwrap();

    let skipped = decode_with_options(
        &encoded,
        DecodeOptions {
            decode_gain_map: false,
            retain_primary_jpeg: true,
            ..DecodeOptions::default()
        },
    )
    .unwrap();
    assert!(skipped.gain_map.is_none());
    assert!(skipped.primary_jpeg.is_some());
    assert!(skipped.ultra_hdr.is_some());

    let retained = decode_with_options(
        &encoded,
        DecodeOptions {
            retain_gain_map_jpeg: true,
            ..DecodeOptions::default()
        },
    )
    .unwrap();
    assert!(
        retained
            .gain_map
            .as_ref()
            .unwrap()
            .jpeg_bytes
            .as_ref()
            .is_some_and(|jpeg| !jpeg.is_empty())
    );
}

#[test]
fn gain_map_packaging_auto_injects_display_p3_icc_for_display_p3_primary() {
    let encoded = Encoder::new(EncodeOptions {
        gain_map: Some(GainMapBundle {
            image: sample_gain_map(),
            metadata: sample_gain_map_metadata(),
            quality: 75,
            progressive: false,
        }),
        ..EncodeOptions::default()
    })
    .encode(&sample_primary())
    .unwrap();
    let inspected = inspect(&encoded).unwrap();

    assert_eq!(
        inspected.primary_metadata.color.icc_profile.as_deref(),
        Some(icc::display_p3())
    );
    assert_eq!(
        inspected.primary_metadata.color.gamut,
        Some(ColorGamut::DisplayP3)
    );
    assert_eq!(
        inspected.primary_metadata.color.transfer,
        Some(ColorTransfer::Srgb)
    );
}

#[test]
fn gain_map_packaging_requires_explicit_icc_for_non_display_p3_primary() {
    let error = Encoder::new(EncodeOptions {
        gain_map: Some(GainMapBundle {
            image: sample_gain_map(),
            metadata: sample_gain_map_metadata(),
            quality: 75,
            progressive: false,
        }),
        ..EncodeOptions::default()
    })
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
fn computed_gain_map_into_bundle_composes_with_encode() {
    let computed = compute_gain_map(
        &sample_hdr(),
        &sample_primary(),
        &ComputeGainMapOptions::default(),
    )
    .unwrap();
    let options = EncodeOptions {
        gain_map: Some(computed.into_bundle(83, false)),
        ..EncodeOptions::ultra_hdr_defaults()
    };

    let encoded = Encoder::new(options).encode(&sample_primary()).unwrap();
    let decoded = decode(&encoded).unwrap();

    assert!(decoded.gain_map.is_some());
    assert_eq!(decoded.gain_map.as_ref().unwrap().gain_map.channels, 1);
}

#[test]
fn decode_uses_xmp_metadata_when_iso_is_absent() {
    let mut encoded = Encoder::new(EncodeOptions {
        gain_map: Some(GainMapBundle {
            image: sample_gain_map(),
            metadata: sample_gain_map_metadata(),
            quality: 80,
            progressive: false,
        }),
        ..EncodeOptions::ultra_hdr_defaults()
    })
    .encode(&sample_primary())
    .unwrap();

    replace_once(
        &mut encoded,
        b"urn:iso:std:iso:ts:21496:-1\0",
        b"urn:xso:std:iso:ts:21496:-1\0",
    );

    let ultra_hdr = decode(&encoded).unwrap().ultra_hdr.unwrap();
    assert!(ultra_hdr.xmp.is_some());
    assert!(ultra_hdr.iso_21496_1.is_none());
    assert_eq!(ultra_hdr.xmp_location, Some(MetadataLocation::GainMap));
    assert_eq!(
        ultra_hdr.gain_map_metadata_source,
        Some(GainMapMetadataSource::Xmp)
    );
    assert!((ultra_hdr.gain_map_metadata.unwrap().hdr_capacity_max - 4.0).abs() < 0.01);
}

#[test]
fn decode_uses_iso_metadata_when_xmp_is_absent() {
    let mut encoded = Encoder::new(EncodeOptions {
        gain_map: Some(GainMapBundle {
            image: sample_gain_map(),
            metadata: sample_gain_map_metadata(),
            quality: 80,
            progressive: false,
        }),
        ..EncodeOptions::ultra_hdr_defaults()
    })
    .encode(&sample_primary())
    .unwrap();

    replace_all(
        &mut encoded,
        b"http://ns.adobe.com/xap/1.0/\0",
        b"http://ns.adobe.com/xaq/1.0/\0",
    );

    let ultra_hdr = decode(&encoded).unwrap().ultra_hdr.unwrap();
    assert!(ultra_hdr.xmp.is_none());
    assert!(ultra_hdr.iso_21496_1.is_some());
    assert_eq!(
        ultra_hdr.iso_21496_1_location,
        Some(MetadataLocation::GainMap)
    );
    assert_eq!(
        ultra_hdr.gain_map_metadata_source,
        Some(GainMapMetadataSource::Iso21496_1)
    );
    assert!((ultra_hdr.gain_map_metadata.unwrap().hdr_capacity_max - 4.0).abs() < 0.01);
}

#[test]
fn decode_prefers_iso_metadata_over_xmp_when_both_are_present() {
    let mut encoded = Encoder::new(EncodeOptions {
        gain_map: Some(GainMapBundle {
            image: sample_gain_map(),
            metadata: sample_gain_map_metadata(),
            quality: 80,
            progressive: false,
        }),
        ..EncodeOptions::ultra_hdr_defaults()
    })
    .encode(&sample_primary())
    .unwrap();

    replace_once(
        &mut encoded,
        b"hdrgm:HDRCapacityMax=\"2.000000\"",
        b"hdrgm:HDRCapacityMax=\"0.000000\"",
    );

    let metadata = decode(&encoded)
        .unwrap()
        .ultra_hdr
        .unwrap()
        .gain_map_metadata
        .unwrap();
    assert!((metadata.hdr_capacity_max - 4.0).abs() < 0.01);
}

#[test]
fn decode_rejects_xmp_fallback_when_base_rendition_is_hdr_true() {
    let mut encoded = Encoder::new(EncodeOptions {
        gain_map: Some(GainMapBundle {
            image: sample_gain_map(),
            metadata: sample_gain_map_metadata(),
            quality: 80,
            progressive: false,
        }),
        ..EncodeOptions::ultra_hdr_defaults()
    })
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

    let ultra_hdr = decode(&encoded).unwrap().ultra_hdr.unwrap();
    assert!(ultra_hdr.xmp.is_some());
    assert!(ultra_hdr.iso_21496_1.is_none());
    assert!(ultra_hdr.gain_map_metadata.is_none());
}

#[test]
fn decode_rejects_xmp_fallback_when_required_fields_are_missing() {
    let mut encoded = Encoder::new(EncodeOptions {
        gain_map: Some(GainMapBundle {
            image: sample_gain_map(),
            metadata: sample_gain_map_metadata(),
            quality: 80,
            progressive: false,
        }),
        ..EncodeOptions::ultra_hdr_defaults()
    })
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

    let ultra_hdr = decode(&encoded).unwrap().ultra_hdr.unwrap();
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
        inspected.primary_metadata.color.icc_profile.as_deref(),
        Some(icc::display_p3())
    );
}

#[test]
fn encode_ultra_hdr_rejects_prepopulated_primary_gain_map() {
    let mut options = UltraHdrEncodeOptions::default();
    options.primary.gain_map = Some(GainMapBundle {
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
    let encoded = Encoder::new(EncodeOptions {
        gain_map: Some(GainMapBundle {
            image: sample_multichannel_gain_map(),
            metadata: sample_multichannel_gain_map_metadata(),
            quality: 82,
            progressive: false,
        }),
        ..EncodeOptions::ultra_hdr_defaults()
    })
    .encode(&sample_primary())
    .unwrap();
    let decoded = decode(&encoded).unwrap();

    let gain_map = decoded.gain_map.as_ref().unwrap();
    assert_eq!(gain_map.image.format, PixelFormat::Rgb8);
    assert_eq!(gain_map.gain_map.channels, 3);
    assert_eq!(gain_map.gain_map.data.len(), 4 * 4 * 3);

    let metadata = decoded.ultra_hdr.unwrap().gain_map_metadata.unwrap();
    assert_eq!(metadata.max_content_boost, [4.0, 8.0, 2.0]);
    assert_eq!(metadata.gamma, [1.0, 1.2, 1.4]);
}

#[test]
fn display_p3_helpers_embed_the_built_in_profile() {
    let color_metadata = ColorMetadata::display_p3();
    assert_eq!(
        color_metadata.icc_profile.as_deref(),
        Some(icc::display_p3())
    );
    assert_eq!(color_metadata.gamut, Some(ColorGamut::DisplayP3));
    assert_eq!(
        color_metadata
            .gamut_info
            .as_ref()
            .and_then(|gamut| gamut.standard),
        Some(ColorGamut::DisplayP3)
    );
    assert_eq!(color_metadata.transfer, Some(ColorTransfer::Srgb));

    let encoded = Encoder::new(EncodeOptions {
        gain_map: Some(GainMapBundle {
            image: sample_gain_map(),
            metadata: sample_gain_map_metadata(),
            quality: 80,
            progressive: false,
        }),
        ..EncodeOptions::ultra_hdr_defaults()
    })
    .encode(&sample_primary())
    .unwrap();
    let inspected = inspect(&encoded).unwrap();

    assert_eq!(
        inspected.primary_metadata.color.icc_profile.as_deref(),
        Some(icc::display_p3())
    );
    assert_eq!(
        inspected.primary_metadata.color.gamut,
        Some(ColorGamut::DisplayP3)
    );
    assert_eq!(
        inspected.primary_metadata.color.transfer,
        Some(ColorTransfer::Srgb)
    );
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
        options.primary_metadata.color.icc_profile.as_deref(),
        Some(icc::display_p3())
    );
    assert!(options.gain_map.is_none());
}

fn replace_once(bytes: &mut [u8], needle: &[u8], replacement: &[u8]) {
    let position = bytes
        .windows(needle.len())
        .position(|window| window == needle)
        .expect("needle present in encoded JPEG");
    bytes[position..position + replacement.len()].copy_from_slice(replacement);
}

fn replace_all(bytes: &mut [u8], needle: &[u8], replacement: &[u8]) {
    let mut start = 0;
    while let Some(position) = bytes[start..]
        .windows(needle.len())
        .position(|window| window == needle)
    {
        let position = start + position;
        bytes[position..position + replacement.len()].copy_from_slice(replacement);
        start = position + replacement.len();
    }
}
