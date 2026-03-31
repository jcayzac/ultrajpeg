use crate::{
    error::{Error, Result},
    metadata::{
        COLOR_MARKER_PREFIX, EncodedUltraHdrMetadata, ISO_NAMESPACE, XMP_NAMESPACE,
        decode_color_metadata, encode_color_metadata, iso_segment_payload,
        parse_ultra_hdr_metadata, xmp_segment_payload,
    },
    types::{
        ColorMetadata, DecodeOptions, GamutInfo, MetadataLocation, PrimaryMetadata,
        UltraHdrMetadata,
    },
};
use img_parts::{
    Bytes, ImageEXIF, ImageICC,
    jpeg::{Jpeg, JpegSegment, markers},
};
use ultrahdr_core::metadata::{MPF_IDENTIFIER, create_mpf_header, find_jpeg_boundaries, parse_mpf};

const EXIF_DATA_PREFIX: &[u8] = b"Exif\0\0";
const ICC_DATA_PREFIX: &[u8] = b"ICC_PROFILE\0";
const EXTENDED_XMP_NAMESPACE: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";
const EXTENDED_XMP_GUID_LEN: usize = 32;
const EXTENDED_XMP_LENGTH_LEN: usize = 4;
const EXTENDED_XMP_OFFSET_LEN: usize = 4;
const EXTENDED_XMP_HEADER_LEN: usize =
    EXTENDED_XMP_GUID_LEN + EXTENDED_XMP_LENGTH_LEN + EXTENDED_XMP_OFFSET_LEN;

pub(crate) struct ParsedContainer<'a> {
    pub(crate) primary_jpeg: &'a [u8],
    pub(crate) gain_map_jpeg: Option<&'a [u8]>,
    pub(crate) primary_metadata: PrimaryMetadata,
    pub(crate) xmp: Option<String>,
    pub(crate) xmp_location: Option<MetadataLocation>,
    pub(crate) iso: Option<Vec<u8>>,
    pub(crate) iso_location: Option<MetadataLocation>,
}

pub(crate) struct InspectedContainer {
    pub(crate) primary_jpeg_len: usize,
    pub(crate) gain_map_jpeg_len: Option<usize>,
    pub(crate) primary_metadata: PrimaryMetadata,
    pub(crate) xmp: Option<String>,
    pub(crate) xmp_location: Option<MetadataLocation>,
    pub(crate) iso: Option<Vec<u8>>,
    pub(crate) iso_location: Option<MetadataLocation>,
}

pub(crate) fn inspect_container(bytes: &[u8]) -> Result<InspectedContainer> {
    let primary_range = primary_range(bytes)?;
    let primary_jpeg = &bytes[primary_range.0..primary_range.1];
    let scanned = scan_primary_metadata(primary_jpeg)?;
    let gain_map_range = gain_map_range(bytes);
    let effective_metadata = effective_ultra_hdr_sources(bytes, &scanned, gain_map_range)?;

    Ok(InspectedContainer {
        primary_jpeg_len: primary_jpeg.len(),
        gain_map_jpeg_len: gain_map_range.map(|range| range.1 - range.0),
        primary_metadata: scanned.primary_metadata,
        xmp: effective_metadata.xmp,
        xmp_location: effective_metadata.xmp_location,
        iso: effective_metadata.iso,
        iso_location: effective_metadata.iso_location,
    })
}

pub(crate) fn parse_container<'a>(
    bytes: &'a [u8],
    options: &DecodeOptions,
) -> Result<ParsedContainer<'a>> {
    let primary_range = primary_range(bytes)?;
    let primary_jpeg = &bytes[primary_range.0..primary_range.1];
    let scanned = scan_primary_metadata(primary_jpeg)?;
    let gain_map_range = gain_map_range(bytes);
    let effective_metadata = effective_ultra_hdr_sources(bytes, &scanned, gain_map_range)?;

    let gain_map_jpeg = if options.decode_gain_map {
        gain_map_range.map(|range| &bytes[range.0..range.1])
    } else {
        None
    };

    Ok(ParsedContainer {
        primary_jpeg,
        gain_map_jpeg,
        primary_metadata: scanned.primary_metadata,
        xmp: effective_metadata.xmp,
        xmp_location: effective_metadata.xmp_location,
        iso: effective_metadata.iso,
        iso_location: effective_metadata.iso_location,
    })
}

pub(crate) fn assemble_container_owned(
    primary_jpeg: Vec<u8>,
    gain_map_jpeg: Option<&[u8]>,
    primary_metadata: &PrimaryMetadata,
    ultra_hdr_metadata: Option<&EncodedUltraHdrMetadata>,
) -> Result<Vec<u8>> {
    assemble_container_impl(
        Bytes::from(primary_jpeg),
        gain_map_jpeg,
        primary_metadata,
        ultra_hdr_metadata,
    )
}

fn assemble_container_impl(
    primary_jpeg: Bytes,
    gain_map_jpeg: Option<&[u8]>,
    primary_metadata: &PrimaryMetadata,
    ultra_hdr_metadata: Option<&EncodedUltraHdrMetadata>,
) -> Result<Vec<u8>> {
    let mut jpeg = Jpeg::from_bytes(primary_jpeg)?;
    let gain_map_jpeg = gain_map_jpeg
        .map(|gain_map_jpeg| rewrite_gain_map_jpeg(gain_map_jpeg, ultra_hdr_metadata))
        .transpose()?;

    if let Some(icc_profile) = primary_metadata.color.icc_profile.clone() {
        jpeg.set_icc_profile(Some(Bytes::from(icc_profile)));
    }
    if let Some(exif) = primary_metadata.exif.clone() {
        jpeg.set_exif(Some(Bytes::from(exif)));
    }

    remove_metadata_segments(&mut jpeg);

    let mut insert_at = metadata_insert_index(&jpeg);

    if let Some(ultra_hdr_metadata) = ultra_hdr_metadata {
        insert_at = insert_xmp_segment(&mut jpeg, insert_at, Some(&ultra_hdr_metadata.primary_xmp));
    }

    if let Some(explicit_color) = encode_color_metadata(&primary_metadata.color) {
        jpeg.segments_mut().insert(
            insert_at,
            JpegSegment::new_with_contents(markers::APP11, Bytes::from(explicit_color)),
        );
        insert_at += 1;
    }

    if let Some(gain_map_jpeg) = gain_map_jpeg.as_deref() {
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
        output.extend_from_slice(&gain_map_jpeg);
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
    primary_metadata: PrimaryMetadata,
    xmp: Option<String>,
    iso: Option<Vec<u8>>,
}

struct EffectiveUltraHdrSources {
    xmp: Option<String>,
    xmp_location: Option<MetadataLocation>,
    iso: Option<Vec<u8>>,
    iso_location: Option<MetadataLocation>,
}

#[derive(Debug)]
struct ExtendedXmpChunk {
    guid: String,
    total_length: u32,
    offset: u32,
    data: Vec<u8>,
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
    let mut extended_xmp_chunks = Vec::new();

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
            markers::APP1 if contents.starts_with(EXTENDED_XMP_NAMESPACE) => {
                extended_xmp_chunks.push(parse_extended_xmp_chunk(contents)?);
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

    let icc_profile = assemble_icc_profile(icc_chunks)?;
    let gamut_info = gamut.map(GamutInfo::from_standard).or_else(|| {
        icc_profile
            .as_deref()
            .and_then(crate::icc::gamut_info_from_profile)
    });
    let gamut = gamut.or_else(|| gamut_info.as_ref().and_then(|info| info.standard));

    Ok(ScannedMetadata {
        primary_metadata: PrimaryMetadata {
            color: ColorMetadata {
                icc_profile,
                gamut,
                gamut_info,
                transfer,
            },
            exif,
        },
        xmp: reassemble_xmp(xmp, extended_xmp_chunks)?,
        iso,
    })
}

fn parse_extended_xmp_chunk(contents: &[u8]) -> Result<ExtendedXmpChunk> {
    let header_start = EXTENDED_XMP_NAMESPACE.len();
    let minimum_len = header_start + EXTENDED_XMP_HEADER_LEN;
    if contents.len() < minimum_len {
        return Err(Error::Container(
            "truncated Adobe extended XMP segment".into(),
        ));
    }

    let guid_bytes = &contents[header_start..header_start + EXTENDED_XMP_GUID_LEN];
    let guid = std::str::from_utf8(guid_bytes)
        .map_err(|error| Error::Metadata(format!("invalid extended XMP GUID: {error}")))?
        .to_owned();

    let total_length_start = header_start + EXTENDED_XMP_GUID_LEN;
    let total_length = u32::from_be_bytes(
        contents[total_length_start..total_length_start + EXTENDED_XMP_LENGTH_LEN]
            .try_into()
            .expect("extended XMP total length field has fixed width"),
    );

    let offset_start = total_length_start + EXTENDED_XMP_LENGTH_LEN;
    let offset = u32::from_be_bytes(
        contents[offset_start..offset_start + EXTENDED_XMP_OFFSET_LEN]
            .try_into()
            .expect("extended XMP offset field has fixed width"),
    );

    Ok(ExtendedXmpChunk {
        guid,
        total_length,
        offset,
        data: contents[offset_start + EXTENDED_XMP_OFFSET_LEN..].to_vec(),
    })
}

fn reassemble_xmp(
    primary_xmp: Option<String>,
    mut extended_chunks: Vec<ExtendedXmpChunk>,
) -> Result<Option<String>> {
    let Some(primary_xmp) = primary_xmp else {
        return Ok(None);
    };

    let Some(extended_guid) = extract_extended_xmp_guid(&primary_xmp) else {
        return Ok(Some(primary_xmp));
    };

    extended_chunks.retain(|chunk| chunk.guid == extended_guid);
    if extended_chunks.is_empty() {
        return Ok(Some(primary_xmp));
    }

    extended_chunks.sort_by_key(|chunk| chunk.offset);

    let expected_total_length = extended_chunks[0].total_length as usize;
    let mut extended = vec![0_u8; expected_total_length];
    let mut filled = vec![false; expected_total_length];

    for chunk in extended_chunks {
        if chunk.total_length as usize != expected_total_length {
            return Err(Error::Container(
                "inconsistent Adobe extended XMP total length".into(),
            ));
        }

        let start = chunk.offset as usize;
        let end = start
            .checked_add(chunk.data.len())
            .ok_or_else(|| Error::Container("Adobe extended XMP offset overflow".into()))?;
        if end > expected_total_length {
            return Err(Error::Container(
                "Adobe extended XMP chunk exceeds advertised total length".into(),
            ));
        }

        extended[start..end].copy_from_slice(&chunk.data);
        filled[start..end].fill(true);
    }

    if filled.iter().any(|filled| !filled) {
        return Err(Error::Container(
            "Adobe extended XMP chunks are incomplete".into(),
        ));
    }

    let extended_xmp = String::from_utf8(extended)
        .map_err(|error| Error::Metadata(format!("invalid UTF-8 extended XMP payload: {error}")))?;

    Ok(Some(format!("{primary_xmp}\n{extended_xmp}")))
}

fn effective_ultra_hdr_sources(
    bytes: &[u8],
    primary: &ScannedMetadata,
    gain_map_range: Option<(usize, usize)>,
) -> Result<EffectiveUltraHdrSources> {
    let primary_metadata = parse_ultra_hdr_metadata(
        primary.xmp.as_deref(),
        primary.xmp.as_ref().map(|_| MetadataLocation::Primary),
        primary.iso.as_deref(),
        primary.iso.as_ref().map(|_| MetadataLocation::Primary),
    )?;
    if has_effective_gain_map_metadata(primary_metadata.as_ref()) {
        return Ok(EffectiveUltraHdrSources {
            xmp: primary.xmp.clone(),
            xmp_location: primary.xmp.as_ref().map(|_| MetadataLocation::Primary),
            iso: primary.iso.clone(),
            iso_location: primary.iso.as_ref().map(|_| MetadataLocation::Primary),
        });
    }

    let Some(gain_map_range) = gain_map_range else {
        return Ok(EffectiveUltraHdrSources {
            xmp: primary.xmp.clone(),
            xmp_location: primary.xmp.as_ref().map(|_| MetadataLocation::Primary),
            iso: primary.iso.clone(),
            iso_location: primary.iso.as_ref().map(|_| MetadataLocation::Primary),
        });
    };

    let gain_map_jpeg = &bytes[gain_map_range.0..gain_map_range.1];
    let gain_map_metadata = scan_primary_metadata(gain_map_jpeg)?;
    let parsed_gain_map_metadata = parse_ultra_hdr_metadata(
        gain_map_metadata.xmp.as_deref(),
        gain_map_metadata
            .xmp
            .as_ref()
            .map(|_| MetadataLocation::GainMap),
        gain_map_metadata.iso.as_deref(),
        gain_map_metadata
            .iso
            .as_ref()
            .map(|_| MetadataLocation::GainMap),
    )?;

    if has_effective_gain_map_metadata(parsed_gain_map_metadata.as_ref()) {
        let xmp_location = gain_map_metadata
            .xmp
            .as_ref()
            .map(|_| MetadataLocation::GainMap);
        let iso_location = gain_map_metadata
            .iso
            .as_ref()
            .map(|_| MetadataLocation::GainMap);
        return Ok(EffectiveUltraHdrSources {
            xmp: gain_map_metadata.xmp,
            xmp_location,
            iso: gain_map_metadata.iso,
            iso_location,
        });
    }

    Ok(EffectiveUltraHdrSources {
        xmp: primary.xmp.clone(),
        xmp_location: primary.xmp.as_ref().map(|_| MetadataLocation::Primary),
        iso: primary.iso.clone(),
        iso_location: primary.iso.as_ref().map(|_| MetadataLocation::Primary),
    })
}

fn has_effective_gain_map_metadata(metadata: Option<&UltraHdrMetadata>) -> bool {
    metadata
        .and_then(|metadata| metadata.gain_map_metadata.as_ref())
        .is_some()
}

fn extract_extended_xmp_guid(xmp: &str) -> Option<String> {
    extract_xmp_attribute(xmp, "xmpNote:HasExtendedXMP")
}

fn extract_xmp_attribute(xmp: &str, attribute: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let pattern = format!("{attribute}={quote}");
        if let Some(start) = xmp.find(&pattern) {
            let value_start = start + pattern.len();
            if let Some(end) = xmp[value_start..].find(quote) {
                return Some(xmp[value_start..value_start + end].to_owned());
            }
        }
    }

    None
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

fn rewrite_gain_map_jpeg(
    gain_map_jpeg: &[u8],
    ultra_hdr_metadata: Option<&EncodedUltraHdrMetadata>,
) -> Result<Vec<u8>> {
    let Some(ultra_hdr_metadata) = ultra_hdr_metadata else {
        return Ok(gain_map_jpeg.to_vec());
    };

    let mut jpeg = Jpeg::from_bytes(Bytes::copy_from_slice(gain_map_jpeg))?;
    remove_embedded_metadata_segments(&mut jpeg);

    let mut insert_at = metadata_insert_index(&jpeg);
    insert_at = insert_xmp_segment(&mut jpeg, insert_at, Some(&ultra_hdr_metadata.gain_map_xmp));
    insert_iso_segment(
        &mut jpeg,
        insert_at,
        Some(ultra_hdr_metadata.gain_map_iso_21496_1.as_slice()),
    );

    Ok(jpeg.encoder().bytes().to_vec())
}

fn insert_xmp_segment(jpeg: &mut Jpeg, insert_at: usize, xmp: Option<&str>) -> usize {
    let Some(xmp) = xmp else {
        return insert_at;
    };

    jpeg.segments_mut().insert(
        insert_at,
        JpegSegment::new_with_contents(markers::APP1, Bytes::from(xmp_segment_payload(xmp))),
    );
    insert_at + 1
}

fn insert_iso_segment(jpeg: &mut Jpeg, insert_at: usize, iso_21496_1: Option<&[u8]>) -> usize {
    let Some(iso_21496_1) = iso_21496_1 else {
        return insert_at;
    };

    jpeg.segments_mut().insert(
        insert_at,
        JpegSegment::new_with_contents(
            markers::APP2,
            Bytes::from(iso_segment_payload(iso_21496_1)),
        ),
    );
    insert_at + 1
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

fn remove_embedded_metadata_segments(jpeg: &mut Jpeg) {
    jpeg.segments_mut().retain(|segment| {
        let contents = segment.contents();
        !((segment.marker() == markers::APP1 && contents.starts_with(XMP_NAMESPACE))
            || (segment.marker() == markers::APP2 && contents.starts_with(ISO_NAMESPACE))
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
