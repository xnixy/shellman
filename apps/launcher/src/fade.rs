//! Content that dissolves into the panel's bottom edge, instead of being cut off flat.
//!
//! The panel is glass, so a gradient painted over the content would have to be opaque to hide it
//! (and would not match what is behind). Instead the content itself is drawn fainter towards the
//! edge: an item that reaches into the last `ZONE` points is drawn once per thin horizontal band,
//! each band clipped and with its own opacity. Items above the zone are drawn once, as usual.

use egui::{Painter, Rect, Ui};

/// How tall the fade-out is, in points.
pub const ZONE: f32 = 28.0;

/// Where the content ends: the y of the panel's bottom edge.
#[derive(Clone, Copy)]
pub struct Fade {
    pub edge: f32,
}

impl Fade {
    /// Call `paint` for an item filling `rect`: once at full strength when it is clear of the
    /// zone, otherwise once per band with the band's opacity. `paint` must only draw inside `rect`.
    pub fn paint(&self, ui: &Ui, rect: Rect, mut paint: impl FnMut(&Painter)) {
        let top = self.edge - ZONE;
        if rect.bottom() <= top {
            paint(ui.painter());
            return;
        }
        if rect.top() >= self.edge {
            return;
        }
        let clip = ui.clip_rect();
        let band = |from: f32, to: f32| {
            let strip = Rect::from_x_y_ranges(clip.x_range(), from..=to);
            ui.painter().with_clip_rect(clip.intersect(strip))
        };
        if rect.top() < top {
            paint(&band(rect.top(), top));
        }
        // One physical pixel per band: taller bands showed as stripes (4 pt was 6 px at 150%).
        let step = 1.0 / ui.ctx().pixels_per_point();
        // Bands sit on one grid for every item, so neighbours line up.
        let first = ((rect.top().max(top) - top) / step).floor();
        let mut y = top + first * step;
        while y < rect.bottom().min(self.edge) {
            let mut p = band(y, y + step);
            p.set_opacity(opacity_at(y + step / 2.0 - top));
            paint(&p);
            y += step;
        }
    }
}

/// How visible content is, `depth` points into the zone: fully at 0, not at all at `ZONE`.
fn opacity_at(depth: f32) -> f32 {
    let t = (depth / ZONE).clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opacity_falls_from_one_to_zero() {
        assert_eq!(opacity_at(0.0), 1.0);
        assert_eq!(opacity_at(ZONE), 0.0);
        assert!(opacity_at(ZONE / 2.0) > 0.45 && opacity_at(ZONE / 2.0) < 0.55);
        assert!(opacity_at(4.0) > opacity_at(12.0));
    }
}
