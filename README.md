# ultrajpeg

`ultrajpeg` is a Rust library for working with JPEG images.

It provides a native Rust API for encoding and decoding plain JPEG images,
MPF-bundled gain-map JPEGs, ICC/EXIF payloads, UltraHDR XMP, and
ISO 21496-1 metadata.

## Highlights

- plain JPEG encode and decode
- metadata-only inspection without pixel decode
- raw Ultra HDR payload parsing for XMP and ISO 21496-1
- structural inspection for MPF-bundled or concatenated JPEG codestreams
- Ultra HDR gain-map packaging and recovery
- SDR-primary preparation for caller-managed HDR workflows
- bundled Display-P3 ICC helper
- structured primary-image color metadata and Ultra HDR metadata
- synchronous API with internal parallel decode where useful

## Main API

The main public API is at the crate root:

- `inspect`
- `decode`
- `decode_with_options`
- `encode`
- `compute_gain_map`
- `encode_ultra_hdr`

The primary public types include:

- `Image`
- `EncodeOptions`
- `PrimaryMetadata`
- `GainMapBundle`
- `DecodedImage`
- `Inspection`
- `UltraHdrMetadata`
- `Encoder`

The docs.rs rustdoc is generated from the user-facing files under `docs/user/`.
