# ultrajpeg

`ultrajpeg` is a Rust library for working with JPEG-based UltraHDR images.

It combines three concerns that are often tangled together in application code:

- JPEG image coding
- JPEG container marker management
- UltraHDR gain-map and metadata semantics

The crate is designed as a public library crate, not as a thin binding to an AGPL or platform-specific implementation. It provides a native Rust API for encoding and decoding plain JPEG images, MPF-bundled gain-map JPEGs, ICC/EXIF payloads, UltraHDR XMP, and ISO 21496-1 metadata.

## Features

### Decode

- Decode a primary JPEG image into pixels
- Extract ICC profiles and EXIF payloads
- Extract explicit color metadata stored by `ultrajpeg`
- Detect MPF-bundled secondary JPEG payloads
- Decode gain-map JPEG payloads
- Parse UltraHDR XMP gain-map metadata
- Parse ISO 21496-1 binary gain-map metadata
- Reconstruct an HDR packed or linear view from a decoded gain map

### Encode

- Encode a primary JPEG image from `ultrahdr_core::RawImage`
- Encode a gain-map JPEG image
- Write ICC profiles and EXIF payloads
- Write explicit color metadata
- Write UltraHDR XMP metadata
- Write ISO 21496-1 metadata
- Assemble an MPF JPEG that bundles the primary image and gain map

### Compatibility Wrappers

The crate also exposes a compatibility surface intended to replace the subset of `mozjpeg_rs` and `ultrahdr` currently used by `site-assets`.

- `ultrajpeg::jpeg` and `ultrajpeg::mozjpeg`
  - simple JPEG encoder wrapper with `Encoder::new`, `.quality`, `.icc_profile`, and `.encode_rgb`
- `ultrajpeg::{CompressedImage, RawImage, Encoder, Decoder, ImgLabel, DecodedPacked, sys}`
  - stateful UltraHDR-style wrappers for:
  - setting an SDR base JPEG
  - setting packed HDR input pixels
  - encoding an UltraHDR JPEG
  - probing gain-map metadata
  - decoding a packed PQ HDR view

## Architecture

The crate is split into layers:

- `codec`
  - JPEG encode/decode
  - backed by `mozjpeg-rs` and `zune-jpeg`
- `container`
  - JPEG marker inspection and rewriting
  - backed by `img-parts`
- `metadata`
  - UltraHDR XMP and ISO 21496-1 handling
  - backed by `ultrahdr-core`
- `compat`
  - compatibility wrappers for existing client code

The public high-level API lives in:

- `ultrajpeg::decode`
- `ultrajpeg::decode_with_options`
- `ultrajpeg::encode`
- `ultrajpeg::UltraJpegEncoder`

## Quick Start

```rust
use ultrahdr_core::{GainMapMetadata, PixelFormat, RawImage};
use ultrajpeg::{EncodeOptions, GainMapEncodeOptions, UltraJpegEncoder};

let primary = RawImage::new(8, 8, PixelFormat::Rgb8)?;
let gain_map = RawImage::new(8, 8, PixelFormat::Gray8)?;

let options = EncodeOptions {
    gain_map: Some(GainMapEncodeOptions {
        image: gain_map,
        metadata: GainMapMetadata::new(),
        quality: 85,
        progressive: false,
    }),
    ..EncodeOptions::default()
};

let bytes = UltraJpegEncoder::new(options).encode(&primary)?;
let decoded = ultrajpeg::decode(&bytes)?;
assert_eq!(decoded.primary_image.width, 8);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Decode Example

```rust
use ultrajpeg::decode;

let bytes = std::fs::read("image.jpg")?;
let decoded = decode(&bytes)?;

println!(
    "{}x{}, gain_map={}",
    decoded.primary_image.width,
    decoded.primary_image.height,
    decoded.gain_map.is_some()
);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Compatibility Example

```rust
use ultrajpeg::{jpeg, sys, CompressedImage, Decoder, Encoder, ImgLabel, RawImage};

let mut base_jpeg = jpeg::Encoder::new(jpeg::Preset::ProgressiveSmallest)
    .quality(90)
    .encode_rgb(&vec![0; 4 * 4 * 3], 4, 4)?;

let mut hdr_pixels = vec![0; 4 * 4 * 4];
let mut hdr_raw = RawImage::packed(
    sys::uhdr_img_fmt::UHDR_IMG_FMT_32bppRGBA1010102,
    4,
    4,
    &mut hdr_pixels,
    sys::uhdr_color_gamut::UHDR_CG_DISPLAY_P3,
    sys::uhdr_color_transfer::UHDR_CT_PQ,
    sys::uhdr_color_range::UHDR_CR_FULL_RANGE,
)?;
let mut base = CompressedImage::from_bytes(
    base_jpeg.as_mut_slice(),
    sys::uhdr_color_gamut::UHDR_CG_BT_709,
    sys::uhdr_color_transfer::UHDR_CT_SRGB,
    sys::uhdr_color_range::UHDR_CR_FULL_RANGE,
);

let mut encoder = Encoder::new()?;
encoder.set_raw_image(&mut hdr_raw, ImgLabel::UHDR_HDR_IMG)?;
encoder.set_compressed_image(&mut base, ImgLabel::UHDR_SDR_IMG)?;
encoder.set_quality(90, ImgLabel::UHDR_BASE_IMG)?;
encoder.set_quality(90, ImgLabel::UHDR_GAIN_MAP_IMG)?;
encoder.set_output_format(sys::uhdr_codec::UHDR_CODEC_JPG)?;
encoder.encode()?;

let encoded = encoder.encoded_stream().unwrap().bytes()?.to_vec();

let mut encoded_owned = encoded.clone();
let mut compressed = CompressedImage::from_bytes(
    encoded_owned.as_mut_slice(),
    sys::uhdr_color_gamut::UHDR_CG_UNSPECIFIED,
    sys::uhdr_color_transfer::UHDR_CT_UNSPECIFIED,
    sys::uhdr_color_range::UHDR_CR_UNSPECIFIED,
);
let mut decoder = Decoder::new()?;
decoder.set_image(&mut compressed)?;
let packed = decoder.decode_packed_view(
    sys::uhdr_img_fmt::UHDR_IMG_FMT_32bppRGBA1010102,
    sys::uhdr_color_transfer::UHDR_CT_PQ,
)?;

assert_eq!(packed.width, 4);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Test Coverage

The repository includes:

- checked-in fixture vectors in [`tests/fixtures/`](/Users/julien.cayzac/github/jcayzac/ultrajpeg/tests/fixtures)
- fixture-backed integration tests for SDR JPEG and UltraHDR JPEG decode paths
- encode/decode round-trip integration tests for the high-level API
- compatibility tests matching the `site-assets` client flow

Fixture vectors currently cover:

- plain SDR JPEG generated by the high-level API
- plain SDR JPEG generated by the JPEG compatibility wrapper
- UltraHDR JPEG generated by the high-level API
- UltraHDR JPEG generated by the UltraHDR compatibility wrapper

## Current Scope

This crate currently targets the scenarios implemented in the public API and compatibility wrappers above. It does not try to be a full clone of every `ultrahdr` or `mozjpeg_rs` API surface, and it does not yet include malformed-input regression vectors or fuzzing infrastructure.

## Release Flow

The repository includes an `xtask` helper for tagged releases:

```bash
cargo run -p xtask -- release
```

It reads `package.version` from the root `Cargo.toml`, requires a clean working tree, creates the matching `v{version}` git tag, and pushes that tag to `origin`.
