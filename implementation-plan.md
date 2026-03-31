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

## API Evolution Plan

This section captures the current plan for the next Ultra HDR API changes in
`ultrajpeg`, based on:

- the current crate design,
- the compliance gaps already identified,
- and current consumer integration feedback.

The goal is to make spec-oriented Ultra HDR JPEG assembly easier for consumers
without pushing consumer-specific image-pipeline policy into this crate.

### Consumer Requirements Retained

The following requirements from current consumer integration feedback are
retained as design inputs for this plan.

#### Input semantics

- For JPEG input, the presence of a gain map is what makes the image
  HDR-capable.
- A JPEG with no gain map should be treated as SDR JPEG.

#### Output semantics

- If gain-map packaging cannot be preserved or derived, the consumer may choose
  to degrade to a plain SDR JPEG instead of failing.
- That degradation policy remains consumer-owned and is not moved into
  `ultrajpeg`.

#### Compliance-oriented output goals

- The primary image must carry an ICC profile.
- Display-P3 is the preferred primary profile when authoring a new adaptive-HDR
  JPEG.
- Both Ultra HDR XMP and ISO 21496-1 metadata should be emitted.
- ISO 21496-1 should be preferred over XMP on decode.
- Single-channel gain maps should be the default unless multichannel is
  explicitly requested.

#### Existing-Ultra-HDR preservation rule

- If the input is already an Ultra HDR JPEG, the existing primary-image profile
  should be preserved rather than overridden.

#### Newly-authored adaptive-HDR rule

- If the input is not already an Ultra HDR JPEG, the chosen SDR primary image
  may be converted to Display-P3 before packaging.
- That conversion decision remains consumer-owned. `ultrajpeg` should make it
  easy to package the chosen result correctly, not perform the policy decision
  itself.

#### Provenance sensitivity

- Some compliance-sensitive consumers may care about metadata provenance
  visibility, even if the immediate implementation keeps the public decode API
  conservative.

### Goals

The next API iteration should make all of the following true:

1. `ultrajpeg` prefers ISO 21496-1 metadata over XMP when both are present.
2. `ultrajpeg` continues to emit both Ultra HDR XMP and ISO 21496-1 metadata
   when packaging gain maps.
3. `ultrajpeg` exposes a first-class public API to compute a gain map from:
   - an HDR image,
   - and a caller-chosen SDR primary image.
4. That API defaults to single-channel gain maps.
5. Multichannel gain maps require explicit opt-in.
6. The new API composes naturally with the existing structured encode path.
7. The crate docs make the policy boundary clear:
   - consumers own color-conversion and output policy,
   - `ultrajpeg` owns gain-map computation and Ultra HDR packaging mechanics.

### Non-Goals

The following should remain outside `ultrajpeg` unless the crate is deliberately
expanded in scope later:

- generic color-space conversion policy,
- source-format heuristics,
- degradation policy such as "fall back to SDR if packaging fails",
- asset-pipeline policy,
- consumer-specific decisions about when Display-P3 should be used.

`ultrajpeg` should make compliant packaging easy, but should not decide the
consumer's broader pipeline behavior.

### Boundary

The intended long-term split is:

#### Consumer owns

- source-format policy,
- HDR-vs-SDR product policy,
- SDR primary image preparation,
- color conversion into the chosen primary space,
- "preserve existing profile vs convert to Display-P3" decisions,
- fallback/degradation behavior when adaptive HDR cannot be preserved.

#### `ultrajpeg` owns

- primary JPEG encoding,
- gain-map computation,
- gain-map JPEG encoding,
- Ultra HDR XMP generation,
- ISO 21496-1 generation,
- MPF container assembly,
- metadata precedence on decode,
- sensible Ultra HDR defaults such as single-channel gain maps.

### Intentional Deviations From The Consumer Proposal

This plan intentionally does not follow the consumer proposal verbatim in a few
places.

#### 1. `compute_gain_map(...)` should not return `GainMapEncodeOptions`

The consumer proposal suggested returning `GainMapEncodeOptions` directly from
the new gain-map computation API.

This plan intentionally changes that.

Reason:

- `GainMapEncodeOptions` mixes:
  - computed gain-map content,
  - and JPEG encoding policy for the gain-map codestream.

That is too coupled for the primary seam we want to expose.

The maintainer-preferred shape is:

- `ComputedGainMap { image, metadata }`

with the caller still choosing:

- gain-map JPEG quality,
- gain-map JPEG progressive settings,
- and final packaging options.

#### 2. Metadata provenance is not being expanded immediately

The consumer feedback raised the possibility of exposing:

- parsed metadata from XMP,
- parsed metadata from ISO 21496-1,
- and the effective winner.

That is a valid future direction, but this plan intentionally does not make it
part of the immediate public API expansion.

Reason:

- ISO-over-XMP precedence can be corrected and documented without increasing the
  public API surface right away.
- Provenance exposure should be added only if a concrete consumer need remains
  after the precedence fix is in place.

#### 3. `encode_ultra_hdr(...)` remains optional

The consumer proposal correctly identified a high-level wrapper as potentially
useful, but this plan intentionally treats it as secondary.

Reason:

- the missing fundamental seam is public gain-map computation,
- not a new top-level convenience wrapper.

If a wrapper is added, it should be implemented after the lower-level seam
exists and should remain a thin composition layer.

### Proposed Public API Shape

The consumer feedback is directionally correct, but the return type should be
slightly different from the draft proposal.

The new public seam should expose gain-map computation itself, not computation
already bundled with JPEG encoding policy.

#### Proposed types

```rust
pub enum GainMapChannels {
    Single,
    Multi,
}

pub struct ComputeGainMapOptions {
    pub channels: GainMapChannels,
}

impl Default for ComputeGainMapOptions {
    fn default() -> Self {
        Self {
            channels: GainMapChannels::Single,
        }
    }
}

pub struct ComputedGainMap {
    pub image: RawImage,
    pub metadata: GainMapMetadata,
}

pub fn compute_gain_map(
    hdr_image: &RawImage,
    primary_image: &RawImage,
    options: &ComputeGainMapOptions,
) -> Result<ComputedGainMap>;
```

#### Why not return `GainMapEncodeOptions` directly

Returning `GainMapEncodeOptions` would mix two different concerns:

- gain-map computation output:
  - image,
  - metadata,
- gain-map JPEG encoding policy:
  - quality,
  - progressive.

Those concerns are adjacent, but they are not the same layer.

Keeping them separate preserves flexibility and avoids over-coupling the new
API to the current encoding struct layout.

#### Composition with the existing encoder

The intended caller flow should be:

```rust
let computed = ultrajpeg::compute_gain_map(&hdr, &primary, &Default::default())?;

let bytes = ultrajpeg::encode(
    &primary,
    &EncodeOptions {
        color_metadata: chosen_color_metadata,
        gain_map: Some(GainMapEncodeOptions {
            image: computed.image,
            metadata: computed.metadata,
            quality: 90,
            progressive: false,
        }),
        ..EncodeOptions::default()
    },
)?;
```

That keeps the seam explicit:

- the consumer chooses the primary image and color policy,
- `ultrajpeg` computes the gain map,
- `ultrajpeg` packages the result.

### Optional Convenience Layer

After the computation helper exists, a thin wrapper may be added:

```rust
pub struct UltraHdrEncodeOptions {
    pub primary: EncodeOptions,
    pub gain_map: ComputeGainMapOptions,
    pub gain_map_quality: u8,
    pub gain_map_progressive: bool,
}

pub fn encode_ultra_hdr(
    hdr_image: &RawImage,
    primary_image: &RawImage,
    options: &UltraHdrEncodeOptions,
) -> Result<Vec<u8>>;
```

This should remain optional and thin.

It must not absorb consumer policy such as:

- whether to preserve an existing primary profile,
- whether to convert to Display-P3,
- whether to degrade to SDR.

### Decode-Side Metadata Policy

The decode path should treat metadata precedence as a crate-level policy.

#### Required behavior

- If ISO 21496-1 metadata is present and parses successfully, it wins.
- If ISO is absent or unusable, parsed XMP is used as fallback.
- The effective `gain_map_metadata` field reflects that precedence.

#### Public documentation requirement

The public docs should state this explicitly.

The precedence must not remain an undocumented implementation detail.

#### Provenance exposure

For now, keep the public type conservative:

- retain:
  - `xmp`,
  - `iso_21496_1`,
  - `gain_map_metadata`.

Do not yet add:

- `parsed_from_xmp`,
- `parsed_from_iso_21496_1`.

Those can be revisited later if a real consumer needs explicit provenance.

### Compliance-Oriented Behavior To Keep

The following current behaviors are useful and should remain unless they create
clear API or policy problems:

1. Emitting both Ultra HDR XMP and ISO 21496-1 metadata when packaging gain
   maps.
2. Defaulting to single-channel gain maps unless the caller explicitly requests
   multichannel.
3. Preserving an existing primary JPEG ICC profile in the compat path.
4. Treating built-in Display-P3 ICC helpers as a convenience, not as a hidden
   global policy engine.

### Implementation Phases

#### Phase 1: Metadata precedence

##### Changes

1. Change Ultra HDR metadata parsing to prefer ISO 21496-1 over XMP.
2. Add tests for:
   - XMP-only inputs,
   - ISO-only inputs,
   - both present and identical,
   - both present and conflicting, with ISO winning.
3. Update rustdoc and README to state the precedence clearly.
4. Update the changelog.

##### Risk

Low.

This is self-contained and does not require a public API expansion.

#### Phase 2: Shared gain-map computation helper

##### Changes

1. Extract the compat encoder's internal gain-map computation into a reusable
   internal helper.
2. Add the public `compute_gain_map(...)` API.
3. Add `GainMapChannels` and `ComputeGainMapOptions`.
4. Default to single-channel computation.
5. Require explicit opt-in for multichannel computation.
6. Keep gain-map JPEG encoding policy outside this API.

##### Internal design

Internally, this should continue to use `ultrahdr_core::compute_gainmap(...)`,
with channel selection translated to the right `GainMapConfig`.

##### Tests

Add coverage for:

- successful compute + structured encode roundtrip,
- single-channel default behavior,
- explicit multichannel opt-in,
- interaction with existing `GainMapEncodeOptions`,
- compatibility with HDR reconstruction.

##### Risk

Moderate.

This is the most important change for consumers, but it is an API addition and
needs careful naming and tests.

#### Phase 3: Compat alignment

##### Changes

1. Reuse the same shared gain-map computation helper in the compat encoder.
2. Preserve the existing compat surface unless a specific behavior is being
   corrected intentionally.
3. Keep the compat ICC behavior aligned with the documented fallback policy.

##### Why

The compat path and the new public helper should not compute gain maps through
separate logic that can drift over time.

##### Tests

Ensure compat behavior remains covered for:

- single-channel default behavior,
- explicit ICC preservation,
- Display-P3 ICC fallback behavior,
- decoded packed HDR reconstruction.

#### Phase 4: Optional convenience wrapper

##### Changes

1. Add `encode_ultra_hdr(...)` only if it stays thin.
2. Document that it expects the caller to have already chosen the SDR primary
   image and profile policy.
3. Ensure it is implemented in terms of:
   - `compute_gain_map(...)`,
   - and the existing structured encode path.

##### Risk

Low to moderate.

The risk is not technical complexity; it is accidentally baking consumer policy
into a convenience API.

### Migration Expectations

The plan is intended to be additive at first.

#### Existing high-level API

- `encode(...)` stays.
- `UltraJpegEncoder` stays.
- `EncodeOptions` stays.
- `GainMapEncodeOptions` stays.

#### Existing compat API

- compat stays supported,
- but the new shared helper becomes the preferred non-compat seam for
  policy-aware consumers.

#### Future cleanup

Once the new helper is in place and adopted, we can later revisit whether some
spec-oriented convenience behavior currently living in compat should move into a
more explicit public Ultra HDR API layer.

### Documentation Requirements

When the work lands, the public docs should clearly explain:

1. what `compute_gain_map(...)` does,
2. that it defaults to single-channel gain maps,
3. that multichannel is explicit opt-in,
4. that the consumer still owns primary-image color policy,
5. that decode prefers ISO over XMP,
6. how to combine:
   - `compute_gain_map(...)`,
   - `ColorMetadata`,
   - `EncodeOptions`,
   - and `encode(...)`.

The README should include at least one example showing:

- consumer-chosen primary image,
- explicit primary color metadata,
- computed gain map,
- final packaging through structured encode.

### Acceptance Criteria

This plan is complete when all of the following are true:

1. ISO 21496-1 is preferred over XMP when both are present.
2. That precedence is documented and tested.
3. `ultrajpeg` exposes a public gain-map computation helper.
4. The helper defaults to single-channel gain maps.
5. Multichannel gain maps require explicit opt-in.
6. The helper composes cleanly with `EncodeOptions` and
   `GainMapEncodeOptions`.
7. Compat reuses the same internal computation path.
8. The relevant behavior is covered by integration tests and rustdoc examples.

### Notes

The central design principle is:

Expose the right seam.

`ultrajpeg` should not absorb the consumer's output policy, but it should
expose the gain-map computation and packaging seam cleanly enough that a
policy-aware consumer no longer has to route through the compat encoder just to
get the right mechanics.
