use std::{sync::LazyLock, time::Duration};

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use ultrahdr_core::{ColorGamut, ColorTransfer, GainMapMetadata, PixelFormat, RawImage};
use ultrajpeg::{
    CompressedImage, DecodeOptions, EncodeOptions, GainMapEncodeOptions, decode,
    decode_with_options, inspect, jpeg, sys,
};

const PLAIN_SDR: &[u8] = include_bytes!("../tests/fixtures/plain-sdr.jpg");
const SAMPLE_ULTRAHDR: &[u8] = include_bytes!("../tests/fixtures/sample-ultrahdr.jpg");
const LARGE_WIDTH: u32 = 2048;
const LARGE_HEIGHT: u32 = 1536;

static LARGE_CORPUS: LazyLock<BenchmarkCorpus> = LazyLock::new(build_large_corpus);

struct BenchmarkCorpus {
    plain: Vec<u8>,
    ultrahdr: Vec<u8>,
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
                CompressedImage::from_slice(
                    SAMPLE_ULTRAHDR,
                    sys::uhdr_color_gamut::UHDR_CG_UNSPECIFIED,
                    sys::uhdr_color_transfer::UHDR_CT_UNSPECIFIED,
                    sys::uhdr_color_range::UHDR_CR_UNSPECIFIED,
                )
            },
            |compressed| {
                let mut decoder = ultrajpeg::Decoder::new().unwrap();
                decoder.set_image_owned(compressed).unwrap();
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
                DecodeOptions {
                    decode_gain_map: false,
                },
            )
            .unwrap()
        });
    });
    group.bench_function("compat_decode_packed_view_ultrahdr", |b| {
        b.iter_batched(
            || {
                let mut decoder = ultrajpeg::Decoder::new().unwrap();
                decoder
                    .set_image_slice(
                        corpus.ultrahdr.as_slice(),
                        sys::uhdr_color_gamut::UHDR_CG_UNSPECIFIED,
                        sys::uhdr_color_transfer::UHDR_CT_UNSPECIFIED,
                        sys::uhdr_color_range::UHDR_CR_UNSPECIFIED,
                    )
                    .unwrap();
                decoder
            },
            |mut decoder| {
                black_box(
                    decoder
                        .decode_packed_view(
                            sys::uhdr_img_fmt::UHDR_IMG_FMT_32bppRGBA1010102,
                            sys::uhdr_color_transfer::UHDR_CT_PQ,
                        )
                        .unwrap(),
                );
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn build_large_corpus() -> BenchmarkCorpus {
    let primary = sample_primary(LARGE_WIDTH, LARGE_HEIGHT);
    let gain_map = sample_gain_map(LARGE_WIDTH, LARGE_HEIGHT);
    let gain_map_metadata = sample_gain_map_metadata();

    let plain = ultrajpeg::encode(&primary, &EncodeOptions::default()).expect("large plain encode");
    let ultrahdr = ultrajpeg::encode(
        &primary,
        &EncodeOptions {
            gain_map: Some(GainMapEncodeOptions {
                image: gain_map,
                metadata: gain_map_metadata,
                quality: 80,
                progressive: false,
            }),
            ..EncodeOptions::ultra_hdr_defaults()
        },
    )
    .expect("large ultrahdr encode");

    BenchmarkCorpus { plain, ultrahdr }
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
    .expect("sample primary image")
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
