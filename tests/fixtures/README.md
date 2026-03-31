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

Reconstruction check
- reconstructed format: Rgba1010102Pq

## Upstream Fixtures

`upstream/ultra-hdr-samples/`
- source: `MishaalRahmanGH/Ultra_HDR_Samples`
- license: CC BY 4.0
- purpose: real-world Ultra HDR decode and metadata inspection coverage
