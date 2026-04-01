# Changelog

## 0.5.0

Detailed migration guide:

- [Migration guide from `0.4.0` to `0.5.0`](docs/user/migration-0.5.md)

### Added

- Added exhaustive rustdoc coverage across the public native API and enabled
  `#![deny(missing_docs)]` for the crate.
- Added expanded crate-level documentation covering major encode, decode,
  inspect, gain-map, color-metadata, provenance, ownership, and limitation
  scenarios for docs.rs consumers.
- Added `docs/maintainer/api-guide.md` as the maintainer-facing summary of the
  implemented stable API shape and its maintenance rules.
- Added `docs/user/migration-0.5.md` with a detailed migration path from
  `0.4.0`.
- Added public raw Ultra HDR payload parsing with
  `parse_gain_map_xmp(...)`, `parse_iso_21496_1(...)`, and the supporting
  `ParsedGainMapXmp` type.
- Added public structural container inspection with
  `inspect_container_layout(...)`, `ContainerKind`, `CodestreamLayout`, and
  `ContainerLayout`.
- Added public SDR-primary preparation for caller-managed HDR workflows with
  `prepare_sdr_primary(...)`, `PreparePrimaryOptions`, and `PreparedPrimary`.

### Changed

- The `0.5.0` line began with the main stable native API refactor and then
  gained additional additive APIs before the full `0.5.0` shape described
  here.
- Removed the wrapper-era compatibility API from the public crate surface.
- Simplified the root API around one native surface with `Image`, `encode`,
  `Encoder`, `decode`, `DecodedImage`, `inspect`, and `Inspection`.
- Renamed `DecodedJpeg` to `DecodedImage`, `InspectedJpeg` to `Inspection`,
  `GainMapEncodeOptions` to `GainMapBundle`, `UltraJpegEncoder` to `Encoder`,
  and `CoreRawImage` to `Image`.
- Split EXIF out of `ColorMetadata` into the new `PrimaryMetadata` type and
  renamed `EncodeOptions::color_metadata` to
  `EncodeOptions::primary_metadata`.
- Promoted metadata provenance into the public API with
  `MetadataLocation` and `GainMapMetadataSource`.
- `decode(...)` no longer retains primary or gain-map JPEG codestream bytes by
  default; retained codestreams are now explicit through `DecodeOptions`.
- `ComputedGainMap::into_encode_options(...)` was replaced by
  `ComputedGainMap::into_bundle(...)`.
- Primary and gain-map encode settings now carry an explicit
  `CompressionEffort`, so callers can request balanced or size-oriented
  encoding without changing the scan-mode flag.
- The `progressive` flags on primary and gain-map encode settings now document
  scan mode only; compression policy is described separately through
  `CompressionEffort`.
- Ultra HDR ISO 21496-1 handling is now interoperable with canonical Ultra HDR
  JPEGs: encode writes the primary version-only APP2 block plus canonical
  gain-map ISO payloads, and decode/raw parsing now accept those canonical
  payloads while keeping compatibility with the older legacy payload layout.
- The primary Ultra HDR container XMP now also declares the `hdrgm`
  namespace/version on its container packet, matching the expected packet shape
  without moving gain-map parameter fields out of the gain-map JPEG XMP.
- The `0.5.0` API surface now closes the main post-refactor gaps that still
  forced direct `ultrahdr-core` usage for some consumers:
  raw Ultra HDR payload parsing, structural bundled-container inspection, and
  supported SDR-primary preparation.
- `prepare_sdr_primary(...)` applies an explicit high-level SDR preparation
  policy, including brightness flooring so the returned primary image composes
  with the crate's default `compute_gain_map(...)` configuration.

### Migration

- Expect source changes if you were using `0.4.0`; this is an intentional
  API-shaping release before `1.0`.
- Port native callers to `Image`, `PrimaryMetadata`, `DecodedImage`,
  `Inspection`, `GainMapBundle`, and `Encoder`.
- When preserving earlier encode behavior explicitly, set
  `compression: CompressionEffort::Balanced` on primary and gain-map encode
  options.
- `CompressionEffort::Smallest` is accepted for both scan modes, but with the
  current backend its extra size-oriented path only affects progressive JPEGs.
- If you relied on default retained codestream bytes from `decode(...)`, switch
  to `decode_with_options(...)` and enable the relevant retention flags
  explicitly.
- If you relied on the old compatibility API, either stay on `0.4.0` for
  now or port to the native root API; the compatibility surface is no longer
  public in `0.5.0`.
- If you currently depend directly on `ultrahdr-core` only for raw `hdrgm:*`
  XMP parsing, raw ISO 21496-1 parsing, or bundled codestream-boundary
  inspection, prefer the new `0.5.0` root APIs instead.
- If you previously hand-rolled HDR-to-SDR preparation just to feed
  `compute_gain_map(...)`, evaluate `prepare_sdr_primary(...)` as the supported
  high-level default path.

## 0.4.0

### Added

- Added a built-in ICC registry Display-P3 profile helper at `ultrajpeg::icc::display_p3()`.
- Added `ColorMetadata::display_p3()` and `EncodeOptions::ultra_hdr_defaults()` as explicit helpers for spec-friendly Ultra HDR primary-image metadata; both set the built-in Display-P3 ICC profile, `ColorGamut::DisplayP3`, and `ColorTransfer::Srgb` together.
- Added structured gamut inspection helpers with `Chromaticity`, `GamutInfo`, `ColorMetadata::gamut_info()`, and `DecodedPacked::gamut_info()`.
- Added a public gain-map computation seam with `GainMapChannels`, `ComputeGainMapOptions`, `ComputedGainMap`, and `compute_gain_map(...)`.
- Added `ComputedGainMap::into_encode_options(...)` so computed gain maps compose directly with `EncodeOptions`.
- Added `UltraHdrEncodeOptions` and the thin `encode_ultra_hdr(...)` convenience wrapper for callers that already chose their SDR primary image.

### Changed

- The UltraHDR compatibility encoder now preserves an existing primary JPEG ICC profile, and if the base JPEG has no ICC while the HDR input gamut is `sys::uhdr_color_gamut::UHDR_CG_DISPLAY_P3`, it injects the crate's built-in Display-P3 ICC profile automatically. Other HDR input gamuts do not trigger ICC auto-injection.
- Ultra HDR metadata parsing now prefers ISO 21496-1 over XMP when both metadata forms are present.
- Ultra HDR XMP fallback now has lightweight defensive rejection in `ultrajpeg`: XMP with `hdrgm:BaseRenditionIsHDR="True"` is ignored, and XMP fallback is ignored when key required fields are missing.
- Ultra HDR encode now follows the spec-shaped metadata split more closely: the primary JPEG carries MPF plus the container/directory XMP, while the gain-map JPEG carries the `hdrgm:*` XMP payload and the ISO 21496-1 payload.
- Ultra HDR decode is now more tolerant of malformed files that still contain usable gain-map semantics: Adobe Extended XMP on the primary JPEG is reassembled, and if the primary JPEG lacks effective gain-map metadata but MPF points to a secondary JPEG with valid `hdrgm:*` XMP or ISO 21496-1 gain-map metadata, the file is still decoded as Ultra HDR.
- Gain-map decoding now supports both single-channel and multichannel gain-map JPEG payloads.
- The compatibility encoder now reuses the same gain-map computation path exposed by the new public `compute_gain_map(...)` API.
- `compat::Decoder::decode_packed_view()` now consults ICC-derived gamut semantics before falling back to the caller hint, so valid Ultra HDR JPEGs with usable primary-image ICC data no longer degrade to `UHDR_CG_UNSPECIFIED` unnecessarily.

### Migration

- If you want a Display-P3-tagged primary image for gain-map output, prefer `..EncodeOptions::ultra_hdr_defaults()` or `ColorMetadata::display_p3()` instead of sourcing and embedding the ICC payload manually in each caller. Those helpers already set the ICC profile, gamut, and transfer together, so you do not need to set the gamut separately.
- If you need structured gamut semantics from decoded metadata or compat packed decode, prefer `ColorMetadata::gamut_info()` or `DecodedPacked::gamut_info()` instead of relying only on the legacy enum returned by `DecodedPacked::meta()`. `meta()` still returns the best matching known gamut when one can be classified safely.
- Compatibility-wrapper callers do not need to inject the built-in Display-P3 ICC manually when encoding from an HDR source tagged with `sys::uhdr_color_gamut::UHDR_CG_DISPLAY_P3`, unless they want a different primary ICC than the preserved base JPEG ICC or the built-in default.
- Policy-aware callers that previously had to route through the compatibility encoder just to compute a gain map can now use `compute_gain_map(...)` and then package through `encode(...)`.
- If you want a single-call wrapper, use `encode_ultra_hdr(...)`, but keep in mind that `UltraHdrEncodeOptions::primary.gain_map` must remain `None` because the gain map is computed by the wrapper itself.
- No public API signatures changed for the metadata-placement fix; the observable change is that encoded Ultra HDR files now place primary container metadata and gain-map metadata in different codestreams, and decode can recover effective gain-map metadata from the secondary JPEG when needed.

## 0.3.0

### Added

- Added generated megapixel benchmark scenarios to `benches/typical.rs`, including realistic plain-JPEG decode, UltraHDR decode, gain-map skipping, and compatibility-wrapper packed HDR decode.
- Added `CompressedImage::from_slice` for borrowed immutable JPEG input.
- Added `Decoder::set_image_slice` for borrowed compat decode setup without a staging `Vec<u8>`.
- Added owned-taking compat setters `Decoder::set_image_owned`, `Encoder::set_raw_image_owned`, and `Encoder::set_compressed_image_owned` so owned buffers can move into the stateful wrappers without an extra clone.

### Changed

- The crate version is now `0.3.0`.
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
