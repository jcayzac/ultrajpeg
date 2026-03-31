use crate::{
    error::{Error, Result},
    types::{ColorMetadata, UltraHdrMetadata},
};
use ultrahdr_core::{
    ColorGamut, ColorTransfer, GainMapMetadata,
    metadata::{
        CONTAINER_NAMESPACE, HDRGM_NAMESPACE, ITEM_NAMESPACE, create_iso_app2_marker,
        create_xmp_app1_marker, deserialize_iso21496, parse_xmp, serialize_iso21496,
    },
};

pub(crate) const XMP_NAMESPACE: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
pub(crate) const ISO_NAMESPACE: &[u8] = b"urn:iso:std:iso:ts:21496:-1\0";
pub(crate) const COLOR_MARKER_PREFIX: &[u8] = b"urn:ultrajpeg:color:1\0";

pub(crate) struct EncodedUltraHdrMetadata {
    pub(crate) primary_xmp: String,
    pub(crate) gain_map_xmp: String,
    pub(crate) gain_map_iso_21496_1: Vec<u8>,
}

pub(crate) fn build_ultra_hdr_metadata(
    metadata: &GainMapMetadata,
    gain_map_length: usize,
) -> EncodedUltraHdrMetadata {
    EncodedUltraHdrMetadata {
        primary_xmp: generate_container_xmp(gain_map_length),
        gain_map_xmp: generate_gain_map_xmp(metadata),
        gain_map_iso_21496_1: serialize_iso21496(metadata),
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

fn generate_container_xmp(gain_map_length: usize) -> String {
    format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Adobe XMP Core">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about=""
        xmlns:Container="{CONTAINER_NAMESPACE}"
        xmlns:Item="{ITEM_NAMESPACE}">
      <Container:Directory>
        <rdf:Seq>
          <rdf:li rdf:parseType="Resource">
            <Container:Item
                Item:Semantic="Primary"
                Item:Mime="image/jpeg"/>
          </rdf:li>
          <rdf:li rdf:parseType="Resource">
            <Container:Item
                Item:Semantic="GainMap"
                Item:Mime="image/jpeg"
                Item:Length="{gain_map_length}"/>
          </rdf:li>
        </rdf:Seq>
      </Container:Directory>
    </rdf:Description>
  </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#
    )
}

fn generate_gain_map_xmp(metadata: &GainMapMetadata) -> String {
    let is_single_channel = metadata.is_single_channel();
    let gain_map_min = format_xmp_values(&metadata.min_content_boost, is_single_channel, true);
    let gain_map_max = format_xmp_values(&metadata.max_content_boost, is_single_channel, true);
    let gamma = format_xmp_values(&metadata.gamma, is_single_channel, false);
    let offset_sdr = format_xmp_values(&metadata.offset_sdr, is_single_channel, false);
    let offset_hdr = format_xmp_values(&metadata.offset_hdr, is_single_channel, false);
    let hdr_capacity_min = metadata.hdr_capacity_min.log2();
    let hdr_capacity_max = metadata.hdr_capacity_max.log2();

    format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Adobe XMP Core">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about=""
        xmlns:hdrgm="{HDRGM_NAMESPACE}"
        hdrgm:Version="1.0"
        hdrgm:GainMapMin="{gain_map_min}"
        hdrgm:GainMapMax="{gain_map_max}"
        hdrgm:Gamma="{gamma}"
        hdrgm:OffsetSDR="{offset_sdr}"
        hdrgm:OffsetHDR="{offset_hdr}"
        hdrgm:HDRCapacityMin="{hdr_capacity_min:.6}"
        hdrgm:HDRCapacityMax="{hdr_capacity_max:.6}"
        hdrgm:BaseRenditionIsHDR="False"/>
  </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#
    )
}

fn format_xmp_values(values: &[f32; 3], single_channel: bool, use_log2: bool) -> String {
    if single_channel {
        let value = if use_log2 {
            values[0].log2()
        } else {
            values[0]
        };
        return format!("{value:.6}");
    }

    let converted = if use_log2 {
        values.map(f32::log2)
    } else {
        *values
    };
    format!(
        "{:.6}, {:.6}, {:.6}",
        converted[0], converted[1], converted[2]
    )
}
