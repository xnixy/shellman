//! Pictures on the clipboard: a device-independent bitmap (`CF_DIB`, `CF_DIBV5`) read into
//! RGBA, and RGBA written as PNG.

/// A picture as rows of RGBA, top row first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

const BI_RGB: u32 = 0;
const BI_BITFIELDS: u32 = 3;
const BI_ALPHABITFIELDS: u32 = 6;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// One channel out of a pixel by its mask, scaled to 0..=255.
fn channel(px: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let max = mask >> shift;
    let v = (px & mask) >> shift;
    ((v * 255 + max / 2) / max) as u8
}

/// A DIB's bytes (a `BITMAPINFOHEADER` or a later one, then masks, colours and pixels) as RGBA.
/// 32, 24 and 8 bits per pixel; None for anything else or anything malformed.
pub fn decode(b: &[u8]) -> Option<Rgba> {
    let header = u32_at(b, 0)? as usize;
    if header < 40 {
        return None;
    }
    let width = i32::from_le_bytes(b.get(4..8)?.try_into().ok()?);
    let height = i32::from_le_bytes(b.get(8..12)?.try_into().ok()?);
    let bits = u16_at(b, 14)?;
    let compression = u32_at(b, 16)?;
    let used = u32_at(b, 32)? as usize;
    if width <= 0 || height == 0 || width > 32_768 || height.unsigned_abs() > 32_768 {
        return None;
    }
    let (w, h) = (width as usize, height.unsigned_abs() as usize);
    let top_down = height < 0;
    // Masks: inside a V4/V5 header, or after a plain one.
    let (masks, after_header) = match compression {
        BI_RGB => (None, header),
        BI_BITFIELDS | BI_ALPHABITFIELDS if header >= 56 => (
            Some([u32_at(b, 40)?, u32_at(b, 44)?, u32_at(b, 48)?, u32_at(b, 52)?]),
            header,
        ),
        BI_BITFIELDS => (
            Some([u32_at(b, header)?, u32_at(b, header + 4)?, u32_at(b, header + 8)?, 0]),
            header + 12,
        ),
        BI_ALPHABITFIELDS => (
            Some([
                u32_at(b, header)?,
                u32_at(b, header + 4)?,
                u32_at(b, header + 8)?,
                u32_at(b, header + 12)?,
            ]),
            header + 16,
        ),
        _ => return None,
    };
    let colours = if bits <= 8 {
        if used == 0 {
            1usize << bits
        } else {
            used
        }
    } else {
        used
    };
    let stride = (w * bits as usize).div_ceil(32) * 4;
    let size = stride.checked_mul(h)?;
    let nominal = after_header + colours * 4;
    // Some writers put the masks after a V5 header as well; the pixels are the last bytes.
    let start = if b.len() >= size && (nominal..=nominal + 12).contains(&(b.len() - size)) {
        b.len() - size
    } else {
        nominal
    };
    let data = b.get(start..start + size)?;
    let palette = b.get(after_header..after_header + colours * 4);
    let mut pixels = vec![0u8; w * h * 4];
    let mut any_alpha = false;
    for y in 0..h {
        let src_row = if top_down { y } else { h - 1 - y };
        let row = &data[src_row * stride..src_row * stride + stride];
        for x in 0..w {
            let out = &mut pixels[(y * w + x) * 4..(y * w + x) * 4 + 4];
            match bits {
                32 => {
                    let px = u32::from_le_bytes(row[x * 4..x * 4 + 4].try_into().ok()?);
                    let [r, g, bl, a] = match masks {
                        Some([mr, mg, mb, ma]) => {
                            [channel(px, mr), channel(px, mg), channel(px, mb), channel(px, ma)]
                        }
                        None => [row[x * 4 + 2], row[x * 4 + 1], row[x * 4], row[x * 4 + 3]],
                    };
                    any_alpha |= a != 0;
                    out.copy_from_slice(&[r, g, bl, a]);
                }
                24 => out.copy_from_slice(&[row[x * 3 + 2], row[x * 3 + 1], row[x * 3], 255]),
                8 => {
                    let i = row[x] as usize * 4;
                    let c = palette?.get(i..i + 4)?;
                    out.copy_from_slice(&[c[2], c[1], c[0], 255]);
                }
                _ => return None,
            }
        }
    }
    // 32-bit pictures with no alpha at all mean opaque, not invisible.
    let has_alpha_channel = bits == 32 && masks.is_none_or(|m| m[3] != 0);
    if bits == 32 && (!any_alpha || !has_alpha_channel) {
        for px in pixels.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
    }
    Some(Rgba {
        width: w as u32,
        height: h as u32,
        pixels,
    })
}

/// RGBA as a PNG file's bytes.
pub fn png(img: &Rgba) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, img.width, img.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().ok()?;
        w.write_image_data(&img.pixels).ok()?;
    }
    Some(out)
}

/// Whether bytes are a PNG file.
pub fn is_png(b: &[u8]) -> bool {
    b.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain 40-byte header.
    fn header(w: i32, h: i32, bits: u16, compression: u32) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&40u32.to_le_bytes());
        b.extend_from_slice(&w.to_le_bytes());
        b.extend_from_slice(&h.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&bits.to_le_bytes());
        b.extend_from_slice(&compression.to_le_bytes());
        b.extend_from_slice(&[0; 20]);
        b
    }

    #[test]
    fn a_bottom_up_24_bit_picture() {
        // 2x2: bottom row blue, green; top row red, white. Rows padded to 8 bytes.
        let mut b = header(2, 2, 24, BI_RGB);
        b.extend_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0]);
        b.extend_from_slice(&[0, 0, 255, 255, 255, 255, 0, 0]);
        let img = decode(&b).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        assert_eq!(&img.pixels[0..4], &[255, 0, 0, 255], "top left is red");
        assert_eq!(&img.pixels[4..8], &[255, 255, 255, 255]);
        assert_eq!(&img.pixels[8..12], &[0, 0, 255, 255], "bottom left is blue");
        assert_eq!(&img.pixels[12..16], &[0, 255, 0, 255]);
    }

    #[test]
    fn a_top_down_32_bit_picture_without_alpha_is_opaque() {
        let mut b = header(1, -2, 32, BI_RGB);
        b.extend_from_slice(&[10, 20, 30, 0, 40, 50, 60, 0]);
        let img = decode(&b).unwrap();
        assert_eq!(img.pixels, [30, 20, 10, 255, 60, 50, 40, 255]);
    }

    #[test]
    fn bitfields_with_alpha_keep_it() {
        // A V5-sized header with masks inside it.
        let mut b = header(1, 1, 32, BI_BITFIELDS);
        b[0..4].copy_from_slice(&124u32.to_le_bytes());
        b.truncate(40);
        for m in [0x00ff_0000u32, 0x0000_ff00, 0x0000_00ff, 0xff00_0000] {
            b.extend_from_slice(&m.to_le_bytes());
        }
        b.resize(124, 0);
        b.extend_from_slice(&0x80_11_22_33u32.to_le_bytes());
        let img = decode(&b).unwrap();
        assert_eq!(img.pixels, [0x11, 0x22, 0x33, 0x80]);
        // The same with the masks repeated after the header, as some writers do.
        let mut extra = b[..124].to_vec();
        extra.extend_from_slice(&[0; 12]);
        extra.extend_from_slice(&0x80_11_22_33u32.to_le_bytes());
        assert_eq!(decode(&extra).unwrap().pixels, [0x11, 0x22, 0x33, 0x80]);
    }

    #[test]
    fn a_paletted_picture() {
        let mut b = header(2, 1, 8, BI_RGB);
        b[32..36].copy_from_slice(&2u32.to_le_bytes());
        b.extend_from_slice(&[0, 0, 255, 0, 0, 255, 0, 0]);
        b.extend_from_slice(&[1, 0, 0, 0]);
        assert_eq!(decode(&b).unwrap().pixels, [0, 255, 0, 255, 255, 0, 0, 255]);
    }

    #[test]
    fn nonsense_is_refused() {
        assert_eq!(decode(&[]), None);
        assert_eq!(decode(&header(0, 1, 32, BI_RGB)), None);
        assert_eq!(decode(&header(1, 1, 32, BI_RGB)), None, "no pixels");
        assert_eq!(decode(&header(1, 1, 4, 2)), None, "RLE");
    }

    #[test]
    fn png_out() {
        let img = Rgba {
            width: 2,
            height: 1,
            pixels: vec![1, 2, 3, 4, 5, 6, 7, 8],
        };
        let bytes = png(&img).unwrap();
        assert!(is_png(&bytes));
        let dec = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut r = dec.read_info().unwrap();
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        r.next_frame(&mut buf).unwrap();
        assert_eq!(buf, img.pixels);
    }
}
