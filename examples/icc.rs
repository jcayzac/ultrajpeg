use ultrahdr_core::{PixelFormat, RawImage};
use ultrajpeg::{ColorMetadata, EncodeOptions, Encoder, PrimaryMetadata, decode};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let primary = RawImage::new(4, 4, PixelFormat::Rgb8)?;
    let options = EncodeOptions {
        primary_metadata: PrimaryMetadata {
            color: ColorMetadata {
                icc_profile: Some(b"demo-icc".to_vec()),
                ..ColorMetadata::default()
            },
            exif: None,
        },
        ..EncodeOptions::default()
    };

    let bytes = Encoder::new(options).encode(&primary)?;
    let decoded = decode(&bytes)?;
    println!(
        "icc bytes: {}",
        decoded
            .primary_metadata
            .color
            .icc_profile
            .unwrap_or_default()
            .len()
    );
    Ok(())
}
