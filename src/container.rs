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

pub(crate) struct ParsedContainer {
    pub(crate) primary_jpeg: Vec<u8>,
    pub(crate) gain_map_jpeg: Option<Vec<u8>>,
    pub(crate) color_metadata: ColorMetadata,
    pub(crate) xmp: Option<String>,
    pub(crate) iso: Option<Vec<u8>>,
}

pub(crate) fn parse_container(bytes: &[u8], options: &DecodeOptions) -> Result<ParsedContainer> {
    let primary_range = primary_range(bytes)?;
    let primary_jpeg = bytes[primary_range.0..primary_range.1].to_vec();
    let jpeg = Jpeg::from_bytes(Bytes::from(primary_jpeg.clone()))?;

    let icc_profile = jpeg.icc_profile().map(|profile| profile.to_vec());
    let exif = jpeg.exif().map(|exif| exif.to_vec());
    let xmp = find_xmp(&jpeg)?;
    let iso = find_iso(&jpeg);
    let (gamut, transfer) = find_explicit_color(&jpeg)?;

    let gain_map_jpeg = if options.decode_gain_map {
        gain_map_range(bytes).map(|range| bytes[range.0..range.1].to_vec())
    } else {
        None
    };

    Ok(ParsedContainer {
        primary_jpeg,
        gain_map_jpeg,
        color_metadata: ColorMetadata {
            icc_profile,
            exif,
            gamut,
            transfer,
        },
        xmp,
        iso,
    })
}

pub(crate) fn assemble_container(
    primary_jpeg: &[u8],
    gain_map_jpeg: Option<&[u8]>,
    color_metadata: &ColorMetadata,
    ultra_hdr_metadata: Option<&UltraHdrMetadata>,
) -> Result<Vec<u8>> {
    let mut jpeg = Jpeg::from_bytes(Bytes::copy_from_slice(primary_jpeg))?;

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
        let primary_len = jpeg.clone().encoder().bytes().len() + header_len;
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

fn find_xmp(jpeg: &Jpeg) -> Result<Option<String>> {
    for segment in jpeg.segments() {
        if segment.marker() == markers::APP1 && segment.contents().starts_with(XMP_NAMESPACE) {
            let payload = &segment.contents()[XMP_NAMESPACE.len()..];
            return String::from_utf8(payload.to_vec())
                .map(Some)
                .map_err(|error| Error::Metadata(format!("invalid UTF-8 XMP payload: {error}")));
        }
    }
    Ok(None)
}

fn find_iso(jpeg: &Jpeg) -> Option<Vec<u8>> {
    jpeg.segments()
        .iter()
        .find(|segment| {
            segment.marker() == markers::APP2 && segment.contents().starts_with(ISO_NAMESPACE)
        })
        .map(|segment| segment.contents()[ISO_NAMESPACE.len()..].to_vec())
}

fn find_explicit_color(
    jpeg: &Jpeg,
) -> Result<(
    Option<ultrahdr_core::ColorGamut>,
    Option<ultrahdr_core::ColorTransfer>,
)> {
    for segment in jpeg.segments() {
        if segment.marker() == markers::APP11 && segment.contents().starts_with(COLOR_MARKER_PREFIX)
        {
            let parsed = decode_color_metadata(segment.contents())?;
            if let Some((gamut, transfer)) = parsed {
                return Ok((Some(gamut), Some(transfer)));
            }
        }
    }

    Ok((None, None))
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
