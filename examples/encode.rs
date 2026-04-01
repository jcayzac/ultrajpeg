use ultrahdr_core::{GainMapMetadata, PixelFormat, RawImage};
use ultrajpeg::{CompressionEffort, EncodeOptions, Encoder, GainMapBundle};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let primary = RawImage::new(8, 8, PixelFormat::Rgb8)?;
    let gain_map = RawImage::new(8, 8, PixelFormat::Gray8)?;

    let options = EncodeOptions {
        gain_map: Some(GainMapBundle {
            image: gain_map,
            metadata: GainMapMetadata::new(),
            quality: 80,
            progressive: false,
            compression: CompressionEffort::Balanced,
        }),
        ..EncodeOptions::default()
    };

    let bytes = Encoder::new(options).encode(&primary)?;
    println!("encoded {} bytes", bytes.len());
    Ok(())
}
