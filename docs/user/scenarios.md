## Major Scenarios

### Plain JPEG Encode

```rust
# use ultrajpeg::{ColorGamut, ColorTransfer, EncodeOptions, Image, PixelFormat};
let image = Image::from_data(
    2,
    2,
    PixelFormat::Rgb8,
    ColorGamut::DisplayP3,
    ColorTransfer::Srgb,
    vec![
        255, 0, 0, 0, 255, 0,
        0, 0, 255, 255, 255, 255,
    ],
)?;

let jpeg = ultrajpeg::encode(&image, &EncodeOptions::default())?;
# let _ = jpeg;
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Plain JPEG Decode

```rust
# use ultrajpeg::decode;
# let bytes = include_bytes!("../../tests/fixtures/plain-sdr.jpg");
let decoded = decode(bytes)?;

assert_eq!(decoded.image.width, 4);
assert!(decoded.gain_map.is_none());
assert!(decoded.primary_jpeg.is_none());
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Inspect Metadata Without Decoding Pixels

```rust
# use ultrajpeg::inspect;
# let bytes = include_bytes!("../../tests/fixtures/sample-ultrahdr.jpg");
let inspection = inspect(bytes)?;

assert!(inspection.primary_jpeg_len > 0);
assert!(inspection.gain_map_jpeg_len.is_some());
assert!(inspection.primary_metadata.color.icc_profile.is_some());
assert!(inspection.ultra_hdr.is_some());
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Inspect Container Layout Without Decoding Pixels

```rust
# use ultrajpeg::{ContainerKind, inspect_container_layout};
# let bytes = include_bytes!("../../tests/fixtures/sample-ultrahdr.jpg");
let layout = inspect_container_layout(bytes)?;

assert_eq!(layout.kind, ContainerKind::Mpf);
assert_eq!(layout.primary_index, 0);
assert_eq!(layout.gain_map_index, Some(1));
assert_eq!(layout.codestreams.len(), 2);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Parse Raw Ultra HDR Payloads Explicitly

```rust
# use ultrajpeg::{inspect, parse_gain_map_xmp, parse_iso_21496_1};
# let bytes = include_bytes!("../../tests/fixtures/sample-ultrahdr.jpg");
let inspection = inspect(bytes)?;
let ultra_hdr = inspection.ultra_hdr.as_ref().unwrap();

let parsed_xmp = parse_gain_map_xmp(ultra_hdr.xmp.as_deref().unwrap())?;
let parsed_iso = parse_iso_21496_1(ultra_hdr.iso_21496_1.as_deref().unwrap())?;

assert!(parsed_xmp.metadata.hdr_capacity_max >= 4.0);
assert!(parsed_iso.hdr_capacity_max >= 4.0);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Encode An Ultra HDR JPEG From An Existing Gain Map

```rust
# use ultrahdr_core::GainMapMetadata;
# use ultrajpeg::{
#     ColorGamut, ColorTransfer, CompressionEffort, EncodeOptions, GainMapBundle, Image,
#     PixelFormat,
# };
let primary = Image::from_data(
    2,
    2,
    PixelFormat::Rgb8,
    ColorGamut::DisplayP3,
    ColorTransfer::Srgb,
    vec![
        255, 0, 0, 0, 255, 0,
        0, 0, 255, 255, 255, 255,
    ],
)?;
let gain_map = Image::from_data(
    2,
    2,
    PixelFormat::Gray8,
    ColorGamut::Bt709,
    ColorTransfer::Linear,
    vec![0, 64, 128, 255],
)?;

let jpeg = ultrajpeg::encode(
    &primary,
    &EncodeOptions {
        gain_map: Some(GainMapBundle {
            image: gain_map,
            metadata: GainMapMetadata::new(),
            quality: 85,
            progressive: false,
            compression: CompressionEffort::Balanced,
        }),
        ..EncodeOptions::ultra_hdr_defaults()
    },
)?;
# let _ = jpeg;
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Compute A Gain Map First, Then Bundle It

```rust
# use ultrajpeg::{
#     ColorGamut, ColorTransfer, CompressionEffort, ComputeGainMapOptions, EncodeOptions,
#     Image, PixelFormat,
# };
let hdr = Image::from_data(
    1,
    1,
    PixelFormat::Rgba32F,
    ColorGamut::DisplayP3,
    ColorTransfer::Linear,
    [1.5f32, 0.5, 0.5, 1.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect(),
)?;
let primary = Image::from_data(
    1,
    1,
    PixelFormat::Rgb8,
    ColorGamut::DisplayP3,
    ColorTransfer::Srgb,
    vec![255, 128, 128],
)?;

let computed = ultrajpeg::compute_gain_map(&hdr, &primary, &ComputeGainMapOptions::default())?;
let jpeg = ultrajpeg::encode(
    &primary,
    &EncodeOptions {
        gain_map: Some(computed.into_bundle(90, false, CompressionEffort::Balanced)),
        ..EncodeOptions::ultra_hdr_defaults()
    },
)?;
# let _ = jpeg;
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Prepare An SDR Primary Image From HDR Pixels

```rust
# use ultrajpeg::{
#     ColorGamut, ColorTransfer, CompressionEffort, EncodeOptions, Image, PixelFormat,
#     PreparePrimaryOptions, compute_gain_map, prepare_sdr_primary,
# };
let hdr = Image::from_data(
    1,
    1,
    PixelFormat::Rgba32F,
    ColorGamut::DisplayP3,
    ColorTransfer::Linear,
    [1.5f32, 0.5, 0.5, 1.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect(),
)?;

let prepared = prepare_sdr_primary(&hdr, &PreparePrimaryOptions::ultra_hdr_defaults())?;
let computed = compute_gain_map(&hdr, &prepared.image, &Default::default())?;
let jpeg = ultrajpeg::encode(
    &prepared.image,
    &EncodeOptions {
        primary_metadata: prepared.metadata.clone(),
        gain_map: Some(computed.into_bundle(90, false, CompressionEffort::Balanced)),
        ..EncodeOptions::default()
    },
)?;
# let _ = jpeg;
# Ok::<(), Box<dyn std::error::Error>>(())
```

If the source HDR peak is known more precisely, pass it explicitly:

```rust
# use ultrajpeg::{PreparePrimaryOptions, prepare_sdr_primary};
# use ultrajpeg::{ColorGamut, ColorTransfer, Image, PixelFormat};
# let hdr = Image::from_data(
#     1,
#     1,
#     PixelFormat::Rgba32F,
#     ColorGamut::DisplayP3,
#     ColorTransfer::Linear,
#     [1.5f32, 0.5, 0.5, 1.0]
#         .into_iter()
#         .flat_map(f32::to_le_bytes)
#         .collect(),
# )?;
let prepared = prepare_sdr_primary(
    &hdr,
    &PreparePrimaryOptions {
        source_peak_nits: Some(4000.0),
        ..PreparePrimaryOptions::ultra_hdr_defaults()
    },
)?;
# let _ = prepared;
# Ok::<(), Box<dyn std::error::Error>>(())
```

### One-Shot Ultra HDR Packaging

```rust
# use ultrajpeg::{
#     ColorGamut, ColorTransfer, Image, PixelFormat, UltraHdrEncodeOptions,
#     encode_ultra_hdr,
# };
let hdr = Image::from_data(
    1,
    1,
    PixelFormat::Rgba32F,
    ColorGamut::DisplayP3,
    ColorTransfer::Linear,
    [1.5f32, 0.5, 0.5, 1.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect(),
)?;
let primary = Image::from_data(
    1,
    1,
    PixelFormat::Rgb8,
    ColorGamut::DisplayP3,
    ColorTransfer::Srgb,
    vec![255, 128, 128],
)?;

let jpeg = encode_ultra_hdr(&hdr, &primary, &UltraHdrEncodeOptions::default())?;
# let _ = jpeg;
# Ok::<(), Box<dyn std::error::Error>>(())
```

For both `EncodeOptions` and `UltraHdrEncodeOptions`, the `progressive` flags
select scan mode only. Use the matching `CompressionEffort` field when you
want to choose between balanced and size-oriented encoding.

### Decode And Reconstruct HDR Output

```rust
# use ultrahdr_core::gainmap::HdrOutputFormat;
# use ultrajpeg::decode;
# let bytes = include_bytes!("../../tests/fixtures/sample-ultrahdr.jpg");
let decoded = decode(bytes)?;
let hdr = decoded.reconstruct_hdr(4.0, HdrOutputFormat::LinearFloat)?;

assert!(decoded.gain_map.is_some());
assert_eq!(hdr.width, decoded.image.width);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Retain Raw JPEG Codestreams Explicitly

```rust
# use ultrajpeg::{DecodeOptions, decode_with_options};
# let bytes = include_bytes!("../../tests/fixtures/sample-ultrahdr.jpg");
let decoded = decode_with_options(
    bytes,
    DecodeOptions {
        retain_primary_jpeg: true,
        retain_gain_map_jpeg: true,
        ..DecodeOptions::default()
    },
)?;

assert!(decoded.primary_jpeg.is_some());
assert!(decoded.gain_map.as_ref().unwrap().jpeg_bytes.is_some());
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Inspect Metadata Provenance

```rust
# use ultrajpeg::{inspect, MetadataLocation};
# let bytes = include_bytes!("../../tests/fixtures/sample-ultrahdr.jpg");
let inspection = inspect(bytes)?;
let ultra_hdr = inspection.ultra_hdr.as_ref().unwrap();

assert!(matches!(
    ultra_hdr.xmp_location,
    Some(MetadataLocation::Primary | MetadataLocation::GainMap)
));
# Ok::<(), Box<dyn std::error::Error>>(())
```
