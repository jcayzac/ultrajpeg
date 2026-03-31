# Stable API Maintenance Guide

## Status

The stable native API refactor described in the planning documents was
implemented in `0.5.0-rc1`.

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
- `decode`
- `decode_with_options`
- `encode`
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

Public module:

- `icc`

Everything else is intended to remain private implementation detail unless a
clear public use case requires exposure.

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

## Color Model Rules

`ColorMetadata` has both:

- `gamut`: convenience named classification
- `gamut_info`: authoritative structured representation

`gamut_info` should remain the richer representation.

`gamut` should continue to be a convenience layer derived from explicit
signaling or ICC-backed classification when possible.

The crate may add richer color helpers later, but it should not collapse
structured gamut information back down to an enum-only model.

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

## Documentation Rules

The crate now builds with `#![deny(missing_docs)]`.

That enforcement is necessary but not sufficient. Maintainers should treat
documentation updates as part of any public-API change.

When changing the public API:

1. Update rustdoc on the item and affected fields.
2. Update the crate-level docs in `README.md`.
3. Update `CHANGELOG.md`.
4. Update `docs/user/migration-0.5.md` or its successor when migration impact exists.
5. Update this guide if the stable maintenance rules changed.

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
