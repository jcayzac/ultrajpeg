use ultrajpeg::{MetadataLocation, decode, inspect};

const PLAIN_SDR: &[u8] = include_bytes!("fixtures/plain-sdr.jpg");
const SAMPLE_ULTRAHDR: &[u8] = include_bytes!("fixtures/sample-ultrahdr.jpg");

#[test]
fn inspect_plain_fixture_exposes_metadata_without_decoding_pixels() {
    let inspected = inspect(PLAIN_SDR).unwrap();

    assert_eq!(inspected.primary_jpeg_len, PLAIN_SDR.len());
    assert!(inspected.gain_map_jpeg_len.is_none());
    assert_eq!(
        inspected
            .primary_metadata
            .color
            .icc_profile
            .as_ref()
            .map(Vec::len),
        Some(19)
    );
    assert!(inspected.primary_metadata.exif.is_some());
    assert!(inspected.ultra_hdr.is_none());
}

#[test]
fn inspect_ultrahdr_fixture_exposes_gain_map_metadata_and_provenance() {
    let inspected = inspect(SAMPLE_ULTRAHDR).unwrap();

    assert!(inspected.primary_jpeg_len < SAMPLE_ULTRAHDR.len());
    assert!(inspected.gain_map_jpeg_len.is_some());
    assert!(inspected.primary_metadata.color.icc_profile.is_some());
    assert!(inspected.ultra_hdr.is_some());

    let ultra_hdr = inspected.ultra_hdr.as_ref().unwrap();
    let metadata = ultra_hdr.gain_map_metadata.as_ref().unwrap();
    assert!(metadata.hdr_capacity_max >= 4.0);
    assert!(matches!(
        ultra_hdr.xmp_location,
        Some(MetadataLocation::Primary | MetadataLocation::GainMap)
    ));
    assert!(matches!(
        ultra_hdr.iso_21496_1_location,
        Some(MetadataLocation::Primary | MetadataLocation::GainMap)
    ));
}

#[test]
fn inspect_succeeds_when_decode_fails_after_sof_corruption() {
    let mut corrupted = SAMPLE_ULTRAHDR.to_vec();
    zero_out_sof_dimensions(&mut corrupted);

    let inspected = inspect(&corrupted).unwrap();
    assert!(inspected.ultra_hdr.is_some());
    assert!(decode(&corrupted).is_err());
}

fn zero_out_sof_dimensions(bytes: &mut [u8]) {
    let mut offset = 2;

    while offset + 1 < bytes.len() {
        assert_eq!(bytes[offset], 0xFF, "invalid JPEG marker prefix");
        while offset < bytes.len() && bytes[offset] == 0xFF {
            offset += 1;
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
            panic!("truncated JPEG marker stream");
        }

        let segment_len = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
        let contents_start = offset + 2;
        let contents_end = offset + segment_len;
        if contents_end > bytes.len() {
            panic!("truncated JPEG segment");
        }

        if matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
            bytes[contents_start + 1] = 0;
            bytes[contents_start + 2] = 0;
            bytes[contents_start + 3] = 0;
            bytes[contents_start + 4] = 0;
            return;
        }

        offset = contents_end;
        if marker == 0xDA {
            break;
        }
    }

    panic!("SOF marker not found");
}

fn marker_has_length(marker: u8) -> bool {
    matches!(marker, 0xC0..=0xFE if !matches!(marker, 0xD0..=0xD9))
}
