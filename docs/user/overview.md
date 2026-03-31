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
- `decode(...)` and `decode_with_options(...)` for pixel decode
- `encode(...)` and `Encoder` for structured JPEG and Ultra HDR authoring
- `compute_gain_map(...)` and `encode_ultra_hdr(...)` for gain-map workflows

## Choosing An Entry Point

Use:

- `inspect(...)` when you only need JPEG, ICC, EXIF, XMP, or ISO 21496-1
  metadata and do not want to decode pixels
- `decode(...)` when you want the decoded primary image and, when present, the
  decoded gain-map image
- `decode_with_options(...)` when you also need to retain the raw primary JPEG
  or gain-map JPEG codestream bytes
- `encode(...)` when you already have the primary image and optional gain-map
  payload you want to package
- `compute_gain_map(...)` when you want to generate a gain map from HDR and SDR
  inputs without encoding yet
- `encode_ultra_hdr(...)` when you want the crate to compute the gain map and
  package the final Ultra HDR JPEG in one step

## Public Surface Summary

The main public API lives at the crate root:

- functions:
  - `inspect`
  - `decode`
  - `decode_with_options`
  - `encode`
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
  - `ComputedGainMap`
  - `GainMapBundle`
  - `EncodeOptions`
  - `UltraHdrEncodeOptions`
  - `Encoder`
- module:
  - `icc`
