# Changelog

## 0.4.0

### Added

- Added a built-in ICC registry Display-P3 profile helper at `ultrajpeg::icc::display_p3()`.
- Added `ColorMetadata::display_p3()` and `EncodeOptions::ultra_hdr_defaults()` as explicit helpers for spec-friendly Ultra HDR primary-image metadata; both set the built-in Display-P3 ICC profile, `ColorGamut::DisplayP3`, and `ColorTransfer::Srgb` together.

### Migration

- If you want a Display-P3-tagged primary image for gain-map output, prefer `..EncodeOptions::ultra_hdr_defaults()` or `ColorMetadata::display_p3()` instead of sourcing and embedding the ICC payload manually in each caller. Those helpers already set the ICC profile, gamut, and transfer together, so you do not need to set the gamut separately.

## 0.3.0

### Added

- Added generated megapixel benchmark scenarios to `benches/typical.rs`, including realistic plain-JPEG decode, UltraHDR decode, gain-map skipping, and compatibility-wrapper packed HDR decode.
- Added `CompressedImage::from_slice` for borrowed immutable JPEG input.
- Added `Decoder::set_image_slice` for borrowed compat decode setup without a staging `Vec<u8>`.
- Added owned-taking compat setters `Decoder::set_image_owned`, `Encoder::set_raw_image_owned`, and `Encoder::set_compressed_image_owned` so owned buffers can move into the stateful wrappers without an extra clone.

### Changed

- The crate version is now `0.3.0-rc1`.
- `Decoder::decode_packed_view()` now uses a faster internal decode path that skips retaining primary and gain-map codestream copies that are irrelevant to packed HDR reconstruction.
- UltraHDR primary-image decode and gain-map decode can now run in parallel internally for larger containers, using Rayon behind a size threshold and without changing the public API shape.
- The benchmark suite now measures larger, more realistic decode paths instead of only tiny fixture-sized scenarios.

### Migration

- Existing code keeps working.
- If you only need compatibility decode, prefer `Decoder::set_image_slice(bytes, ...)` over constructing a temporary owned buffer just to call `set_image`.
- If you already have an owned `CompressedImage` or `RawImage`, prefer `set_image_owned`, `set_raw_image_owned`, or `set_compressed_image_owned` to avoid the clone performed by the legacy mut-borrow setter methods.
- `cargo bench --bench typical` now includes heavier megapixel scenarios, so benchmark runs take longer than in `0.2.0`.

## 0.2.0

### Added

- Added `ultrajpeg::inspect`, a metadata-only container inspection path that parses JPEG markers without decoding pixel data.
- Added Criterion benchmarks in `benches/typical.rs` for representative SDR JPEG, UltraHDR JPEG, and compatibility-wrapper scenarios.
- Added a `cargo release` alias for the release xtask.
- Added integration coverage for metadata-only inspection and for cases where marker parsing succeeds even though pixel decoding fails later.
- Added owned-buffer helpers for the compatibility wrappers: `CompressedImage::from_vec` and `RawImage::packed_owned`.

### Changed

- `Decoder::gainmap_metadata()` now uses the metadata-only inspection path instead of decoding the primary image.
- Decode-side container parsing now works on borrowed JPEG slices instead of cloning the primary codestream before reading markers.
- Container assembly now uses the parsed JPEG length directly when computing MPF offsets instead of serializing the primary JPEG a second time.
- JPEG decode now passes borrowed input directly to `zune-jpeg` instead of cloning the input buffer first.
- Compatibility wrapper image buffers are now borrowing-based internally, which removes eager copies in `CompressedImage::from_bytes` and `RawImage::packed`.

### Migration

- Most call sites do not need changes.
- If you explicitly annotate compatibility-wrapper types, add an elided lifetime such as `CompressedImage<'_>`, `RawImage<'_>`, `Encoder<'_>`, or `Decoder<'_>`.
- If you want owned compatibility buffers instead of borrowed ones, use `CompressedImage::from_vec` and `RawImage::packed_owned`.
- If you only need metadata, prefer `ultrajpeg::inspect` over `decode` or `decode_with_options`.
