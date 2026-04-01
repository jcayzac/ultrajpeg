use img_parts::{Bytes, ImageEXIF, ImageICC, jpeg::Jpeg};
use ultrahdr_core::{ColorGamut, ColorTransfer, PixelFormat, gainmap::HdrOutputFormat};
use ultrajpeg::{
    DecodeOptions, GainMapMetadataSource, MetadataLocation, decode, decode_with_options, inspect,
};

const PLAIN_SDR: &[u8] = include_bytes!("fixtures/plain-sdr.jpg");
const PLAIN_SDR_COMPAT: &[u8] = include_bytes!("fixtures/plain-sdr-compat.jpg");
const SAMPLE_ULTRAHDR: &[u8] = include_bytes!("fixtures/sample-ultrahdr.jpg");
const SAMPLE_ULTRAHDR_COMPAT: &[u8] = include_bytes!("fixtures/sample-ultrahdr-compat.jpg");

#[test]
fn plain_fixture_decodes_expected_metadata() {
    let decoded = decode(PLAIN_SDR).unwrap();

    assert_eq!(decoded.image.width, 4);
    assert_eq!(decoded.image.height, 4);
    assert_eq!(decoded.image.format, PixelFormat::Rgb8);
    assert_eq!(
        decoded.primary_metadata.color.gamut,
        Some(ColorGamut::DisplayP3)
    );
    assert_eq!(
        decoded.primary_metadata.color.transfer,
        Some(ColorTransfer::Srgb)
    );
    assert_eq!(
        decoded
            .primary_metadata
            .color
            .icc_profile
            .as_ref()
            .map(Vec::len),
        Some(19)
    );
    assert!(decoded.primary_metadata.exif.is_some());
    assert!(decoded.ultra_hdr.is_none());
    assert!(decoded.gain_map.is_none());
}

#[test]
fn plain_compat_fixture_decodes_expected_metadata() {
    let decoded = decode(PLAIN_SDR_COMPAT).unwrap();

    assert_eq!(decoded.image.width, 4);
    assert_eq!(decoded.image.height, 4);
    assert_eq!(decoded.image.format, PixelFormat::Rgb8);
    assert_eq!(
        decoded
            .primary_metadata
            .color
            .icc_profile
            .as_ref()
            .map(Vec::len),
        Some(19)
    );
    assert!(decoded.primary_metadata.exif.is_none());
    assert!(decoded.ultra_hdr.is_none());
    assert!(decoded.gain_map.is_none());
}

#[test]
fn plain_fixtures_contain_expected_jpeg_markers() {
    let plain = Jpeg::from_bytes(Bytes::copy_from_slice(PLAIN_SDR)).unwrap();
    assert!(plain.icc_profile().is_some());
    assert!(plain.exif().is_some());

    let compat = Jpeg::from_bytes(Bytes::copy_from_slice(PLAIN_SDR_COMPAT)).unwrap();
    assert!(compat.icc_profile().is_some());
    assert!(compat.exif().is_none());
}

#[test]
fn ultrahdr_fixtures_decode_expected_metadata() {
    for (bytes, expected_gain_map_width, expected_gain_map_height) in
        [(SAMPLE_ULTRAHDR, 4, 4), (SAMPLE_ULTRAHDR_COMPAT, 1, 1)]
    {
        let decoded = decode_with_options(
            bytes,
            DecodeOptions {
                retain_gain_map_jpeg: true,
                ..DecodeOptions::default()
            },
        )
        .unwrap();

        assert_eq!(decoded.image.width, 4);
        assert_eq!(decoded.image.height, 4);
        assert_eq!(decoded.image.format, PixelFormat::Rgb8);
        assert!(decoded.gain_map.is_some());

        let gain_map = decoded.gain_map.as_ref().unwrap();
        assert_eq!(gain_map.image.width, expected_gain_map_width);
        assert_eq!(gain_map.image.height, expected_gain_map_height);
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
        assert!(ultra_hdr.xmp.as_deref().unwrap().contains("hdrgm:Version"));
        assert!(ultra_hdr.iso_21496_1.is_some());
        assert!(matches!(
            ultra_hdr.xmp_location,
            Some(MetadataLocation::Primary | MetadataLocation::GainMap)
        ));
        assert!(matches!(
            ultra_hdr.iso_21496_1_location,
            Some(MetadataLocation::Primary | MetadataLocation::GainMap)
        ));
        assert_eq!(
            ultra_hdr.gain_map_metadata_source,
            Some(GainMapMetadataSource::Iso21496_1)
        );

        let metadata = ultra_hdr.gain_map_metadata.as_ref().unwrap();
        assert!(metadata.hdr_capacity_max >= 4.0);
    }
}

#[test]
fn ultrahdr_fixture_skip_gain_map_still_exposes_metadata() {
    let decoded = decode_with_options(
        SAMPLE_ULTRAHDR,
        DecodeOptions {
            decode_gain_map: false,
            ..DecodeOptions::default()
        },
    )
    .unwrap();

    assert!(decoded.gain_map.is_none());
    assert!(decoded.ultra_hdr.is_some());
}

#[test]
fn ultrahdr_fixture_reconstructs_hdr_output() {
    let decoded = decode(SAMPLE_ULTRAHDR).unwrap();
    let reconstructed = decoded
        .reconstruct_hdr(4.0, HdrOutputFormat::Pq1010102)
        .unwrap();

    assert_eq!(reconstructed.width, 4);
    assert_eq!(reconstructed.height, 4);
    assert_eq!(reconstructed.format, PixelFormat::Rgba1010102Pq);
    assert_eq!(reconstructed.data.len(), 4 * 4 * 4);
}

#[test]
fn retained_ultrahdr_codestreams_decode_without_panicking() {
    let decoded = decode_with_options(
        SAMPLE_ULTRAHDR,
        DecodeOptions {
            retain_primary_jpeg: true,
            retain_gain_map_jpeg: true,
            ..DecodeOptions::default()
        },
    )
    .unwrap();

    let primary = decoded.primary_jpeg.as_ref().unwrap();
    let gain_map = decoded
        .gain_map
        .as_ref()
        .unwrap()
        .jpeg_bytes
        .as_ref()
        .unwrap();

    let decoded_primary = std::panic::catch_unwind(|| decode(primary));
    let decoded_gain_map = std::panic::catch_unwind(|| decode(gain_map));

    let decoded_primary = decoded_primary
        .expect("primary codestream decode panicked")
        .unwrap();
    let decoded_gain_map = decoded_gain_map
        .expect("gain-map codestream decode panicked")
        .unwrap();

    assert!(decoded_primary.gain_map.is_none());
    assert!(decoded_gain_map.gain_map.is_none());
}

#[test]
fn inspect_matches_decoded_primary_metadata_for_fixture() {
    let inspected = inspect(SAMPLE_ULTRAHDR).unwrap();
    let decoded = decode(SAMPLE_ULTRAHDR).unwrap();

    assert_eq!(inspected.primary_metadata, decoded.primary_metadata);
    assert_eq!(
        inspected
            .ultra_hdr
            .as_ref()
            .unwrap()
            .gain_map_metadata_source,
        decoded.ultra_hdr.as_ref().unwrap().gain_map_metadata_source
    );
}
