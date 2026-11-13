//! The picture under the pointer while an item is dragged out of a fanned stack: one rounded card
//! in the Islands look with the item's icon glyph and name, as Files draws its own (see
//! `apps/files/src/app/drag_image.rs`, which this is a trimmed copy of: a single card, no badge).
//!
//! Drawn pixel by pixel, shapes by their distance from the edge for smooth corners, with the
//! window's own fonts for the text.

use egui::{Color32, FontId, Pos2, Rect, Vec2};
use shellman_core::transfer::drag::Image;
use shellman_ui::theme::{colors, glyph, icon_font};

/// The picture's size, in points.
const SIZE: Vec2 = Vec2::new(140.0, 112.0);
const CARD: Rect = Rect { min: Pos2::new(8.0, 8.0), max: Pos2::new(132.0, 104.0) };
const RADIUS: f32 = 18.0;

/// A canvas of premultiplied RGBA, `scale` pixels to the point.
struct Canvas {
    w: usize,
    h: usize,
    scale: f32,
    px: Vec<[f32; 4]>,
}

impl Canvas {
    fn new(size: Vec2, scale: f32) -> Self {
        let (w, h) = ((size.x * scale).ceil() as usize, (size.y * scale).ceil() as usize);
        Self { w, h, scale, px: vec![[0.0; 4]; w * h] }
    }

    fn blend(&mut self, x: usize, y: usize, colour: Color32, coverage: f32) {
        if x >= self.w || y >= self.h || coverage <= 0.0 {
            return;
        }
        let [r, g, b, a] = colour.to_array();
        let k = coverage.min(1.0);
        let a = a as f32 / 255.0 * k;
        let src = [r as f32 / 255.0 * k, g as f32 / 255.0 * k, b as f32 / 255.0 * k, a];
        let d = &mut self.px[y * self.w + x];
        for i in 0..4 {
            d[i] = src[i] + d[i] * (1.0 - a);
        }
    }

    /// A rounded rectangle (points), filled, with a line of `width` inside its edge.
    fn rounded(&mut self, r: Rect, radius: f32, fill: Color32, line: Color32, width: f32) {
        let s = self.scale;
        let (r, radius, width) = (Rect::from_min_max(r.min * s, r.max * s), radius * s, width * s);
        let (x0, x1) = (r.min.x.floor().max(0.0) as usize, r.max.x.ceil() as usize);
        let (y0, y1) = (r.min.y.floor().max(0.0) as usize, r.max.y.ceil() as usize);
        let c = r.center();
        let half = r.size() / 2.0 - Vec2::splat(radius);
        for y in y0..y1.min(self.h) {
            for x in x0..x1.min(self.w) {
                let p = Vec2::new(x as f32 + 0.5 - c.x, y as f32 + 0.5 - c.y);
                let q = Vec2::new(p.x.abs() - half.x, p.y.abs() - half.y);
                let d = Vec2::new(q.x.max(0.0), q.y.max(0.0)).length() + q.x.max(q.y).min(0.0) - radius;
                let cover = (0.5 - d).clamp(0.0, 1.0);
                if cover <= 0.0 {
                    continue;
                }
                self.blend(x, y, fill, cover);
                if width > 0.0 {
                    let inner = (0.5 - (d + width)).clamp(0.0, 1.0);
                    self.blend(x, y, line, cover - inner);
                }
            }
        }
    }

    /// One line of text centred on `centre` (points), cut with an ellipsis to `max_w`.
    fn text(&mut self, ctx: &egui::Context, centre: Pos2, text: &str, font: FontId, colour: Color32, max_w: f32) {
        let s = self.scale;
        let shown = cut(text, |t| ctx.fonts(|f| f.layout_no_wrap(t.to_owned(), font.clone(), colour)).size().x <= max_w);
        let galley = ctx.fonts(|f| f.layout_no_wrap(shown, font.clone(), colour));
        let origin = centre - galley.size() / 2.0;
        let atlas = ctx.fonts(|f| f.texture_atlas());
        let atlas = atlas.lock();
        let img = atlas.image();
        for row in &galley.rows {
            for g in &row.glyphs {
                let uv = g.uv_rect;
                if uv.is_nothing() {
                    continue;
                }
                let left_top = (origin + g.pos.to_vec2() + uv.offset) * s;
                let (lx, ly) = (left_top.x.round() as i64, left_top.y.round() as i64);
                for ty in uv.min[1]..uv.max[1] {
                    for tx in uv.min[0]..uv.max[0] {
                        let cover = img.pixels[ty as usize * img.size[0] + tx as usize];
                        let (x, y) = (lx + i64::from(tx - uv.min[0]), ly + i64::from(ty - uv.min[1]));
                        if x >= 0 && y >= 0 {
                            self.blend(x as usize, y as usize, colour, cover);
                        }
                    }
                }
            }
        }
    }

    /// As premultiplied BGRA bytes, top row first.
    fn bgra(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.px.len() * 4);
        for p in &self.px {
            let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            out.extend_from_slice(&[b(p[2]), b(p[1]), b(p[0]), b(p[3])]);
        }
        out
    }
}

/// `name`, or its start and its end around an ellipsis, as long as `fits` accepts it.
fn cut(name: &str, fits: impl Fn(&str) -> bool) -> String {
    if fits(name) {
        return name.to_owned();
    }
    let chars: Vec<char> = name.chars().collect();
    let tail = (chars.len() / 4).min(6);
    let tail: String = chars[chars.len() - tail..].iter().collect();
    let make = |head: usize| format!("{}…{tail}", chars[..head].iter().collect::<String>());
    let (mut lo, mut hi) = (0, chars.len().saturating_sub(tail.chars().count()));
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if fits(&make(mid)) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    make(lo)
}

/// The picture for dragging the item at `path`, `scale` pixels to the point.
pub fn render(ctx: &egui::Context, path: &std::path::Path, scale: f32) -> Option<Image> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let c = colors();
    let scale = scale.clamp(1.0, 4.0);
    let mut cv = Canvas::new(SIZE, scale);
    let css = |r, g, b, a| Color32::from_rgba_unmultiplied(r, g, b, a);
    let (fill, line) = if c.light { (css(255, 255, 255, 240), css(0, 0, 0, 34)) } else { (css(46, 48, 56, 240), css(255, 255, 255, 40)) };
    cv.rounded(CARD, RADIUS, fill, line, 1.0);
    let (g, tint) = if path.is_dir() { (glyph::FOLDER_FILL, c.accent_fill) } else { (glyph::FILE, c.text) };
    let well = Rect::from_min_max(CARD.min + Vec2::splat(9.0), egui::pos2(CARD.max.x - 9.0, CARD.max.y - 28.0));
    cv.rounded(well, 12.0, shellman_ui::theme::with_alpha(tint, if c.light { 34 } else { 44 }), Color32::TRANSPARENT, 0.0);
    cv.text(ctx, well.center(), &g.to_string(), icon_font(26.0), tint, well.width());
    cv.text(ctx, egui::pos2(CARD.center().x, CARD.max.y - 15.0), &name, FontId::proportional(11.5), c.text, CARD.width() - 18.0);
    Some(Image {
        width: cv.w as u32,
        height: cv.h as u32,
        bgra: cv.bgra(),
        hotspot: (((CARD.min.x + 24.0) * scale) as i32, ((CARD.min.y + 20.0) * scale) as i32),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_has_soft_edges_and_a_solid_middle() {
        let mut cv = Canvas::new(Vec2::new(40.0, 40.0), 2.0);
        cv.rounded(Rect::from_min_max(egui::pos2(5.0, 5.0), egui::pos2(35.0, 35.0)), 8.0, Color32::WHITE, Color32::TRANSPARENT, 0.0);
        assert!((cv.px[40 * cv.w + 40][3] - 1.0).abs() < 1e-4);
        assert_eq!(cv.px[0][3], 0.0);
        let bytes = cv.bgra();
        assert_eq!(bytes.len(), 80 * 80 * 4);
        for px in bytes.as_chunks::<4>().0 {
            assert!(px[0] <= px[3] && px[1] <= px[3] && px[2] <= px[3]);
        }
    }

    #[test]
    fn long_names_are_cut_in_the_middle_and_short_ones_left_alone() {
        let under = |n: usize| move |t: &str| t.chars().count() <= n;
        assert_eq!(cut("short.txt", under(24)), "short.txt");
        let c = cut("A very long file name for the final report.pdf", under(20));
        assert!(c.chars().count() <= 20 && c.contains('…') && c.ends_with(".pdf"), "{c}");
    }

    /// Writes the picture for a file and a folder as 32-bit BMPs in `%TEMP%`, to look at.
    #[test]
    #[ignore = "writes files to look at"]
    fn dump_pictures() {
        let ctx = egui::Context::default();
        shellman_ui::theme::install_fonts(&ctx);
        ctx.set_pixels_per_point(2.0);
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        let dir = std::env::temp_dir();
        for (name, path) in [("file", std::path::PathBuf::from("C:/Windows/win.ini")), ("folder", std::path::PathBuf::from("C:/Windows"))] {
            let img = render(&ctx, &path, 2.0).expect("a picture");
            let mut out = Vec::new();
            let size = img.bgra.len() as u32;
            out.extend_from_slice(b"BM");
            out.extend_from_slice(&(54 + size).to_le_bytes());
            out.extend_from_slice(&[0, 0, 0, 0]);
            out.extend_from_slice(&54u32.to_le_bytes());
            out.extend_from_slice(&40u32.to_le_bytes());
            out.extend_from_slice(&(img.width as i32).to_le_bytes());
            out.extend_from_slice(&(-(img.height as i32)).to_le_bytes());
            out.extend_from_slice(&1u16.to_le_bytes());
            out.extend_from_slice(&32u16.to_le_bytes());
            out.extend_from_slice(&[0; 24]);
            out.extend_from_slice(&img.bgra);
            std::fs::write(dir.join(format!("shelf-drag-{name}.bmp")), out).unwrap();
        }
    }
}
