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
- `hdrgm:*` XMP on the gain-map JPEG
- ISO 21496-1 metadata on the gain-map JPEG

### What Gets Resolved On Decode

On decode and inspect, the crate resolves the effective gain-map metadata from
the available payloads and exposes where those payloads came from.

That means callers do not need to parse raw XMP or raw ISO 21496-1 bytes
themselves unless they want to.

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
