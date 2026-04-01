# Stable API Maintenance Guide

## Status

The stable native API refactor described in the planning documents is
implemented for `0.5.0`.

This file is the maintainer-facing summary of the implemented public contract,
the intended invariants behind it, and the rules future changes should follow.

Historical design material remains in:

- `docs/maintainer/implementation-plan.md`
- `docs/maintainer/stable-api-contract.md`

The user-facing migration material lives in:

- `docs/user/migration-0.5.md`

## Public API Shape

The public API is intentionally centered on one native surface at the crate
root.

Functions:

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

Root types:

- `Image`
- `PixelFormat`
- `ColorGamut`
- `ColorTransfer`
- `GainMap`
- `GainMapMetadata`
- `HdrOutputFormat`
- `Error`
- `Result`
- `ChromaSubsampling`
- `Chromaticity`
- `GamutInfo`
- `ColorMetadata`
- `ParsedGainMapXmp`
- `ContainerKind`
- `CodestreamLayout`
- `ContainerLayout`
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
- `EncodeOptions`
- `UltraHdrEncodeOptions`
- `Encoder`

Public module:

- `icc`

Everything else is intended to remain private implementation detail unless a
clear public use case requires exposure.

## Additional Public Contracts

The issue-`#4` additions follow the same stable API rules rather than opening a
second low-level surface.

- `parse_gain_map_xmp(...)` and `parse_iso_21496_1(...)` are intentionally raw.
  They must not silently apply the crate's decode-time precedence or defensive
  recovery rules.
- `inspect_container_layout(...)` is structural inspection only. It should
  expose codestream boundaries and primary/gain-map indices without becoming a
  generic JPEG surgery API by accident.
- `prepare_sdr_primary(...)` is the supported high-level bridge for
  caller-managed HDR workflows. It must return matching pixels and
  `PrimaryMetadata`, and the two should continue to be treated as a pair.

## Naming Rules

The stable naming direction is:

- noun pairs should line up with the top-level verbs:
  - `decode` / `DecodedImage`
  - `inspect` / `Inspection`
  - `encode` / `Encoder`
- the root image type is always `Image`
- `PrimaryMetadata` is the physical metadata attached to the primary JPEG
- `UltraHdrMetadata` is the effective gain-map-oriented metadata recovered from
  the container
- `GainMapBundle` is an encode-time payload object, not a policy object

Do not reintroduce wrapper-era or backend-era naming into the root API.

## Ownership And Allocation Rules

The stable allocation policy is part of the API contract.

- `inspect(...)` must not decode image pixels.
- `decode(...)` must not retain JPEG codestream bytes by default.
- `decode_with_options(...)` is the only public path for retained codestream
  bytes.
- `DecodedImage::primary_jpeg` and `DecodedGainMap::jpeg_bytes` must stay
  `Option<Vec<u8>>`, not always-filled buffers.
- Prefer borrowing and one-pass parsing internally when public behavior does
  not require ownership.

Future optimizations should preserve these semantics.

## Metadata Model Rules

The split between `PrimaryMetadata` and `UltraHdrMetadata` is intentional.

`PrimaryMetadata` covers:

- `ColorMetadata`
- EXIF payload

`UltraHdrMetadata` covers:

- effective XMP payload
- effective ISO 21496-1 payload
- parsed effective gain-map metadata
- provenance for those payloads

Do not move EXIF back into `ColorMetadata`.

Raw payload parsing is intentionally separate from `UltraHdrMetadata` recovery:

- `UltraHdrMetadata` remains the crate's effective decoded view
- the raw parse helpers remain the escape hatch for explicit validation or
  comparison workflows

## Color Model Rules

`ColorMetadata` has both:

- `gamut`: convenience named classification
- `gamut_info`: authoritative structured representation

`gamut_info` should remain the richer representation.

`gamut` should continue to be a convenience layer derived from explicit
signaling or ICC-backed classification when possible.

The crate may add richer color helpers later, but it should not collapse
structured gamut information back down to an enum-only model.

`prepare_sdr_primary(...)` is the current high-level color-policy helper. Any
future color helpers should remain consistent with it instead of inventing a
parallel convenience story.

## Numeric Robustness Rules

Public numeric inputs that drive HDR or SDR math are part of the supported
contract and must be validated explicitly.

- `prepare_sdr_primary(...)` must reject non-finite peak values.
- peak or boost values must be finite and positive, not silently normalized
  from `NaN`, `inf`, or negative inputs.
- `DecodedImage::reconstruct_hdr(...)` must validate the effective gain-map
  metadata before applying logarithmic or multiplicative math.
- non-finite or structurally invalid reconstruction metadata is caller error
  and should fail with `Error::InvalidInput`.

Output handling follows a different rule:

- bounded integer output paths may sanitize non-finite intermediates before
  packing bytes
- those saturation semantics must stay consistent across scalar and SIMD paths
- linear-float HDR outputs should not be clamped merely to hide invalid math

SIMD policy follows from that:

- one-shot API validation remains scalar
- hot per-pixel paths should keep robustness checks SIMD-friendly when
  practical
- do not add scalar-only hot-loop validation if the same behavior can be
  expressed with masks and blends without changing semantics

## Ultra HDR Recovery Rules

Current behavior is intentionally pragmatic:

- effective metadata may come from the primary JPEG or gain-map JPEG
- ISO 21496-1 takes precedence over XMP when both are valid
- malformed-but-usable files may still decode as Ultra HDR when MPF points to a
  valid secondary gain-map JPEG that carries usable gain-map metadata

This recovery behavior is user-visible. Changes here need:

- explicit tests
- changelog notes
- migration notes when externally observable

Do not make the raw metadata parsers mirror these recovery rules. They exist so
callers can inspect inconsistent raw payloads directly.

## Encoding Rules

Gain-map packaging currently guarantees:

- MPF-bundled output
- container or directory XMP on the primary JPEG
- `hdrgm:*` XMP on the gain-map JPEG
- ISO 21496-1 on the gain-map JPEG

Primary ICC behavior is also part of the contract:

- keep caller-provided primary ICC as-is
- if the caller is packaging a gain map and the resolved primary color is
  Display-P3 plus sRGB with no explicit ICC, inject the bundled Display-P3 ICC
- otherwise fail rather than guessing

`prepare_sdr_primary(...)` inherits those rules by returning matching
`PrimaryMetadata`. For Display-P3 output it should continue to return bundled
Display-P3 ICC metadata automatically.

## Documentation Rules

The crate now builds with `#![deny(missing_docs)]`.

That enforcement is necessary but not sufficient. Maintainers should treat
documentation updates as part of any public-API change.

In particular, rustdoc on the public item itself must explicitly call out any
non-obvious user-visible behavior, including:

- fallback and precedence rules
- automatic metadata injection
- validation rules for numeric inputs
- sanitization behavior when packing bounded integer outputs
- retained-buffer or borrowing behavior
- structural recovery of malformed-but-usable inputs
- implicit clamping, flooring, or normalization
- parallelism or work-avoidance behavior when it affects expectations

Do not leave those details only in changelog entries, tests, implementation
comments, or maintainer-only docs.

When changing the public API:

1. Update rustdoc on the item and affected fields.
2. Update the crate-level docs in `README.md`.
3. Update `CHANGELOG.md`.
4. Update `docs/user/migration-0.5.md` or its successor when migration impact exists.
5. Update this guide if the stable maintenance rules changed.

## Benchmark Policy

The repository contains two benchmark tiers:

- the default benchmark suite, which must stay reasonable for normal local use
  and for bench-target smoke coverage under `cargo test --all-targets`
- large real-fixture benchmarks, which are for manual optimization work only

Large real-fixture benchmarks must not be part of CI or git-hook default
execution.

The current opt-in switch is:

- `ULTRAJPEG_BENCH_REAL_FIXTURES=1`

In particular, the large `reconstruct_hdr` fixture cases in
`benches/typical.rs` are only enabled when that environment variable is set.

Use that mode only when explicitly investigating performance on real large
inputs. When doing so, save a Criterion baseline first so comparisons stay
meaningful.

## Versioning Guidance Before 1.0

The crate is still pre-`1.0`, so API evolution is allowed, but it should be
disciplined.

Prefer:

- renames only when they materially improve coherence
- explicit migration examples
- additive metadata fields when possible
- behavior changes that reduce hidden work or ambiguity

Avoid:

- reintroducing compatibility wrappers at the root
- backend-specific public policy types unless they are unavoidable
- convenience APIs that hide important color or metadata policy
