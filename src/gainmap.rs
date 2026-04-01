use crate::{
    Error, Result,
    types::{
        ColorMetadata, ComputeGainMapOptions, ComputedGainMap, EncodeOptions, GainMapBundle,
        GainMapChannels, PreparePrimaryOptions, PreparedPrimary, PrimaryMetadata,
        UltraHdrEncodeOptions,
    },
};
use rayon::prelude::*;
use ultrahdr_core::{
    ColorGamut, ColorTransfer, GainMapConfig, PixelFormat, RawImage, Unstoppable,
    color::{
        bt2390_tonemap, convert_gamut, hlg_eotf, pq_eotf, rgb_to_luminance, soft_clip_gamut,
        srgb_eotf, srgb_oetf,
    },
    gainmap::compute_gainmap,
};

const PREPARE_PRIMARY_PARALLEL_THRESHOLD_PIXELS: usize = 256 * 256;
const DEFAULT_MAX_GAIN_MAP_BOOST: f32 = 6.0;

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

    if width * height >= PREPARE_PRIMARY_PARALLEL_THRESHOLD_PIXELS {
        data.par_chunks_mut(row_len)
            .enumerate()
            .for_each(|(y, row)| {
                fill_prepared_row(row, image, y as u32, options, source_peak_nits)
            });
    } else {
        for (y, row) in data.chunks_mut(row_len).enumerate() {
            fill_prepared_row(row, image, y as u32, options, source_peak_nits);
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

    if options.target_peak_nits <= 0.0 {
        return Err(Error::InvalidInput(
            "PreparePrimaryOptions::target_peak_nits must be positive".into(),
        ));
    }

    if options.source_peak_nits.is_some_and(|value| value <= 0.0) {
        return Err(Error::InvalidInput(
            "PreparePrimaryOptions::source_peak_nits must be positive when set".into(),
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

fn fill_prepared_row(
    row: &mut [u8],
    image: &RawImage,
    y: u32,
    options: &PreparePrimaryOptions,
    source_peak_nits: f32,
) {
    for x in 0..image.width {
        let source_nits = source_rgb_nits(image, x, y, source_peak_nits);
        let target_nits = convert_gamut(source_nits, image.gamut, options.target_gamut);
        let sdr_linear = tone_map_to_sdr(
            target_nits,
            options.target_gamut,
            source_peak_nits,
            options.target_peak_nits,
        );
        let minimum_linear = [
            (target_nits[0] / source_peak_nits / DEFAULT_MAX_GAIN_MAP_BOOST).max(0.0),
            (target_nits[1] / source_peak_nits / DEFAULT_MAX_GAIN_MAP_BOOST).max(0.0),
            (target_nits[2] / source_peak_nits / DEFAULT_MAX_GAIN_MAP_BOOST).max(0.0),
        ];
        let output = srgb8_from_linear([
            sdr_linear[0].max(minimum_linear[0]),
            sdr_linear[1].max(minimum_linear[1]),
            sdr_linear[2].max(minimum_linear[2]),
        ]);
        let offset = x as usize * 3;
        row[offset] = output[0];
        row[offset + 1] = output[1];
        row[offset + 2] = output[2];
    }
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

fn source_rgb_nits(image: &RawImage, x: u32, y: u32, source_peak_nits: f32) -> [f32; 3] {
    let rgb = source_pixel_components(image, x, y);
    match effective_source_transfer(image) {
        ColorTransfer::Pq => [
            pq_eotf(rgb[0]) * 10_000.0,
            pq_eotf(rgb[1]) * 10_000.0,
            pq_eotf(rgb[2]) * 10_000.0,
        ],
        ColorTransfer::Hlg => [
            hlg_eotf(rgb[0], source_peak_nits),
            hlg_eotf(rgb[1], source_peak_nits),
            hlg_eotf(rgb[2], source_peak_nits),
        ],
        ColorTransfer::Srgb => [
            srgb_eotf(rgb[0]) * source_peak_nits,
            srgb_eotf(rgb[1]) * source_peak_nits,
            srgb_eotf(rgb[2]) * source_peak_nits,
        ],
        ColorTransfer::Linear => [
            rgb[0] * source_peak_nits,
            rgb[1] * source_peak_nits,
            rgb[2] * source_peak_nits,
        ],
    }
}

fn tone_map_to_sdr(
    rgb_nits: [f32; 3],
    gamut: ColorGamut,
    source_peak_nits: f32,
    target_peak_nits: f32,
) -> [f32; 3] {
    let luminance = rgb_to_luminance(rgb_nits, gamut);
    if luminance <= 0.0 {
        return [0.0; 3];
    }

    let source_normalized = luminance / source_peak_nits;
    let target_normalized = if source_peak_nits > target_peak_nits {
        bt2390_tonemap(source_normalized, 1.0, target_peak_nits / source_peak_nits)
    } else {
        source_normalized
    };
    let luminance_ratio = if source_normalized > 0.0 {
        target_normalized / source_normalized
    } else {
        0.0
    };

    soft_clip_gamut([
        rgb_nits[0] / source_peak_nits * luminance_ratio,
        rgb_nits[1] / source_peak_nits * luminance_ratio,
        rgb_nits[2] / source_peak_nits * luminance_ratio,
    ])
}

fn srgb8_from_linear(rgb: [f32; 3]) -> [u8; 3] {
    [
        (srgb_oetf(rgb[0].clamp(0.0, 1.0)) * 255.0).round() as u8,
        (srgb_oetf(rgb[1].clamp(0.0, 1.0)) * 255.0).round() as u8,
        (srgb_oetf(rgb[2].clamp(0.0, 1.0)) * 255.0).round() as u8,
    ]
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
