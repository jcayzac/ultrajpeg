use img_parts::{ImageEXIF, ImageICC, jpeg::Jpeg};
use ultrahdr_core::{ColorGamut, ColorTransfer, PixelFormat, gainmap::HdrOutputFormat};
use ultrajpeg::{
    CompressedImage, DecodeOptions, Decoder as CompatDecoder, decode, decode_with_options, sys,
};

const PLAIN_SDR: &[u8] = include_bytes!("fixtures/plain-sdr.jpg");
const PLAIN_SDR_COMPAT: &[u8] = include_bytes!("fixtures/plain-sdr-compat.jpg");
const SAMPLE_ULTRAHDR: &[u8] = include_bytes!("fixtures/sample-ultrahdr.jpg");
const SAMPLE_ULTRAHDR_COMPAT: &[u8] = include_bytes!("fixtures/sample-ultrahdr-compat.jpg");

#[test]
fn plain_fixture_decodes_expected_metadata() {
    let decoded = decode(PLAIN_SDR).unwrap();

    assert_eq!(decoded.primary_image.width, 4);
    assert_eq!(decoded.primary_image.height, 4);
    assert_eq!(decoded.primary_image.format, PixelFormat::Rgb8);
    assert_eq!(decoded.color_metadata.gamut, Some(ColorGamut::DisplayP3));
    assert_eq!(decoded.color_metadata.transfer, Some(ColorTransfer::Srgb));
    assert_eq!(
        decoded.color_metadata.icc_profile.as_ref().map(Vec::len),
        Some(19)
    );
    assert!(decoded.color_metadata.exif.is_some());
    assert!(decoded.ultra_hdr.is_none());
    assert!(decoded.gain_map.is_none());
}

#[test]
fn plain_compat_fixture_decodes_expected_metadata() {
    let decoded = decode(PLAIN_SDR_COMPAT).unwrap();

    assert_eq!(decoded.primary_image.width, 4);
    assert_eq!(decoded.primary_image.height, 4);
    assert_eq!(decoded.primary_image.format, PixelFormat::Rgb8);
    assert_eq!(
        decoded.color_metadata.icc_profile.as_ref().map(Vec::len),
        Some(19)
    );
    assert!(decoded.color_metadata.exif.is_none());
    assert!(decoded.ultra_hdr.is_none());
    assert!(decoded.gain_map.is_none());
}

#[test]
fn plain_fixtures_contain_expected_jpeg_markers() {
    let plain = Jpeg::from_bytes(img_parts::Bytes::copy_from_slice(PLAIN_SDR)).unwrap();
    assert!(plain.icc_profile().is_some());
    assert!(plain.exif().is_some());

    let compat = Jpeg::from_bytes(img_parts::Bytes::copy_from_slice(PLAIN_SDR_COMPAT)).unwrap();
    assert!(compat.icc_profile().is_some());
    assert!(compat.exif().is_none());
}

#[test]
fn ultrahdr_fixtures_decode_expected_metadata() {
    for (bytes, expected_gain_map_width, expected_gain_map_height) in
        [(SAMPLE_ULTRAHDR, 4, 4), (SAMPLE_ULTRAHDR_COMPAT, 1, 1)]
    {
        let decoded = decode(bytes).unwrap();

        assert_eq!(decoded.primary_image.width, 4);
        assert_eq!(decoded.primary_image.height, 4);
        assert_eq!(decoded.primary_image.format, PixelFormat::Rgb8);
        assert!(decoded.gain_map.is_some());

        let gain_map = decoded.gain_map.as_ref().unwrap();
        assert_eq!(gain_map.image.width, expected_gain_map_width);
        assert_eq!(gain_map.image.height, expected_gain_map_height);
        assert_eq!(gain_map.image.format, PixelFormat::Gray8);
        assert!(!gain_map.jpeg_bytes.is_empty());

        let ultra_hdr = decoded.ultra_hdr.as_ref().unwrap();
        assert!(ultra_hdr.xmp.as_deref().unwrap().contains("hdrgm:Version"));
        assert!(ultra_hdr.iso_21496_1.is_some());

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
fn compat_decoder_probes_gain_map_metadata_only_for_ultrahdr_vectors() {
    let mut plain = PLAIN_SDR.to_vec();
    let mut plain_compressed = CompressedImage::from_bytes(
        plain.as_mut_slice(),
        sys::uhdr_color_gamut::UHDR_CG_UNSPECIFIED,
        sys::uhdr_color_transfer::UHDR_CT_UNSPECIFIED,
        sys::uhdr_color_range::UHDR_CR_UNSPECIFIED,
    );
    let mut decoder = CompatDecoder::new().unwrap();
    decoder.set_image(&mut plain_compressed).unwrap();
    assert!(decoder.gainmap_metadata().unwrap().is_none());

    let mut hdr = SAMPLE_ULTRAHDR_COMPAT.to_vec();
    let mut hdr_compressed = CompressedImage::from_bytes(
        hdr.as_mut_slice(),
        sys::uhdr_color_gamut::UHDR_CG_UNSPECIFIED,
        sys::uhdr_color_transfer::UHDR_CT_UNSPECIFIED,
        sys::uhdr_color_range::UHDR_CR_UNSPECIFIED,
    );
    let mut decoder = CompatDecoder::new().unwrap();
    decoder.set_image(&mut hdr_compressed).unwrap();
    assert!(decoder.gainmap_metadata().unwrap().is_some());
}

#[test]
fn compat_decoder_reads_pq_packed_view_from_ultrahdr_fixtures() {
    for bytes in [SAMPLE_ULTRAHDR, SAMPLE_ULTRAHDR_COMPAT] {
        let mut owned = bytes.to_vec();
        let mut compressed = CompressedImage::from_bytes(
            owned.as_mut_slice(),
            sys::uhdr_color_gamut::UHDR_CG_UNSPECIFIED,
            sys::uhdr_color_transfer::UHDR_CT_UNSPECIFIED,
            sys::uhdr_color_range::UHDR_CR_UNSPECIFIED,
        );
        let mut decoder = CompatDecoder::new().unwrap();
        decoder.set_image(&mut compressed).unwrap();

        let decoded = decoder
            .decode_packed_view(
                sys::uhdr_img_fmt::UHDR_IMG_FMT_32bppRGBA1010102,
                sys::uhdr_color_transfer::UHDR_CT_PQ,
            )
            .unwrap();

        assert_eq!(decoded.width, 4);
        assert_eq!(decoded.height, 4);
        assert_eq!(decoded.data.len(), 4 * 4 * 4);

        let (gamut, transfer, range) = decoded.meta();
        assert_eq!(gamut, sys::uhdr_color_gamut::UHDR_CG_DISPLAY_P3);
        assert_eq!(transfer, sys::uhdr_color_transfer::UHDR_CT_PQ);
        assert_eq!(range, sys::uhdr_color_range::UHDR_CR_FULL_RANGE);
    }
}
