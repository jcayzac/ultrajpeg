use crate::{
    error::{Error, Result},
    metadata::{
        COLOR_MARKER_PREFIX, ISO_NAMESPACE, XMP_NAMESPACE, decode_color_metadata,
        encode_color_metadata, iso_segment_payload, xmp_segment_payload,
    },
    types::{ColorMetadata, DecodeOptions, UltraHdrMetadata},
};
use img_parts::{
    Bytes, ImageEXIF, ImageICC,
    jpeg::{Jpeg, JpegSegment, markers},
};
use ultrahdr_core::metadata::{MPF_IDENTIFIER, create_mpf_header, find_jpeg_boundaries, parse_mpf};

const EXIF_DATA_PREFIX: &[u8] = b"Exif\0\0";
const ICC_DATA_PREFIX: &[u8] = b"ICC_PROFILE\0";

pub(crate) struct ParsedContainer<'a> {
    pub(crate) primary_jpeg: &'a [u8],
    pub(crate) gain_map_jpeg: Option<&'a [u8]>,
    pub(crate) color_metadata: ColorMetadata,
    pub(crate) xmp: Option<String>,
    pub(crate) iso: Option<Vec<u8>>,
}

pub(crate) struct InspectedContainer {
    pub(crate) primary_jpeg_len: usize,
    pub(crate) gain_map_jpeg_len: Option<usize>,
    pub(crate) color_metadata: ColorMetadata,
    pub(crate) xmp: Option<String>,
    pub(crate) iso: Option<Vec<u8>>,
}

pub(crate) fn inspect_container(bytes: &[u8]) -> Result<InspectedContainer> {
    let primary_range = primary_range(bytes)?;
    let primary_jpeg = &bytes[primary_range.0..primary_range.1];
    let scanned = scan_primary_metadata(primary_jpeg)?;

    Ok(InspectedContainer {
        primary_jpeg_len: primary_jpeg.len(),
        gain_map_jpeg_len: gain_map_range(bytes).map(|range| range.1 - range.0),
        color_metadata: scanned.color_metadata,
        xmp: scanned.xmp,
        iso: scanned.iso,
    })
}

pub(crate) fn parse_container<'a>(
    bytes: &'a [u8],
    options: &DecodeOptions,
) -> Result<ParsedContainer<'a>> {
    let primary_range = primary_range(bytes)?;
    let primary_jpeg = &bytes[primary_range.0..primary_range.1];
    let scanned = scan_primary_metadata(primary_jpeg)?;

    let gain_map_jpeg = if options.decode_gain_map {
        gain_map_range(bytes).map(|range| &bytes[range.0..range.1])
    } else {
        None
    };

    Ok(ParsedContainer {
        primary_jpeg,
        gain_map_jpeg,
        color_metadata: scanned.color_metadata,
        xmp: scanned.xmp,
        iso: scanned.iso,
    })
}

pub(crate) fn assemble_container(
    primary_jpeg: &[u8],
    gain_map_jpeg: Option<&[u8]>,
    color_metadata: &ColorMetadata,
    ultra_hdr_metadata: Option<&UltraHdrMetadata>,
) -> Result<Vec<u8>> {
    assemble_container_impl(
        Bytes::copy_from_slice(primary_jpeg),
        gain_map_jpeg,
        color_metadata,
        ultra_hdr_metadata,
    )
}

pub(crate) fn assemble_container_owned(
    primary_jpeg: Vec<u8>,
    gain_map_jpeg: Option<&[u8]>,
    color_metadata: &ColorMetadata,
    ultra_hdr_metadata: Option<&UltraHdrMetadata>,
) -> Result<Vec<u8>> {
    assemble_container_impl(
        Bytes::from(primary_jpeg),
        gain_map_jpeg,
        color_metadata,
        ultra_hdr_metadata,
    )
}

fn assemble_container_impl(
    primary_jpeg: Bytes,
    gain_map_jpeg: Option<&[u8]>,
    color_metadata: &ColorMetadata,
    ultra_hdr_metadata: Option<&UltraHdrMetadata>,
) -> Result<Vec<u8>> {
    let mut jpeg = Jpeg::from_bytes(primary_jpeg)?;

    if let Some(icc_profile) = color_metadata.icc_profile.clone() {
        jpeg.set_icc_profile(Some(Bytes::from(icc_profile)));
    }
    if let Some(exif) = color_metadata.exif.clone() {
        jpeg.set_exif(Some(Bytes::from(exif)));
    }

    remove_metadata_segments(&mut jpeg);

    let mut insert_at = metadata_insert_index(&jpeg);

    if let Some(ultra_hdr_metadata) = ultra_hdr_metadata {
        if let Some(xmp) = ultra_hdr_metadata.xmp.as_deref() {
            jpeg.segments_mut().insert(
                insert_at,
                JpegSegment::new_with_contents(
                    markers::APP1,
                    Bytes::from(xmp_segment_payload(xmp)),
                ),
            );
            insert_at += 1;
        }

        if let Some(iso_21496_1) = ultra_hdr_metadata.iso_21496_1.as_deref() {
            jpeg.segments_mut().insert(
                insert_at,
                JpegSegment::new_with_contents(
                    markers::APP2,
                    Bytes::from(iso_segment_payload(iso_21496_1)),
                ),
            );
            insert_at += 1;
        }
    }

    if let Some(explicit_color) = encode_color_metadata(color_metadata) {
        jpeg.segments_mut().insert(
            insert_at,
            JpegSegment::new_with_contents(markers::APP11, Bytes::from(explicit_color)),
        );
        insert_at += 1;
    }

    if let Some(gain_map_jpeg) = gain_map_jpeg {
        let insertion_offset = byte_offset_for_index(&jpeg, insert_at);
        let header_len = create_mpf_header(0, gain_map_jpeg.len(), Some(insertion_offset)).len();
        let primary_len = jpeg.len() + header_len;
        let mpf_segment =
            create_mpf_header(primary_len, gain_map_jpeg.len(), Some(insertion_offset));
        jpeg.segments_mut().insert(
            insert_at,
            JpegSegment::new_with_contents(markers::APP2, Bytes::from(mpf_segment[4..].to_vec())),
        );
    }

    let mut output = jpeg.encoder().bytes().to_vec();
    if let Some(gain_map_jpeg) = gain_map_jpeg {
        output.extend_from_slice(gain_map_jpeg);
    }
    Ok(output)
}

fn primary_range(bytes: &[u8]) -> Result<(usize, usize)> {
    if let Ok(images) = parse_mpf(bytes)
        && let Some(range) = images.first().copied()
    {
        return Ok(range);
    }

    find_jpeg_boundaries(bytes)
        .into_iter()
        .next()
        .ok_or_else(|| Error::Container("could not locate a JPEG codestream".into()))
}

fn gain_map_range(bytes: &[u8]) -> Option<(usize, usize)> {
    if let Ok(images) = parse_mpf(bytes)
        && let Some(range) = images.get(1).copied()
    {
        return Some(range);
    }

    let boundaries = find_jpeg_boundaries(bytes);
    if boundaries.len() > 1 {
        return boundaries.get(1).copied();
    }

    None
}

struct ScannedMetadata {
    color_metadata: ColorMetadata,
    xmp: Option<String>,
    iso: Option<Vec<u8>>,
}

fn scan_primary_metadata(primary_jpeg: &[u8]) -> Result<ScannedMetadata> {
    if primary_jpeg.len() < 4 || primary_jpeg[0] != markers::P || primary_jpeg[1] != markers::SOI {
        return Err(Error::Container("invalid JPEG signature".into()));
    }

    let mut offset = 2;
    let mut icc_chunks = Vec::new();
    let mut exif = None;
    let mut xmp = None;
    let mut iso = None;
    let mut gamut = None;
    let mut transfer = None;

    while offset + 1 < primary_jpeg.len() {
        if primary_jpeg[offset] != markers::P {
            return Err(Error::Container(format!(
                "invalid JPEG marker prefix at byte offset {offset}"
            )));
        }

        while offset < primary_jpeg.len() && primary_jpeg[offset] == markers::P {
            offset += 1;
        }
        if offset >= primary_jpeg.len() {
            return Err(Error::Container("truncated JPEG marker stream".into()));
        }

        let marker = primary_jpeg[offset];
        offset += 1;

        if marker == markers::EOI {
            break;
        }
        if !marker_has_length(marker) {
            continue;
        }

        let contents = next_segment_contents(primary_jpeg, &mut offset, marker)?;

        match marker {
            markers::APP1 if contents.starts_with(EXIF_DATA_PREFIX) => {
                exif = Some(contents[EXIF_DATA_PREFIX.len()..].to_vec());
            }
            markers::APP1 if contents.starts_with(XMP_NAMESPACE) => {
                let payload = &contents[XMP_NAMESPACE.len()..];
                xmp = Some(String::from_utf8(payload.to_vec()).map_err(|error| {
                    Error::Metadata(format!("invalid UTF-8 XMP payload: {error}"))
                })?);
            }
            markers::APP2 if contents.starts_with(ICC_DATA_PREFIX) => {
                let chunk = parse_icc_chunk(contents)?;
                icc_chunks.push(chunk);
            }
            markers::APP2 if contents.starts_with(ISO_NAMESPACE) => {
                iso = Some(contents[ISO_NAMESPACE.len()..].to_vec());
            }
            markers::APP11 if contents.starts_with(COLOR_MARKER_PREFIX) => {
                if let Some((parsed_gamut, parsed_transfer)) = decode_color_metadata(contents)? {
                    gamut = Some(parsed_gamut);
                    transfer = Some(parsed_transfer);
                }
            }
            _ => {}
        }

        if marker == markers::SOS {
            break;
        }
    }

    Ok(ScannedMetadata {
        color_metadata: ColorMetadata {
            icc_profile: assemble_icc_profile(icc_chunks)?,
            exif,
            gamut,
            transfer,
        },
        xmp,
        iso,
    })
}

fn parse_icc_chunk(contents: &[u8]) -> Result<(u8, u8, &[u8])> {
    if contents.len() < ICC_DATA_PREFIX.len() + 2 {
        return Err(Error::Metadata("truncated ICC profile segment".into()));
    }

    Ok((
        contents[ICC_DATA_PREFIX.len()],
        contents[ICC_DATA_PREFIX.len() + 1],
        &contents[ICC_DATA_PREFIX.len() + 2..],
    ))
}

fn assemble_icc_profile(mut chunks: Vec<(u8, u8, &[u8])>) -> Result<Option<Vec<u8>>> {
    if chunks.is_empty() {
        return Ok(None);
    }

    chunks.sort_by_key(|(seqno, _, _)| *seqno);
    let expected = chunks[0].1;
    if expected as usize != chunks.len() {
        return Err(Error::Metadata("incomplete ICC profile segment set".into()));
    }
    let mut profile = Vec::new();

    for (index, (seqno, total, data)) in chunks.into_iter().enumerate() {
        let expected_seqno = (index + 1) as u8;
        if seqno != expected_seqno || total != expected {
            return Err(Error::Metadata(
                "invalid ICC profile segment ordering".into(),
            ));
        }
        profile.extend_from_slice(data);
    }

    Ok(Some(profile))
}

fn next_segment_contents<'a>(bytes: &'a [u8], offset: &mut usize, marker: u8) -> Result<&'a [u8]> {
    if *offset + 2 > bytes.len() {
        return Err(Error::Container(format!(
            "truncated JPEG segment 0x{marker:02x}"
        )));
    }

    let segment_len = u16::from_be_bytes([bytes[*offset], bytes[*offset + 1]]) as usize;
    if segment_len < 2 {
        return Err(Error::Container(format!(
            "invalid JPEG segment length for marker 0x{marker:02x}"
        )));
    }

    let contents_start = *offset + 2;
    let contents_end = *offset + segment_len;
    if contents_end > bytes.len() {
        return Err(Error::Container(format!(
            "truncated JPEG segment payload for marker 0x{marker:02x}"
        )));
    }

    *offset = contents_end;
    Ok(&bytes[contents_start..contents_end])
}

fn marker_has_length(marker: u8) -> bool {
    matches!(
        marker,
        markers::RST0..=markers::RST7
            | markers::APP0..=markers::APP15
            | markers::SOF0..=markers::SOF15
            | markers::SOS
            | markers::COM
            | markers::DQT
            | markers::DRI
    )
}

fn remove_metadata_segments(jpeg: &mut Jpeg) {
    jpeg.segments_mut().retain(|segment| {
        let contents = segment.contents();
        !((segment.marker() == markers::APP1 && contents.starts_with(XMP_NAMESPACE))
            || (segment.marker() == markers::APP2
                && (contents.starts_with(ISO_NAMESPACE) || contents.starts_with(MPF_IDENTIFIER)))
            || (segment.marker() == markers::APP11 && contents.starts_with(COLOR_MARKER_PREFIX)))
    });
}

fn metadata_insert_index(jpeg: &Jpeg) -> usize {
    jpeg.segments()
        .iter()
        .position(|segment| {
            !matches!(
                segment.marker(),
                markers::APP0..=markers::APP15 | markers::COM
            )
        })
        .unwrap_or(jpeg.segments().len())
}

fn byte_offset_for_index(jpeg: &Jpeg, segment_index: usize) -> usize {
    let prefix_len = jpeg
        .segments()
        .iter()
        .take(segment_index)
        .map(JpegSegment::len)
        .sum::<usize>();
    2 + prefix_len
}
