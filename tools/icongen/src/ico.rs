//! Writes a Windows .ico: 256 px as an embedded PNG, smaller sizes as classic 32-bit DIBs
//! (the form every Windows version reads).

fn png_bytes(rgba: &[u8], size: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, size, size);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Best);
    enc.write_header().unwrap().write_image_data(rgba).unwrap();
    out
}

/// A DIB image: header, bottom-up BGRA pixels, then an all-zero AND mask (alpha does the work).
fn dib_bytes(rgba: &[u8], size: u32) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(40u32.to_le_bytes());
    out.extend((size as i32).to_le_bytes());
    out.extend((size as i32 * 2).to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(32u16.to_le_bytes());
    out.extend([0u8; 24]);
    let row_len = size as usize * 4;
    for row in (0..size as usize).rev() {
        for px in rgba[row * row_len..(row + 1) * row_len].chunks(4) {
            out.extend([px[2], px[1], px[0], px[3]]);
        }
    }
    out.extend(vec![0u8; (size as usize).div_ceil(32) * 4 * size as usize]);
    out
}

/// `images` are (size, straight RGBA) pairs.
pub fn write(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let blobs: Vec<Vec<u8>> =
        images.iter().map(|(s, p)| if *s >= 256 { png_bytes(p, *s) } else { dib_bytes(p, *s) }).collect();
    let mut out = vec![0, 0, 1, 0];
    out.extend((images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for ((size, _), blob) in images.iter().zip(&blobs) {
        let dim = if *size >= 256 { 0 } else { *size as u8 };
        out.extend([dim, dim, 0, 0]);
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend((blob.len() as u32).to_le_bytes());
        out.extend(offset.to_le_bytes());
        offset += blob.len() as u32;
    }
    for blob in blobs {
        out.extend(blob);
    }
    out
}
