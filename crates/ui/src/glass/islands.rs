//! Islands: several separate pieces of glass under one window, with the desktop showing between
//! them. The window itself is transparent and stays whole (the gaps are still the window's, so
//! they can move it); each island is a rounded piece of the panel's glass.
//!
//! An app lays its islands out in egui points every frame, hands them to
//! [`super::GlassPanel::set_islands`], and draws its content on top. The panel changes only what
//! moved. Where composition is not available, [`paint_solid`] paints the islands in egui
//! instead, as solid rounded fills.

use super::style::{content_frame, Style};
use crate::theme;

/// One piece of glass, in points of the window's client area.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Island {
    pub rect: egui::Rect,
    /// Corner radius in points; never more than half the shorter side.
    pub radius: f32,
    /// How much of the glass shows, 0 to 1. Less than 1 lets the sharp desktop through, for a
    /// faint backing behind content that has its own surfaces.
    pub opacity: f32,
    /// Whether it casts the style's shadow.
    pub shadow: bool,
}

impl Island {
    /// A full piece of glass with a shadow.
    pub fn new(rect: egui::Rect, radius: f32) -> Self {
        Self {
            rect,
            radius,
            opacity: 1.0,
            shadow: true,
        }
    }

    /// A pill: as round as the rectangle allows.
    pub fn pill(rect: egui::Rect) -> Self {
        Self::new(rect, rect.height().min(rect.width()) / 2.0)
    }

    /// The same island, only `opacity` of it showing and without a shadow.
    pub fn faint(self, opacity: f32) -> Self {
        Self {
            opacity,
            shadow: false,
            ..self
        }
    }
}

/// An island's rectangle (left, top, right, bottom) and radius, in physical pixels of the client
/// area at `scale` pixels per point. Edges are rounded to whole pixels the way egui places its
/// own drawing, so the glass and the content painted on it line up.
pub fn to_pixels(island: &Island, scale: f32) -> ([i32; 4], f32) {
    let r = island.rect;
    let px = |v: f32| (v * scale).round() as i32;
    let edges = [px(r.left()), px(r.top()), px(r.right()), px(r.bottom())];
    let (w, h) = (edges[2] - edges[0], edges[3] - edges[1]);
    let radius = (island.radius * scale)
        .min(w.min(h) as f32 / 2.0)
        .max(0.0);
    (edges, radius)
}

/// The islands as egui shapes, for a window without composed glass: each a nearly solid rounded
/// fill with a faint rim (see [`content_frame`]). Paint them under the content, e.g. into a
/// `Shape::Noop` reserved at the start of the frame (`painter.set(idx, Shape::Vec(..))`).
pub fn solid_shapes(islands: &[Island], style: &Style) -> Vec<egui::Shape> {
    let frame = content_frame(false, style);
    islands
        .iter()
        .map(|island| {
            let radius = island.radius.min(island.rect.height().min(island.rect.width()) / 2.0);
            let fill = theme::with_alpha(
                frame.fill,
                (frame.fill.a() as f32 * island.opacity.clamp(0.0, 1.0)).round() as u8,
            );
            egui::Shape::Rect(egui::epaint::RectShape::new(
                island.rect,
                radius.round().clamp(0.0, 255.0) as u8,
                fill,
                frame.stroke,
                egui::StrokeKind::Inside,
            ))
        })
        .collect()
}

/// Make the gaps of an islands window move it: everything in `rect` (normally the whole
/// screen rect) that no later widget takes drags the window, and a double click maximizes or
/// restores it. Call it first in the frame, before any island's content, so the content wins.
pub fn drag_gaps(ui: &mut egui::Ui, rect: egui::Rect) -> egui::Response {
    let resp = ui.interact(rect, egui::Id::new("island-gaps"), egui::Sense::click_and_drag());
    crate::widgets::window_drag(ui, &resp);
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, Rect};

    #[test]
    fn points_become_whole_pixels() {
        let i = Island::new(Rect::from_min_max(pos2(10.0, 10.3), pos2(110.0, 60.0)), 12.0);
        let (edges, radius) = to_pixels(&i, 1.5);
        assert_eq!(edges, [15, 15, 165, 90]);
        assert_eq!(radius, 18.0);
    }

    #[test]
    fn a_pill_is_half_its_height_round() {
        let i = Island::pill(Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(300.0, 52.0)));
        assert_eq!(i.radius, 26.0);
        // A tall narrow pill (the rail) takes half its width.
        let rail = Island::pill(Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(60.0, 400.0)));
        assert_eq!(rail.radius, 30.0);
        let (_, r) = to_pixels(&rail, 2.0);
        assert_eq!(r, 60.0);
    }

    #[test]
    fn radius_never_passes_half_the_shorter_side() {
        let i = Island::new(Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(40.0, 20.0)), 99.0);
        assert_eq!(to_pixels(&i, 1.25).1, 12.5);
        let empty = Island::new(Rect::from_min_size(pos2(5.0, 5.0), egui::vec2(0.0, 0.0)), 8.0);
        assert_eq!(to_pixels(&empty, 2.0).1, 0.0);
    }

    #[test]
    fn faint_drops_the_shadow() {
        let i = Island::new(Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(10.0, 10.0)), 4.0);
        let f = i.faint(0.4);
        assert!(!f.shadow && f.opacity == 0.4 && f.rect == i.rect);
        assert!(i.shadow && i.opacity == 1.0);
    }
}
