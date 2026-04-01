use std::{sync::LazyLock, time::Duration};

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use ultrahdr_core::{ColorGamut, ColorTransfer, GainMapMetadata, PixelFormat, RawImage};
use ultrajpeg::{
    CompressionEffort, DecodeOptions, EncodeOptions, GainMapBundle, compute_gain_map, decode,
    decode_with_options, encode_ultra_hdr, inspect,
};

const PLAIN_SDR: &[u8] = include_bytes!("../tests/fixtures/plain-sdr.jpg");
const SAMPLE_ULTRAHDR: &[u8] = include_bytes!("../tests/fixtures/sample-ultrahdr.jpg");
const LARGE_WIDTH: u32 = 2048;
const LARGE_HEIGHT: u32 = 1536;

static LARGE_CORPUS: LazyLock<BenchmarkCorpus> = LazyLock::new(build_large_corpus);

struct BenchmarkCorpus {
    plain: Vec<u8>,
    ultrahdr: Vec<u8>,
    hdr: RawImage,
    primary: RawImage,
}

fn typical_benches(c: &mut Criterion) {
    fixture_benches(c);
    realistic_benches(c, &LARGE_CORPUS);
}

fn fixture_benches(c: &mut Criterion) {
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

    let primary = sample_primary(4, 4);
    let gain_map = sample_gain_map(4, 4);
    let gain_map_metadata = sample_gain_map_metadata();
    let hdr = sample_hdr(4, 4);

    let mut group = c.benchmark_group("encode");
    group.bench_function("plain", |b| {
        let options = EncodeOptions::default();
        b.iter(|| ultrajpeg::encode(black_box(&primary), black_box(&options)).unwrap());
    });
    group.bench_function("ultrahdr", |b| {
        let options = EncodeOptions {
            gain_map: Some(GainMapBundle {
                image: gain_map.clone(),
                metadata: gain_map_metadata.clone(),
                quality: 80,
                progressive: false,
                compression: CompressionEffort::Balanced,
            }),
            ..EncodeOptions::ultra_hdr_defaults()
        };
        b.iter(|| ultrajpeg::encode(black_box(&primary), black_box(&options)).unwrap());
    });
    group.bench_function("compute_gain_map", |b| {
        b.iter(|| {
            compute_gain_map(
                black_box(&hdr),
                black_box(&primary),
                black_box(&Default::default()),
            )
            .unwrap()
        });
    });
    group.finish();
}

fn realistic_benches(c: &mut Criterion, corpus: &BenchmarkCorpus) {
    let mut group = c.benchmark_group("realistic");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(8));

    group.throughput(Throughput::Bytes(corpus.plain.len() as u64));
    group.bench_function("inspect_plain", |b| {
        b.iter(|| inspect(black_box(corpus.plain.as_slice())).unwrap());
    });
    group.bench_function("decode_plain", |b| {
        b.iter(|| decode(black_box(corpus.plain.as_slice())).unwrap());
    });

    group.throughput(Throughput::Bytes(corpus.ultrahdr.len() as u64));
    group.bench_function("inspect_ultrahdr", |b| {
        b.iter(|| inspect(black_box(corpus.ultrahdr.as_slice())).unwrap());
    });
    group.bench_function("decode_ultrahdr", |b| {
        b.iter(|| decode(black_box(corpus.ultrahdr.as_slice())).unwrap());
    });
    group.bench_function("decode_ultrahdr_skip_gain_map", |b| {
        b.iter(|| {
            decode_with_options(
                black_box(corpus.ultrahdr.as_slice()),
                black_box(DecodeOptions {
                    decode_gain_map: false,
                    ..DecodeOptions::default()
                }),
            )
            .unwrap()
        });
    });
    group.bench_function("encode_ultra_hdr", |b| {
        b.iter(|| {
            encode_ultra_hdr(
                black_box(&corpus.hdr),
                black_box(&corpus.primary),
                black_box(&Default::default()),
            )
            .unwrap()
        });
    });

    group.finish();
}

fn build_large_corpus() -> BenchmarkCorpus {
    let primary = sample_primary(LARGE_WIDTH, LARGE_HEIGHT);
    let gain_map = sample_gain_map(LARGE_WIDTH, LARGE_HEIGHT);
    let gain_map_metadata = sample_gain_map_metadata();
    let hdr = sample_hdr(LARGE_WIDTH, LARGE_HEIGHT);

    let plain = ultrajpeg::encode(&primary, &EncodeOptions::default()).expect("large plain encode");
    let ultrahdr = ultrajpeg::encode(
        &primary,
        &EncodeOptions {
            gain_map: Some(GainMapBundle {
                image: gain_map,
                metadata: gain_map_metadata,
                quality: 80,
                progressive: false,
                compression: CompressionEffort::Balanced,
            }),
            ..EncodeOptions::ultra_hdr_defaults()
        },
    )
    .expect("large ultrahdr encode");

    BenchmarkCorpus {
        plain,
        ultrahdr,
        hdr,
        primary,
    }
}

fn sample_primary(width: u32, height: u32) -> RawImage {
    let mut data = Vec::with_capacity(width as usize * height as usize * 3);
    for y in 0..height {
        for x in 0..width {
            data.push(((x * 255) / width.max(1)) as u8);
            data.push(((y * 255) / height.max(1)) as u8);
            data.push((((x ^ y) * 255) / width.max(height).max(1)) as u8);
        }
    }

    RawImage::from_data(
        width,
        height,
        PixelFormat::Rgb8,
        ColorGamut::DisplayP3,
        ColorTransfer::Srgb,
        data,
    )
    .unwrap()
}

fn sample_gain_map(width: u32, height: u32) -> RawImage {
    let mut data = Vec::with_capacity(width as usize * height as usize);
    for y in 0..height {
        for x in 0..width {
            data.push((((x + y) * 255) / (width + height).max(1)) as u8);
        }
    }

    RawImage::from_data(
        width,
        height,
        PixelFormat::Gray8,
        ColorGamut::Bt709,
        ColorTransfer::Linear,
        data,
    )
    .unwrap()
}

fn sample_hdr(width: u32, height: u32) -> RawImage {
    let mut data = Vec::with_capacity(width as usize * height as usize * 16);
    for y in 0..height {
        for x in 0..width {
            let r = 1.0 + x as f32 / width.max(1) as f32;
            let g = 1.0 + y as f32 / height.max(1) as f32;
            let b = 0.5 + (x ^ y) as f32 / width.max(height).max(1) as f32;
            for value in [r, g, b, 1.0] {
                data.extend_from_slice(&value.to_le_bytes());
            }
        }
    }

    RawImage::from_data(
        width,
        height,
        PixelFormat::Rgba32F,
        ColorGamut::DisplayP3,
        ColorTransfer::Linear,
        data,
    )
    .unwrap()
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

criterion_group!(typical, typical_benches);
criterion_main!(typical);
