use crate::{
    Result,
    types::{
        ComputeGainMapOptions, ComputedGainMap, EncodeOptions, GainMapBundle, GainMapChannels,
        UltraHdrEncodeOptions,
    },
};
use ultrahdr_core::{
    ColorGamut, ColorTransfer, GainMapConfig, PixelFormat, RawImage, Unstoppable,
    gainmap::compute_gainmap,
};

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
