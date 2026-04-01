## Limitations And Non-Goals

The crate deliberately does not:

- choose an SDR primary image for you implicitly during encode
- downscale, filter, or otherwise reshape gain maps automatically
- synthesize arbitrary ICC profiles
- infer complete color policy from partial hints
- act as a full conformance validator for every malformed Ultra HDR file shape

The caller remains responsible for:

- selecting the SDR primary image, unless it explicitly uses
  `prepare_sdr_primary(...)`
- deciding EXIF policy
- providing explicit ICC data when the primary image is not Display-P3 plus
  sRGB and a gain map is being bundled
- choosing the desired HDR reconstruction output format and display boost

Current limitations:

- the public API targets JPEG and MPF-bundled gain-map JPEG workflows
- Ultra HDR decode can recover some malformed files, but recovery is pragmatic,
  not a guarantee of full conformance validation
- `inspect_container_layout(...)` is structural inspection only; it is not yet a
  public generic MPF rewrite API
- the crate re-encodes JPEG pixel data on encode; it is not a marker-only
  remuxer for arbitrary already-encoded primary and gain-map codestream pairs
- `CompressionEffort::Smallest` currently provides an extra size-oriented
  backend path only for progressive JPEGs; sequential JPEGs still accept it for
  API consistency, but currently use the same effective backend settings as
  `CompressionEffort::Balanced`
- ICC parsing is currently used to recover structured gamut information, not to
  expose a full public ICC inspection API
