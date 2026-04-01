//! Built-in ICC profiles for common JPEG workflows.
//!
//! The embedded Display-P3 profile is the ICC registry payload distributed by
//! the International Color Consortium for unrestricted embedding and
//! redistribution.
//!
//! The crate also uses ICC profiles internally to recover structured gamut
//! information when explicit crate-specific gamut signaling is absent.

use crate::types::{Chromaticity, GamutInfo};
use ultrahdr_core::ColorGamut;

/// Raw Display-P3 ICC profile bytes.
///
/// This is the ICC registry `DisplayP3.icc` payload shipped with the crate so
/// callers can embed a standards-friendly Display-P3 profile without carrying a
/// separate asset in their application.
///
/// This function only returns raw profile bytes. It does not by itself attach
/// the profile to JPEG output or update any other color metadata fields.
#[must_use]
pub fn display_p3() -> &'static [u8] {
    include_bytes!("../assets/icc/DisplayP3.icc")
}

const TAG_TABLE_OFFSET: usize = 128;
const ICC_XYZ_TYPE: &[u8; 4] = b"XYZ ";
const ICC_SF32_TYPE: &[u8; 4] = b"sf32";
const ICC_CHAD_TAG: &[u8; 4] = b"chad";
const ICC_RXYZ_TAG: &[u8; 4] = b"rXYZ";
const ICC_GXYZ_TAG: &[u8; 4] = b"gXYZ";
const ICC_BXYZ_TAG: &[u8; 4] = b"bXYZ";
const ICC_WTPT_TAG: &[u8; 4] = b"wtpt";
const GAMUT_MATCH_EPSILON: f32 = 0.015;

#[derive(Clone, Copy)]
struct Xyz {
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Clone, Copy)]
struct Matrix3([[f64; 3]; 3]);

pub(crate) fn gamut_info_from_profile(profile: &[u8]) -> Option<GamutInfo> {
    let mut red = parse_xyz_tag(profile, ICC_RXYZ_TAG)?;
    let mut green = parse_xyz_tag(profile, ICC_GXYZ_TAG)?;
    let mut blue = parse_xyz_tag(profile, ICC_BXYZ_TAG)?;
    let mut white = parse_xyz_tag(profile, ICC_WTPT_TAG)?;

    if let Some(chad) = parse_matrix_tag(profile, ICC_CHAD_TAG)
        && let Some(inverse) = chad.inverse()
    {
        red = inverse.apply(red);
        green = inverse.apply(green);
        blue = inverse.apply(blue);
        white = inverse.apply(white);
    }

    let red = xyz_to_xy(red)?;
    let green = xyz_to_xy(green)?;
    let blue = xyz_to_xy(blue)?;
    let white = xyz_to_xy(white)?;

    let mut info = GamutInfo {
        standard: None,
        red,
        green,
        blue,
        white,
    };
    info.standard = classify_gamut(&info);
    Some(info)
}

fn parse_xyz_tag(profile: &[u8], signature: &[u8; 4]) -> Option<Xyz> {
    let payload = find_tag_payload(profile, signature)?;
    if payload.len() < 20 || &payload[..4] != ICC_XYZ_TYPE {
        return None;
    }

    Some(Xyz {
        x: parse_s15fixed16(&payload[8..12])?,
        y: parse_s15fixed16(&payload[12..16])?,
        z: parse_s15fixed16(&payload[16..20])?,
    })
}

fn parse_matrix_tag(profile: &[u8], signature: &[u8; 4]) -> Option<Matrix3> {
    let payload = find_tag_payload(profile, signature)?;
    if payload.len() < 44 || &payload[..4] != ICC_SF32_TYPE {
        return None;
    }

    let mut values = [0.0f64; 9];
    for (index, value) in values.iter_mut().enumerate() {
        let start = 8 + index * 4;
        *value = parse_s15fixed16(&payload[start..start + 4])?;
    }

    Some(Matrix3([
        [values[0], values[1], values[2]],
        [values[3], values[4], values[5]],
        [values[6], values[7], values[8]],
    ]))
}

fn find_tag_payload<'a>(profile: &'a [u8], signature: &[u8; 4]) -> Option<&'a [u8]> {
    if profile.len() < TAG_TABLE_OFFSET + 4 {
        return None;
    }

    let tag_count = u32::from_be_bytes(
        profile[TAG_TABLE_OFFSET..TAG_TABLE_OFFSET + 4]
            .try_into()
            .ok()?,
    ) as usize;
    let table_len = tag_count.checked_mul(12)?;
    let table_end = TAG_TABLE_OFFSET.checked_add(4 + table_len)?;
    if profile.len() < table_end {
        return None;
    }

    for index in 0..tag_count {
        let entry = TAG_TABLE_OFFSET + 4 + index * 12;
        if &profile[entry..entry + 4] != signature {
            continue;
        }

        let offset = u32::from_be_bytes(profile[entry + 4..entry + 8].try_into().ok()?) as usize;
        let len = u32::from_be_bytes(profile[entry + 8..entry + 12].try_into().ok()?) as usize;
        let end = offset.checked_add(len)?;
        if end > profile.len() {
            return None;
        }
        return Some(&profile[offset..end]);
    }

    None
}

fn parse_s15fixed16(bytes: &[u8]) -> Option<f64> {
    let raw = i32::from_be_bytes(bytes.try_into().ok()?);
    Some(f64::from(raw) / 65536.0)
}

fn xyz_to_xy(xyz: Xyz) -> Option<Chromaticity> {
    let sum = xyz.x + xyz.y + xyz.z;
    if !sum.is_finite() || sum.abs() <= f64::EPSILON {
        return None;
    }

    Some(Chromaticity {
        x: (xyz.x / sum) as f32,
        y: (xyz.y / sum) as f32,
    })
}

fn classify_gamut(info: &GamutInfo) -> Option<ColorGamut> {
    for standard in [ColorGamut::Bt709, ColorGamut::DisplayP3, ColorGamut::Bt2100] {
        let reference = GamutInfo::from_standard(standard);
        if chromaticity_matches(info.red, reference.red)
            && chromaticity_matches(info.green, reference.green)
            && chromaticity_matches(info.blue, reference.blue)
            && chromaticity_matches(info.white, reference.white)
        {
            return Some(standard);
        }
    }

    None
}

fn chromaticity_matches(lhs: Chromaticity, rhs: Chromaticity) -> bool {
    (lhs.x - rhs.x).abs() <= GAMUT_MATCH_EPSILON && (lhs.y - rhs.y).abs() <= GAMUT_MATCH_EPSILON
}

impl Matrix3 {
    fn apply(self, value: Xyz) -> Xyz {
        Xyz {
            x: self.0[0][0] * value.x + self.0[0][1] * value.y + self.0[0][2] * value.z,
            y: self.0[1][0] * value.x + self.0[1][1] * value.y + self.0[1][2] * value.z,
            z: self.0[2][0] * value.x + self.0[2][1] * value.y + self.0[2][2] * value.z,
        }
    }

    fn inverse(self) -> Option<Self> {
        let m = self.0;
        let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
        if det.abs() <= f64::EPSILON {
            return None;
        }
        let inv_det = 1.0 / det;

        Some(Self([
            [
                (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv_det,
                (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_det,
                (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_det,
            ],
            [
                (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv_det,
                (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_det,
                (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv_det,
            ],
            [
                (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv_det,
                (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv_det,
                (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv_det,
            ],
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::{display_p3, gamut_info_from_profile};
    use ultrahdr_core::ColorGamut;

    #[test]
    fn display_p3_profile_classifies_correctly() {
        let gamut = gamut_info_from_profile(display_p3()).expect("display-p3 gamut info");
        assert_eq!(gamut.standard, Some(ColorGamut::DisplayP3));
    }

    #[test]
    fn invalid_profile_returns_none() {
        assert!(gamut_info_from_profile(b"not-an-icc-profile").is_none());
    }
}
