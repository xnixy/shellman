//! The three icons: a plate in the accent color with a white glyph on it (a dark one on a very
//! light accent, so it still reads).
//! Below 32 px the glyphs are redrawn chunkier rather than shrunk, so they stay legible.

use crate::shapes::{union_dist, RRect};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    /// Four rounded dots, like the top bar's launcher glyph.
    Shellman,
    Files,
    Settings,
}

/// One flat white part of the glyph: its shapes and an alpha.
struct Layer {
    shapes: Vec<RRect>,
    alpha: f32,
}

fn layers(icon: Kind, small: bool) -> Vec<Layer> {
    let solid = |shapes: Vec<RRect>| Layer { shapes, alpha: 1.0 };
    match icon {
        Kind::Shellman => {
            let r = if small { 0.125 } else { 0.105 };
            let d = if small { 0.15 } else { 0.14 };
            let dots = [(-d, -d), (d, -d), (-d, d), (d, d)];
            vec![solid(dots.iter().map(|(dx, dy)| RRect::circle(0.5 + dx, 0.5 + dy, r)).collect())]
        }
        Kind::Files if small => vec![solid(vec![
            RRect::new(0.15, 0.24, 0.47, 0.5, 0.06),
            RRect::new(0.13, 0.34, 0.87, 0.78, 0.07),
        ])],
        Kind::Files => vec![
            // The back of the folder with its tab, a little see-through...
            Layer {
                shapes: vec![RRect::new(0.18, 0.25, 0.47, 0.5, 0.055), RRect::new(0.16, 0.34, 0.84, 0.74, 0.06)],
                alpha: 0.62,
            },
            // ...and the solid front panel.
            solid(vec![RRect::new(0.16, 0.45, 0.84, 0.75, 0.06)]),
        ],
        Kind::Settings => {
            let (th, kr, ys): (f32, f32, [f32; 3]) =
                if small { (0.045, 0.088, [0.27, 0.5, 0.73]) } else { (0.055, 0.082, [0.31, 0.5, 0.69]) };
            let xs = [0.36, 0.64, 0.46];
            let tracks = ys.iter().map(|&y| RRect::new(0.2, y - th, 0.8, y + th, th)).collect();
            let knobs = ys.iter().zip(xs).map(|(&y, x)| RRect::circle(x, y, kr)).collect();
            let alpha = if small { 0.78 } else { 0.6 };
            vec![Layer { shapes: tracks, alpha }, solid(knobs)]
        }
    }
}

/// The plate: a Windows 11 style rounded square, lighter at the top.
const PLATE: RRect = RRect::new(0.04, 0.04, 0.96, 0.96, 0.22);

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Straight-alpha RGBA of one point of the icon; each channel is 0..1.
fn sample(small: bool, glyph: &[Layer], accent: [f32; 3], ink: [f32; 3], x: f32, y: f32) -> [f32; 4] {
    if PLATE.dist(x, y) > 0.0 {
        return [0.0; 4];
    }
    // Top is lifted toward white, bottom sinks toward black, both gently.
    let t = ((y - 0.04) / 0.92).clamp(0.0, 1.0);
    let mut c = accent.map(|v| {
        if t < 0.5 {
            mix(mix(v, 1.0, 0.16), v, t * 2.0)
        } else {
            mix(v, v * 0.82, (t - 0.5) * 2.0)
        }
    });
    // A soft shadow under a white glyph lifts it off the plate (big sizes only).
    if !small && ink[0] == 1.0 && glyph.iter().any(|l| union_dist(&l.shapes, x, y - 0.02) < 0.0) {
        c = c.map(|v| v * 0.82);
    }
    for l in glyph {
        if union_dist(&l.shapes, x, y) < 0.0 {
            for k in 0..3 {
                c[k] = mix(c[k], ink[k], l.alpha);
            }
        }
    }
    [c[0], c[1], c[2], 1.0]
}

/// Renders the icon at `size` px in `accent`: an 8x8 grid of samples a pixel, averaged in
/// premultiplied alpha. Straight RGBA out, `size * size * 4` bytes.
pub fn render(icon: Kind, size: u32, accent: [u8; 3]) -> Vec<u8> {
    let accent = accent.map(|v| v as f32 / 255.0);
    // Perceived brightness picks a white or a dark glyph; the default rose is well on the white side.
    let lum = 0.2126 * accent[0] + 0.7152 * accent[1] + 0.0722 * accent[2];
    let ink = if lum > 0.6 { accent.map(|v| v * 0.15) } else { [1.0; 3] };
    let small = size < 32;
    let glyph = layers(icon, small);
    let n = 8u32;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let mut acc = [0.0f32; 4];
            for sy in 0..n {
                for sx in 0..n {
                    let x = (px as f32 + (sx as f32 + 0.5) / n as f32) / size as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / n as f32) / size as f32;
                    let s = sample(small, &glyph, accent, ink, x, y);
                    for k in 0..3 {
                        acc[k] += s[k] * s[3];
                    }
                    acc[3] += s[3];
                }
            }
            let a = acc[3] / (n * n) as f32;
            for k in 0..3 {
                let v = if acc[3] > 0.0 { acc[k] / acc[3] } else { 0.0 };
                out.push((v * 255.0 + 0.5) as u8);
            }
            out.push((a * 255.0 + 0.5) as u8);
        }
    }
    out
}
