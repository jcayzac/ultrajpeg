# Fixture Vectors

This directory contains two kinds of committed test data:

- small synthetic fixtures generated for stable, compact roundtrip coverage
- attributed upstream fixtures under `upstream/` for real-world decode coverage

`plain-sdr.jpg`
- primary dimensions: 4x4
- gain map: false
- icc bytes: 19

`plain-sdr-compat.jpg`
- primary dimensions: 4x4
- gain map: false
- icc bytes: 19

`sample-ultrahdr.jpg`
- primary dimensions: 4x4
- gain map: true
- xmp: true
- iso21496-1: true

`sample-ultrahdr-compat.jpg`
- primary dimensions: 4x4
- gain map: true
- xmp: true
- iso21496-1: true

Synthetic-fixture policy
- `sample-ultrahdr.jpg` and `sample-ultrahdr-compat.jpg` are compact synthetic
  regression fixtures only.
- They are acceptable for small deterministic tests, examples, and smoke
  benchmarks.
- They must not be treated as trusted interop/reference fixtures.
- They must not drive visual-quality reports or conclusions about real-world
  Ultra HDR behavior.

Reconstruction check
- reconstructed format: Rgba1010102Pq

## Upstream Fixtures

`upstream/hdr-jpeg-iso-21496-1/`
- source: copied from the attributed fixture set used by `ultrashiny-cli`
- files: `original.jpg`, `ATTRIBUTION.md`
- purpose: large real ISO 21496-1 / Ultra HDR decode and reconstruction coverage

`upstream/hdr-jpeg-mishaal/`
- source: `MishaalRahmanGH/Ultra_HDR_Samples`
- files: `original.jpg`, `ATTRIBUTION.md`
- purpose: canonical trusted real-world Ultra HDR fixture for decode and
  metadata inspection coverage

`upstream/hdr-jpeg-apple-gain-map/`
- source: copied from the attributed fixture set used by `ultrashiny-cli`
- files: `original.jpg`, `ATTRIBUTION.md`
- purpose: large real Apple gain-map JPEG decode coverage
