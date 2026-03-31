# ultrajpeg

`ultrajpeg` is a Rust library for working with JPEG images.

It provides a native Rust API for encoding and decoding plain JPEG images, MPF-bundled gain-map JPEGs, ICC/EXIF payloads, UltraHDR XMP, and ISO 21496-1 metadata.

## Features

### Decode

- Inspect container metadata without decoding image pixels
- Decode a primary JPEG image into pixels
- Extract ICC profiles and EXIF payloads
- Extract explicit color metadata stored by `ultrajpeg`
- Detect MPF-bundled secondary JPEG payloads
- Decode gain-map JPEG payloads
- Parse UltraHDR XMP gain-map metadata
- Parse ISO 21496-1 binary gain-map metadata
- Reconstruct an HDR packed or linear view from a decoded gain map
- Decode large UltraHDR primary and gain-map JPEG payloads in parallel internally

### Encode

- Encode a primary JPEG image from `ultrahdr_core::RawImage`
- Encode a gain-map JPEG image
- Ship a built-in Display-P3 ICC profile helper and a spec-friendly Ultra HDR preset
- Write ICC profiles and EXIF payloads
- Write explicit color metadata
- Write UltraHDR XMP metadata
- Write ISO 21496-1 metadata
- Assemble an MPF JPEG that bundles the primary image and gain map

### Compatibility Wrappers

The crate also exposes compatibility wrappers for a small `mozjpeg_rs`-style JPEG encoding surface and an `ultrahdr`-style stateful encoding and decoding surface.

- `ultrajpeg::jpeg` and `ultrajpeg::mozjpeg`
  - simple JPEG encoder wrapper with `Encoder::new`, `.quality`, `.icc_profile`, and `.encode_rgb`
- `ultrajpeg::{CompressedImage, RawImage, Encoder, Decoder, ImgLabel, DecodedPacked, sys}`
  - stateful UltraHDR-style wrappers for:
  - setting an SDR base JPEG
  - setting packed HDR input pixels
  - encoding an UltraHDR JPEG
  - borrowing JPEG input slices without copying them
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

- `ultrajpeg::icc::display_p3`
- `ultrajpeg::inspect`
- `ultrajpeg::decode`
- `ultrajpeg::decode_with_options`
- `ultrajpeg::encode`
- `ultrajpeg::UltraJpegEncoder`

## Performance Notes

- `ultrajpeg::inspect` parses container markers without decoding pixels.
- The compatibility wrappers can borrow JPEG input directly with `CompressedImage::from_slice` and `Decoder::set_image_slice`.
- Large UltraHDR decodes may use internal Rayon-based parallelism for primary-image and gain-map JPEG decode. This is internal only; there is no async API and no thread-management API exposed by the crate.

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
    ..EncodeOptions::ultra_hdr_defaults()
};

let bytes = UltraJpegEncoder::new(options).encode(&primary)?;
let decoded = ultrajpeg::decode(&bytes)?;
assert_eq!(decoded.primary_image.width, 8);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Built-In Display-P3 ICC

When you want the crate to provide the primary-image ICC profile for a gain-map
JPEG, use either the raw ICC helper or the higher-level preset.

`ColorMetadata::display_p3()` and `EncodeOptions::ultra_hdr_defaults()` both
set all three pieces together:

- the embedded Display-P3 ICC profile
- `ColorGamut::DisplayP3`
- `ColorTransfer::Srgb`

You do not need to set the gamut separately when using those helpers.

```rust
use ultrahdr_core::{ColorGamut, ColorTransfer};
use ultrajpeg::{ColorMetadata, EncodeOptions, icc};

let raw_profile = icc::display_p3();
let color_metadata = ColorMetadata::display_p3();
let options = EncodeOptions::ultra_hdr_defaults();

assert_eq!(color_metadata.icc_profile.as_deref(), Some(raw_profile));
assert_eq!(color_metadata.gamut, Some(ColorGamut::DisplayP3));
assert_eq!(color_metadata.transfer, Some(ColorTransfer::Srgb));
assert_eq!(options.color_metadata.icc_profile.as_deref(), Some(raw_profile));
assert_eq!(options.color_metadata.gamut, Some(ColorGamut::DisplayP3));
assert_eq!(options.color_metadata.transfer, Some(ColorTransfer::Srgb));
```

## Decode Example

```rust,no_run
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

## Metadata-Only Inspection

```rust,no_run
use ultrajpeg::inspect;

let bytes = std::fs::read("image.jpg")?;
let inspected = inspect(&bytes)?;

println!(
    "primary_bytes={}, gain_map_bytes={:?}",
    inspected.primary_jpeg_len,
    inspected.gain_map_jpeg_len
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

let mut decoder = Decoder::new()?;
decoder.set_image_slice(
    encoded.as_slice(),
    sys::uhdr_color_gamut::UHDR_CG_UNSPECIFIED,
    sys::uhdr_color_transfer::UHDR_CT_UNSPECIFIED,
    sys::uhdr_color_range::UHDR_CR_UNSPECIFIED,
)?;
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
- metadata-only integration tests for plain JPEG and UltraHDR marker parsing
- encode/decode round-trip integration tests for the high-level API
- compatibility tests for the wrapper APIs
- representative Criterion benchmarks in `benches/typical.rs`, including generated megapixel UltraHDR scenarios

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
cargo release
```

It reads `package.version` from the root `Cargo.toml`, requires a clean working tree, creates the matching `v{version}` git tag, and pushes that tag to `origin`.

## Benchmarking

Run the representative benchmark suite with:

```bash
cargo bench --bench typical
```
