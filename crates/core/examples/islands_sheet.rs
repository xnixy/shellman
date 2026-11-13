//! A contact sheet of the built-in Islands pack, for looking at it whole:
//!
//!     cargo run -p shellman-core --example islands_sheet -- <out-dir>
//!
//! Writes `islands-sheet.png` (every icon at 96, 48, 24 and 16 px, on dark and on light glass
//! colours; 24 and 16 are the small variants, drawn at 1:1) and `islands-brands.png` (each app's
//! tile next to that app's real icon from this machine, where it is installed).

use egui::{Color32, ColorImage};
use shellman_core::icon_pack::{builtin, raster};
use std::path::Path;

const DARK: Color32 = Color32::from_rgb(22, 26, 38);
const LIGHT: Color32 = Color32::from_rgb(233, 236, 243);

/// Apps whose tiles are compared with the real thing, and where the real thing may be.
const BRANDS: &[(&str, &[&str])] = &[
    ("firefox", &[r"C:\Program Files\Mozilla Firefox\firefox.exe"]),
    ("discord", &[r"%LOCALAPPDATA%\Discord\app-1.0.9268\Discord.exe", r"%LOCALAPPDATA%\Discord\Update.exe"]),
    ("steam", &[r"C:\Program Files (x86)\Steam\steam.exe"]),
    ("1password", &[r"shell:AppsFolder\Agilebits.1Password_amwd9z03whsfe!Agilebits.OnePassword"]),
    ("tailscale", &[r"C:\Program Files\Tailscale\tailscale-ipn.exe"]),
    ("zed", &[r"%LOCALAPPDATA%\Programs\Zed\Zed.exe"]),
    ("git", &[r"C:\Program Files\Git\git-bash.exe"]),
    ("xbox", &[r"shell:AppsFolder\Microsoft.GamingApp_8wekyb3d8bbwe!Microsoft.Xbox.App"]),
    ("google-chrome", &[r"C:\Program Files\Google\Chrome\Application\chrome.exe"]),
    ("microsoft-edge", &[r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"]),
    ("brave-browser", &[]),
    ("spotify", &[]),
    ("visual-studio-code", &[]),
    ("slack", &[]),
    ("telegram", &[]),
    ("whatsapp", &[]),
    ("zoom", &[]),
    ("com.obsproject.studio", &[]),
    ("vlc", &[]),
    ("notion", &[]),
    ("obsidian", &[]),
    ("github-desktop", &[]),
    ("qbittorrent", &[]),
    ("archive-manager", &[r"C:\Program Files\7-Zip\7zFM.exe"]),
];

struct Canvas {
    w: usize,
    h: usize,
    px: Vec<[f32; 3]>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Self {
        Self { w, h, px: vec![[0.0; 3]; w * h] }
    }

    fn fill(&mut self, x0: usize, y0: usize, w: usize, h: usize, c: Color32) {
        for y in y0..(y0 + h).min(self.h) {
            for x in x0..(x0 + w).min(self.w) {
                self.px[y * self.w + x] = [c.r() as f32, c.g() as f32, c.b() as f32];
            }
        }
    }

    /// Premultiplied source over what is there.
    fn draw(&mut self, img: &ColorImage, x0: usize, y0: usize, size: usize) {
        let [iw, ih] = img.size;
        for y in 0..size.min(ih) {
            for x in 0..size.min(iw) {
                let s = img.pixels[y * iw + x];
                let a = s.a() as f32 / 255.0;
                let d = &mut self.px[(y0 + y) * self.w + x0 + x];
                for (i, v) in [s.r(), s.g(), s.b()].into_iter().enumerate() {
                    d[i] = v as f32 + d[i] * (1.0 - a);
                }
            }
        }
    }

    fn save(&self, path: &Path) {
        let mut data = Vec::with_capacity(self.w * self.h * 3);
        for p in &self.px {
            data.extend(p.iter().map(|v| v.round().clamp(0.0, 255.0) as u8));
        }
        let file = std::fs::File::create(path).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), self.w as u32, self.h as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.write_header().unwrap().write_image_data(&data).unwrap();
    }
}

fn icon(name: &str, px: u32) -> ColorImage {
    raster::svg_file(&builtin::path(name), px).unwrap_or_else(|| panic!("{name}"))
}

fn real_icon(paths: &[&str], px: i32) -> Option<ColorImage> {
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    paths.iter().map(|p| p.replace("%LOCALAPPDATA%", &local)).find_map(|p| {
        if !p.starts_with("shell:") && !Path::new(&p).exists() {
            return None;
        }
        shellman_core::thumbs::shell_image(Path::new(&p), px)
    })
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let out = Path::new(&out);
    std::fs::create_dir_all(out).unwrap();
    let names: Vec<&str> = builtin::FILES.iter().map(|(n, _, _)| *n).collect();

    // Every icon: a cell of 96 + 48 + 24 + 16 side by side, eight cells a row, dark then light.
    let cols = 8;
    let cell_w = 8 + 96 + 8 + 48 + 8 + 24 + 8 + 16 + 16;
    let cell_h = 112;
    let rows = names.len().div_ceil(cols);
    let panel_h = rows * cell_h + 16;
    let mut sheet = Canvas::new(cols * cell_w + 16, panel_h * 2);
    for (p, bg) in [DARK, LIGHT].into_iter().enumerate() {
        sheet.fill(0, p * panel_h, sheet.w, panel_h, bg);
        for (i, name) in names.iter().enumerate() {
            let x = 16 + (i % cols) * cell_w;
            let y = p * panel_h + 16 + (i / cols) * cell_h;
            sheet.draw(&icon(name, 96), x, y, 96);
            sheet.draw(&icon(name, 48 * 2).resized_half(), x + 104, y + 24, 48);
            sheet.draw(&raster::svg_file(&builtin::path(name), 24).unwrap(), x + 160, y + 36, 24);
            sheet.draw(&raster::svg_file(&builtin::path(name), 16).unwrap(), x + 192, y + 40, 16);
        }
    }
    sheet.save(&out.join("islands-sheet.png"));

    // The small variants alone, at 24 and 16 px, blown up 4x (nearest), to judge them pixel by pixel.
    let k = 4;
    let cell = (24 + 4 + 16 + 8) * k;
    let small_cols = 11;
    let small_rows = names.len().div_ceil(small_cols);
    let small_h = small_rows * (24 + 8) * k + 8 * k;
    let mut small = Canvas::new(small_cols * cell + 8 * k, small_h * 2);
    for (p, bg) in [DARK, LIGHT].into_iter().enumerate() {
        small.fill(0, p * small_h, small.w, small_h, bg);
        for (i, name) in names.iter().enumerate() {
            let x = 8 * k + (i % small_cols) * cell;
            let y = p * small_h + 8 * k + (i / small_cols) * (24 + 8) * k;
            small.draw(&upscale(&raster::svg_file(&builtin::path(name), 24).unwrap(), k), x, y, 24 * k);
            small.draw(&upscale(&raster::svg_file(&builtin::path(name), 16).unwrap(), k), x + 28 * k, y + 4 * k, 16 * k);
        }
    }
    small.save(&out.join("islands-small.png"));
    std::fs::write(out.join("islands-sheet.txt"), names.join("\n")).unwrap();

    // Brands: the tile, then the real icon (or a gap), on dark and light.
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
    }
    let per_row = 6;
    let bw = 16 + 96 + 12 + 96 + 24;
    let bh = 120;
    let brows = BRANDS.len().div_ceil(per_row);
    let mut brands = Canvas::new(per_row * bw + 16, brows * bh * 2 + 16);
    let mut found = Vec::new();
    for (p, bg) in [DARK, LIGHT].into_iter().enumerate() {
        brands.fill(0, p * brows * bh, brands.w, brows * bh + if p == 1 { 16 } else { 0 }, bg);
        for (i, (name, paths)) in BRANDS.iter().enumerate() {
            let x = 16 + (i % per_row) * bw;
            let y = p * brows * bh + 16 + (i / per_row) * bh;
            brands.draw(&icon(name, 96), x, y, 96);
            if let Some(real) = real_icon(paths, 96) {
                if p == 0 {
                    found.push(*name);
                }
                let s = real.size[0].min(96);
                brands.draw(&real, x + 108 + (96 - s) / 2, y + (96 - real.size[1].min(96)) / 2, 96);
            }
        }
    }
    brands.save(&out.join("islands-brands.png"));
    let order: Vec<String> = BRANDS.iter().map(|(n, _)| format!("{n}{}", if found.contains(n) { " (with real icon)" } else { "" })).collect();
    std::fs::write(out.join("islands-brands.txt"), order.join("\n")).unwrap();
    println!("{} icons, {} real icons found; written to {}", names.len(), found.len(), out.display());
}

fn upscale(img: &ColorImage, k: usize) -> ColorImage {
    let [w, h] = img.size;
    let mut out = ColorImage::new([w * k, h * k], Color32::TRANSPARENT);
    for y in 0..h * k {
        for x in 0..w * k {
            out.pixels[y * w * k + x] = img.pixels[(y / k) * w + x / k];
        }
    }
    out
}

trait Half {
    fn resized_half(&self) -> ColorImage;
}

impl Half for ColorImage {
    /// A box-filtered half size, as a texture's mipmap would show it.
    fn resized_half(&self) -> ColorImage {
        let [w, h] = self.size;
        let mut out = ColorImage::new([w / 2, h / 2], Color32::TRANSPARENT);
        for y in 0..h / 2 {
            for x in 0..w / 2 {
                let mut acc = [0u32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let p = self.pixels[(2 * y + dy) * w + 2 * x + dx];
                    for (a, v) in acc.iter_mut().zip([p.r(), p.g(), p.b(), p.a()]) {
                        *a += v as u32;
                    }
                }
                out.pixels[y * (w / 2) + x] = Color32::from_rgba_premultiplied((acc[0] / 4) as u8, (acc[1] / 4) as u8, (acc[2] / 4) as u8, (acc[3] / 4) as u8);
            }
        }
        out
    }
}
