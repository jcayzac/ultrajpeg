use crate::{
    Error, Result,
    types::{
        ColorMetadata, ComputeGainMapOptions, ComputedGainMap, EncodeOptions, GainMapBundle,
        GainMapChannels, PreparePrimaryOptions, PreparedPrimary, PrimaryMetadata,
        UltraHdrEncodeOptions,
    },
};
#[cfg(target_arch = "aarch64")]
use archmage::NeonToken;
use archmage::ScalarToken;
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
use archmage::SimdToken;
#[cfg(target_arch = "x86_64")]
use archmage::X64V3Token;
use magetypes::simd::{backends::F32x8Backend, generic::f32x8};
use rayon::prelude::*;
use ultrahdr_core::{
    ColorGamut, ColorTransfer, GainMapConfig, PixelFormat, RawImage, Unstoppable,
    color::{
        Matrix3x3, bt2390_tonemap, convert_gamut, gamut_conversion_matrix, hlg_eotf,
        luma_coefficients, pq_eotf, rgb_to_luminance, soft_clip_gamut, srgb_eotf, srgb_oetf,
    },
    gainmap::compute_gainmap,
};

const PREPARE_PRIMARY_PARALLEL_THRESHOLD_PIXELS: usize = 256 * 256;
const DEFAULT_MAX_GAIN_MAP_BOOST: f32 = 6.0;
const PREPARE_PRIMARY_SIMD_LANES: usize = 8;
const PACKED_10BIT_LUT_SIZE: usize = 1024;
const SRGB_8BIT_LUT_SIZE: usize = 256;

pub(crate) fn compute_gain_map_impl(
    hdr_image: &RawImage,
    primary_image: &RawImage,
    options: &ComputeGainMapOptions,
) -> Result<ComputedGainMap> {
    let config = GainMapConfig {
        multi_channel: matches!(options.channels, GainMapChannels::Multi),
        ..GainMapConfig::default()
    };

    let (gain_map, metadata) = compute_gainmap(hdr_image, primary_image, &config, Unstoppable)?;
    let image = RawImage::from_data(
        gain_map.width,
        gain_map.height,
        match gain_map.channels {
            1 => PixelFormat::Gray8,
            3 => PixelFormat::Rgb8,
            other => unreachable!("unsupported computed gain-map channels {other}"),
        },
        ColorGamut::Bt709,
        ColorTransfer::Linear,
        gain_map.data,
    )?;

    Ok(ComputedGainMap { image, metadata })
}

pub(crate) fn ultra_hdr_encode_options(
    primary: &EncodeOptions,
    computed: ComputedGainMap,
    options: &UltraHdrEncodeOptions,
) -> EncodeOptions {
    EncodeOptions {
        gain_map: Some(GainMapBundle {
            image: computed.image,
            metadata: computed.metadata,
            quality: options.gain_map_quality,
            progressive: options.gain_map_progressive,
            compression: options.gain_map_compression,
        }),
        ..primary.clone()
    }
}

pub(crate) fn prepare_primary_impl(
    image: &RawImage,
    options: &PreparePrimaryOptions,
) -> Result<PreparedPrimary> {
    validate_prepare_primary_input(image, options)?;

    let source_transfer = effective_source_transfer(image);
    let source_peak_nits = options
        .source_peak_nits
        .unwrap_or_else(|| default_source_peak_nits(source_transfer));
    let width = image.width as usize;
    let height = image.height as usize;
    let row_len = width * 3;
    let mut data = vec![0_u8; row_len * height];
    let dispatch = detect_prepare_primary_dispatch();
    let context = PreparePrimaryContext::new(image, options, source_transfer, source_peak_nits);

    if width * height >= PREPARE_PRIMARY_PARALLEL_THRESHOLD_PIXELS {
        data.par_chunks_mut(row_len)
            .enumerate()
            .for_each(|(y, row)| fill_prepared_row(dispatch, row, image, y as u32, &context));
    } else {
        for (y, row) in data.chunks_mut(row_len).enumerate() {
            fill_prepared_row(dispatch, row, image, y as u32, &context);
        }
    }

    let image = RawImage::from_data(
        image.width,
        image.height,
        PixelFormat::Rgb8,
        options.target_gamut,
        ColorTransfer::Srgb,
        data,
    )?;

    Ok(PreparedPrimary {
        image,
        metadata: prepared_primary_metadata(options.target_gamut),
    })
}

fn validate_prepare_primary_input(image: &RawImage, options: &PreparePrimaryOptions) -> Result<()> {
    if !matches!(
        options.target_gamut,
        ColorGamut::Bt709 | ColorGamut::DisplayP3
    ) {
        return Err(Error::InvalidInput(
            "prepare_sdr_primary currently supports Bt709 and DisplayP3 output only".into(),
        ));
    }

    if !options.target_peak_nits.is_finite() || options.target_peak_nits <= 0.0 {
        return Err(Error::InvalidInput(
            "PreparePrimaryOptions::target_peak_nits must be finite and positive".into(),
        ));
    }

    if options
        .source_peak_nits
        .is_some_and(|value| !value.is_finite() || value <= 0.0)
    {
        return Err(Error::InvalidInput(
            "PreparePrimaryOptions::source_peak_nits must be finite and positive when set".into(),
        ));
    }

    match image.format {
        PixelFormat::Rgb8
        | PixelFormat::Rgba8
        | PixelFormat::Rgba16F
        | PixelFormat::Rgba32F
        | PixelFormat::Rgba1010102Pq
        | PixelFormat::Rgba1010102Hlg => {}
        _ => {
            return Err(Error::UnsupportedFormat(
                "prepare_sdr_primary input image format",
            ));
        }
    }

    Ok(())
}

#[derive(Clone, Copy)]
enum PreparePrimaryDispatch {
    Scalar,
    #[cfg(target_arch = "x86_64")]
    X64V3(X64V3Token),
    #[cfg(target_arch = "aarch64")]
    Neon(NeonToken),
}

fn detect_prepare_primary_dispatch() -> PreparePrimaryDispatch {
    #[cfg(target_arch = "x86_64")]
    if let Some(token) = X64V3Token::summon() {
        return PreparePrimaryDispatch::X64V3(token);
    }

    #[cfg(target_arch = "aarch64")]
    if let Some(token) = NeonToken::summon() {
        return PreparePrimaryDispatch::Neon(token);
    }

    PreparePrimaryDispatch::Scalar
}

fn fill_prepared_row(
    dispatch: PreparePrimaryDispatch,
    row: &mut [u8],
    image: &RawImage,
    y: u32,
    context: &PreparePrimaryContext,
) {
    match (image.format, context.source_transfer) {
        (PixelFormat::Rgba32F, ColorTransfer::Linear) => {
            fill_prepared_row_rgba32f_linear(dispatch, row, image, y, context)
        }
        (PixelFormat::Rgba16F, ColorTransfer::Linear) => {
            fill_prepared_row_rgba16f_linear(dispatch, row, image, y, context)
        }
        (PixelFormat::Rgba1010102Pq, _) => {
            fill_prepared_row_pq_1010102(dispatch, row, image, y, context)
        }
        (PixelFormat::Rgba1010102Hlg, _) => {
            fill_prepared_row_hlg_1010102(dispatch, row, image, y, context)
        }
        _ => fill_prepared_row_generic(dispatch, row, image, y, context),
    }
}

struct PreparePrimaryContext {
    source_gamut: ColorGamut,
    source_transfer: ColorTransfer,
    target_gamut: ColorGamut,
    gamut_matrix: Matrix3x3,
    luma_coeffs: [f32; 3],
    source_peak_nits: f32,
    target_peak_ratio: f32,
    bt2390_ks: f32,
    inv_source_peak_nits: f32,
    minimum_linear_scale: f32,
    needs_tonemap: bool,
    srgb_8bit_lut: [f32; SRGB_8BIT_LUT_SIZE],
    pq_10bit_lut: [f32; PACKED_10BIT_LUT_SIZE],
    hlg_10bit_lut: [f32; PACKED_10BIT_LUT_SIZE],
}

impl PreparePrimaryContext {
    fn new(
        image: &RawImage,
        options: &PreparePrimaryOptions,
        source_transfer: ColorTransfer,
        source_peak_nits: f32,
    ) -> Self {
        let inv_source_peak_nits = 1.0 / source_peak_nits;
        Self {
            source_gamut: image.gamut,
            source_transfer,
            target_gamut: options.target_gamut,
            gamut_matrix: gamut_conversion_matrix(image.gamut, options.target_gamut),
            luma_coeffs: luma_coefficients(options.target_gamut),
            source_peak_nits,
            target_peak_ratio: options.target_peak_nits * inv_source_peak_nits,
            bt2390_ks: (1.5 * options.target_peak_nits * inv_source_peak_nits - 0.5)
                .clamp(0.0, 1.0),
            inv_source_peak_nits,
            minimum_linear_scale: inv_source_peak_nits / DEFAULT_MAX_GAIN_MAP_BOOST,
            needs_tonemap: source_peak_nits > options.target_peak_nits,
            srgb_8bit_lut: core::array::from_fn(|value| {
                srgb_eotf(value as f32 / (SRGB_8BIT_LUT_SIZE as f32 - 1.0)) * source_peak_nits
            }),
            pq_10bit_lut: core::array::from_fn(|value| {
                pq_eotf(value as f32 / (PACKED_10BIT_LUT_SIZE as f32 - 1.0)) * 10_000.0
            }),
            hlg_10bit_lut: core::array::from_fn(|value| {
                hlg_eotf(
                    value as f32 / (PACKED_10BIT_LUT_SIZE as f32 - 1.0),
                    source_peak_nits,
                )
            }),
        }
    }
}

struct SourceChunk {
    r: [f32; PREPARE_PRIMARY_SIMD_LANES],
    g: [f32; PREPARE_PRIMARY_SIMD_LANES],
    b: [f32; PREPARE_PRIMARY_SIMD_LANES],
}

impl SourceChunk {
    fn new() -> Self {
        Self {
            r: [0.0; PREPARE_PRIMARY_SIMD_LANES],
            g: [0.0; PREPARE_PRIMARY_SIMD_LANES],
            b: [0.0; PREPARE_PRIMARY_SIMD_LANES],
        }
    }

    fn set_lane(&mut self, lane: usize, source_nits: [f32; 3]) {
        self.r[lane] = source_nits[0];
        self.g[lane] = source_nits[1];
        self.b[lane] = source_nits[2];
    }
}

struct PreparedChunk {
    sdr_r: [f32; PREPARE_PRIMARY_SIMD_LANES],
    sdr_g: [f32; PREPARE_PRIMARY_SIMD_LANES],
    sdr_b: [f32; PREPARE_PRIMARY_SIMD_LANES],
    min_r: [f32; PREPARE_PRIMARY_SIMD_LANES],
    min_g: [f32; PREPARE_PRIMARY_SIMD_LANES],
    min_b: [f32; PREPARE_PRIMARY_SIMD_LANES],
}

impl PreparedChunk {}

fn fill_prepared_row_generic(
    dispatch: PreparePrimaryDispatch,
    row: &mut [u8],
    image: &RawImage,
    y: u32,
    context: &PreparePrimaryContext,
) {
    let mut x = 0u32;
    while x as usize + PREPARE_PRIMARY_SIMD_LANES <= image.width as usize {
        let mut source = SourceChunk::new();

        for lane in 0..PREPARE_PRIMARY_SIMD_LANES {
            source.set_lane(lane, source_rgb_nits(image, x + lane as u32, y, context));
        }

        let prepared = prepare_linear_output_chunk(dispatch, &source, context);
        write_prepared_chunk(dispatch, row, x as usize, &prepared);
        x += PREPARE_PRIMARY_SIMD_LANES as u32;
    }

    for tail_x in x..image.width {
        let source_nits = source_rgb_nits(image, tail_x, y, context);
        let (sdr_linear, minimum_linear) = prepare_linear_output(source_nits, context);
        write_prepared_pixel(row, tail_x as usize, sdr_linear, minimum_linear);
    }
}

fn fill_prepared_row_rgba32f_linear(
    dispatch: PreparePrimaryDispatch,
    row: &mut [u8],
    image: &RawImage,
    y: u32,
    context: &PreparePrimaryContext,
) {
    let row_start = y as usize * image.stride as usize;
    let source = &image.data[row_start..row_start + image.width as usize * 16];
    fill_prepared_row_linear_blocks(
        dispatch,
        row,
        source,
        image.width as usize,
        context,
        read_rgba32f_linear_nits,
    );
}

fn fill_prepared_row_rgba16f_linear(
    dispatch: PreparePrimaryDispatch,
    row: &mut [u8],
    image: &RawImage,
    y: u32,
    context: &PreparePrimaryContext,
) {
    let row_start = y as usize * image.stride as usize;
    let source = &image.data[row_start..row_start + image.width as usize * 8];
    fill_prepared_row_linear_blocks(
        dispatch,
        row,
        source,
        image.width as usize,
        context,
        read_rgba16f_linear_nits,
    );
}

fn fill_prepared_row_linear_blocks(
    dispatch: PreparePrimaryDispatch,
    row: &mut [u8],
    source: &[u8],
    width: usize,
    context: &PreparePrimaryContext,
    read_nits: fn(&[u8], &PreparePrimaryContext) -> [f32; 3],
) {
    let mut pixel_start = 0usize;
    if width == 0 {
        return;
    }

    let bytes_per_pixel = source.len() / width;
    let mut chunk_iter = source.chunks_exact(PREPARE_PRIMARY_SIMD_LANES * bytes_per_pixel);
    for chunk in &mut chunk_iter {
        let mut source_chunk = SourceChunk::new();

        for (lane, pixel) in chunk.chunks_exact(bytes_per_pixel).enumerate() {
            source_chunk.set_lane(lane, read_nits(pixel, context));
        }

        let prepared = prepare_linear_output_chunk(dispatch, &source_chunk, context);
        write_prepared_chunk(dispatch, row, pixel_start, &prepared);
        pixel_start += PREPARE_PRIMARY_SIMD_LANES;
    }

    for pixel in chunk_iter.remainder().chunks_exact(bytes_per_pixel) {
        let (sdr_linear, minimum_linear) =
            prepare_linear_output(read_nits(pixel, context), context);
        write_prepared_pixel(row, pixel_start, sdr_linear, minimum_linear);
        pixel_start += 1;
    }
}

fn fill_prepared_row_pq_1010102(
    dispatch: PreparePrimaryDispatch,
    row: &mut [u8],
    image: &RawImage,
    y: u32,
    context: &PreparePrimaryContext,
) {
    let row_start = y as usize * image.stride as usize;
    let source = &image.data[row_start..row_start + image.width as usize * 4];
    fill_prepared_row_packed_1010102(
        dispatch,
        row,
        source,
        image.width as usize,
        context,
        &context.pq_10bit_lut,
    );
}

fn fill_prepared_row_hlg_1010102(
    dispatch: PreparePrimaryDispatch,
    row: &mut [u8],
    image: &RawImage,
    y: u32,
    context: &PreparePrimaryContext,
) {
    let row_start = y as usize * image.stride as usize;
    let source = &image.data[row_start..row_start + image.width as usize * 4];
    fill_prepared_row_packed_1010102(
        dispatch,
        row,
        source,
        image.width as usize,
        context,
        &context.hlg_10bit_lut,
    );
}

fn fill_prepared_row_packed_1010102(
    dispatch: PreparePrimaryDispatch,
    row: &mut [u8],
    source: &[u8],
    width: usize,
    context: &PreparePrimaryContext,
    lut: &[f32; PACKED_10BIT_LUT_SIZE],
) {
    let mut pixel_start = 0usize;
    let mut chunks = source.chunks_exact(PREPARE_PRIMARY_SIMD_LANES * 4);

    for chunk in &mut chunks {
        let mut source_chunk = SourceChunk::new();

        for (lane, pixel) in chunk.chunks_exact(4).enumerate() {
            let packed = u32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]);
            source_chunk.set_lane(
                lane,
                [
                    lut[(packed & 0x3ff) as usize],
                    lut[((packed >> 10) & 0x3ff) as usize],
                    lut[((packed >> 20) & 0x3ff) as usize],
                ],
            );
        }

        let prepared = prepare_linear_output_chunk(dispatch, &source_chunk, context);
        write_prepared_chunk(dispatch, row, pixel_start, &prepared);
        pixel_start += PREPARE_PRIMARY_SIMD_LANES;
    }

    for pixel in chunks.remainder().chunks_exact(4).take(width - pixel_start) {
        let packed = u32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]);
        let source_nits = [
            lut[(packed & 0x3ff) as usize],
            lut[((packed >> 10) & 0x3ff) as usize],
            lut[((packed >> 20) & 0x3ff) as usize],
        ];
        let (sdr_linear, minimum_linear) = prepare_linear_output(source_nits, context);
        write_prepared_pixel(row, pixel_start, sdr_linear, minimum_linear);
        pixel_start += 1;
    }
}

fn prepare_linear_output_chunk(
    dispatch: PreparePrimaryDispatch,
    source: &SourceChunk,
    context: &PreparePrimaryContext,
) -> PreparedChunk {
    match dispatch {
        PreparePrimaryDispatch::Scalar => {
            prepare_linear_output_chunk_simd(ScalarToken, source, context)
        }
        #[cfg(target_arch = "x86_64")]
        PreparePrimaryDispatch::X64V3(token) => {
            prepare_linear_output_chunk_simd(token, source, context)
        }
        #[cfg(target_arch = "aarch64")]
        PreparePrimaryDispatch::Neon(token) => {
            prepare_linear_output_chunk_simd(token, source, context)
        }
    }
}

fn prepare_linear_output_chunk_simd<T: F32x8Backend>(
    token: T,
    source: &SourceChunk,
    context: &PreparePrimaryContext,
) -> PreparedChunk {
    let zero = f32x8::<T>::zero(token);
    let inv_source_peak = f32x8::<T>::splat(token, context.inv_source_peak_nits);
    let minimum_scale = f32x8::<T>::splat(token, context.minimum_linear_scale);

    let source_r = f32x8::<T>::from_array(token, source.r);
    let source_g = f32x8::<T>::from_array(token, source.g);
    let source_b = f32x8::<T>::from_array(token, source.b);

    let matrix = &context.gamut_matrix.0;
    let target_r = source_r * f32x8::<T>::splat(token, matrix[0][0])
        + source_g * f32x8::<T>::splat(token, matrix[0][1])
        + source_b * f32x8::<T>::splat(token, matrix[0][2]);
    let target_g = source_r * f32x8::<T>::splat(token, matrix[1][0])
        + source_g * f32x8::<T>::splat(token, matrix[1][1])
        + source_b * f32x8::<T>::splat(token, matrix[1][2]);
    let target_b = source_r * f32x8::<T>::splat(token, matrix[2][0])
        + source_g * f32x8::<T>::splat(token, matrix[2][1])
        + source_b * f32x8::<T>::splat(token, matrix[2][2]);

    let luma = &context.luma_coeffs;
    let luminance = target_r * f32x8::<T>::splat(token, luma[0])
        + target_g * f32x8::<T>::splat(token, luma[1])
        + target_b * f32x8::<T>::splat(token, luma[2]);
    let source_normalized = luminance * inv_source_peak;
    let target_normalized = if context.needs_tonemap {
        bt2390_tonemap_chunk(token, source_normalized, context)
    } else {
        source_normalized
    };
    let positive = luminance.simd_gt(zero);
    let luminance_ratio = f32x8::<T>::blend(
        positive,
        target_normalized / source_normalized.max(f32x8::<T>::splat(token, f32::MIN_POSITIVE)),
        zero,
    );

    let clipped = soft_clip_gamut_chunk(
        token,
        target_r * inv_source_peak * luminance_ratio,
        target_g * inv_source_peak * luminance_ratio,
        target_b * inv_source_peak * luminance_ratio,
    );

    PreparedChunk {
        sdr_r: clipped.0.to_array(),
        sdr_g: clipped.1.to_array(),
        sdr_b: clipped.2.to_array(),
        min_r: (target_r * minimum_scale).max(zero).to_array(),
        min_g: (target_g * minimum_scale).max(zero).to_array(),
        min_b: (target_b * minimum_scale).max(zero).to_array(),
    }
}

fn bt2390_tonemap_chunk<T: F32x8Backend>(
    token: T,
    scene_linear: f32x8<T>,
    context: &PreparePrimaryContext,
) -> f32x8<T> {
    let ks = f32x8::<T>::splat(token, context.bt2390_ks);
    let one = f32x8::<T>::splat(token, 1.0);
    let target_peak = f32x8::<T>::splat(token, context.target_peak_ratio);
    let t = (scene_linear - ks) / f32x8::<T>::splat(token, 1.0 - context.bt2390_ks);
    let t2 = t * t;
    let t3 = t2 * t;

    let a = t3 * f32x8::<T>::splat(token, 2.0) - t2 * f32x8::<T>::splat(token, 3.0) + one;
    let b = t3 - t2 * f32x8::<T>::splat(token, 2.0) + t;
    let c = t2 * f32x8::<T>::splat(token, 3.0) - t3 * f32x8::<T>::splat(token, 2.0);
    let spline = (a * ks + b * f32x8::<T>::splat(token, 1.0 - context.bt2390_ks) + c) * target_peak;

    f32x8::<T>::blend(scene_linear.simd_lt(ks), scene_linear, spline)
}

fn soft_clip_gamut_chunk<T: F32x8Backend>(
    token: T,
    r: f32x8<T>,
    g: f32x8<T>,
    b: f32x8<T>,
) -> (f32x8<T>, f32x8<T>, f32x8<T>) {
    let zero = f32x8::<T>::zero(token);
    let one = f32x8::<T>::splat(token, 1.0);
    let max_channel = r.max(g).max(b);
    let safe_max = max_channel.max(f32x8::<T>::splat(token, f32::MIN_POSITIVE));
    let scale = f32x8::<T>::blend(max_channel.simd_gt(one), one / safe_max, one);
    (
        (r * scale).max(zero),
        (g * scale).max(zero),
        (b * scale).max(zero),
    )
}

fn prepare_linear_output(
    source_nits: [f32; 3],
    context: &PreparePrimaryContext,
) -> ([f32; 3], [f32; 3]) {
    let target_nits = convert_gamut(source_nits, context.source_gamut, context.target_gamut);
    let sdr_linear = tone_map_to_sdr(target_nits, context);
    let minimum_linear = [
        (target_nits[0] * context.minimum_linear_scale).max(0.0),
        (target_nits[1] * context.minimum_linear_scale).max(0.0),
        (target_nits[2] * context.minimum_linear_scale).max(0.0),
    ];
    (sdr_linear, minimum_linear)
}

fn write_prepared_chunk(
    dispatch: PreparePrimaryDispatch,
    row: &mut [u8],
    pixel_start: usize,
    prepared: &PreparedChunk,
) {
    let output_r = srgb8_channel_from_linear_chunk(dispatch, prepared.sdr_r, prepared.min_r);
    let output_g = srgb8_channel_from_linear_chunk(dispatch, prepared.sdr_g, prepared.min_g);
    let output_b = srgb8_channel_from_linear_chunk(dispatch, prepared.sdr_b, prepared.min_b);

    for lane in 0..PREPARE_PRIMARY_SIMD_LANES {
        let offset = (pixel_start + lane) * 3;
        row[offset] = output_r[lane];
        row[offset + 1] = output_g[lane];
        row[offset + 2] = output_b[lane];
    }
}

fn write_prepared_pixel(
    row: &mut [u8],
    pixel_index: usize,
    sdr_linear: [f32; 3],
    minimum_linear: [f32; 3],
) {
    let output = srgb8_from_linear([
        sdr_linear[0].max(minimum_linear[0]),
        sdr_linear[1].max(minimum_linear[1]),
        sdr_linear[2].max(minimum_linear[2]),
    ]);
    let offset = pixel_index * 3;
    row[offset] = output[0];
    row[offset + 1] = output[1];
    row[offset + 2] = output[2];
}

fn srgb8_channel_from_linear_chunk(
    dispatch: PreparePrimaryDispatch,
    linear: [f32; PREPARE_PRIMARY_SIMD_LANES],
    minimum: [f32; PREPARE_PRIMARY_SIMD_LANES],
) -> [u8; PREPARE_PRIMARY_SIMD_LANES] {
    match dispatch {
        PreparePrimaryDispatch::Scalar => srgb8_channel_from_linear_chunk_scalar(linear, minimum),
        #[cfg(target_arch = "x86_64")]
        PreparePrimaryDispatch::X64V3(token) => {
            srgb8_channel_from_linear_chunk_simd(token, linear, minimum)
        }
        #[cfg(target_arch = "aarch64")]
        PreparePrimaryDispatch::Neon(token) => {
            srgb8_channel_from_linear_chunk_simd(token, linear, minimum)
        }
    }
}

fn srgb8_channel_from_linear_chunk_scalar(
    linear: [f32; PREPARE_PRIMARY_SIMD_LANES],
    minimum: [f32; PREPARE_PRIMARY_SIMD_LANES],
) -> [u8; PREPARE_PRIMARY_SIMD_LANES] {
    core::array::from_fn(|index| quantize_srgb8(linear[index].max(minimum[index])))
}

fn srgb8_channel_from_linear_chunk_simd<T: F32x8Backend>(
    token: T,
    linear: [f32; PREPARE_PRIMARY_SIMD_LANES],
    minimum: [f32; PREPARE_PRIMARY_SIMD_LANES],
) -> [u8; PREPARE_PRIMARY_SIMD_LANES] {
    // Keep the exact scalar sRGB transfer curve for byte-stable output while
    // using SIMD for the surrounding max/clamp/scale/round work.
    let zero = f32x8::<T>::zero(token);
    let one = f32x8::<T>::splat(token, 1.0);
    let scale = f32x8::<T>::splat(token, 255.0);
    let clamped = f32x8::<T>::from_array(token, linear)
        .max(f32x8::<T>::from_array(token, minimum))
        .clamp(zero, one);
    let mut encoded = sanitize_finite_chunk(token, clamped).to_array();

    for value in &mut encoded {
        *value = srgb_oetf(*value);
    }

    let rounded = (f32x8::<T>::from_array(token, encoded) * scale)
        .round()
        .to_array();
    let mut output = [0_u8; PREPARE_PRIMARY_SIMD_LANES];
    for (dst, src) in output.iter_mut().zip(rounded) {
        *dst = src as u8;
    }
    output
}

fn sanitize_finite_chunk<T: F32x8Backend>(token: T, value: f32x8<T>) -> f32x8<T> {
    let zero = f32x8::<T>::zero(token);
    let max_finite = f32x8::<T>::splat(token, f32::MAX);
    let min_finite = f32x8::<T>::splat(token, -f32::MAX);

    let without_nan = f32x8::<T>::blend(value.simd_eq(value), value, zero);
    let without_pos_inf = f32x8::<T>::blend(without_nan.simd_gt(max_finite), zero, without_nan);
    f32x8::<T>::blend(without_pos_inf.simd_lt(min_finite), zero, without_pos_inf)
}

fn prepared_primary_metadata(target_gamut: ColorGamut) -> PrimaryMetadata {
    let color = match target_gamut {
        ColorGamut::Bt709 => ColorMetadata::bt709_srgb(),
        ColorGamut::DisplayP3 => ColorMetadata::display_p3(),
        ColorGamut::Bt2100 => unreachable!("validated unsupported prepare_sdr_primary gamut"),
    };

    PrimaryMetadata { color, exif: None }
}

fn effective_source_transfer(image: &RawImage) -> ColorTransfer {
    match image.format {
        PixelFormat::Rgba1010102Pq => ColorTransfer::Pq,
        PixelFormat::Rgba1010102Hlg => ColorTransfer::Hlg,
        _ => image.transfer,
    }
}

fn default_source_peak_nits(transfer: ColorTransfer) -> f32 {
    match transfer {
        ColorTransfer::Pq => 10_000.0,
        ColorTransfer::Hlg | ColorTransfer::Linear => 1_000.0,
        ColorTransfer::Srgb => 203.0,
    }
}

fn source_rgb_nits(image: &RawImage, x: u32, y: u32, context: &PreparePrimaryContext) -> [f32; 3] {
    let row = y * image.stride;
    match image.format {
        PixelFormat::Rgb8 if matches!(context.source_transfer, ColorTransfer::Srgb) => {
            let index = (row + x * 3) as usize;
            return [
                context.srgb_8bit_lut[image.data[index] as usize],
                context.srgb_8bit_lut[image.data[index + 1] as usize],
                context.srgb_8bit_lut[image.data[index + 2] as usize],
            ];
        }
        PixelFormat::Rgba8 if matches!(context.source_transfer, ColorTransfer::Srgb) => {
            let index = (row + x * 4) as usize;
            return [
                context.srgb_8bit_lut[image.data[index] as usize],
                context.srgb_8bit_lut[image.data[index + 1] as usize],
                context.srgb_8bit_lut[image.data[index + 2] as usize],
            ];
        }
        _ => {}
    }

    let rgb = source_pixel_components(image, x, y);
    match context.source_transfer {
        ColorTransfer::Pq => [
            pq_eotf(rgb[0]) * 10_000.0,
            pq_eotf(rgb[1]) * 10_000.0,
            pq_eotf(rgb[2]) * 10_000.0,
        ],
        ColorTransfer::Hlg => [
            hlg_eotf(rgb[0], context.source_peak_nits),
            hlg_eotf(rgb[1], context.source_peak_nits),
            hlg_eotf(rgb[2], context.source_peak_nits),
        ],
        ColorTransfer::Srgb => [
            srgb_eotf(rgb[0]) * context.source_peak_nits,
            srgb_eotf(rgb[1]) * context.source_peak_nits,
            srgb_eotf(rgb[2]) * context.source_peak_nits,
        ],
        ColorTransfer::Linear => [
            rgb[0] * context.source_peak_nits,
            rgb[1] * context.source_peak_nits,
            rgb[2] * context.source_peak_nits,
        ],
    }
}

fn tone_map_to_sdr(rgb_nits: [f32; 3], context: &PreparePrimaryContext) -> [f32; 3] {
    let luminance = rgb_to_luminance(rgb_nits, context.target_gamut);
    if luminance <= 0.0 {
        return [0.0; 3];
    }

    let source_normalized = luminance * context.inv_source_peak_nits;
    let target_normalized = if context.needs_tonemap {
        bt2390_tonemap(source_normalized, 1.0, context.target_peak_ratio)
    } else {
        source_normalized
    };
    let luminance_ratio = if source_normalized > 0.0 {
        target_normalized / source_normalized
    } else {
        0.0
    };

    soft_clip_gamut([
        rgb_nits[0] * context.inv_source_peak_nits * luminance_ratio,
        rgb_nits[1] * context.inv_source_peak_nits * luminance_ratio,
        rgb_nits[2] * context.inv_source_peak_nits * luminance_ratio,
    ])
}

fn srgb8_from_linear(rgb: [f32; 3]) -> [u8; 3] {
    [
        quantize_srgb8(rgb[0]),
        quantize_srgb8(rgb[1]),
        quantize_srgb8(rgb[2]),
    ]
}

#[inline(always)]
fn quantize_srgb8(value: f32) -> u8 {
    let linear = if value.is_nan() {
        0.0
    } else if value.is_infinite() {
        if value.is_sign_positive() { 1.0 } else { 0.0 }
    } else if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let encoded = srgb_oetf(linear);
    if !encoded.is_finite() {
        return 0;
    }
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

fn source_pixel_components(image: &RawImage, x: u32, y: u32) -> [f32; 3] {
    let row = y * image.stride;
    match image.format {
        PixelFormat::Rgb8 => {
            let index = (row + x * 3) as usize;
            [
                image.data[index] as f32 / 255.0,
                image.data[index + 1] as f32 / 255.0,
                image.data[index + 2] as f32 / 255.0,
            ]
        }
        PixelFormat::Rgba8 => {
            let index = (row + x * 4) as usize;
            [
                image.data[index] as f32 / 255.0,
                image.data[index + 1] as f32 / 255.0,
                image.data[index + 2] as f32 / 255.0,
            ]
        }
        PixelFormat::Rgba16F => {
            let index = (row + x * 8) as usize;
            [
                half::f16::from_le_bytes([image.data[index], image.data[index + 1]]).to_f32(),
                half::f16::from_le_bytes([image.data[index + 2], image.data[index + 3]]).to_f32(),
                half::f16::from_le_bytes([image.data[index + 4], image.data[index + 5]]).to_f32(),
            ]
        }
        PixelFormat::Rgba32F => {
            let index = (row + x * 16) as usize;
            [
                f32::from_le_bytes([
                    image.data[index],
                    image.data[index + 1],
                    image.data[index + 2],
                    image.data[index + 3],
                ]),
                f32::from_le_bytes([
                    image.data[index + 4],
                    image.data[index + 5],
                    image.data[index + 6],
                    image.data[index + 7],
                ]),
                f32::from_le_bytes([
                    image.data[index + 8],
                    image.data[index + 9],
                    image.data[index + 10],
                    image.data[index + 11],
                ]),
            ]
        }
        PixelFormat::Rgba1010102Pq | PixelFormat::Rgba1010102Hlg => {
            let index = (row + x * 4) as usize;
            let packed = u32::from_le_bytes([
                image.data[index],
                image.data[index + 1],
                image.data[index + 2],
                image.data[index + 3],
            ]);
            [
                (packed & 0x3ff) as f32 / 1023.0,
                ((packed >> 10) & 0x3ff) as f32 / 1023.0,
                ((packed >> 20) & 0x3ff) as f32 / 1023.0,
            ]
        }
        _ => unreachable!("validated unsupported prepare_sdr_primary format"),
    }
}

fn read_rgba32f_linear_nits(pixel: &[u8], context: &PreparePrimaryContext) -> [f32; 3] {
    [
        f32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]) * context.source_peak_nits,
        f32::from_le_bytes([pixel[4], pixel[5], pixel[6], pixel[7]]) * context.source_peak_nits,
        f32::from_le_bytes([pixel[8], pixel[9], pixel[10], pixel[11]]) * context.source_peak_nits,
    ]
}

fn read_rgba16f_linear_nits(pixel: &[u8], context: &PreparePrimaryContext) -> [f32; 3] {
    [
        half::f16::from_le_bytes([pixel[0], pixel[1]]).to_f32() * context.source_peak_nits,
        half::f16::from_le_bytes([pixel[2], pixel[3]]).to_f32() * context.source_peak_nits,
        half::f16::from_le_bytes([pixel[4], pixel[5]]).to_f32() * context.source_peak_nits,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb8_chunk_matches_scalar_reference() {
        let linear = [0.0, 0.001, 0.0031307, 0.0031308, 0.0035, 0.18, 0.5, 1.0];
        let minimum = [0.0, 0.0, 0.0, 0.0, 0.002, 0.1, 0.25, 0.9];

        let simd = srgb8_channel_from_linear_chunk(PreparePrimaryDispatch::Scalar, linear, minimum);
        let scalar = core::array::from_fn(|i| {
            (srgb_oetf(linear[i].max(minimum[i]).clamp(0.0, 1.0)) * 255.0).round() as u8
        });

        assert_eq!(simd, scalar);
    }

    #[test]
    fn specialized_rgba32f_row_matches_reference() {
        assert_specialized_row_matches_reference(sample_rgba32f());
    }

    #[test]
    fn specialized_rgba16f_row_matches_reference() {
        assert_specialized_row_matches_reference(sample_rgba16f());
    }

    #[test]
    fn specialized_pq_row_matches_reference() {
        assert_specialized_row_matches_reference(sample_pq());
    }

    #[test]
    fn specialized_hlg_row_matches_reference() {
        assert_specialized_row_matches_reference(sample_hlg());
    }

    fn assert_specialized_row_matches_reference(image: RawImage) {
        assert_specialized_row_matches_reference_with_dispatch(
            image,
            PreparePrimaryDispatch::Scalar,
        );
    }

    fn assert_specialized_row_matches_reference_with_dispatch(
        image: RawImage,
        dispatch: PreparePrimaryDispatch,
    ) {
        let options = PreparePrimaryOptions::ultra_hdr_defaults();
        let source_transfer = effective_source_transfer(&image);
        let source_peak_nits = default_source_peak_nits(source_transfer);
        let context =
            PreparePrimaryContext::new(&image, &options, source_transfer, source_peak_nits);
        let mut reference = vec![0_u8; image.width as usize * 3];
        let mut optimized = vec![0_u8; image.width as usize * 3];

        reference_fill_prepared_row(&mut reference, &image, 0, &context);
        fill_prepared_row(dispatch, &mut optimized, &image, 0, &context);

        assert_eq!(optimized, reference);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn specialized_rows_match_reference_with_x64v3_when_available() {
        if let Some(token) = X64V3Token::summon() {
            assert_specialized_row_matches_reference_with_dispatch(
                sample_rgba32f(),
                PreparePrimaryDispatch::X64V3(token),
            );
            assert_specialized_row_matches_reference_with_dispatch(
                sample_rgba16f(),
                PreparePrimaryDispatch::X64V3(token),
            );
            assert_specialized_row_matches_reference_with_dispatch(
                sample_pq(),
                PreparePrimaryDispatch::X64V3(token),
            );
            assert_specialized_row_matches_reference_with_dispatch(
                sample_hlg(),
                PreparePrimaryDispatch::X64V3(token),
            );
        }
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn specialized_rows_match_reference_with_neon_when_available() {
        if let Some(token) = NeonToken::summon() {
            assert_specialized_row_matches_reference_with_dispatch(
                sample_rgba32f(),
                PreparePrimaryDispatch::Neon(token),
            );
            assert_specialized_row_matches_reference_with_dispatch(
                sample_rgba16f(),
                PreparePrimaryDispatch::Neon(token),
            );
            assert_specialized_row_matches_reference_with_dispatch(
                sample_pq(),
                PreparePrimaryDispatch::Neon(token),
            );
            assert_specialized_row_matches_reference_with_dispatch(
                sample_hlg(),
                PreparePrimaryDispatch::Neon(token),
            );
        }
    }

    #[test]
    fn quantize_srgb8_sanitizes_non_finite_values() {
        assert_eq!(quantize_srgb8(f32::NAN), 0);
        assert_eq!(quantize_srgb8(f32::INFINITY), 255);
    }

    #[test]
    fn scalar_chunk_sanitizes_non_finite_values() {
        let linear = [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            0.5,
            1.0,
            0.0,
            0.25,
            0.75,
        ];
        let minimum = [0.0; PREPARE_PRIMARY_SIMD_LANES];

        let output =
            srgb8_channel_from_linear_chunk(PreparePrimaryDispatch::Scalar, linear, minimum);

        assert_eq!(output[0], 0);
        assert_eq!(output[1], 255);
        assert_eq!(output[2], 0);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn x64v3_chunk_sanitizes_non_finite_values_when_available() {
        if let Some(token) = X64V3Token::summon() {
            let linear = [
                f32::NAN,
                f32::INFINITY,
                f32::NEG_INFINITY,
                0.5,
                1.0,
                0.0,
                0.25,
                0.75,
            ];
            let minimum = [0.0; PREPARE_PRIMARY_SIMD_LANES];

            let output = srgb8_channel_from_linear_chunk(
                PreparePrimaryDispatch::X64V3(token),
                linear,
                minimum,
            );

            assert_eq!(output[0], 0);
            assert_eq!(output[1], 255);
            assert_eq!(output[2], 0);
        }
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn neon_chunk_sanitizes_non_finite_values_when_available() {
        if let Some(token) = NeonToken::summon() {
            let linear = [
                f32::NAN,
                f32::INFINITY,
                f32::NEG_INFINITY,
                0.5,
                1.0,
                0.0,
                0.25,
                0.75,
            ];
            let minimum = [0.0; PREPARE_PRIMARY_SIMD_LANES];

            let output = srgb8_channel_from_linear_chunk(
                PreparePrimaryDispatch::Neon(token),
                linear,
                minimum,
            );

            assert_eq!(output[0], 0);
            assert_eq!(output[1], 255);
            assert_eq!(output[2], 0);
        }
    }

    fn reference_fill_prepared_row(
        row: &mut [u8],
        image: &RawImage,
        y: u32,
        context: &PreparePrimaryContext,
    ) {
        for x in 0..image.width {
            let source_nits = reference_source_rgb_nits(image, x, y, context);
            let (sdr_linear, minimum_linear) = prepare_linear_output(source_nits, context);
            write_prepared_pixel(row, x as usize, sdr_linear, minimum_linear);
        }
    }

    fn reference_source_rgb_nits(
        image: &RawImage,
        x: u32,
        y: u32,
        context: &PreparePrimaryContext,
    ) -> [f32; 3] {
        let rgb = source_pixel_components(image, x, y);
        match context.source_transfer {
            ColorTransfer::Pq => [
                pq_eotf(rgb[0]) * 10_000.0,
                pq_eotf(rgb[1]) * 10_000.0,
                pq_eotf(rgb[2]) * 10_000.0,
            ],
            ColorTransfer::Hlg => [
                hlg_eotf(rgb[0], context.source_peak_nits),
                hlg_eotf(rgb[1], context.source_peak_nits),
                hlg_eotf(rgb[2], context.source_peak_nits),
            ],
            ColorTransfer::Srgb => [
                srgb_eotf(rgb[0]) * context.source_peak_nits,
                srgb_eotf(rgb[1]) * context.source_peak_nits,
                srgb_eotf(rgb[2]) * context.source_peak_nits,
            ],
            ColorTransfer::Linear => [
                rgb[0] * context.source_peak_nits,
                rgb[1] * context.source_peak_nits,
                rgb[2] * context.source_peak_nits,
            ],
        }
    }

    fn sample_rgba32f() -> RawImage {
        let mut data = Vec::with_capacity((PREPARE_PRIMARY_SIMD_LANES + 1) * 16);
        let samples = [
            [2.0_f32, 0.0, 0.0, 1.0],
            [1.5, 0.6, 0.0, 1.0],
            [0.4, 1.8, 0.0, 1.0],
            [0.3, 0.4, 1.6, 1.0],
            [0.0, 1.5, 0.0, 1.0],
            [0.0, 0.7, 1.6, 1.0],
            [0.0, 1.8, 1.4, 1.0],
            [1.8, 0.0, 1.5, 1.0],
            [1.2, 1.2, 1.2, 1.0],
        ];
        for rgba in samples {
            for value in rgba {
                data.extend_from_slice(&value.to_le_bytes());
            }
        }

        RawImage::from_data(
            (PREPARE_PRIMARY_SIMD_LANES + 1) as u32,
            1,
            PixelFormat::Rgba32F,
            ColorGamut::DisplayP3,
            ColorTransfer::Linear,
            data,
        )
        .unwrap()
    }

    fn sample_rgba16f() -> RawImage {
        let mut data = Vec::with_capacity((PREPARE_PRIMARY_SIMD_LANES + 1) * 8);
        for rgba in sample_pixels() {
            for value in rgba {
                data.extend_from_slice(&half::f16::from_f32(value).to_le_bytes());
            }
        }

        RawImage::from_data(
            (PREPARE_PRIMARY_SIMD_LANES + 1) as u32,
            1,
            PixelFormat::Rgba16F,
            ColorGamut::DisplayP3,
            ColorTransfer::Linear,
            data,
        )
        .unwrap()
    }

    fn sample_pq() -> RawImage {
        let mut data = Vec::with_capacity((PREPARE_PRIMARY_SIMD_LANES + 1) * 4);
        for rgb in sample_packed_rgb() {
            data.extend_from_slice(&pack_1010102([
                ultrahdr_core::color::pq_oetf(rgb[0]),
                ultrahdr_core::color::pq_oetf(rgb[1]),
                ultrahdr_core::color::pq_oetf(rgb[2]),
            ]));
        }

        RawImage::from_data(
            (PREPARE_PRIMARY_SIMD_LANES + 1) as u32,
            1,
            PixelFormat::Rgba1010102Pq,
            ColorGamut::DisplayP3,
            ColorTransfer::Pq,
            data,
        )
        .unwrap()
    }

    fn sample_hlg() -> RawImage {
        let mut data = Vec::with_capacity((PREPARE_PRIMARY_SIMD_LANES + 1) * 4);
        for rgb in sample_packed_rgb() {
            data.extend_from_slice(&pack_1010102([
                ultrahdr_core::color::hlg_oetf(rgb[0]),
                ultrahdr_core::color::hlg_oetf(rgb[1]),
                ultrahdr_core::color::hlg_oetf(rgb[2]),
            ]));
        }

        RawImage::from_data(
            (PREPARE_PRIMARY_SIMD_LANES + 1) as u32,
            1,
            PixelFormat::Rgba1010102Hlg,
            ColorGamut::DisplayP3,
            ColorTransfer::Hlg,
            data,
        )
        .unwrap()
    }

    fn sample_pixels() -> [[f32; 4]; PREPARE_PRIMARY_SIMD_LANES + 1] {
        [
            [2.0_f32, 0.0, 0.0, 1.0],
            [1.5, 0.6, 0.0, 1.0],
            [0.4, 1.8, 0.0, 1.0],
            [0.3, 0.4, 1.6, 1.0],
            [0.0, 1.5, 0.0, 1.0],
            [0.0, 0.7, 1.6, 1.0],
            [0.0, 1.8, 1.4, 1.0],
            [1.8, 0.0, 1.5, 1.0],
            [1.2, 1.2, 1.2, 1.0],
        ]
    }

    fn sample_packed_rgb() -> [[f32; 3]; PREPARE_PRIMARY_SIMD_LANES + 1] {
        [
            [0.10, 0.08, 0.05],
            [0.20, 0.18, 0.15],
            [0.25, 0.32, 0.20],
            [0.30, 0.26, 0.40],
            [0.42, 0.36, 0.28],
            [0.55, 0.44, 0.30],
            [0.60, 0.52, 0.48],
            [0.72, 0.61, 0.54],
            [0.85, 0.74, 0.62],
        ]
    }

    fn pack_1010102(rgb: [f32; 3]) -> [u8; 4] {
        let pack = |value: f32| -> u32 { (value.clamp(0.0, 1.0) * 1023.0).round() as u32 };
        (pack(rgb[0]) | (pack(rgb[1]) << 10) | (pack(rgb[2]) << 20) | (0b11 << 30)).to_le_bytes()
    }
}
