use ultrajpeg::decode;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).expect("pass a JPEG path");
    let bytes = std::fs::read(path)?;
    let decoded = decode(&bytes)?;
    println!(
        "{}x{}, gain_map={}",
        decoded.image.width,
        decoded.image.height,
        decoded.gain_map.is_some()
    );
    Ok(())
}
