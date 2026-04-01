## Ultra HDR Metadata Behavior

`UltraHdrMetadata` exposes the effective metadata used by the crate after
fallback and recovery logic.

It includes:

- `xmp` and `xmp_location`
- `iso_21496_1` and `iso_21496_1_location`
- `gain_map_metadata`
- `gain_map_metadata_source`

Important behavior:

- ISO 21496-1 is preferred over XMP when both are present and valid
- metadata may come from the primary JPEG or the gain-map JPEG
- the crate can recover malformed-but-usable files where the primary JPEG
  metadata is incomplete but MPF still points to a gain-map JPEG that carries
  usable gain-map semantics

### What Gets Written On Encode

When `EncodeOptions::gain_map` is `Some(...)`, the crate writes:

- MPF directory metadata on the primary JPEG
- container or directory XMP on the primary JPEG
- a version-only ISO 21496-1 APP2 block on the primary JPEG
- `hdrgm:*` XMP on the gain-map JPEG
- canonical ISO 21496-1 gain-map metadata on the gain-map JPEG

### What Gets Resolved On Decode

On decode and inspect, the crate resolves the effective gain-map metadata from
the available payloads and exposes where those payloads came from.

That means callers do not need to parse raw XMP or raw ISO 21496-1 bytes
themselves unless they want to.

When callers do want to reason about the raw payloads directly, the crate also
provides:

- `parse_gain_map_xmp(...)`
- `parse_iso_21496_1(...)`

Those entry points are intentionally raw:

- they do not apply decode-time precedence
- they do not apply the crate's defensive fallback filters
- they are meant for explicit validation and comparison workflows

`parse_iso_21496_1(...)` expects one gain-map ISO payload. The primary
JPEG's four-byte version-only ISO APP2 block is structural only and returns an
error if passed to that raw parser directly.

## Container Structure Inspection

`inspect_container_layout(...)` exposes:

- codestream offsets and lengths
- whether the input was recognized as MPF or only as concatenated JPEG
  codestreams
- which codestreams the crate treats as the primary and gain-map JPEG payloads

This surface is intentionally structural and inspection-oriented. It does not
yet expose a generic public MPF rewrite API.

## SDR Primary Preparation

`prepare_sdr_primary(...)` is the supported high-level bridge for workflows
where the caller:

- starts from HDR pixels
- resizes, crops, or otherwise edits those pixels first
- then needs an SDR primary image before calling `compute_gain_map(...)`

The helper returns both:

- the prepared `Rgb8` primary image
- matching `PrimaryMetadata`

That metadata should be used together with the returned image on subsequent
`encode(...)` calls.

The current helper:

- supports `Rgb8`, `Rgba8`, `Rgba16F`, `Rgba32F`, `Rgba1010102Pq`, and
  `Rgba1010102Hlg` inputs
- produces sRGB-transfer output in either BT.709 or Display-P3 gamut
- floors the SDR primary brightness so the default `compute_gain_map(...)`
  path stays within the crate's default gain-map boost envelope
- injects bundled Display-P3 ICC metadata automatically when Display-P3 output
  is requested

### Policy Notes For `prepare_sdr_primary(...)`

`prepare_sdr_primary(...)` is a supported default policy, not a promise to
reproduce every caller's preferred SDR rendering intent.

In practice:

- if you already have a caller-chosen SDR primary image, keep using that image
  with `compute_gain_map(...)` and `encode(...)`
- if you want `ultrajpeg` to derive a reasonable SDR primary for a transformed
  HDR image, use `prepare_sdr_primary(...)`
- if you know the source HDR peak more precisely, set
  `PreparePrimaryOptions::source_peak_nits` explicitly instead of relying on
  the transfer-based default

Current defaults are:

- PQ input with `source_peak_nits: None` assumes `10000` nits
- HLG input with `source_peak_nits: None` assumes `1000` nits
- linear input with `source_peak_nits: None` assumes `1000` nits
- sRGB input with `source_peak_nits: None` assumes `203` nits

The helper also enforces a small but important compatibility rule: it floors
the derived SDR primary brightness so the returned image composes with the
crate's default `compute_gain_map(...)` configuration instead of immediately
falling outside the default gain-map boost range.

That makes the default workflow easier to use, but it also means the output is
not just a naive one-pass tone map. If you need exact custom SDR rendering
intent, prepare the SDR primary image yourself and treat `prepare_sdr_primary`
as the optional convenience path rather than as the only supported one.

## Ownership And Performance Semantics

The public API is explicit about allocation behavior:

- `inspect(...)` does not decode pixels
- `decode(...)` decodes pixels and retains no raw JPEG codestreams by default
- `decode_with_options(...)` is the explicit escape hatch for retained JPEG
  codestream bytes
- large Ultra HDR decodes may use internal Rayon-based parallelism, but the
  public API remains synchronous

The crate is designed so that callers do not accidentally retain large input
codestreams unless they opt in.
