use crate::{Error, Result};
use ultrahdr_core::{
    ColorTransfer, GainMap, GainMapMetadata, PixelFormat, RawImage, Unstoppable,
    color::{pq_oetf, srgb_eotf, srgb_oetf},
    gainmap::{HdrOutputFormat, apply_gainmap},
};

const SRGB_8BIT_LUT_SIZE: usize = 256;
const PQ_OUTPUT_SCALE: f32 = 203.0 / 10_000.0;

#[derive(Clone, Copy)]
struct AxisSample {
    start: u32,
    end: u32,
    frac: f32,
}

struct GainMapLut {
    table: Box<[f32; 256 * 3]>,
}

impl GainMapLut {
    fn new(metadata: &GainMapMetadata, weight: f32) -> Self {
        let mut table = Box::new([0.0f32; 256 * 3]);

        for channel in 0..3 {
            let gamma = metadata.gamma[channel];
            let log_min = metadata.min_content_boost[channel].ln();
            let log_max = metadata.max_content_boost[channel].ln();
            let log_range = log_max - log_min;

            for i in 0..256 {
                let normalized = i as f32 / 255.0;
                let linear = if gamma != 1.0 && gamma > 0.0 {
                    normalized.powf(1.0 / gamma)
                } else {
                    normalized
                };
                let log_gain = log_min + linear * log_range;
                table[channel * 256 + i] = (log_gain * weight).exp();
            }
        }

        Self { table }
    }

    #[inline(always)]
    fn lookup(&self, byte_value: u8, channel: usize) -> f32 {
        self.table[channel * 256 + byte_value as usize]
    }
}

struct ReconstructContext<'a> {
    sdr: &'a RawImage,
    gainmap: &'a GainMap,
    metadata: &'a GainMapMetadata,
    gain_lut: GainMapLut,
    srgb_lut: [f32; SRGB_8BIT_LUT_SIZE],
    x_samples: Vec<AxisSample>,
    y_samples: Vec<AxisSample>,
    output_format: HdrOutputFormat,
}

impl<'a> ReconstructContext<'a> {
    fn new(
        sdr: &'a RawImage,
        gainmap: &'a GainMap,
        metadata: &'a GainMapMetadata,
        display_boost: f32,
        output_format: HdrOutputFormat,
    ) -> Self {
        Self {
            sdr,
            gainmap,
            metadata,
            gain_lut: GainMapLut::new(metadata, calculate_weight(display_boost, metadata)),
            srgb_lut: core::array::from_fn(|value| srgb_eotf(value as f32 / 255.0)),
            x_samples: build_axis_samples(sdr.width, gainmap.width),
            y_samples: build_axis_samples(sdr.height, gainmap.height),
            output_format,
        }
    }
}

pub(crate) fn reconstruct_hdr_image(
    sdr: &RawImage,
    gainmap: &GainMap,
    metadata: &GainMapMetadata,
    display_boost: f32,
    output_format: HdrOutputFormat,
) -> Result<RawImage> {
    validate_reconstruct_inputs(display_boost, metadata)?;

    if let Some(result) =
        try_reconstruct_hdr_fast_path(sdr, gainmap, metadata, display_boost, output_format)
    {
        return result;
    }

    apply_gainmap(
        sdr,
        gainmap,
        metadata,
        display_boost,
        output_format,
        Unstoppable,
    )
    .map_err(Into::into)
}

fn validate_reconstruct_inputs(display_boost: f32, metadata: &GainMapMetadata) -> Result<()> {
    validate_positive_finite("display_boost", display_boost)?;
    validate_positive_finite("hdr_capacity_min", metadata.hdr_capacity_min)?;
    validate_positive_finite("hdr_capacity_max", metadata.hdr_capacity_max)?;
    if metadata.hdr_capacity_max < metadata.hdr_capacity_min {
        return Err(Error::InvalidInput(
            "hdr_capacity_max must be >= hdr_capacity_min".into(),
        ));
    }

    for channel in 0..3 {
        validate_positive_finite(
            &format!("min_content_boost[{channel}]"),
            metadata.min_content_boost[channel],
        )?;
        validate_positive_finite(
            &format!("max_content_boost[{channel}]"),
            metadata.max_content_boost[channel],
        )?;
        if metadata.max_content_boost[channel] < metadata.min_content_boost[channel] {
            return Err(Error::InvalidInput(format!(
                "max_content_boost[{channel}] must be >= min_content_boost[{channel}]"
            )));
        }
        validate_positive_finite(&format!("gamma[{channel}]"), metadata.gamma[channel])?;
        validate_finite(
            &format!("offset_sdr[{channel}]"),
            metadata.offset_sdr[channel],
        )?;
        validate_finite(
            &format!("offset_hdr[{channel}]"),
            metadata.offset_hdr[channel],
        )?;
    }

    Ok(())
}

fn validate_positive_finite(name: &str, value: f32) -> Result<()> {
    if !value.is_finite() || value <= 0.0 {
        return Err(Error::InvalidInput(format!(
            "{name} must be finite and > 0"
        )));
    }
    Ok(())
}

fn validate_finite(name: &str, value: f32) -> Result<()> {
    if !value.is_finite() {
        return Err(Error::InvalidInput(format!("{name} must be finite")));
    }
    Ok(())
}

fn try_reconstruct_hdr_fast_path(
    sdr: &RawImage,
    gainmap: &GainMap,
    metadata: &GainMapMetadata,
    display_boost: f32,
    output_format: HdrOutputFormat,
) -> Option<Result<RawImage>> {
    if !matches!(sdr.format, PixelFormat::Rgb8 | PixelFormat::Rgba8) {
        return None;
    }
    if !matches!(
        output_format,
        HdrOutputFormat::LinearFloat | HdrOutputFormat::Pq1010102 | HdrOutputFormat::Srgb8
    ) {
        return None;
    }
    if !matches!(gainmap.channels, 1 | 3) {
        return None;
    }

    Some(reconstruct_hdr_fast_path(
        sdr,
        gainmap,
        metadata,
        display_boost,
        output_format,
    ))
}

fn reconstruct_hdr_fast_path(
    sdr: &RawImage,
    gainmap: &GainMap,
    metadata: &GainMapMetadata,
    display_boost: f32,
    output_format: HdrOutputFormat,
) -> Result<RawImage> {
    let width = sdr.width;
    let height = sdr.height;
    let context = ReconstructContext::new(sdr, gainmap, metadata, display_boost, output_format);

    let mut output = match output_format {
        HdrOutputFormat::LinearFloat => {
            let mut img = RawImage::new(width, height, PixelFormat::Rgba32F)?;
            img.transfer = ColorTransfer::Linear;
            img.gamut = sdr.gamut;
            img
        }
        HdrOutputFormat::Pq1010102 => {
            let mut img = RawImage::new(width, height, PixelFormat::Rgba1010102Pq)?;
            img.transfer = ColorTransfer::Pq;
            img.gamut = sdr.gamut;
            img
        }
        HdrOutputFormat::Srgb8 => {
            let mut img = RawImage::new(width, height, PixelFormat::Rgba8)?;
            img.transfer = ColorTransfer::Srgb;
            img.gamut = sdr.gamut;
            img
        }
    };

    let row_stride = output.stride as usize;
    for (y, row) in output.data.chunks_mut(row_stride).enumerate() {
        reconstruct_row(row, y as u32, &context);
    }

    Ok(output)
}

fn reconstruct_row(row: &mut [u8], y: u32, context: &ReconstructContext<'_>) {
    let y_sample = context.y_samples[y as usize];

    match context.output_format {
        HdrOutputFormat::LinearFloat => {
            for (x, x_sample) in context.x_samples.iter().copied().enumerate() {
                let sdr_linear = get_sdr_linear_fast(context.sdr, x, y, &context.srgb_lut);
                let gain =
                    sample_gainmap_lut_fast(context.gainmap, &context.gain_lut, x_sample, y_sample);
                let hdr = apply_gain_fast(sdr_linear, gain, context.metadata);
                write_linear_float_pixel(row, x, hdr);
            }
        }
        HdrOutputFormat::Pq1010102 => {
            for (x, x_sample) in context.x_samples.iter().copied().enumerate() {
                let sdr_linear = get_sdr_linear_fast(context.sdr, x, y, &context.srgb_lut);
                let gain =
                    sample_gainmap_lut_fast(context.gainmap, &context.gain_lut, x_sample, y_sample);
                let hdr = apply_gain_fast(sdr_linear, gain, context.metadata);
                write_pq1010102_pixel(row, x, hdr);
            }
        }
        HdrOutputFormat::Srgb8 => {
            for (x, x_sample) in context.x_samples.iter().copied().enumerate() {
                let sdr_linear = get_sdr_linear_fast(context.sdr, x, y, &context.srgb_lut);
                let gain =
                    sample_gainmap_lut_fast(context.gainmap, &context.gain_lut, x_sample, y_sample);
                let hdr = apply_gain_fast(sdr_linear, gain, context.metadata);
                write_srgb8_pixel(row, x, hdr);
            }
        }
    }
}

fn build_axis_samples(image_len: u32, gain_map_len: u32) -> Vec<AxisSample> {
    (0..image_len)
        .map(|value| {
            let gain_map_coord = (value as f32 / image_len as f32) * gain_map_len as f32;
            let start = (gain_map_coord.floor() as u32).min(gain_map_len - 1);
            let end = (start + 1).min(gain_map_len - 1);
            AxisSample {
                start,
                end,
                frac: gain_map_coord - gain_map_coord.floor(),
            }
        })
        .collect()
}

#[inline(always)]
fn calculate_weight(display_boost: f32, metadata: &GainMapMetadata) -> f32 {
    let log_display = display_boost.max(1.0).ln();
    let log_min = metadata.hdr_capacity_min.max(1.0).ln();
    let log_max = metadata.hdr_capacity_max.max(1.0).ln();

    if log_max <= log_min {
        return 1.0;
    }

    ((log_display - log_min) / (log_max - log_min)).clamp(0.0, 1.0)
}

#[inline(always)]
fn get_sdr_linear_fast(
    sdr: &RawImage,
    x: usize,
    y: u32,
    srgb_lut: &[f32; SRGB_8BIT_LUT_SIZE],
) -> [f32; 3] {
    let row_start = y as usize * sdr.stride as usize;
    match sdr.format {
        PixelFormat::Rgb8 => {
            let index = row_start + x * 3;
            [
                srgb_lut[sdr.data[index] as usize],
                srgb_lut[sdr.data[index + 1] as usize],
                srgb_lut[sdr.data[index + 2] as usize],
            ]
        }
        PixelFormat::Rgba8 => {
            let index = row_start + x * 4;
            [
                srgb_lut[sdr.data[index] as usize],
                srgb_lut[sdr.data[index + 1] as usize],
                srgb_lut[sdr.data[index + 2] as usize],
            ]
        }
        _ => unreachable!("unsupported fast-path SDR format"),
    }
}

#[inline(always)]
fn sample_gainmap_lut_fast(
    gainmap: &GainMap,
    lut: &GainMapLut,
    x_sample: AxisSample,
    y_sample: AxisSample,
) -> [f32; 3] {
    if gainmap.channels == 1 {
        let row0 = y_sample.start as usize * gainmap.width as usize;
        let row1 = y_sample.end as usize * gainmap.width as usize;
        let g00 = lut.lookup(gainmap.data[row0 + x_sample.start as usize], 0);
        let g10 = lut.lookup(gainmap.data[row0 + x_sample.end as usize], 0);
        let g01 = lut.lookup(gainmap.data[row1 + x_sample.start as usize], 0);
        let g11 = lut.lookup(gainmap.data[row1 + x_sample.end as usize], 0);
        let gain = bilinear(g00, g10, g01, g11, x_sample.frac, y_sample.frac);
        [gain, gain, gain]
    } else {
        let row0 = y_sample.start as usize * gainmap.width as usize * 3;
        let row1 = y_sample.end as usize * gainmap.width as usize * 3;
        let x0 = x_sample.start as usize * 3;
        let x1 = x_sample.end as usize * 3;
        [
            bilinear(
                lut.lookup(gainmap.data[row0 + x0], 0),
                lut.lookup(gainmap.data[row0 + x1], 0),
                lut.lookup(gainmap.data[row1 + x0], 0),
                lut.lookup(gainmap.data[row1 + x1], 0),
                x_sample.frac,
                y_sample.frac,
            ),
            bilinear(
                lut.lookup(gainmap.data[row0 + x0 + 1], 1),
                lut.lookup(gainmap.data[row0 + x1 + 1], 1),
                lut.lookup(gainmap.data[row1 + x0 + 1], 1),
                lut.lookup(gainmap.data[row1 + x1 + 1], 1),
                x_sample.frac,
                y_sample.frac,
            ),
            bilinear(
                lut.lookup(gainmap.data[row0 + x0 + 2], 2),
                lut.lookup(gainmap.data[row0 + x1 + 2], 2),
                lut.lookup(gainmap.data[row1 + x0 + 2], 2),
                lut.lookup(gainmap.data[row1 + x1 + 2], 2),
                x_sample.frac,
                y_sample.frac,
            ),
        ]
    }
}

#[inline(always)]
fn bilinear(v00: f32, v10: f32, v01: f32, v11: f32, fx: f32, fy: f32) -> f32 {
    let top = v00 * (1.0 - fx) + v10 * fx;
    let bottom = v01 * (1.0 - fx) + v11 * fx;
    top * (1.0 - fy) + bottom * fy
}

#[inline(always)]
fn apply_gain_fast(sdr_linear: [f32; 3], gain: [f32; 3], metadata: &GainMapMetadata) -> [f32; 3] {
    [
        (sdr_linear[0] + metadata.offset_sdr[0]) * gain[0] - metadata.offset_hdr[0],
        (sdr_linear[1] + metadata.offset_sdr[1]) * gain[1] - metadata.offset_hdr[1],
        (sdr_linear[2] + metadata.offset_sdr[2]) * gain[2] - metadata.offset_hdr[2],
    ]
}

#[inline(always)]
fn write_linear_float_pixel(row: &mut [u8], x: usize, hdr: [f32; 3]) {
    let index = x * 16;
    row[index..index + 4].copy_from_slice(&hdr[0].to_le_bytes());
    row[index + 4..index + 8].copy_from_slice(&hdr[1].to_le_bytes());
    row[index + 8..index + 12].copy_from_slice(&hdr[2].to_le_bytes());
    row[index + 12..index + 16].copy_from_slice(&1.0f32.to_le_bytes());
}

#[inline(always)]
fn write_pq1010102_pixel(row: &mut [u8], x: usize, hdr: [f32; 3]) {
    let r = quantize_pq1010102(hdr[0]);
    let g = quantize_pq1010102(hdr[1]);
    let b = quantize_pq1010102(hdr[2]);
    let packed = r | (g << 10) | (b << 20) | (3u32 << 30);
    let index = x * 4;
    row[index..index + 4].copy_from_slice(&packed.to_le_bytes());
}

#[inline(always)]
fn write_srgb8_pixel(row: &mut [u8], x: usize, hdr: [f32; 3]) {
    let index = x * 4;
    row[index] = quantize_srgb8(hdr[0]);
    row[index + 1] = quantize_srgb8(hdr[1]);
    row[index + 2] = quantize_srgb8(hdr[2]);
    row[index + 3] = 255;
}

#[inline(always)]
fn quantize_pq1010102(value: f32) -> u32 {
    let linear = if value.is_nan() {
        0.0
    } else if value.is_infinite() {
        if value.is_sign_positive() {
            f32::INFINITY
        } else {
            0.0
        }
    } else if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    };
    let encoded = pq_oetf(linear * PQ_OUTPUT_SCALE);
    if !encoded.is_finite() {
        return if linear.is_sign_positive() { 1023 } else { 0 };
    }
    (encoded * 1023.0).round().clamp(0.0, 1023.0) as u32
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

#[cfg(test)]
mod tests {
    use super::{quantize_pq1010102, quantize_srgb8, reconstruct_hdr_image};
    use ultrahdr_core::{
        ColorGamut, ColorTransfer, GainMap, GainMapMetadata, PixelFormat, RawImage, Unstoppable,
        gainmap::{HdrOutputFormat, apply_gainmap},
    };

    #[test]
    fn fast_path_matches_reference_single_channel_linear_float() {
        assert_fast_path_matches_reference(PixelFormat::Rgb8, 1, HdrOutputFormat::LinearFloat);
    }

    #[test]
    fn fast_path_matches_reference_single_channel_pq() {
        assert_fast_path_matches_reference(PixelFormat::Rgb8, 1, HdrOutputFormat::Pq1010102);
    }

    #[test]
    fn fast_path_matches_reference_multichannel_srgb() {
        assert_fast_path_matches_reference(PixelFormat::Rgba8, 3, HdrOutputFormat::Srgb8);
    }

    fn assert_fast_path_matches_reference(
        sdr_format: PixelFormat,
        gain_map_channels: u8,
        output_format: HdrOutputFormat,
    ) {
        let sdr = sample_sdr(sdr_format);
        let gain_map = sample_gain_map(gain_map_channels);
        let metadata = sample_metadata(gain_map_channels);

        let fast = reconstruct_hdr_image(&sdr, &gain_map, &metadata, 4.0, output_format).unwrap();
        let reference =
            apply_gainmap(&sdr, &gain_map, &metadata, 4.0, output_format, Unstoppable).unwrap();

        assert_eq!(fast.format, reference.format);
        assert_eq!(fast.width, reference.width);
        assert_eq!(fast.height, reference.height);
        assert_eq!(fast.gamut, reference.gamut);
        assert_eq!(fast.transfer, reference.transfer);
        assert_eq!(fast.data, reference.data);
    }

    #[test]
    fn reconstruct_rejects_non_finite_display_boost() {
        let sdr = sample_sdr(PixelFormat::Rgb8);
        let gain_map = sample_gain_map(1);
        let metadata = sample_metadata(1);

        let error =
            reconstruct_hdr_image(&sdr, &gain_map, &metadata, f32::NAN, HdrOutputFormat::Srgb8)
                .unwrap_err();

        assert!(error.to_string().contains("display_boost"));
    }

    #[test]
    fn reconstruct_rejects_non_finite_metadata() {
        let sdr = sample_sdr(PixelFormat::Rgb8);
        let gain_map = sample_gain_map(1);
        let mut metadata = sample_metadata(1);
        metadata.max_content_boost[0] = f32::INFINITY;

        let error = reconstruct_hdr_image(&sdr, &gain_map, &metadata, 4.0, HdrOutputFormat::Srgb8)
            .unwrap_err();

        assert!(error.to_string().contains("max_content_boost[0]"));
    }

    #[test]
    fn quantizers_sanitize_non_finite_values() {
        assert_eq!(quantize_srgb8(f32::NAN), 0);
        assert_eq!(quantize_srgb8(f32::INFINITY), 255);
        assert_eq!(quantize_pq1010102(f32::NEG_INFINITY), 0);
        assert_eq!(quantize_pq1010102(f32::INFINITY), 1023);
    }

    fn sample_sdr(format: PixelFormat) -> RawImage {
        let width = 7;
        let height = 5;
        let mut data = Vec::with_capacity(
            width as usize * height as usize * if format == PixelFormat::Rgb8 { 3 } else { 4 },
        );

        for y in 0..height {
            for x in 0..width {
                let rgb = [
                    ((x * 255) / width.max(1)) as u8,
                    ((y * 255) / height.max(1)) as u8,
                    (((x + y) * 255) / (width + height).max(1)) as u8,
                ];
                data.extend_from_slice(&rgb);
                if format == PixelFormat::Rgba8 {
                    data.push(255);
                }
            }
        }

        RawImage::from_data(
            width,
            height,
            format,
            ColorGamut::DisplayP3,
            ColorTransfer::Srgb,
            data,
        )
        .unwrap()
    }

    fn sample_gain_map(channels: u8) -> GainMap {
        let width = 3;
        let height = 2;
        let mut gain_map = if channels == 1 {
            GainMap::new(width, height).unwrap()
        } else {
            GainMap::new_multichannel(width, height).unwrap()
        };

        if channels == 1 {
            for (index, value) in gain_map.data.iter_mut().enumerate() {
                *value = [32, 96, 160, 224, 128, 200][index];
            }
        } else {
            for (index, pixel) in gain_map.data.chunks_exact_mut(3).enumerate() {
                let values = [
                    [16, 64, 112],
                    [48, 96, 144],
                    [80, 128, 176],
                    [112, 160, 208],
                    [144, 192, 224],
                    [176, 208, 240],
                ][index];
                pixel.copy_from_slice(&values);
            }
        }

        gain_map
    }

    fn sample_metadata(channels: u8) -> GainMapMetadata {
        if channels == 1 {
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
        } else {
            GainMapMetadata {
                max_content_boost: [3.0, 4.0, 5.0],
                min_content_boost: [1.0, 1.2, 1.4],
                gamma: [1.0, 1.1, 1.2],
                offset_sdr: [1.0 / 64.0; 3],
                offset_hdr: [1.0 / 64.0; 3],
                hdr_capacity_min: 1.0,
                hdr_capacity_max: 5.0,
                use_base_color_space: true,
            }
        }
    }
}
