use ultrahdr_core::{ColorGamut, ColorTransfer, PixelFormat, gainmap::HdrOutputFormat};
use ultrajpeg::{
    CompressionEffort, ContainerKind, DecodeOptions, EncodeOptions, MetadataLocation,
    PreparePrimaryOptions, compute_gain_map, decode, inspect, inspect_container_layout,
    parse_gain_map_xmp, parse_iso_21496_1, prepare_sdr_primary,
};

const PLAIN_SDR: &[u8] = include_bytes!("fixtures/plain-sdr.jpg");
const SAMPLE_ULTRAHDR: &[u8] = include_bytes!("fixtures/sample-ultrahdr.jpg");

fn sample_hdr() -> ultrajpeg::Image {
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

    ultrajpeg::Image::from_data(
        4,
        4,
        PixelFormat::Rgba32F,
        ColorGamut::DisplayP3,
        ColorTransfer::Linear,
        pixels,
    )
    .unwrap()
}

#[test]
fn raw_xmp_parser_handles_fixture_payload() {
    let inspection = inspect(SAMPLE_ULTRAHDR).unwrap();
    let ultra_hdr = inspection.ultra_hdr.as_ref().unwrap();
    let parsed = parse_gain_map_xmp(ultra_hdr.xmp.as_deref().unwrap()).unwrap();

    assert!(parsed.metadata.hdr_capacity_max >= 4.0);
    assert!(matches!(
        ultra_hdr.xmp_location,
        Some(MetadataLocation::Primary | MetadataLocation::GainMap)
    ));
}

#[test]
fn raw_xmp_parser_remains_available_for_payloads_filtered_during_decode() {
    let xmp = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description
      xmlns:hdrgm="http://ns.adobe.com/hdr-gain-map/1.0/"
      hdrgm:Version="1.0"
      hdrgm:GainMapMin="0.000000"
      hdrgm:GainMapMax="2.000000"
      hdrgm:Gamma="1.000000"
      hdrgm:OffsetSDR="0.015625"
      hdrgm:OffsetHDR="0.015625"
      hdrgm:HDRCapacityMin="0.000000"
      hdrgm:HDRCapacityMax="2.000000"
      hdrgm:BaseRenditionIsHDR="True"/>
  </rdf:RDF>
</x:xmpmeta>"#;

    let parsed = parse_gain_map_xmp(xmp).unwrap();
    assert!((parsed.metadata.max_content_boost[0] - 4.0).abs() < 0.01);
}

#[test]
fn raw_iso_parser_handles_fixture_payload() {
    let inspection = inspect(SAMPLE_ULTRAHDR).unwrap();
    let ultra_hdr = inspection.ultra_hdr.as_ref().unwrap();
    let parsed = parse_iso_21496_1(ultra_hdr.iso_21496_1.as_deref().unwrap()).unwrap();

    assert!(parsed.hdr_capacity_max >= 4.0);
}

#[test]
fn container_layout_reports_plain_and_mpf_fixtures() {
    let plain = inspect_container_layout(PLAIN_SDR).unwrap();
    assert_eq!(plain.kind, ContainerKind::Jpeg);
    assert_eq!(plain.codestreams.len(), 1);
    assert_eq!(plain.primary_index, 0);
    assert_eq!(plain.gain_map_index, None);
    assert_eq!(plain.codestreams[0].offset, 0);
    assert_eq!(plain.codestreams[0].len, PLAIN_SDR.len());

    let ultra_hdr = inspect_container_layout(SAMPLE_ULTRAHDR).unwrap();
    assert_eq!(ultra_hdr.kind, ContainerKind::Mpf);
    assert_eq!(ultra_hdr.codestreams.len(), 2);
    assert_eq!(ultra_hdr.primary_index, 0);
    assert_eq!(ultra_hdr.gain_map_index, Some(1));
    assert_eq!(ultra_hdr.codestreams[0].offset, 0);
    assert!(ultra_hdr.codestreams[0].len < SAMPLE_ULTRAHDR.len());
    assert!(ultra_hdr.codestreams[1].offset > 0);
    assert!(ultra_hdr.codestreams[1].len > 0);
}

#[test]
fn prepare_sdr_primary_returns_matching_pixels_and_metadata() {
    let prepared =
        prepare_sdr_primary(&sample_hdr(), &PreparePrimaryOptions::ultra_hdr_defaults()).unwrap();

    assert_eq!(prepared.image.width, 4);
    assert_eq!(prepared.image.height, 4);
    assert_eq!(prepared.image.format, PixelFormat::Rgb8);
    assert_eq!(prepared.image.gamut, ColorGamut::DisplayP3);
    assert_eq!(prepared.image.transfer, ColorTransfer::Srgb);
    assert_eq!(prepared.image.data.len(), 4 * 4 * 3);
    assert_eq!(prepared.metadata.color.gamut, Some(ColorGamut::DisplayP3));
    assert_eq!(prepared.metadata.color.transfer, Some(ColorTransfer::Srgb));
    assert!(prepared.metadata.color.icc_profile.is_some());
}

#[test]
fn prepared_primary_composes_with_gain_map_and_encode_workflows() {
    let hdr = sample_hdr();
    let prepared = prepare_sdr_primary(&hdr, &PreparePrimaryOptions::ultra_hdr_defaults()).unwrap();
    let computed = compute_gain_map(&hdr, &prepared.image, &Default::default()).unwrap();
    let encoded = ultrajpeg::encode(
        &prepared.image,
        &EncodeOptions {
            primary_metadata: prepared.metadata.clone(),
            gain_map: Some(computed.into_bundle(90, false, CompressionEffort::Balanced)),
            ..EncodeOptions::default()
        },
    )
    .unwrap();

    let decoded = decode_with_retained_gain_map(&encoded);
    assert!(decoded.ultra_hdr.is_some());
    assert!(decoded.gain_map.is_some());
}

#[test]
fn prepare_sdr_primary_rejects_bt2100_output() {
    let error = prepare_sdr_primary(
        &sample_hdr(),
        &PreparePrimaryOptions {
            target_gamut: ColorGamut::Bt2100,
            ..PreparePrimaryOptions::default()
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("Bt709 and DisplayP3"));
}

#[test]
fn prepare_sdr_primary_rejects_non_finite_peaks() {
    let target_peak_error = prepare_sdr_primary(
        &sample_hdr(),
        &PreparePrimaryOptions {
            target_peak_nits: f32::NAN,
            ..PreparePrimaryOptions::default()
        },
    )
    .unwrap_err();
    assert!(
        target_peak_error
            .to_string()
            .contains("finite and positive")
    );

    let source_peak_error = prepare_sdr_primary(
        &sample_hdr(),
        &PreparePrimaryOptions {
            source_peak_nits: Some(f32::INFINITY),
            ..PreparePrimaryOptions::default()
        },
    )
    .unwrap_err();
    assert!(
        source_peak_error
            .to_string()
            .contains("finite and positive")
    );
}

#[test]
fn reconstruct_hdr_rejects_non_finite_display_boost() {
    let decoded = decode(SAMPLE_ULTRAHDR).unwrap();
    let error = decoded
        .reconstruct_hdr(f32::NAN, HdrOutputFormat::LinearFloat)
        .unwrap_err();

    assert!(error.to_string().contains("display_boost"));
}

#[test]
fn reconstruct_hdr_rejects_invalid_effective_metadata() {
    let mut decoded = decode(SAMPLE_ULTRAHDR).unwrap();
    decoded
        .gain_map
        .as_mut()
        .unwrap()
        .metadata
        .as_mut()
        .unwrap()
        .max_content_boost[0] = 0.0;

    let error = decoded
        .reconstruct_hdr(4.0, HdrOutputFormat::LinearFloat)
        .unwrap_err();

    assert!(error.to_string().contains("max_content_boost[0]"));
}

fn decode_with_retained_gain_map(bytes: &[u8]) -> ultrajpeg::DecodedImage {
    ultrajpeg::decode_with_options(
        bytes,
        DecodeOptions {
            retain_gain_map_jpeg: true,
            ..DecodeOptions::default()
        },
    )
    .unwrap()
}
