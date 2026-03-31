use crate::{
    error::{Error, Result},
    types::{ColorMetadata, UltraHdrMetadata},
};
use ultrahdr_core::{
    ColorGamut, ColorTransfer, GainMapMetadata,
    metadata::{
        create_iso_app2_marker, create_xmp_app1_marker, deserialize_iso21496, generate_xmp,
        parse_xmp, serialize_iso21496,
    },
};

pub(crate) const XMP_NAMESPACE: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
pub(crate) const ISO_NAMESPACE: &[u8] = b"urn:iso:std:iso:ts:21496:-1\0";
pub(crate) const COLOR_MARKER_PREFIX: &[u8] = b"urn:ultrajpeg:color:1\0";

pub(crate) fn build_ultra_hdr_metadata(
    metadata: &GainMapMetadata,
    gain_map_length: usize,
) -> UltraHdrMetadata {
    UltraHdrMetadata {
        xmp: Some(generate_xmp(metadata, gain_map_length)),
        iso_21496_1: Some(serialize_iso21496(metadata)),
        gain_map_metadata: Some(metadata.clone()),
    }
}

pub(crate) fn parse_ultra_hdr_metadata(
    xmp: Option<&str>,
    iso: Option<&[u8]>,
) -> Result<Option<UltraHdrMetadata>> {
    let gain_map_from_xmp = xmp
        .filter(|xmp_data| xmp_passes_defensive_checks(xmp_data))
        .and_then(|xmp_data| parse_xmp(xmp_data).ok().map(|pair| pair.0));
    let gain_map_from_iso = iso.and_then(|iso_data| deserialize_iso21496(iso_data).ok());
    let gain_map_metadata = gain_map_from_iso.or(gain_map_from_xmp);

    if xmp.is_none() && iso.is_none() && gain_map_metadata.is_none() {
        return Ok(None);
    }

    Ok(Some(UltraHdrMetadata {
        xmp: xmp.map(ToOwned::to_owned),
        iso_21496_1: iso.map(ToOwned::to_owned),
        gain_map_metadata,
    }))
}

fn xmp_passes_defensive_checks(xmp: &str) -> bool {
    if xmp_contains_base_rendition_is_hdr_true(xmp) {
        return false;
    }

    if !looks_like_ultra_hdr_xmp(xmp) {
        return true;
    }

    let required_fields = ["hdrgm:Version", "hdrgm:GainMapMax", "hdrgm:HDRCapacityMax"];
    required_fields.into_iter().all(|field| xmp.contains(field))
}

fn looks_like_ultra_hdr_xmp(xmp: &str) -> bool {
    xmp.contains("hdrgm:")
        || xmp.contains("Item:Semantic=\"GainMap\"")
        || xmp.contains("Item:Semantic='GainMap'")
}

fn xmp_contains_base_rendition_is_hdr_true(xmp: &str) -> bool {
    [
        "hdrgm:BaseRenditionIsHDR=\"True",
        "hdrgm:BaseRenditionIsHDR='True",
        "hdrgm:BaseRenditionIsHDR=\"true",
        "hdrgm:BaseRenditionIsHDR='true",
    ]
    .into_iter()
    .any(|needle| xmp.contains(needle))
}

pub(crate) fn xmp_segment_payload(xmp: &str) -> Vec<u8> {
    create_xmp_app1_marker(xmp)[4..].to_vec()
}

pub(crate) fn iso_segment_payload(iso_21496_1: &[u8]) -> Vec<u8> {
    create_iso_app2_marker(iso_21496_1)[4..].to_vec()
}

pub(crate) fn encode_color_metadata(color_metadata: &ColorMetadata) -> Option<Vec<u8>> {
    if color_metadata.gamut.is_none() && color_metadata.transfer.is_none() {
        return None;
    }

    let gamut = encode_gamut(color_metadata.gamut.unwrap_or(ColorGamut::Bt709));
    let transfer = encode_transfer(color_metadata.transfer.unwrap_or(ColorTransfer::Srgb));

    Some([COLOR_MARKER_PREFIX, &[1, gamut, transfer]].concat())
}

pub(crate) fn decode_color_metadata(payload: &[u8]) -> Result<Option<(ColorGamut, ColorTransfer)>> {
    if !payload.starts_with(COLOR_MARKER_PREFIX) {
        return Ok(None);
    }

    let fields = &payload[COLOR_MARKER_PREFIX.len()..];
    if fields.len() != 3 {
        return Err(Error::Metadata(
            "invalid explicit color marker payload".into(),
        ));
    }
    if fields[0] != 1 {
        return Err(Error::Metadata(format!(
            "unsupported explicit color marker version {}",
            fields[0]
        )));
    }

    Ok(Some((
        decode_gamut(fields[1])?,
        decode_transfer(fields[2])?,
    )))
}

fn encode_gamut(gamut: ColorGamut) -> u8 {
    match gamut {
        ColorGamut::Bt709 => 0,
        ColorGamut::DisplayP3 => 1,
        ColorGamut::Bt2100 => 2,
    }
}

fn decode_gamut(value: u8) -> Result<ColorGamut> {
    match value {
        0 => Ok(ColorGamut::Bt709),
        1 => Ok(ColorGamut::DisplayP3),
        2 => Ok(ColorGamut::Bt2100),
        _ => Err(Error::Metadata(format!("unknown gamut value {value}"))),
    }
}

fn encode_transfer(transfer: ColorTransfer) -> u8 {
    match transfer {
        ColorTransfer::Srgb => 0,
        ColorTransfer::Linear => 1,
        ColorTransfer::Pq => 2,
        ColorTransfer::Hlg => 3,
    }
}

fn decode_transfer(value: u8) -> Result<ColorTransfer> {
    match value {
        0 => Ok(ColorTransfer::Srgb),
        1 => Ok(ColorTransfer::Linear),
        2 => Ok(ColorTransfer::Pq),
        3 => Ok(ColorTransfer::Hlg),
        _ => Err(Error::Metadata(format!("unknown transfer value {value}"))),
    }
}
