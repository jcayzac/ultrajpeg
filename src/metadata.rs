use crate::{
    error::{Error, Result},
    types::{
        ColorMetadata, GainMapMetadataSource, MetadataLocation, ParsedGainMapXmp, UltraHdrMetadata,
        UltraHdrMetadataEmission,
    },
};
use ultrahdr_core::{
    ColorGamut, ColorTransfer, GainMapMetadata,
    metadata::{
        CONTAINER_NAMESPACE, HDRGM_NAMESPACE, ITEM_NAMESPACE, create_iso_app2_marker,
        create_xmp_app1_marker, parse_xmp,
    },
};

pub(crate) const XMP_NAMESPACE: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
pub(crate) const ISO_NAMESPACE: &[u8] = b"urn:iso:std:iso:ts:21496:-1\0";
pub(crate) const COLOR_MARKER_PREFIX: &[u8] = b"urn:ultrajpeg:color:1\0";
const ISO_MIN_VERSION: u16 = 0;
const ISO_WRITER_VERSION: u16 = 0;
const ISO_FLAG_MULTI_CHANNEL: u8 = 0x80;
const ISO_FLAG_USE_BASE_COLOR_SPACE: u8 = 0x40;
const ISO_FLAG_BACKWARD_DIRECTION: u8 = 0x04;
const ISO_FLAG_COMMON_DENOMINATOR: u8 = 0x08;
const LEGACY_ISO_VERSION: u8 = 0;
const LEGACY_ISO_FLAG_MULTI_CHANNEL: u8 = 0x01;
const LEGACY_ISO_FLAG_USE_BASE_COLOR_SPACE: u8 = 0x02;
const LEGACY_ISO_FLAG_BACKWARD_DIRECTION: u8 = 0x04;
const ISO_PREFERRED_DENOMINATOR_SHIFT: u32 = 28;

pub(crate) struct EncodedUltraHdrMetadata {
    pub(crate) emit_primary_container_xmp: bool,
    pub(crate) primary_iso_21496_1: Option<Vec<u8>>,
    pub(crate) gain_map_xmp: Option<String>,
    pub(crate) gain_map_iso_21496_1: Option<Vec<u8>>,
}

pub(crate) fn build_ultra_hdr_metadata(
    metadata: &GainMapMetadata,
    emission: UltraHdrMetadataEmission,
) -> Result<EncodedUltraHdrMetadata> {
    Ok(EncodedUltraHdrMetadata {
        emit_primary_container_xmp: emission.emit_primary_container_xmp,
        primary_iso_21496_1: emission
            .emit_iso_21496_1
            .then(serialize_primary_iso_21496_1),
        gain_map_xmp: emission
            .emit_gain_map_xmp
            .then(|| generate_gain_map_xmp(metadata)),
        gain_map_iso_21496_1: if emission.emit_iso_21496_1 {
            Some(serialize_gain_map_iso_21496_1(metadata)?)
        } else {
            None
        },
    })
}

pub(crate) fn parse_ultra_hdr_metadata(
    xmp: Option<&str>,
    xmp_location: Option<MetadataLocation>,
    iso: Option<&[u8]>,
    iso_location: Option<MetadataLocation>,
) -> Result<Option<UltraHdrMetadata>> {
    let gain_map_from_xmp = xmp
        .filter(|xmp_data| xmp_passes_defensive_checks(xmp_data))
        .and_then(|xmp_data| parse_xmp(xmp_data).ok().map(|pair| pair.0));
    let gain_map_from_iso =
        iso.and_then(|iso_data| parse_iso_21496_1_internal(iso_data, true).ok().flatten());
    let (gain_map_metadata, gain_map_metadata_source) = if let Some(metadata) = gain_map_from_iso {
        (Some(metadata), Some(GainMapMetadataSource::Iso21496_1))
    } else if let Some(metadata) = gain_map_from_xmp {
        (Some(metadata), Some(GainMapMetadataSource::Xmp))
    } else {
        (None, None)
    };

    if xmp.is_none() && iso.is_none() && gain_map_metadata.is_none() {
        return Ok(None);
    }

    Ok(Some(UltraHdrMetadata {
        xmp: xmp.map(ToOwned::to_owned),
        xmp_location,
        iso_21496_1: iso.map(ToOwned::to_owned),
        iso_21496_1_location: iso_location,
        gain_map_metadata,
        gain_map_metadata_source,
    }))
}

pub(crate) fn parse_gain_map_xmp_raw(xmp: &str) -> Result<ParsedGainMapXmp> {
    let (metadata, gain_map_jpeg_len) = parse_xmp(xmp)?;
    Ok(ParsedGainMapXmp {
        metadata,
        gain_map_jpeg_len,
    })
}

pub(crate) fn parse_iso_21496_1_raw(iso_21496_1: &[u8]) -> Result<GainMapMetadata> {
    parse_iso_21496_1_internal(iso_21496_1, false)?.ok_or_else(|| {
        Error::Metadata("ISO 21496-1 version-only payload does not carry gain-map metadata".into())
    })
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

fn serialize_primary_iso_21496_1() -> Vec<u8> {
    let mut data = Vec::with_capacity(4);
    write_u16(&mut data, ISO_MIN_VERSION);
    write_u16(&mut data, ISO_WRITER_VERSION);
    data
}

fn serialize_gain_map_iso_21496_1(metadata: &GainMapMetadata) -> Result<Vec<u8>> {
    let channel_count = if metadata_has_identical_channels(metadata) {
        1
    } else {
        3
    };
    let mut data = Vec::with_capacity(5 + 16 + channel_count * 40);
    write_u16(&mut data, ISO_MIN_VERSION);
    write_u16(&mut data, ISO_WRITER_VERSION);

    let mut flags = 0u8;
    if channel_count == 3 {
        flags |= ISO_FLAG_MULTI_CHANNEL;
    }
    if metadata.use_base_color_space {
        flags |= ISO_FLAG_USE_BASE_COLOR_SPACE;
    }
    data.push(flags);

    write_unsigned_fraction(
        &mut data,
        metadata.hdr_capacity_min.log2(),
        "hdr_capacity_min",
    )?;
    write_unsigned_fraction(
        &mut data,
        metadata.hdr_capacity_max.log2(),
        "hdr_capacity_max",
    )?;

    for index in 0..channel_count {
        write_signed_fraction(
            &mut data,
            metadata.min_content_boost[index].log2(),
            "min_content_boost",
        )?;
        write_signed_fraction(
            &mut data,
            metadata.max_content_boost[index].log2(),
            "max_content_boost",
        )?;
        write_unsigned_fraction(&mut data, metadata.gamma[index], "gamma")?;
        write_signed_fraction(&mut data, metadata.offset_sdr[index], "offset_sdr")?;
        write_signed_fraction(&mut data, metadata.offset_hdr[index], "offset_hdr")?;
    }

    Ok(data)
}

fn parse_iso_21496_1_internal(
    iso_21496_1: &[u8],
    allow_version_only: bool,
) -> Result<Option<GainMapMetadata>> {
    if is_version_only_iso_21496_1(iso_21496_1) {
        return if allow_version_only {
            Ok(None)
        } else {
            Err(Error::Metadata(
                "ISO 21496-1 version-only payload does not carry gain-map metadata".into(),
            ))
        };
    }

    if looks_like_canonical_iso_21496_1(iso_21496_1) {
        return parse_iso_21496_1_canonical(iso_21496_1).map(Some);
    }

    parse_iso_21496_1_legacy(iso_21496_1).map(Some)
}

fn is_version_only_iso_21496_1(data: &[u8]) -> bool {
    data.len() == 4 && read_u16(data, 0).ok() == Some(ISO_MIN_VERSION)
}

fn looks_like_canonical_iso_21496_1(data: &[u8]) -> bool {
    if data.len() < 5 || read_u16(data, 0).ok() != Some(ISO_MIN_VERSION) {
        return false;
    }

    let Some(flags) = data.get(4).copied() else {
        return false;
    };
    let channel_count = if flags & ISO_FLAG_MULTI_CHANNEL != 0 {
        3
    } else {
        1
    };
    let explicit_len = 5 + 16 + channel_count * 40;
    let common_denominator_len = 5 + 12 + channel_count * 20;

    data.len() == explicit_len || data.len() == common_denominator_len
}

fn parse_iso_21496_1_canonical(data: &[u8]) -> Result<GainMapMetadata> {
    if data.len() < 5 {
        return Err(Error::Metadata("ISO 21496-1 payload is too short".into()));
    }

    let min_version = read_u16(data, 0)?;
    if min_version != ISO_MIN_VERSION {
        return Err(Error::Metadata(format!(
            "unsupported ISO 21496-1 minimum version {min_version}"
        )));
    }

    let _writer_version = read_u16(data, 2)?;
    let flags = read_u8(data, 4)?;
    let channel_count = if flags & ISO_FLAG_MULTI_CHANNEL != 0 {
        3
    } else {
        1
    };
    let use_base_color_space = flags & ISO_FLAG_USE_BASE_COLOR_SPACE != 0;
    let backward_direction = flags & ISO_FLAG_BACKWARD_DIRECTION != 0;
    let use_common_denominator = flags & ISO_FLAG_COMMON_DENOMINATOR != 0;

    let mut pos = 5;
    let mut fractions = Iso21496Fractions {
        use_base_color_space,
        backward_direction,
        ..Default::default()
    };

    if use_common_denominator {
        let common_denominator = read_u32(data, pos)?;
        pos += 4;

        fractions.base_hdr_headroom_numerator = read_u32(data, pos)?;
        fractions.base_hdr_headroom_denominator = common_denominator;
        pos += 4;
        fractions.alternate_hdr_headroom_numerator = read_u32(data, pos)?;
        fractions.alternate_hdr_headroom_denominator = common_denominator;
        pos += 4;

        for index in 0..channel_count {
            fractions.gain_map_min_numerator[index] = read_i32(data, pos)?;
            fractions.gain_map_min_denominator[index] = common_denominator;
            pos += 4;

            fractions.gain_map_max_numerator[index] = read_i32(data, pos)?;
            fractions.gain_map_max_denominator[index] = common_denominator;
            pos += 4;

            fractions.gamma_numerator[index] = read_u32(data, pos)?;
            fractions.gamma_denominator[index] = common_denominator;
            pos += 4;

            fractions.base_offset_numerator[index] = read_i32(data, pos)?;
            fractions.base_offset_denominator[index] = common_denominator;
            pos += 4;

            fractions.alternate_offset_numerator[index] = read_i32(data, pos)?;
            fractions.alternate_offset_denominator[index] = common_denominator;
            pos += 4;
        }
    } else {
        fractions.base_hdr_headroom_numerator = read_u32(data, pos)?;
        pos += 4;
        fractions.base_hdr_headroom_denominator = read_u32(data, pos)?;
        pos += 4;
        fractions.alternate_hdr_headroom_numerator = read_u32(data, pos)?;
        pos += 4;
        fractions.alternate_hdr_headroom_denominator = read_u32(data, pos)?;
        pos += 4;

        for index in 0..channel_count {
            fractions.gain_map_min_numerator[index] = read_i32(data, pos)?;
            pos += 4;
            fractions.gain_map_min_denominator[index] = read_u32(data, pos)?;
            pos += 4;

            fractions.gain_map_max_numerator[index] = read_i32(data, pos)?;
            pos += 4;
            fractions.gain_map_max_denominator[index] = read_u32(data, pos)?;
            pos += 4;

            fractions.gamma_numerator[index] = read_u32(data, pos)?;
            pos += 4;
            fractions.gamma_denominator[index] = read_u32(data, pos)?;
            pos += 4;

            fractions.base_offset_numerator[index] = read_i32(data, pos)?;
            pos += 4;
            fractions.base_offset_denominator[index] = read_u32(data, pos)?;
            pos += 4;

            fractions.alternate_offset_numerator[index] = read_i32(data, pos)?;
            pos += 4;
            fractions.alternate_offset_denominator[index] = read_u32(data, pos)?;
            pos += 4;
        }
    }

    if pos != data.len() {
        return Err(Error::Metadata(
            "unexpected trailing bytes in ISO 21496-1 payload".into(),
        ));
    }

    fractions.expand_single_channel(channel_count);
    fractions.into_gain_map_metadata()
}

fn parse_iso_21496_1_legacy(data: &[u8]) -> Result<GainMapMetadata> {
    if data.len() < 2 {
        return Err(Error::Metadata(
            "legacy ISO 21496-1 payload is too short".into(),
        ));
    }

    let version = data[0];
    if version != LEGACY_ISO_VERSION {
        return Err(Error::Metadata(format!(
            "unsupported legacy ISO 21496-1 version {version}"
        )));
    }

    let flags = data[1];
    let channel_count = if flags & LEGACY_ISO_FLAG_MULTI_CHANNEL != 0 {
        3
    } else {
        1
    };
    let mut pos = 2;
    let mut fractions = Iso21496Fractions {
        use_base_color_space: flags & LEGACY_ISO_FLAG_USE_BASE_COLOR_SPACE != 0,
        backward_direction: flags & LEGACY_ISO_FLAG_BACKWARD_DIRECTION != 0,
        ..Default::default()
    };

    fractions.base_hdr_headroom_numerator = read_u32(data, pos)?;
    pos += 4;
    fractions.base_hdr_headroom_denominator = read_u32(data, pos)?;
    pos += 4;
    fractions.alternate_hdr_headroom_numerator = read_u32(data, pos)?;
    pos += 4;
    fractions.alternate_hdr_headroom_denominator = read_u32(data, pos)?;
    pos += 4;

    for index in 0..channel_count {
        fractions.gain_map_min_numerator[index] = read_i32(data, pos)?;
        pos += 4;
        fractions.gain_map_min_denominator[index] = read_u32(data, pos)?;
        pos += 4;

        fractions.gain_map_max_numerator[index] = read_i32(data, pos)?;
        pos += 4;
        fractions.gain_map_max_denominator[index] = read_u32(data, pos)?;
        pos += 4;

        fractions.gamma_numerator[index] = read_u32(data, pos)?;
        pos += 4;
        fractions.gamma_denominator[index] = read_u32(data, pos)?;
        pos += 4;

        fractions.base_offset_numerator[index] = read_i32(data, pos)?;
        pos += 4;
        fractions.base_offset_denominator[index] = read_u32(data, pos)?;
        pos += 4;

        fractions.alternate_offset_numerator[index] = read_i32(data, pos)?;
        pos += 4;
        fractions.alternate_offset_denominator[index] = read_u32(data, pos)?;
        pos += 4;
    }

    if pos != data.len() {
        return Err(Error::Metadata(
            "unexpected trailing bytes in legacy ISO 21496-1 payload".into(),
        ));
    }

    fractions.expand_single_channel(channel_count);
    fractions.into_gain_map_metadata()
}

#[derive(Default)]
struct Iso21496Fractions {
    gain_map_min_numerator: [i32; 3],
    gain_map_min_denominator: [u32; 3],
    gain_map_max_numerator: [i32; 3],
    gain_map_max_denominator: [u32; 3],
    gamma_numerator: [u32; 3],
    gamma_denominator: [u32; 3],
    base_offset_numerator: [i32; 3],
    base_offset_denominator: [u32; 3],
    alternate_offset_numerator: [i32; 3],
    alternate_offset_denominator: [u32; 3],
    base_hdr_headroom_numerator: u32,
    base_hdr_headroom_denominator: u32,
    alternate_hdr_headroom_numerator: u32,
    alternate_hdr_headroom_denominator: u32,
    backward_direction: bool,
    use_base_color_space: bool,
}

impl Iso21496Fractions {
    fn expand_single_channel(&mut self, channel_count: usize) {
        if channel_count == 3 {
            return;
        }

        for index in 1..3 {
            self.gain_map_min_numerator[index] = self.gain_map_min_numerator[0];
            self.gain_map_min_denominator[index] = self.gain_map_min_denominator[0];
            self.gain_map_max_numerator[index] = self.gain_map_max_numerator[0];
            self.gain_map_max_denominator[index] = self.gain_map_max_denominator[0];
            self.gamma_numerator[index] = self.gamma_numerator[0];
            self.gamma_denominator[index] = self.gamma_denominator[0];
            self.base_offset_numerator[index] = self.base_offset_numerator[0];
            self.base_offset_denominator[index] = self.base_offset_denominator[0];
            self.alternate_offset_numerator[index] = self.alternate_offset_numerator[0];
            self.alternate_offset_denominator[index] = self.alternate_offset_denominator[0];
        }
    }

    fn into_gain_map_metadata(self) -> Result<GainMapMetadata> {
        if self.backward_direction {
            return Err(Error::Metadata(
                "backward-direction ISO 21496-1 payloads are not supported".into(),
            ));
        }

        let mut metadata = GainMapMetadata {
            hdr_capacity_min: exp2_unsigned_fraction(
                self.base_hdr_headroom_numerator,
                self.base_hdr_headroom_denominator,
                "hdr_capacity_min",
            )?,
            hdr_capacity_max: exp2_unsigned_fraction(
                self.alternate_hdr_headroom_numerator,
                self.alternate_hdr_headroom_denominator,
                "hdr_capacity_max",
            )?,
            use_base_color_space: self.use_base_color_space,
            ..Default::default()
        };

        for index in 0..3 {
            metadata.min_content_boost[index] = exp2_signed_fraction(
                self.gain_map_min_numerator[index],
                self.gain_map_min_denominator[index],
                "min_content_boost",
            )?;
            metadata.max_content_boost[index] = exp2_signed_fraction(
                self.gain_map_max_numerator[index],
                self.gain_map_max_denominator[index],
                "max_content_boost",
            )?;
            metadata.gamma[index] = decode_unsigned_fraction(
                self.gamma_numerator[index],
                self.gamma_denominator[index],
                "gamma",
            )?;
            metadata.offset_sdr[index] = decode_signed_fraction(
                self.base_offset_numerator[index],
                self.base_offset_denominator[index],
                "offset_sdr",
            )?;
            metadata.offset_hdr[index] = decode_signed_fraction(
                self.alternate_offset_numerator[index],
                self.alternate_offset_denominator[index],
                "offset_hdr",
            )?;
        }

        Ok(metadata)
    }
}

fn metadata_has_identical_channels(metadata: &GainMapMetadata) -> bool {
    channels_identical(&metadata.min_content_boost)
        && channels_identical(&metadata.max_content_boost)
        && channels_identical(&metadata.gamma)
        && channels_identical(&metadata.offset_sdr)
        && channels_identical(&metadata.offset_hdr)
}

fn channels_identical(values: &[f32; 3]) -> bool {
    values[0] == values[1] && values[1] == values[2]
}

fn write_u16(buf: &mut Vec<u8>, value: u16) {
    buf.extend_from_slice(&value.to_be_bytes());
}

fn write_u32(buf: &mut Vec<u8>, value: u32) {
    buf.extend_from_slice(&value.to_be_bytes());
}

fn write_i32(buf: &mut Vec<u8>, value: i32) {
    buf.extend_from_slice(&value.to_be_bytes());
}

fn write_unsigned_fraction(buf: &mut Vec<u8>, value: f32, field: &str) -> Result<()> {
    let (numerator, denominator) = encode_unsigned_fraction(value, field)?;
    write_u32(buf, numerator);
    write_u32(buf, denominator);
    Ok(())
}

fn write_signed_fraction(buf: &mut Vec<u8>, value: f32, field: &str) -> Result<()> {
    let (numerator, denominator) = encode_signed_fraction(value, field)?;
    write_i32(buf, numerator);
    write_u32(buf, denominator);
    Ok(())
}

fn encode_unsigned_fraction(value: f32, field: &str) -> Result<(u32, u32)> {
    if !value.is_finite() || value < 0.0 {
        return Err(Error::Metadata(format!(
            "{field} cannot be represented as an unsigned ISO 21496-1 fraction"
        )));
    }

    for shift in (0..=ISO_PREFERRED_DENOMINATOR_SHIFT).rev() {
        let denominator = 1_u32 << shift;
        let scaled = value as f64 * denominator as f64;
        if scaled.is_finite() && scaled <= u32::MAX as f64 + 0.5 {
            return Ok((scaled.round() as u32, denominator));
        }
    }

    Err(Error::Metadata(format!(
        "{field} is out of range for ISO 21496-1 unsigned fraction encoding"
    )))
}

fn encode_signed_fraction(value: f32, field: &str) -> Result<(i32, u32)> {
    if !value.is_finite() {
        return Err(Error::Metadata(format!(
            "{field} cannot be represented as a finite ISO 21496-1 fraction"
        )));
    }

    for shift in (0..=ISO_PREFERRED_DENOMINATOR_SHIFT).rev() {
        let denominator = 1_u32 << shift;
        let scaled = value as f64 * denominator as f64;
        if scaled.is_finite() && scaled >= i32::MIN as f64 - 0.5 && scaled <= i32::MAX as f64 + 0.5
        {
            return Ok((scaled.round() as i32, denominator));
        }
    }

    Err(Error::Metadata(format!(
        "{field} is out of range for ISO 21496-1 signed fraction encoding"
    )))
}

fn decode_unsigned_fraction(numerator: u32, denominator: u32, field: &str) -> Result<f32> {
    if denominator == 0 {
        return Err(Error::Metadata(format!(
            "{field} uses a zero denominator in ISO 21496-1 metadata"
        )));
    }

    Ok(numerator as f32 / denominator as f32)
}

fn decode_signed_fraction(numerator: i32, denominator: u32, field: &str) -> Result<f32> {
    if denominator == 0 {
        return Err(Error::Metadata(format!(
            "{field} uses a zero denominator in ISO 21496-1 metadata"
        )));
    }

    Ok(numerator as f32 / denominator as f32)
}

fn exp2_unsigned_fraction(numerator: u32, denominator: u32, field: &str) -> Result<f32> {
    let value = decode_unsigned_fraction(numerator, denominator, field)?;
    let decoded = 2.0f32.powf(value);
    if decoded.is_finite() {
        Ok(decoded)
    } else {
        Err(Error::Metadata(format!(
            "{field} overflows when decoding ISO 21496-1 metadata"
        )))
    }
}

fn exp2_signed_fraction(numerator: i32, denominator: u32, field: &str) -> Result<f32> {
    let value = decode_signed_fraction(numerator, denominator, field)?;
    let decoded = 2.0f32.powf(value);
    if decoded.is_finite() {
        Ok(decoded)
    } else {
        Err(Error::Metadata(format!(
            "{field} overflows when decoding ISO 21496-1 metadata"
        )))
    }
}

fn read_u8(data: &[u8], offset: usize) -> Result<u8> {
    data.get(offset)
        .copied()
        .ok_or_else(|| Error::Metadata("truncated ISO 21496-1 payload".into()))
}

fn read_u16(data: &[u8], offset: usize) -> Result<u16> {
    let bytes = data
        .get(offset..offset + 2)
        .ok_or_else(|| Error::Metadata("truncated ISO 21496-1 payload".into()))?;
    Ok(u16::from_be_bytes(
        bytes
            .try_into()
            .expect("two-byte ISO 21496-1 field has fixed width"),
    ))
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or_else(|| Error::Metadata("truncated ISO 21496-1 payload".into()))?;
    Ok(u32::from_be_bytes(
        bytes
            .try_into()
            .expect("four-byte ISO 21496-1 field has fixed width"),
    ))
}

fn read_i32(data: &[u8], offset: usize) -> Result<i32> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or_else(|| Error::Metadata("truncated ISO 21496-1 payload".into()))?;
    Ok(i32::from_be_bytes(
        bytes
            .try_into()
            .expect("four-byte ISO 21496-1 field has fixed width"),
    ))
}

pub(crate) fn encode_color_metadata(color_metadata: &ColorMetadata) -> Option<Vec<u8>> {
    let gamut = color_metadata.gamut.or_else(|| {
        color_metadata
            .gamut_info
            .as_ref()
            .and_then(|info| info.standard)
    });
    if gamut.is_none() && color_metadata.transfer.is_none() {
        return None;
    }

    let gamut = encode_gamut(gamut.unwrap_or(ColorGamut::Bt709));
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
        xmlns:Item="{ITEM_NAMESPACE}"
        xmlns:hdrgm="{HDRGM_NAMESPACE}"
        hdrgm:Version="1.0">
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

pub(crate) fn container_xmp_for_gain_map_length(gain_map_length: usize) -> String {
    generate_container_xmp(gain_map_length)
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

#[cfg(test)]
mod tests {
    use super::{
        ISO_FLAG_USE_BASE_COLOR_SPACE, parse_iso_21496_1_canonical, parse_iso_21496_1_raw,
        serialize_gain_map_iso_21496_1,
    };
    use ultrahdr_core::{
        GainMapMetadata, metadata::serialize_iso21496 as serialize_legacy_iso21496,
    };

    #[test]
    fn canonical_iso_serializer_matches_issue_6_fixture() {
        let metadata = GainMapMetadata {
            min_content_boost: [1.0024664; 3],
            max_content_boost: [5.5792727; 3],
            gamma: [1.0; 3],
            offset_sdr: [1.0 / 64.0; 3],
            offset_hdr: [1.0 / 64.0; 3],
            hdr_capacity_min: 1.0,
            hdr_capacity_max: 6.0000024,
            use_base_color_space: true,
        };

        let serialized = serialize_gain_map_iso_21496_1(&metadata).unwrap();
        assert_eq!(
            serialized,
            vec![
                0x00,
                0x00,
                0x00,
                0x00,
                ISO_FLAG_USE_BASE_COLOR_SPACE,
                0x00,
                0x00,
                0x00,
                0x00,
                0x10,
                0x00,
                0x00,
                0x00,
                0x29,
                0x5c,
                0x02,
                0x40,
                0x10,
                0x00,
                0x00,
                0x00,
                0x00,
                0x0e,
                0x8e,
                0x94,
                0x10,
                0x00,
                0x00,
                0x00,
                0x27,
                0xae,
                0x65,
                0x40,
                0x10,
                0x00,
                0x00,
                0x00,
                0x10,
                0x00,
                0x00,
                0x00,
                0x10,
                0x00,
                0x00,
                0x00,
                0x00,
                0x40,
                0x00,
                0x00,
                0x10,
                0x00,
                0x00,
                0x00,
                0x00,
                0x40,
                0x00,
                0x00,
                0x10,
                0x00,
                0x00,
                0x00,
            ]
        );
    }

    #[test]
    fn canonical_iso_parser_matches_issue_6_fixture() {
        let payload = [
            0x00,
            0x00,
            0x00,
            0x00,
            ISO_FLAG_USE_BASE_COLOR_SPACE,
            0x00,
            0x00,
            0x00,
            0x00,
            0x10,
            0x00,
            0x00,
            0x00,
            0x29,
            0x5c,
            0x02,
            0x40,
            0x10,
            0x00,
            0x00,
            0x00,
            0x00,
            0x0e,
            0x8e,
            0x94,
            0x10,
            0x00,
            0x00,
            0x00,
            0x27,
            0xae,
            0x65,
            0x40,
            0x10,
            0x00,
            0x00,
            0x00,
            0x10,
            0x00,
            0x00,
            0x00,
            0x10,
            0x00,
            0x00,
            0x00,
            0x00,
            0x40,
            0x00,
            0x00,
            0x10,
            0x00,
            0x00,
            0x00,
            0x00,
            0x40,
            0x00,
            0x00,
            0x10,
            0x00,
            0x00,
            0x00,
        ];

        let parsed = parse_iso_21496_1_canonical(&payload).unwrap();
        assert!((parsed.min_content_boost[0] - 1.0024664).abs() < 0.00001);
        assert!((parsed.max_content_boost[0] - 5.5792727).abs() < 0.00001);
        assert!((parsed.gamma[0] - 1.0).abs() < f32::EPSILON);
        assert!((parsed.offset_sdr[0] - (1.0 / 64.0)).abs() < f32::EPSILON);
        assert!((parsed.offset_hdr[0] - (1.0 / 64.0)).abs() < f32::EPSILON);
        assert!((parsed.hdr_capacity_min - 1.0).abs() < f32::EPSILON);
        assert!((parsed.hdr_capacity_max - 6.0000024).abs() < 0.0001);
        assert!(parsed.use_base_color_space);
    }

    #[test]
    fn raw_iso_parser_accepts_legacy_payloads_for_compatibility() {
        let metadata = GainMapMetadata {
            min_content_boost: [1.0; 3],
            max_content_boost: [4.0; 3],
            gamma: [1.0; 3],
            offset_sdr: [1.0 / 64.0; 3],
            offset_hdr: [1.0 / 64.0; 3],
            hdr_capacity_min: 1.0,
            hdr_capacity_max: 4.0,
            use_base_color_space: true,
        };

        let parsed = parse_iso_21496_1_raw(&serialize_legacy_iso21496(&metadata)).unwrap();
        assert!((parsed.max_content_boost[0] - 4.0).abs() < 0.01);
        assert!((parsed.hdr_capacity_max - 4.0).abs() < 0.01);
        assert!(parsed.use_base_color_space);
    }

    #[test]
    fn raw_iso_parser_rejects_primary_version_only_payloads() {
        let error = parse_iso_21496_1_raw(&[0x00, 0x00, 0x00, 0x00]).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("version-only payload does not carry gain-map metadata")
        );
    }
}
