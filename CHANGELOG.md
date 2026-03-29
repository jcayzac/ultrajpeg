# Changelog

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
