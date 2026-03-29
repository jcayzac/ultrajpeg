use ultrahdr_core::{GainMapMetadata, PixelFormat, RawImage};
use ultrajpeg::{EncodeOptions, GainMapEncodeOptions, UltraJpegEncoder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let primary = RawImage::new(8, 8, PixelFormat::Rgb8)?;
    let gain_map = RawImage::new(8, 8, PixelFormat::Gray8)?;

    let options = EncodeOptions {
        gain_map: Some(GainMapEncodeOptions {
            image: gain_map,
            metadata: GainMapMetadata::new(),
            quality: 80,
            progressive: false,
        }),
        ..EncodeOptions::default()
    };

    let bytes = UltraJpegEncoder::new(options).encode(&primary)?;
    println!("encoded {} bytes", bytes.len());
    Ok(())
}
