# Ultrajpeg Implementation Plan

## Goal

Build a public Rust crate named `ultrajpeg` that can encode and decode JPEG-based HDR images with gain maps, color signaling, ICC profiles, and UltraHDR-related metadata, without depending on AGPL-licensed components.

The crate should combine a permissive-license JPEG codec stack with `ultrahdr_core` for gain-map and metadata semantics.

## Core Product Requirements

The public crate must support all of the following:

- Decode a primary JPEG image into pixels.
- Decode embedded gain map information from JPEG container metadata and associated JPEG payloads.
- Decode color signaling information, including ICC profiles.
- Decode XMP and ISO 21496-1 metadata relevant to HDR / gain-map workflows.
- Encode a primary JPEG image.
- Encode a gain map JPEG image.
- Encode color signaling, including ICC profiles.
- Encode required UltraHDR metadata into the JPEG container.
- Preserve or rewrite JPEG marker layout in a standards-compatible way.

## Technical Constraints

- `zenjpeg` must not be used.
- `ultrahdr_core` may be used and is expected to be the core source of UltraHDR-specific logic.
- The implementation must remain compatible with permissive public-crate licensing goals.
- JPEG image coding must be separated from JPEG container management.
- Metadata handling must be separated from raw JPEG segment manipulation.
- The crate must be usable as a public library crate with documented, stable APIs.
- The implementation should be pure Rust where practical, but the main hard constraint is permissive licensing and public-crate usability.

## Proposed Dependency Stack

### Required

- `ultrahdr_core`
  - Gain map math.
  - XMP / ISO 21496-1 metadata parsing and generation.
  - Tone mapping and color-related HDR support.

- `img-parts`
  - JPEG segment enumeration and rewriting.
  - APP marker insertion, replacement, and extraction.
  - ICC / EXIF / marker-preserving container surgery.

- `zune-jpeg`
  - JPEG decoding to image buffers.

### Candidate encoder backend

Primary candidate:

- `mozjpeg_rs`
  - JPEG encoding backend for base image and gain map JPEG payloads.

Fallback plan if the encoder proves insufficient:

- swap the encoding backend behind an internal trait, so the public crate API does not depend directly on one specific encoder crate.

### Optional

- `xmpkit`
  - Only if `ultrahdr_core` does not already cover the XMP authoring/parsing needs cleanly enough.
  - Avoid introducing it if it duplicates functionality already available in `ultrahdr_core`.

## High-Level Architecture

The crate should be structured around four layers.

### 1. Image codec layer

Responsibilities:

- Encode pixel buffers into JPEG byte streams.
- Decode JPEG byte streams into pixel buffers.

Planned crates:

- Encode: `mozjpeg_rs`.
- Decode: `zune-jpeg`.

This layer must know nothing about UltraHDR metadata conventions beyond basic JPEG encoding parameters.

### 2. JPEG container layer

Responsibilities:

- Read JPEG segments.
- Insert, remove, or replace APP markers.
- Preserve existing image codestream bytes where possible.
- Extract ICC, XMP, and UltraHDR-related payloads.

Planned crate:

- `img-parts`.

This layer must treat metadata and gain-map payloads as structured binary payloads, not as image-processing concepts.

### 3. UltraHDR semantics layer

Responsibilities:

- Gain map metadata encoding and decoding.
- XMP and ISO 21496-1 metadata generation/parsing.
- Gain map application and HDR reconstruction.
- Color and tone-mapping support needed to interpret or produce HDR outputs.

Planned crate:

- `ultrahdr_core`.

This layer must remain independent of the JPEG codec backend.

### 4. Integration layer (`ultrajpeg` itself)

Responsibilities:

- Public API.
- Orchestrating image encode/decode, marker parsing, and metadata interpretation.
- Producing final JPEG files.
- Exposing ergonomic encode/decode entry points.

## Public API Shape

The public API should be small, explicit, and versionable.

### Decode-side types

Candidate types:

- `DecodedJpeg`
  - Primary image pixels.
  - Pixel format / dimensions.
  - ICC profile if present.
  - Color signaling metadata if present.
  - Gain map JPEG payload if present.
  - Parsed HDR metadata if present.

- `DecodedGainMap`
  - Gain map image pixels.
  - Parsed gain map metadata.

- `ColorMetadata`
  - ICC profile.
  - Any explicit signaling fields the crate supports.

### Encode-side types

Candidate types:

- `EncodeOptions`
  - JPEG quality / chroma settings.
  - Progressive/baseline options if supported.
  - ICC profile.
  - Explicit color signaling.

- `GainMapEncodeOptions`
  - Gain map JPEG encoding options.
  - Gain map semantic metadata.

- `UltraJpegEncoder`
  - Encodes base image + gain map + metadata into final JPEG bytes.

### Error model

- Separate codec, container, and metadata errors internally.
- Expose a single public error enum with layered variants.
- Include enough context for debugging malformed files.

## Encode Pipeline

The encode path should work as follows:

1. Accept input primary image pixels.
2. Accept either:
   - a precomputed gain map image and metadata, or
   - enough inputs to derive one later if that feature is added.
3. Encode the primary image to JPEG bytes using the selected encoder backend.
4. Encode the gain map image to JPEG bytes using the same encoder backend.
5. Use `ultrahdr_core` to generate the required metadata payloads.
6. Use `img-parts` to construct the final JPEG container:
   - primary JPEG codestream
   - gain map references/payloads
   - ICC profile segments
   - XMP segments
   - ISO 21496-1 or related HDR segments
7. Emit final JPEG bytes.

## Decode Pipeline

The decode path should work as follows:

1. Parse JPEG bytes with `img-parts`.
2. Extract:
   - ICC profile
   - XMP payloads
   - UltraHDR / gain-map-related APP payloads
   - any embedded gain map JPEG payload references or container structures
3. Use `ultrahdr_core` to parse HDR metadata and gain-map metadata.
4. Decode the primary JPEG image via `zune-jpeg`.
5. Decode the gain map JPEG via `zune-jpeg` if present.
6. Return structured decode output.
7. Optionally expose helper APIs to apply the gain map and reconstruct display-ready output.

## JPEG Container Responsibilities

This is a critical area and must be treated as a first-class subsystem.

The implementation must support:

- APP marker inspection.
- APP marker insertion.
- APP marker removal/replacement.
- ICC profile extraction and rewriting.
- XMP extraction and rewriting.
- Marker ordering validation.
- Preservation of unrelated markers where possible.
- Container round-tripping without re-encoding image data unless necessary.

## Metadata Responsibilities

The implementation must support:

- Reading XMP relevant to UltraHDR/gain maps.
- Writing XMP relevant to UltraHDR/gain maps.
- Reading ISO 21496-1 metadata if stored in JPEG-associated metadata payloads.
- Writing ISO 21496-1 metadata where required.
- Exposing enough structured metadata in Rust types that client code does not need to parse raw XML or raw APP payload bytes unless it chooses to.

## Versioning and Compatibility Policy

- The crate is public and should follow semver carefully.
- Metadata models that mirror standards should avoid unnecessary abstraction churn.
- Encoder backend choice must remain an internal detail where possible.
- If backend-specific options are exposed, they must be contained in backend-specific opt-in APIs.

## Milestones

### Milestone 0: Investigation and spike

Deliverables:

- Validate that `mozjpeg_rs`, `zune-jpeg`, `img-parts`, and `ultrahdr_core` can coexist cleanly.
- Build a tiny prototype that:
  - encodes a JPEG,
  - injects an APP marker,
  - reads it back,
  - decodes the image.

Exit criteria:

- No blocking incompatibility in bytes/ownership/types.
- `img-parts` proves sufficient for required marker rewriting.

### Milestone 1: Repository bootstrap

Deliverables:

- Public crate scaffold.
- License, README, CI skeleton.
- Initial module layout.
- Rustdoc overview and crate goals.

Exit criteria:

- `cargo check`, `cargo test`, `cargo doc` green in CI.

### Milestone 2: JPEG container subsystem

Deliverables:

- Internal wrapper over `img-parts`.
- Marker read/write helpers.
- ICC extraction/insertion helpers.
- XMP extraction/insertion helpers.

Exit criteria:

- Round-trip tests for JPEG markers and ICC/XMP retention.

### Milestone 3: Metadata subsystem

Deliverables:

- Integration with `ultrahdr_core`.
- Internal metadata model.
- Translation between JPEG marker payloads and Rust structs.

Exit criteria:

- Known metadata payloads parse and serialize deterministically.

### Milestone 4: Decode MVP

Deliverables:

- Decode primary JPEG image.
- Extract ICC/profile/signaling metadata.
- Detect and decode gain map JPEG payloads.
- Parse associated UltraHDR metadata.

Exit criteria:

- Decode fixture files into structured Rust values.

### Milestone 5: Encode MVP

Deliverables:

- Encode base JPEG.
- Encode gain map JPEG.
- Write ICC/XMP/UltraHDR metadata.
- Produce final JPEG container.

Exit criteria:

- Encoded files can be parsed back by the crate.
- Round-trip metadata tests pass.

### Milestone 6: HDR reconstruction helpers

Deliverables:

- Optional APIs to apply gain maps using `ultrahdr_core`.
- Conversion helpers for common output formats.

Exit criteria:

- Visual and numeric fixture comparisons are stable.

### Milestone 7: Interop and conformance

Deliverables:

- Real-world test vectors.
- Compatibility checks against target readers/writers.
- Regression corpus.

Exit criteria:

- Known-good samples decode correctly.
- Generated files are accepted by target decoders.

### Milestone 8: Public API stabilization

Deliverables:

- Finalize type names and module structure.
- Write examples.
- Finalize docs.rs landing page.

Exit criteria:

- Crate API is coherent and documented.

## Testing Strategy

### Unit tests

- Marker parsing.
- ICC extraction/insertion.
- XMP extraction/insertion.
- Metadata round-trip serialization.

### Integration tests

- Encode a base JPEG + gain map + metadata and decode it back.
- Verify no data loss for ICC and metadata payloads.
- Verify correct behavior with files lacking gain maps.

### Fixture tests

- Small committed JPEG fixtures.
- HDR/gain-map fixtures where licensing permits.
- Regression corpus for malformed markers and edge cases.

### Interop tests

- Compare outputs with external readers when possible.
- Verify generated files are accepted by target ecosystems.

### Fuzzing / robustness

- Marker parser fuzzing.
- Metadata parser fuzzing.
- Malformed JPEG container handling.

## CI Plan

Run on:

- Linux
- macOS
- Windows

Required jobs:

- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`
- `cargo doc`

Later additions:

- fixture/interoperability jobs
- fuzzing on a scheduled basis

## Documentation Plan

The public crate must ship with:

- README explaining what UltraJPEG is and what standards/features it supports.
- docs.rs summary with a concrete explanation of the crate’s layering.
- examples for:
  - decoding an HDR JPEG with gain maps
  - encoding a JPEG with gain map metadata
  - reading/writing ICC profiles

The docs must prominently explain:

- what the crate can decode from JPEG images
- what the crate can encode into JPEG images
- that JPEG image coding, container management, and metadata semantics are separate layers

## Risks

### 1. Marker-layout incompatibility

Risk:

- The chosen container-writing approach may produce files that are technically valid but not accepted by target readers.

Mitigation:

- Add interop fixtures early.
- Preserve marker ordering rules explicitly.

### 2. Encoder backend limitations

Risk:

- `mozjpeg_rs` may not expose enough control for the desired JPEG characteristics.

Mitigation:

- Hide the backend behind an internal abstraction.
- Keep the option to swap the encoder.

### 3. Metadata overlap between crates

Risk:

- `xmpkit` and `ultrahdr_core` may overlap awkwardly.

Mitigation:

- Prefer one source of truth for XMP payload generation.
- Add `xmpkit` only if it reduces work materially.

### 4. Decode-path ambiguity

Risk:

- Real-world files may not all use identical conventions for carrying gain maps and metadata.

Mitigation:

- Build tolerant parsing with structured error reporting.
- Support partial decode when possible.

## Non-Goals for V1

- Reimplementing a full JPEG codec from scratch.
- Building a full image editor or viewer.
- Exposing every possible JPEG marker as a public stable API in the first release.
- Supporting every HDR metadata convention beyond the specifically targeted UltraHDR-related ones.

## Recommended First Build Order

1. Milestone 0 spike.
2. Marker round-trip prototype with `img-parts`.
3. Decode MVP with `zune-jpeg` + `ultrahdr_core`.
4. Encode MVP with `mozjpeg_rs` + `img-parts`.
5. Metadata write/read round-trip.
6. Public API cleanup.
7. Interop testing.
8. First public pre-release.

## Immediate Next Actions

- Create the crate scaffold.
- Add a small prototype proving:
  - JPEG encode via `mozjpeg_rs`
  - JPEG decode via `zune-jpeg`
  - APP marker insertion/extraction via `img-parts`
  - metadata struct generation/parsing via `ultrahdr_core`
- Decide whether `xmpkit` is actually needed after the prototype.
