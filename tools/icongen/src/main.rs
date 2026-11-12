//! Draws shellman's icons. Run from the repo root:
//!
//!     cargo run --manifest-path tools/icongen/Cargo.toml
//!
//! Writes `assets/icons/{shellman,files,settings}.ico`, `files-64.rgba` and `settings-64.rgba` (the window icons the apps
//! hands to eframe) and `preview.png` (every icon on dark and light, for a quick look).

mod art;
mod ico;
mod shapes;

use art::Icon;
use std::{fs, path::Path};

const SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];
const ICONS: [(&str, Icon); 3] = [("shellman", Icon::Shellman), ("files", Icon::Files), ("settings", Icon::Settings)];

fn main() {
    let dir = Path::new("assets/icons");
    fs::create_dir_all(dir).unwrap();
    for (name, icon) in ICONS {
        let images: Vec<_> = SIZES.iter().map(|&s| (s, art::render(icon, s))).collect();
        fs::write(dir.join(format!("{name}.ico")), ico::write(&images)).unwrap();
    }
    fs::write(dir.join("files-64.rgba"), art::render(Icon::Files, 64)).unwrap();
    fs::write(dir.join("settings-64.rgba"), art::render(Icon::Settings, 64)).unwrap();
    write_preview(&dir.join("preview.png"));
}

/// Over a background color, straight alpha.
fn blend(bg: [u8; 3], px: &[u8]) -> [u8; 3] {
    let a = px[3] as f32 / 255.0;
    [0, 1, 2].map(|k| (px[k] as f32 * a + bg[k] as f32 * (1.0 - a)) as u8)
}

/// A sheet: per icon and background, 256, 48, 32, 16 px at true size, then 16 and 32 blown up 4x.
fn write_preview(path: &Path) {
    const BAND: u32 = 280;
    const VIEWS: [(u32, u32); 6] = [(256, 1), (48, 1), (32, 1), (16, 1), (16, 4), (32, 4)];
    let w = 16 + VIEWS.iter().map(|(s, z)| s * z + 16).sum::<u32>();
    let h = BAND * 6;
    let mut sheet = vec![0u8; (w * h * 3) as usize];
    let mut row = 0;
    for (_, icon) in ICONS {
        for bg in [[32u8, 32, 32], [243, 243, 243]] {
            let band = (row * BAND * w * 3) as usize..((row + 1) * BAND * w * 3) as usize;
            for p in sheet[band].chunks_mut(3) {
                p.copy_from_slice(&bg);
            }
            let mut x = 16;
            for (size, zoom) in VIEWS {
                let img = art::render(icon, size);
                let y0 = row * BAND + (BAND - size * zoom) / 2;
                for y in 0..size * zoom {
                    for xx in 0..size * zoom {
                        let src = (((y / zoom) * size + xx / zoom) * 4) as usize;
                        let dst = (((y0 + y) * w + x + xx) * 3) as usize;
                        sheet[dst..dst + 3].copy_from_slice(&blend(bg, &img[src..src + 4]));
                    }
                }
                x += size * zoom + 16;
            }
            row += 1;
        }
    }
    let mut enc = png::Encoder::new(fs::File::create(path).unwrap(), w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&sheet).unwrap();
}
