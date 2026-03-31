use ultrahdr_core::{ColorGamut, PixelFormat};
use ultrajpeg::{
    DecodeOptions, GainMapMetadataSource, MetadataLocation, decode, decode_with_options, inspect,
};

const UPSTREAM_ULTRAHDR: &[u8] =
    include_bytes!("fixtures/upstream/ultra-hdr-samples/Ultra_HDR_Samples_Originals_05.jpg");

#[test]
fn upstream_ultrahdr_fixture_inspects_expected_metadata() {
    let inspected = inspect(UPSTREAM_ULTRAHDR).unwrap();

    assert!(inspected.primary_jpeg_len < UPSTREAM_ULTRAHDR.len());
    assert!(inspected.gain_map_jpeg_len.is_some());
    assert!(inspected.primary_metadata.color.icc_profile.is_some());
    assert_eq!(
        inspected
            .primary_metadata
            .color
            .gamut_info
            .as_ref()
            .and_then(|gamut| gamut.standard),
        Some(ColorGamut::DisplayP3)
    );

    let ultra_hdr = inspected.ultra_hdr.as_ref().unwrap();
    assert!(ultra_hdr.xmp.is_some());
    assert!(ultra_hdr.gain_map_metadata.is_some());
    assert_eq!(ultra_hdr.xmp_location, Some(MetadataLocation::GainMap));
    assert!(ultra_hdr.iso_21496_1_location.is_none());
}

#[test]
fn upstream_ultrahdr_fixture_decodes_expected_structure() {
    let decoded = decode_with_gain_map_bytes();

    assert_eq!(decoded.image.width, 4080);
    assert_eq!(decoded.image.height, 3072);
    assert_eq!(decoded.image.format, PixelFormat::Rgb8);
    assert!(decoded.primary_metadata.color.icc_profile.is_some());

    let gain_map = decoded.gain_map.as_ref().unwrap();
    assert!(gain_map.image.width > 0);
    assert!(gain_map.image.height > 0);
    assert!(matches!(
        gain_map.image.format,
        PixelFormat::Gray8 | PixelFormat::Rgb8
    ));
    assert!(
        gain_map
            .jpeg_bytes
            .as_ref()
            .is_some_and(|jpeg| !jpeg.is_empty())
    );

    let ultra_hdr = decoded.ultra_hdr.as_ref().unwrap();
    assert!(ultra_hdr.xmp.is_some());
    assert!(ultra_hdr.gain_map_metadata.is_some());
    assert_eq!(
        ultra_hdr.gain_map_metadata_source,
        Some(GainMapMetadataSource::Xmp)
    );
}

#[test]
fn upstream_ultrahdr_decode_recovers_display_p3_from_icc() {
    let decoded = decode(UPSTREAM_ULTRAHDR).unwrap();

    assert_eq!(decoded.image.gamut, ColorGamut::DisplayP3);
    assert_eq!(
        decoded
            .primary_metadata
            .color
            .gamut_info
            .as_ref()
            .and_then(|gamut| gamut.standard),
        Some(ColorGamut::DisplayP3)
    );
}

fn decode_with_gain_map_bytes() -> ultrajpeg::DecodedImage {
    decode_with_options(
        UPSTREAM_ULTRAHDR,
        DecodeOptions {
            retain_gain_map_jpeg: true,
            ..DecodeOptions::default()
        },
    )
    .unwrap()
}
