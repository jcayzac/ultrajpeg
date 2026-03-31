# Migration Guide: `0.4.0-rc6` to `0.5.0-rc1`

This guide covers migration from the latest pre-refactor API in
`0.4.0-rc6` to the native stable-API direction implemented in
`0.5.0-rc1`.

## Summary

`0.5.0-rc1` removes the wrapper-era public API and keeps one coherent native
surface at the crate root.

The biggest changes are:

- the compatibility API is no longer public
- the main image type is now `ultrajpeg::Image`
- `DecodedJpeg` became `DecodedImage`
- `InspectedJpeg` became `Inspection`
- `GainMapEncodeOptions` became `GainMapBundle`
- `UltraJpegEncoder` became `Encoder`
- `EncodeOptions::color_metadata` became `EncodeOptions::primary_metadata`
- EXIF moved out of `ColorMetadata` into `PrimaryMetadata`
- `decode(...)` no longer retains raw JPEG codestream bytes by default
- `ComputedGainMap::into_encode_options(...)` became `into_bundle(...)`

## Import Mapping

Old:

```text
use ultrajpeg::{
    ColorMetadata, DecodeOptions, EncodeOptions, GainMapEncodeOptions,
    UltraJpegEncoder, decode, inspect,
};
```

New:

```text
use ultrajpeg::{
    ColorMetadata, DecodeOptions, EncodeOptions, Encoder, GainMapBundle,
    PrimaryMetadata, decode, inspect,
};
```

## Type Renames

Direct renames:

- `CoreRawImage` -> `Image`
- `DecodedJpeg` -> `DecodedImage`
- `InspectedJpeg` -> `Inspection`
- `GainMapEncodeOptions` -> `GainMapBundle`
- `UltraJpegEncoder` -> `Encoder`

Method rename:

- `ComputedGainMap::into_encode_options(...)` -> `ComputedGainMap::into_bundle(...)`

## Metadata Model Changes

### `ColorMetadata`

Old `ColorMetadata` bundled together:

- ICC profile
- EXIF payload
- gamut
- transfer

New `ColorMetadata` contains only color-related state:

- `icc_profile`
- `gamut`
- `gamut_info`
- `transfer`

EXIF moved to `PrimaryMetadata`.

### `PrimaryMetadata`

New:

```text
pub struct PrimaryMetadata {
    pub color: ColorMetadata,
    pub exif: Option<Vec<u8>>,
}
```

If you previously wrote:

```text
let options = EncodeOptions {
    color_metadata: ColorMetadata {
        icc_profile: Some(profile),
        exif: Some(exif),
        gamut: Some(ColorGamut::DisplayP3),
        transfer: Some(ColorTransfer::Srgb),
    },
    ..EncodeOptions::default()
};
```

You now write:

```text
let options = EncodeOptions {
    primary_metadata: PrimaryMetadata {
        color: ColorMetadata {
            icc_profile: Some(profile),
            gamut: Some(ColorGamut::DisplayP3),
            gamut_info: None,
            transfer: Some(ColorTransfer::Srgb),
        },
        exif: Some(exif),
    },
    ..EncodeOptions::default()
};
```

## Encode Migration

### Old

```text
let bytes = UltraJpegEncoder::new(options).encode(&primary)?;
```

### New

Either:

```text
let bytes = Encoder::new(options).encode(&primary)?;
```

Or, when you do not need a reusable encoder instance:

```text
let bytes = ultrajpeg::encode(&primary, &options)?;
```

### Gain-map payload

Old:

```text
gain_map: Some(GainMapEncodeOptions {
    image,
    metadata,
    quality,
    progressive,
})
```

New:

```text
gain_map: Some(GainMapBundle {
    image,
    metadata,
    quality,
    progressive,
})
```

### `compute_gain_map(...)`

Old:

```text
let computed = ultrajpeg::compute_gain_map(&hdr, &primary, &Default::default())?;
let options = EncodeOptions {
    gain_map: Some(computed.into_encode_options(90, false)),
    ..EncodeOptions::ultra_hdr_defaults()
};
```

New:

```text
let computed = ultrajpeg::compute_gain_map(&hdr, &primary, &Default::default())?;
let options = EncodeOptions {
    gain_map: Some(computed.into_bundle(90, false)),
    ..EncodeOptions::ultra_hdr_defaults()
};
```

## Decode Migration

### Result type and fields

Old:

```text
let decoded = ultrajpeg::decode(bytes)?;
let image = decoded.primary_image;
let icc = decoded.color_metadata.icc_profile;
let exif = decoded.color_metadata.exif;
```

New:

```text
let decoded = ultrajpeg::decode(bytes)?;
let image = decoded.image;
let icc = decoded.primary_metadata.color.icc_profile;
let exif = decoded.primary_metadata.exif;
```

Field mapping:

- `decoded.primary_image` -> `decoded.image`
- `decoded.color_metadata` -> `decoded.primary_metadata.color`
- `decoded.color_metadata.exif` -> `decoded.primary_metadata.exif`

### Default codestream retention changed

In `0.4.0-rc6`, `decode(...)` retained the raw primary JPEG and gain-map JPEG
codestreams by default.

In `0.5.0-rc1`, `decode(...)` retains neither by default.

If you previously relied on:

- `decoded.primary_jpeg`
- `decoded.gain_map.as_ref().unwrap().jpeg_bytes`

you must opt in explicitly:

```text
let decoded = ultrajpeg::decode_with_options(
    bytes,
    ultrajpeg::DecodeOptions {
        retain_primary_jpeg: true,
        retain_gain_map_jpeg: true,
        ..Default::default()
    },
)?;
```

Also note the field types changed:

- old `primary_jpeg: Vec<u8>`
- new `primary_jpeg: Option<Vec<u8>>`
- old `jpeg_bytes: Vec<u8>`
- new `jpeg_bytes: Option<Vec<u8>>`

## Inspect Migration

Old:

```text
let inspected = ultrajpeg::inspect(bytes)?;
let icc = inspected.color_metadata.icc_profile;
```

New:

```text
let inspected = ultrajpeg::inspect(bytes)?;
let icc = inspected.primary_metadata.color.icc_profile;
```

Field mapping:

- `inspected.color_metadata` -> `inspected.primary_metadata.color`

## New Metadata Provenance

`UltraHdrMetadata` now exposes provenance:

- `xmp_location`
- `iso_21496_1_location`
- `gain_map_metadata_source`

If your previous code only consumed `gain_map_metadata`, it can keep doing so.

If you need to know whether effective metadata came from the primary JPEG or
the gain-map JPEG, or whether parsed effective gain-map metadata came from ISO
21496-1 or XMP, you can now inspect those fields directly.

## `ColorMetadata::gamut_info`

In `0.4.0-rc6`, `ColorMetadata` exposed a `gamut_info()` helper method.

In `0.5.0-rc1`, `gamut_info` is a field:

Old:

```text
let standard = decoded
    .color_metadata
    .gamut_info()
    .as_ref()
    .and_then(|info| info.standard);
```

New:

```text
let standard = decoded
    .primary_metadata
    .color
    .gamut_info
    .as_ref()
    .and_then(|info| info.standard);
```

The semantics are the same: `gamut_info` is the richer, authoritative gamut
representation, and `gamut` remains the convenience named classification.

## Display-P3 Helpers

The helpers still exist, but their placement changed through the
`PrimaryMetadata` split.

These remain available:

- `ColorMetadata::display_p3()`
- `EncodeOptions::ultra_hdr_defaults()`
- `icc::display_p3()`

When packaging a gain map, the crate still auto-injects the bundled Display-P3
ICC profile when:

- the resolved primary image is Display-P3 plus sRGB
- and no explicit primary ICC profile is already present

## `encode_ultra_hdr(...)`

This API remains, but the `primary` subtree inside `UltraHdrEncodeOptions`
inherits the new `EncodeOptions` structure.

That means:

- `options.primary.primary_metadata` now holds the primary JPEG metadata
- `options.primary.gain_map` must still remain `None`

## Compatibility API Removal

The most disruptive change is that the wrapper-era compatibility API is no
longer public.

Removed root exports include:

- `CompressedImage`
- `RawImage`
- `Decoder`
- `EncodedStream`
- `DecodedPacked`
- `ImgLabel`
- `sys`
- `jpeg`
- `mozjpeg`
- the compatibility `Encoder`

If your code depended on that API, you have two options:

1. stay on `0.4.0-rc6` for now
2. port to the native root API

For most users, the target surface should now be:

- `Image`
- `encode(...)` or `Encoder`
- `decode(...)` and `DecodedImage`
- `inspect(...)` and `Inspection`

## Migration Checklist

1. Replace legacy type names with the new native names.
2. Replace `EncodeOptions::color_metadata` with `EncodeOptions::primary_metadata`.
3. Move EXIF payload handling into `PrimaryMetadata::exif`.
4. Replace `GainMapEncodeOptions` with `GainMapBundle`.
5. Replace `UltraJpegEncoder::new(...).encode(...)` with either
   `Encoder::new(...).encode(...)` or `encode(...)`.
6. Replace `ComputedGainMap::into_encode_options(...)` with `into_bundle(...)`.
7. Audit all decode call sites that relied on retained codestream bytes and add
   `DecodeOptions` retention flags explicitly.
8. Update any `gamut_info()` method calls to field access.
9. If you used the compatibility API, plan a full port or remain on
   `0.4.0-rc6`.

## Rationale For The Break

The `0.5.0-rc1` changes are intentionally opinionated:

- one coherent root API instead of mixed native and wrapper-era surfaces
- explicit ownership behavior
- better separation between primary-JPEG metadata and Ultra HDR metadata
- more discoverable naming
- less hidden allocation in the default decode path

That is why this release is a migration-heavy pre-`1.0` step rather than an
incremental rename-only release.
