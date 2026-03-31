use ultrahdr_core::{ColorGamut, PixelFormat};
use ultrajpeg::{CompressedImage, Decoder as CompatDecoder, decode, inspect, sys};

const UPSTREAM_ULTRAHDR: &[u8] =
    include_bytes!("fixtures/upstream/ultra-hdr-samples/Ultra_HDR_Samples_Originals_05.jpg");

#[test]
fn upstream_ultrahdr_fixture_inspects_expected_metadata() {
    let inspected = inspect(UPSTREAM_ULTRAHDR).unwrap();

    assert!(inspected.primary_jpeg_len < UPSTREAM_ULTRAHDR.len());
    assert!(inspected.gain_map_jpeg_len.is_some());
    assert!(inspected.color_metadata.icc_profile.is_some());

    let ultra_hdr = inspected.ultra_hdr.as_ref().unwrap();
    assert!(ultra_hdr.xmp.is_some());
    assert!(ultra_hdr.gain_map_metadata.is_some());
}

#[test]
fn upstream_ultrahdr_fixture_decodes_expected_structure() {
    let decoded = decode(UPSTREAM_ULTRAHDR).unwrap();

    assert_eq!(decoded.primary_image.width, 4080);
    assert_eq!(decoded.primary_image.height, 3072);
    assert_eq!(decoded.primary_image.format, PixelFormat::Rgb8);
    assert!(decoded.color_metadata.icc_profile.is_some());

    let gain_map = decoded.gain_map.as_ref().unwrap();
    assert!(gain_map.image.width > 0);
    assert!(gain_map.image.height > 0);
    assert!(matches!(
        gain_map.image.format,
        PixelFormat::Gray8 | PixelFormat::Rgb8
    ));
    assert!(!gain_map.jpeg_bytes.is_empty());

    let ultra_hdr = decoded.ultra_hdr.as_ref().unwrap();
    assert!(ultra_hdr.xmp.is_some());
    assert!(ultra_hdr.gain_map_metadata.is_some());
}

#[test]
fn compat_decoder_probes_metadata_for_upstream_ultrahdr_fixture() {
    let mut owned = UPSTREAM_ULTRAHDR.to_vec();
    let mut compressed = CompressedImage::from_bytes(
        owned.as_mut_slice(),
        sys::uhdr_color_gamut::UHDR_CG_UNSPECIFIED,
        sys::uhdr_color_transfer::UHDR_CT_UNSPECIFIED,
        sys::uhdr_color_range::UHDR_CR_UNSPECIFIED,
    );
    let mut decoder = CompatDecoder::new().unwrap();
    decoder.set_image(&mut compressed).unwrap();

    assert!(decoder.gainmap_metadata().unwrap().is_some());
}

#[test]
fn compat_decoder_recovers_display_p3_gamut_from_upstream_fixture_icc() {
    let mut owned = UPSTREAM_ULTRAHDR.to_vec();
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

    let (gamut, transfer, range) = decoded.meta();
    assert_eq!(gamut, sys::uhdr_color_gamut::UHDR_CG_DISPLAY_P3);
    assert_eq!(transfer, sys::uhdr_color_transfer::UHDR_CT_PQ);
    assert_eq!(range, sys::uhdr_color_range::UHDR_CR_FULL_RANGE);

    let gamut_info = decoded.gamut_info().unwrap();
    assert_eq!(gamut_info.standard, Some(ColorGamut::DisplayP3));
}
