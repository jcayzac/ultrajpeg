use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use ultrahdr_core::{ColorGamut, ColorTransfer, GainMapMetadata, PixelFormat, RawImage};
use ultrajpeg::{CompressedImage, EncodeOptions, GainMapEncodeOptions, decode, inspect, jpeg, sys};

const PLAIN_SDR: &[u8] = include_bytes!("../tests/fixtures/plain-sdr.jpg");
const SAMPLE_ULTRAHDR: &[u8] = include_bytes!("../tests/fixtures/sample-ultrahdr.jpg");

fn typical_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("inspect");
    for (name, bytes) in [("plain", PLAIN_SDR), ("ultrahdr", SAMPLE_ULTRAHDR)] {
        group.throughput(Throughput::Bytes(bytes.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), &bytes, |b, bytes| {
            b.iter(|| inspect(black_box(bytes)).unwrap());
        });
    }
    group.finish();

    let mut group = c.benchmark_group("decode");
    for (name, bytes) in [("plain", PLAIN_SDR), ("ultrahdr", SAMPLE_ULTRAHDR)] {
        group.throughput(Throughput::Bytes(bytes.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), &bytes, |b, bytes| {
            b.iter(|| decode(black_box(bytes)).unwrap());
        });
    }
    group.finish();

    let primary = sample_primary();
    let gain_map = sample_gain_map();
    let gain_map_metadata = sample_gain_map_metadata();

    let mut group = c.benchmark_group("encode");
    group.bench_function("plain", |b| {
        let options = EncodeOptions::default();
        b.iter(|| ultrajpeg::encode(black_box(&primary), black_box(&options)).unwrap());
    });
    group.bench_function("ultrahdr", |b| {
        let options = EncodeOptions {
            gain_map: Some(GainMapEncodeOptions {
                image: gain_map.clone(),
                metadata: gain_map_metadata.clone(),
                quality: 80,
                progressive: false,
            }),
            ..EncodeOptions::default()
        };
        b.iter(|| ultrajpeg::encode(black_box(&primary), black_box(&options)).unwrap());
    });
    group.finish();

    let mut group = c.benchmark_group("compat");
    group.bench_function("gainmap_metadata", |b| {
        b.iter_batched(
            || {
                CompressedImage::from_vec(
                    SAMPLE_ULTRAHDR.to_vec(),
                    sys::uhdr_color_gamut::UHDR_CG_UNSPECIFIED,
                    sys::uhdr_color_transfer::UHDR_CT_UNSPECIFIED,
                    sys::uhdr_color_range::UHDR_CR_UNSPECIFIED,
                )
            },
            |mut compressed| {
                let mut decoder = ultrajpeg::Decoder::new().unwrap();
                decoder.set_image(&mut compressed).unwrap();
                black_box(decoder.gainmap_metadata().unwrap());
            },
            criterion::BatchSize::SmallInput,
        );
    });
    group.bench_function("jpeg_encode_rgb", |b| {
        let encoder = jpeg::Encoder::new(jpeg::Preset::ProgressiveSmallest).quality(90);
        b.iter(|| {
            encoder
                .encode_rgb(
                    black_box(primary.data.as_slice()),
                    primary.width,
                    primary.height,
                )
                .unwrap()
        });
    });
    group.finish();
}

fn sample_primary() -> RawImage {
    RawImage::from_data(
        4,
        4,
        PixelFormat::Rgb8,
        ColorGamut::DisplayP3,
        ColorTransfer::Srgb,
        vec![
            255, 0, 0, 255, 128, 0, 255, 255, 0, 255, 255, 255, //
            0, 255, 0, 0, 128, 255, 0, 255, 255, 255, 0, 255, //
            0, 0, 255, 32, 64, 255, 96, 160, 255, 200, 240, 255, //
            16, 16, 16, 64, 64, 64, 160, 160, 160, 240, 240, 240, //
        ],
    )
    .expect("sample primary image")
}

fn sample_gain_map() -> RawImage {
    RawImage::from_data(
        4,
        4,
        PixelFormat::Gray8,
        ColorGamut::Bt709,
        ColorTransfer::Linear,
        vec![
            0, 32, 64, 96, //
            32, 64, 96, 128, //
            64, 96, 128, 160, //
            96, 128, 160, 192, //
        ],
    )
    .expect("sample gain map image")
}

fn sample_gain_map_metadata() -> GainMapMetadata {
    GainMapMetadata {
        max_content_boost: [4.0; 3],
        min_content_boost: [1.0; 3],
        gamma: [1.0; 3],
        offset_sdr: [1.0 / 64.0; 3],
        offset_hdr: [1.0 / 64.0; 3],
        hdr_capacity_min: 1.0,
        hdr_capacity_max: 4.0,
        use_base_color_space: true,
    }
}

criterion_group!(benches, typical_benches);
criterion_main!(benches);
