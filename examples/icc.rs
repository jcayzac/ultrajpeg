use ultrahdr_core::{PixelFormat, RawImage};
use ultrajpeg::{ColorMetadata, EncodeOptions, UltraJpegEncoder, decode};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let primary = RawImage::new(4, 4, PixelFormat::Rgb8)?;
    let options = EncodeOptions {
        color_metadata: ColorMetadata {
            icc_profile: Some(b"demo-icc".to_vec()),
            ..ColorMetadata::default()
        },
        ..EncodeOptions::default()
    };

    let bytes = UltraJpegEncoder::new(options).encode(&primary)?;
    let decoded = decode(&bytes)?;
    println!(
        "icc bytes: {}",
        decoded.color_metadata.icc_profile.unwrap_or_default().len()
    );
    Ok(())
}
