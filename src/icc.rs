//! Built-in ICC profiles for common JPEG workflows.
//!
//! The embedded Display-P3 profile is the ICC registry payload distributed by
//! the International Color Consortium for unrestricted embedding and
//! redistribution.

/// Raw Display-P3 ICC profile bytes.
///
/// This is the ICC registry `DisplayP3.icc` payload shipped with the crate so
/// callers can embed a standards-friendly Display-P3 profile without carrying a
/// separate asset in their application.
#[must_use]
pub fn display_p3() -> &'static [u8] {
    include_bytes!("../assets/icc/DisplayP3.icc")
}
