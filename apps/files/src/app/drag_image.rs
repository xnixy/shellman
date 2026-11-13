//! The picture under the pointer while items are dragged out: a small stack of cards in the
//! Islands look (one card per item, up to three), the first item's icon and name on the front
//! card, and a count badge in the accent when there is more than one.
//!
//! It is drawn here pixel by pixel (shapes by their distance from the edge, for smooth corners),
//! with the window's own fonts for the text, so it looks like the window it came from.

use egui::{Color32, FontId, Pos2, Rect, Vec2};
use shellman_core::fsops::Entry;
use shellman_core::transfer::drag::Image;
use shellman_ui::theme::{self, colors, icon_font, semibold_font};

/// The picture's size, in points.
const SIZE: Vec2 = Vec2::new(150.0, 128.0);
/// The front card, in points.
const CARD: Rect = Rect {
    min: Pos2::new(10.0, 26.0),
    max: Pos2::new(120.0, 118.0),
};
/// How far each card behind is shifted.
const STEP: Vec2 = Vec2::new(7.0, -7.0);
const CARD_RADIUS: f32 = 18.0;

/// A canvas of premultiplied RGBA, `scale` pixels to the point.
pub(super) struct Canvas {
    w: usize,
    h: usize,
    scale: f32,
    px: Vec<[f32; 4]>,
}

impl Canvas {
    pub(super) fn new(size: Vec2, scale: f32) -> Self {
        let (w, h) = ((size.x * scale).ceil() as usize, (size.y * scale).ceil() as usize);
        Self {
            w,
            h,
            scale,
            px: vec![[0.0; 4]; w * h],
        }
    }

    /// Lay `colour` over pixel (x, y) at `coverage`.
    fn blend(&mut self, x: usize, y: usize, colour: Color32, coverage: f32) {
        if x >= self.w || y >= self.h || coverage <= 0.0 {
            return;
        }
        let [r, g, b, a] = colour.to_array();
        // `Color32` is premultiplied already.
        let a = a as f32 / 255.0 * coverage.min(1.0);
        let k = coverage.min(1.0);
        let src = [r as f32 / 255.0 * k, g as f32 / 255.0 * k, b as f32 / 255.0 * k, a];
        let d = &mut self.px[y * self.w + x];
        for i in 0..4 {
            d[i] = src[i] + d[i] * (1.0 - a);
        }
    }

    /// A rounded rectangle (in points), filled, with a line of `line` width inside its edge.
    pub(super) fn rounded(&mut self, r: Rect, radius: f32, fill: Color32, line: Color32, width: f32) {
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
                let outside = Vec2::new(q.x.max(0.0), q.y.max(0.0)).length();
                let d = outside + q.x.max(q.y).min(0.0) - radius;
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

    /// One line of text, centred on `centre` (in points), cut to `max_w`.
    pub(super) fn text(&mut self, ctx: &egui::Context, centre: Pos2, text: &str, font: FontId, colour: Color32, max_w: f32) {
        let s = self.scale;
        let shown = super::text::middle_ellipsis(text, |t| {
            ctx.fonts(|f| f.layout_no_wrap(t.to_owned(), font.clone(), colour)).size().x <= max_w
        });
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
                        let x = lx + i64::from(tx - uv.min[0]);
                        let y = ly + i64::from(ty - uv.min[1]);
                        if x >= 0 && y >= 0 {
                            self.blend(x as usize, y as usize, colour, cover);
                        }
                    }
                }
            }
        }
    }

    /// As premultiplied BGRA bytes, top row first.
    pub(super) fn bgra(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.px.len() * 4);
        for p in &self.px {
            let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            out.extend_from_slice(&[b(p[2]), b(p[1]), b(p[0]), b(p[3])]);
        }
        out
    }

    #[cfg(test)]
    fn alpha_at(&self, x: usize, y: usize) -> f32 {
        self.px[y * self.w + x][3]
    }
}

/// The picture for dragging `items`, at `scale` pixels to the point.
pub(super) fn render(ctx: &egui::Context, items: &[Entry], scale: f32) -> Option<Image> {
    let first = items.first()?;
    let c = colors();
    let scale = scale.clamp(1.0, 4.0);
    let mut cv = Canvas::new(SIZE, scale);
    let css = |r, g, b, a| Color32::from_rgba_unmultiplied(r, g, b, a);
    let (fill, back, line) = if c.light {
        (css(255, 255, 255, 240), css(246, 247, 250, 225), css(0, 0, 0, 34))
    } else {
        (css(46, 48, 56, 240), css(38, 40, 47, 225), css(255, 255, 255, 40))
    };
    // The cards behind, back to front.
    let behind = items.len().clamp(1, 3) - 1;
    for i in (1..=behind).rev() {
        cv.rounded(CARD.translate(STEP * i as f32), CARD_RADIUS, back, line, 1.0);
    }
    cv.rounded(CARD, CARD_RADIUS, fill, line, 1.0);
    // The first item's icon and name.
    let (g, gcol) = super::rows::file_icon(first, &c);
    let well = Rect::from_min_max(CARD.min + Vec2::splat(9.0), egui::pos2(CARD.max.x - 9.0, CARD.max.y - 28.0));
    cv.rounded(well, 12.0, theme::with_alpha(gcol, if c.light { 34 } else { 44 }), Color32::TRANSPARENT, 0.0);
    cv.text(ctx, well.center(), &g.to_string(), icon_font(26.0), gcol, well.width());
    cv.text(
        ctx,
        egui::pos2(CARD.center().x, CARD.max.y - 15.0),
        &first.name,
        FontId::proportional(11.5),
        c.text,
        CARD.width() - 18.0,
    );
    // How many.
    if items.len() > 1 {
        let n = if items.len() > 999 { "999+".to_owned() } else { items.len().to_string() };
        let font = semibold_font(12.0);
        let w = ctx.fonts(|f| f.layout_no_wrap(n.clone(), font.clone(), c.on_accent)).size().x;
        let at = egui::pos2(CARD.max.x - 4.0, CARD.min.y + 2.0);
        let badge = Rect::from_center_size(at, Vec2::new((w + 14.0).max(24.0), 24.0));
        cv.rounded(badge.expand(2.0), 14.0, fill, Color32::TRANSPARENT, 0.0);
        cv.rounded(badge, 12.0, c.accent_fill, Color32::TRANSPARENT, 0.0);
        cv.text(ctx, badge.center(), &n, font, c.on_accent, 60.0);
    }
    Some(Image {
        width: cv.w as u32,
        height: cv.h as u32,
        bgra: cv.bgra(),
        // The pointer holds the front card a little in from its top left.
        hotspot: (
            ((CARD.min.x + 24.0) * scale) as i32,
            ((CARD.min.y + 20.0) * scale) as i32,
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounded_shapes_have_soft_edges_and_solid_middles() {
        let mut cv = Canvas::new(Vec2::new(40.0, 40.0), 2.0);
        cv.rounded(
            Rect::from_min_max(egui::pos2(5.0, 5.0), egui::pos2(35.0, 35.0)),
            8.0,
            Color32::WHITE,
            Color32::TRANSPARENT,
            0.0,
        );
        assert!((cv.alpha_at(40, 40) - 1.0).abs() < 1e-4, "the middle is solid");
        assert_eq!(cv.alpha_at(0, 0), 0.0, "outside is clear");
        // The very corner is cut away by the radius.
        assert!(cv.alpha_at(11, 11) < 0.05);
        // Along the corner's curve, pixels are partly covered.
        let edge: Vec<f32> = (10..20).map(|x| cv.alpha_at(x, 13)).collect();
        assert!(edge.iter().any(|&a| a > 0.05 && a < 0.95), "{edge:?}");
        let bytes = cv.bgra();
        assert_eq!(bytes.len(), 80 * 80 * 4);
        // Premultiplied: no channel above alpha.
        for px in bytes.as_chunks::<4>().0 {
            assert!(px[0] <= px[3] && px[1] <= px[3] && px[2] <= px[3]);
        }
    }
}
