# ultrajpeg

`ultrajpeg` is a Rust library for working with JPEG images.

It provides a native Rust API for encoding and decoding plain JPEG images,
MPF-bundled gain-map JPEGs, ICC/EXIF payloads, UltraHDR XMP, and
ISO 21496-1 metadata.

## What The Crate Does

`ultrajpeg` sits at the point where three concerns meet:

- JPEG pixel coding
- JPEG container metadata and marker layout
- Ultra HDR gain-map packaging and recovery

The crate is synchronous and native-first. The public API is centered around:

- `inspect(...)` for metadata-only inspection
- `inspect_container_layout(...)` for codestream-boundary inspection
- `decode(...)` and `decode_with_options(...)` for pixel decode
- `encode(...)` and `Encoder` for structured JPEG and Ultra HDR authoring
- `parse_gain_map_xmp(...)` and `parse_iso_21496_1(...)` for raw payload parsing
- `prepare_sdr_primary(...)` for caller-managed HDR workflows
- `compute_gain_map(...)` and `encode_ultra_hdr(...)` for gain-map workflows

## Choosing An Entry Point

Use:

- `inspect(...)` when you only need JPEG, ICC, EXIF, XMP, or ISO 21496-1
  metadata and do not want to decode pixels
- `decode(...)` when you want the decoded primary image and, when present, the
  decoded gain-map image
- `inspect_container_layout(...)` when you need codestream offsets and lengths
  without decoding pixels
- `decode_with_options(...)` when you also need to retain the raw primary JPEG
  or gain-map JPEG codestream bytes
- `encode(...)` when you already have the primary image and optional gain-map
  payload you want to package
  choose scan mode with `progressive` and size-vs-time policy with
  `CompressionEffort`
- `parse_gain_map_xmp(...)` or `parse_iso_21496_1(...)` when you need to
  validate or compare raw Ultra HDR metadata payloads yourself
- `prepare_sdr_primary(...)` when you manage HDR pixel transforms yourself and
  need a supported SDR primary image plus matching metadata before computing a
  gain map
  use your own SDR primary instead when you already have a caller-specific SDR
  rendering policy you want to preserve exactly
- `compute_gain_map(...)` when you want to generate a gain map from HDR and SDR
  inputs without encoding yet
- `encode_ultra_hdr(...)` when you want the crate to compute the gain map and
  package the final Ultra HDR JPEG in one step

## Public Surface Summary

The main public API lives at the crate root:

- functions:
  - `inspect`
  - `inspect_container_layout`
  - `decode`
  - `decode_with_options`
  - `encode`
  - `parse_gain_map_xmp`
  - `parse_iso_21496_1`
  - `prepare_sdr_primary`
  - `compute_gain_map`
  - `encode_ultra_hdr`
- core types:
  - `Image`
  - `PixelFormat`
  - `ColorGamut`
  - `ColorTransfer`
  - `GainMap`
  - `GainMapMetadata`
  - `HdrOutputFormat`
- structured crate types:
  - `ParsedGainMapXmp`
  - `ContainerKind`
  - `CodestreamLayout`
  - `ContainerLayout`
  - `ColorMetadata`
  - `PrimaryMetadata`
  - `UltraHdrMetadata`
  - `MetadataLocation`
  - `GainMapMetadataSource`
  - `DecodedGainMap`
  - `DecodedImage`
  - `Inspection`
  - `DecodeOptions`
  - `GainMapChannels`
  - `ComputeGainMapOptions`
  - `PreparePrimaryOptions`
  - `PreparedPrimary`
  - `ComputedGainMap`
  - `GainMapBundle`
  - `CompressionEffort`
  - `EncodeOptions`
  - `UltraHdrEncodeOptions`
  - `Encoder`
- module:
  - `icc`
