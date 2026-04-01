use std::{sync::LazyLock, time::Duration};

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use half::f16;
use ultrahdr_core::{
    ColorGamut, ColorTransfer, GainMapMetadata, PixelFormat, RawImage,
    color::{hlg_oetf, pq_oetf},
};
use ultrajpeg::{
    CompressionEffort, DecodeOptions, DecodedImage, EncodeOptions, GainMapBundle, HdrOutputFormat,
    PreparePrimaryOptions, compute_gain_map, decode, decode_with_options, encode_ultra_hdr,
    inspect, prepare_sdr_primary,
};

const PLAIN_SDR: &[u8] = include_bytes!("../tests/fixtures/plain-sdr.jpg");
const SAMPLE_ULTRAHDR: &[u8] = include_bytes!("../tests/fixtures/sample-ultrahdr.jpg");
const LARGE_WIDTH: u32 = 2048;
const LARGE_HEIGHT: u32 = 1536;
const REAL_FIXTURE_BENCH_ENV: &str = "ULTRAJPEG_BENCH_REAL_FIXTURES";
const REAL_ULTRAHDR_ISO_21496_1_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/upstream/hdr-jpeg-iso-21496-1/original.jpg"
);

static LARGE_CORPUS: LazyLock<BenchmarkCorpus> = LazyLock::new(build_large_corpus);

struct BenchmarkCorpus {
    plain: Vec<u8>,
    ultrahdr: Vec<u8>,
    decoded_ultrahdr: DecodedImage,
    decoded_real_ultrahdr_iso: Option<DecodedImage>,
    hdr_rgba32f: RawImage,
    hdr_rgba16f: RawImage,
    hdr_pq: RawImage,
    hdr_hlg: RawImage,
    primary: RawImage,
}

fn typical_benches(c: &mut Criterion) {
    fixture_benches(c);
    realistic_benches(c, &LARGE_CORPUS);
    compute_gain_map_benches(c, &LARGE_CORPUS);
    reconstruct_hdr_benches(c, &LARGE_CORPUS);
    prepare_primary_benches(c, &LARGE_CORPUS);
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
    let mut group = c.benchmark_group("encode");
    group.bench_function("plain", |b| {
        let options = EncodeOptions::default();
        b.iter(|| black_box(ultrajpeg::encode(black_box(&primary), black_box(&options)).unwrap()));
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
        b.iter(|| black_box(ultrajpeg::encode(black_box(&primary), black_box(&options)).unwrap()));
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
        b.iter(|| black_box(inspect(black_box(corpus.plain.as_slice())).unwrap()));
    });
    group.bench_function("decode_plain", |b| {
        b.iter(|| black_box(decode(black_box(corpus.plain.as_slice())).unwrap()));
    });

    group.throughput(Throughput::Bytes(corpus.ultrahdr.len() as u64));
    group.bench_function("inspect_ultrahdr", |b| {
        b.iter(|| black_box(inspect(black_box(corpus.ultrahdr.as_slice())).unwrap()));
    });
    group.bench_function("decode_ultrahdr", |b| {
        b.iter(|| black_box(decode(black_box(corpus.ultrahdr.as_slice())).unwrap()));
    });
    group.bench_function("decode_ultrahdr_skip_gain_map", |b| {
        b.iter(|| {
            black_box(
                decode_with_options(
                    black_box(corpus.ultrahdr.as_slice()),
                    black_box(DecodeOptions {
                        decode_gain_map: false,
                        ..DecodeOptions::default()
                    }),
                )
                .unwrap(),
            )
        });
    });
    group.bench_function("encode_ultra_hdr", |b| {
        b.iter(|| {
            black_box(
                encode_ultra_hdr(
                    black_box(&corpus.hdr_rgba32f),
                    black_box(&corpus.primary),
                    black_box(&Default::default()),
                )
                .unwrap(),
            )
        });
    });

    group.finish();
}

fn compute_gain_map_benches(c: &mut Criterion, corpus: &BenchmarkCorpus) {
    let mut group = c.benchmark_group("compute_gain_map");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(8));
    group.throughput(Throughput::Elements(
        (corpus.hdr_rgba32f.width as u64) * (corpus.hdr_rgba32f.height as u64),
    ));

    let options = ultrajpeg::ComputeGainMapOptions::default();
    group.bench_function("rgba32f_realistic", |b| {
        b.iter(|| {
            black_box(
                compute_gain_map(
                    black_box(&corpus.hdr_rgba32f),
                    black_box(&corpus.primary),
                    black_box(&options),
                )
                .unwrap(),
            )
        });
    });

    group.finish();
}

fn prepare_primary_benches(c: &mut Criterion, corpus: &BenchmarkCorpus) {
    let mut group = c.benchmark_group("prepare_sdr_primary");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(15));

    let options = PreparePrimaryOptions::ultra_hdr_defaults();
    for (name, image) in [
        ("rgba32f", &corpus.hdr_rgba32f),
        ("rgba16f", &corpus.hdr_rgba16f),
        ("pq_1010102", &corpus.hdr_pq),
        ("hlg_1010102", &corpus.hdr_hlg),
    ] {
        group.throughput(Throughput::Elements(
            (image.width as u64) * (image.height as u64),
        ));
        group.bench_with_input(BenchmarkId::from_parameter(name), image, |b, image| {
            b.iter(|| {
                black_box(prepare_sdr_primary(black_box(image), black_box(&options)).unwrap())
            });
        });
    }

    group.finish();
}

fn reconstruct_hdr_benches(c: &mut Criterion, corpus: &BenchmarkCorpus) {
    let mut group = c.benchmark_group("reconstruct_hdr");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(20));
    group.throughput(Throughput::Elements(
        (corpus.decoded_ultrahdr.image.width as u64)
            * (corpus.decoded_ultrahdr.image.height as u64),
    ));

    group.bench_function("linear_float_synthetic", |b| {
        b.iter(|| {
            black_box(
                corpus
                    .decoded_ultrahdr
                    .reconstruct_hdr(black_box(4.0), black_box(HdrOutputFormat::LinearFloat))
                    .unwrap(),
            )
        });
    });

    group.bench_function("pq_1010102_synthetic", |b| {
        b.iter(|| {
            black_box(
                corpus
                    .decoded_ultrahdr
                    .reconstruct_hdr(black_box(4.0), black_box(HdrOutputFormat::Pq1010102))
                    .unwrap(),
            )
        });
    });

    if let Some(decoded_real_ultrahdr_iso) = corpus.decoded_real_ultrahdr_iso.as_ref() {
        group.throughput(Throughput::Elements(
            (decoded_real_ultrahdr_iso.image.width as u64)
                * (decoded_real_ultrahdr_iso.image.height as u64),
        ));

        group.bench_function("linear_float_iso_21496_1_fixture", |b| {
            b.iter(|| {
                black_box(
                    decoded_real_ultrahdr_iso
                        .reconstruct_hdr(black_box(4.0), black_box(HdrOutputFormat::LinearFloat))
                        .unwrap(),
                )
            });
        });

        group.bench_function("pq_1010102_iso_21496_1_fixture", |b| {
            b.iter(|| {
                black_box(
                    decoded_real_ultrahdr_iso
                        .reconstruct_hdr(black_box(4.0), black_box(HdrOutputFormat::Pq1010102))
                        .unwrap(),
                )
            });
        });
    }

    group.finish();
}

fn build_large_corpus() -> BenchmarkCorpus {
    let primary = sample_primary(LARGE_WIDTH, LARGE_HEIGHT);
    let gain_map = sample_gain_map(LARGE_WIDTH, LARGE_HEIGHT);
    let gain_map_metadata = sample_gain_map_metadata();
    let hdr_rgba32f = sample_hdr_rgba32f(LARGE_WIDTH, LARGE_HEIGHT);
    let hdr_rgba16f = sample_hdr_rgba16f(LARGE_WIDTH, LARGE_HEIGHT);
    let hdr_pq = sample_hdr_pq_1010102(LARGE_WIDTH, LARGE_HEIGHT);
    let hdr_hlg = sample_hdr_hlg_1010102(LARGE_WIDTH, LARGE_HEIGHT);

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
    let decoded_ultrahdr = decode(&ultrahdr).expect("large ultrahdr decode");
    let decoded_real_ultrahdr_iso = real_fixture_benches_enabled().then(|| {
        let real_ultrahdr_iso =
            std::fs::read(REAL_ULTRAHDR_ISO_21496_1_PATH).expect("real ISO 21496-1 fixture bytes");
        decode(&real_ultrahdr_iso).expect("real ISO 21496-1 fixture decode")
    });

    BenchmarkCorpus {
        plain,
        ultrahdr,
        decoded_ultrahdr,
        decoded_real_ultrahdr_iso,
        hdr_rgba32f,
        hdr_rgba16f,
        hdr_pq,
        hdr_hlg,
        primary,
    }
}

// Large real-fixture reconstruction benchmarks are intentionally opt-in so
// they stay out of CI, hooks, and `cargo test --all-targets` by default.
fn real_fixture_benches_enabled() -> bool {
    std::env::var_os(REAL_FIXTURE_BENCH_ENV).is_some()
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

fn sample_hdr_rgba32f(width: u32, height: u32) -> RawImage {
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

fn sample_hdr_rgba16f(width: u32, height: u32) -> RawImage {
    let mut data = Vec::with_capacity(width as usize * height as usize * 8);
    for y in 0..height {
        for x in 0..width {
            let r = 1.0 + x as f32 / width.max(1) as f32;
            let g = 1.0 + y as f32 / height.max(1) as f32;
            let b = 0.5 + (x ^ y) as f32 / width.max(height).max(1) as f32;
            for value in [r, g, b, 1.0] {
                data.extend_from_slice(&f16::from_f32(value).to_le_bytes());
            }
        }
    }

    RawImage::from_data(
        width,
        height,
        PixelFormat::Rgba16F,
        ColorGamut::DisplayP3,
        ColorTransfer::Linear,
        data,
    )
    .unwrap()
}

fn sample_hdr_pq_1010102(width: u32, height: u32) -> RawImage {
    let mut data = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let rgb = [
                0.10 + 0.55 * x as f32 / width.max(1) as f32,
                0.08 + 0.60 * y as f32 / height.max(1) as f32,
                0.05 + 0.45 * (x ^ y) as f32 / width.max(height).max(1) as f32,
            ];
            data.extend_from_slice(&pack_1010102([
                pq_oetf(rgb[0].clamp(0.0, 1.0)),
                pq_oetf(rgb[1].clamp(0.0, 1.0)),
                pq_oetf(rgb[2].clamp(0.0, 1.0)),
            ]));
        }
    }

    RawImage::from_data(
        width,
        height,
        PixelFormat::Rgba1010102Pq,
        ColorGamut::DisplayP3,
        ColorTransfer::Pq,
        data,
    )
    .unwrap()
}

fn sample_hdr_hlg_1010102(width: u32, height: u32) -> RawImage {
    let mut data = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let rgb = [
                0.15 + 0.70 * x as f32 / width.max(1) as f32,
                0.10 + 0.75 * y as f32 / height.max(1) as f32,
                0.06 + 0.55 * (x ^ y) as f32 / width.max(height).max(1) as f32,
            ];
            data.extend_from_slice(&pack_1010102([
                hlg_oetf(rgb[0].clamp(0.0, 1.0)),
                hlg_oetf(rgb[1].clamp(0.0, 1.0)),
                hlg_oetf(rgb[2].clamp(0.0, 1.0)),
            ]));
        }
    }

    RawImage::from_data(
        width,
        height,
        PixelFormat::Rgba1010102Hlg,
        ColorGamut::DisplayP3,
        ColorTransfer::Hlg,
        data,
    )
    .unwrap()
}

fn pack_1010102(rgb: [f32; 3]) -> [u8; 4] {
    let pack = |value: f32| -> u32 { (value.clamp(0.0, 1.0) * 1023.0).round() as u32 };
    let value = pack(rgb[0]) | (pack(rgb[1]) << 10) | (pack(rgb[2]) << 20) | (0b11 << 30);
    value.to_le_bytes()
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
