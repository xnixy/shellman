//! Writes shellman's exe icons. Run from the repo root:
//!
//!     cargo run --manifest-path tools/icongen/Cargo.toml
//!
//! The drawing lives in `crates/icons` (the apps use it too, to follow the accent at run time).
//! This writes `assets/icons/{shellman,files,settings}.ico` in the default accent, plus two
//! pictures for a quick look: `preview.png` (default accent, dark and light backgrounds) and
//! `preview-accents.png` (the same icons in several accents).

mod ico;

use shellman_icons::{render, Kind, DEFAULT_ACCENT};
use std::{fs, path::Path};

const SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];
const KINDS: [(&str, Kind); 3] = [("shellman", Kind::Shellman), ("files", Kind::Files), ("settings", Kind::Settings)];
const ACCENTS: [[u8; 3]; 12] = [
    DEFAULT_ACCENT,
    [0, 103, 192],
    [28, 138, 76],
    [136, 64, 200],
    [204, 132, 0],
    [255, 220, 80],
    [0, 0, 0],
    [26, 26, 26],
    [128, 128, 128],
    [255, 255, 255],
    [16, 24, 80],
    [244, 244, 246],
];

fn main() {
    let dir = Path::new("assets/icons");
    fs::create_dir_all(dir).unwrap();
    for (name, kind) in KINDS {
        let images: Vec<_> = SIZES.iter().map(|&s| (s, render(kind, s, DEFAULT_ACCENT))).collect();
        fs::write(dir.join(format!("{name}.ico")), ico::write(&images)).unwrap();
    }
    write_preview(&dir.join("preview.png"));
    write_accents(&dir.join("preview-accents.png"));
}

/// Over a background color, straight alpha.
fn blend(bg: [u8; 3], px: &[u8]) -> [u8; 3] {
    let a = px[3] as f32 / 255.0;
    [0, 1, 2].map(|k| (px[k] as f32 * a + bg[k] as f32 * (1.0 - a)) as u8)
}

/// A picture to fill in by `put`, which draws RGBA at a position over the band's background.
struct Sheet {
    w: u32,
    pixels: Vec<u8>,
}

impl Sheet {
    fn new(w: u32, h: u32) -> Self {
        Self { w, pixels: vec![0; (w * h * 3) as usize] }
    }

    fn fill(&mut self, x0: u32, y0: u32, w: u32, h: u32, bg: [u8; 3]) {
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                let at = ((y * self.w + x) * 3) as usize;
                self.pixels[at..at + 3].copy_from_slice(&bg);
            }
        }
    }

    /// Draws a `size` px image enlarged `zoom` times (nearest) with its corner at (x, y).
    fn put(&mut self, x: u32, y: u32, rgba: &[u8], size: u32, zoom: u32, bg: [u8; 3]) {
        for dy in 0..size * zoom {
            for dx in 0..size * zoom {
                let src = (((dy / zoom) * size + dx / zoom) * 4) as usize;
                let at = (((y + dy) * self.w + x + dx) * 3) as usize;
                self.pixels[at..at + 3].copy_from_slice(&blend(bg, &rgba[src..src + 4]));
            }
        }
    }

    fn save(&self, path: &Path) {
        let h = self.pixels.len() as u32 / (self.w * 3);
        let mut enc = png::Encoder::new(fs::File::create(path).unwrap(), self.w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&self.pixels).unwrap();
    }
}

const DARK: [u8; 3] = [32, 32, 32];
const LIGHT: [u8; 3] = [243, 243, 243];

/// Per icon and background: 256, 48, 32, 16 px at true size, then 16 and 32 blown up 4x.
fn write_preview(path: &Path) {
    const BAND: u32 = 280;
    const VIEWS: [(u32, u32); 6] = [(256, 1), (48, 1), (32, 1), (16, 1), (16, 4), (32, 4)];
    let w = 16 + VIEWS.iter().map(|(s, z)| s * z + 16).sum::<u32>();
    let mut sheet = Sheet::new(w, BAND * 6);
    let mut row = 0;
    for (_, kind) in KINDS {
        for bg in [DARK, LIGHT] {
            sheet.fill(0, row * BAND, w, BAND, bg);
            let mut x = 16;
            for (size, zoom) in VIEWS {
                let img = render(kind, size, DEFAULT_ACCENT);
                sheet.put(x, row * BAND + (BAND - size * zoom) / 2, &img, size, zoom, bg);
                x += size * zoom + 16;
            }
            row += 1;
        }
    }
    sheet.save(path);
}

/// One row per accent; each icon at 96, 32 and 16 px, on dark (left) and light (right).
fn write_accents(path: &Path) {
    const BAND: u32 = 112;
    let half = 16 + 3 * (96 + 32 + 16 + 4 * 16);
    let mut sheet = Sheet::new(half * 2, BAND * ACCENTS.len() as u32);
    for (row, accent) in ACCENTS.iter().enumerate() {
        for (side, bg) in [DARK, LIGHT].into_iter().enumerate() {
            let mut x = side as u32 * half + 16;
            sheet.fill(side as u32 * half, row as u32 * BAND, half, BAND, bg);
            for (_, kind) in KINDS {
                for size in [96, 32, 16] {
                    let img = render(kind, size, *accent);
                    sheet.put(x, row as u32 * BAND + (BAND - size) / 2, &img, size, 1, bg);
                    x += size + 16;
                }
                x += 16;
            }
        }
    }
    sheet.save(path);
}
