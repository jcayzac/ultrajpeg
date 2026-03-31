## Color, ICC Profiles, And Gamuts

`ultrajpeg` intentionally does not collapse all color semantics into one opaque
type.

Instead, the stable model separates:

- `icc_profile`: the embedded ICC payload, if present
- `gamut`: a convenience named gamut classification
- `gamut_info`: the authoritative structured gamut representation when gamut
  coordinates could be recovered
- `transfer`: the explicitly tracked transfer function

This matters because a JPEG may contain:

- an ICC profile with precise primaries and white point
- explicit crate-tracked gamut and transfer signaling
- both
- or neither

`gamut_info` is the richer result. `gamut` is only the best matching named
classification when one is available.

### Bundled Display-P3 Helper

```rust
# use ultrajpeg::{ColorGamut, ColorMetadata, ColorTransfer, EncodeOptions};
let color = ColorMetadata::display_p3();
assert_eq!(color.icc_profile.as_deref(), Some(ultrajpeg::icc::display_p3()));
assert_eq!(color.gamut, Some(ColorGamut::DisplayP3));
assert_eq!(color.transfer, Some(ColorTransfer::Srgb));

let options = EncodeOptions::ultra_hdr_defaults();
assert_eq!(
    options.primary_metadata.color.icc_profile.as_deref(),
    Some(ultrajpeg::icc::display_p3())
);
# Ok::<(), Box<dyn std::error::Error>>(())
```

When packaging a gain map:

- if `EncodeOptions::primary_metadata.color.icc_profile` is already set, it is
  embedded as-is
- if no ICC profile is set and the resolved primary image is Display-P3 plus
  sRGB, `ultrajpeg` injects the bundled Display-P3 ICC profile automatically
- otherwise gain-map packaging fails with `Error::InvalidInput`

The crate does not synthesize arbitrary ICC profiles.

### Practical Guidance

If you want a spec-friendly Display-P3 primary image for Ultra HDR packaging,
prefer:

- `ColorMetadata::display_p3()`
- `EncodeOptions::ultra_hdr_defaults()`
- `icc::display_p3()`

If your primary image uses a different color space, provide the explicit ICC
profile you want embedded in the primary JPEG instead of relying on the
Display-P3 helper path.
