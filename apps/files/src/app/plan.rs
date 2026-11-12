//! Where the islands go, from the window's size alone: pure geometry, tested without a window.
//!
//! ```text
//!            [ tabs pill ]                         [ sort | - [] x ]
//!            [ < > ^ o  Home / Projects / shellman      search  ##  ]
//!  [rail]    content (cards straight on the desktop, or a list on an island)
//!  [ .. ]
//!                     [ 3 selected  12 MB  Copy  Rename  Delete ]
//! ```
//!
//! Everything else is gap: the desktop shows through it, and it moves the window.

use egui::{pos2, Rect};

/// From the window's edge to the islands. The resize border lies inside it.
pub(super) const MARGIN: f32 = 12.0;
/// Between islands.
pub(super) const GAP: f32 = 10.0;
/// The row of tabs, and the window buttons beside it.
pub(super) const TABS_H: f32 = 40.0;
/// The bar under it: navigation, the path, search and the view.
pub(super) const BAR_H: f32 = 50.0;
/// Between the two top rows: they read as one group.
const ROW_GAP: f32 = 8.0;
pub(super) const RAIL_W: f32 = 56.0;
/// The band along the bottom that holds the selection pill (or the item count).
pub(super) const BAND_H: f32 = 44.0;
/// The window buttons' pill: Sort, then minimize, maximize and close.
pub(super) const CONTROLS_W: f32 = 212.0;

/// The fixed islands of one frame, and the areas the sized-to-fit ones may use.
#[derive(Debug, Clone, Copy)]
pub(super) struct Plan {
    /// Where the tabs pill may grow, from its left end.
    pub(super) tabs_row: Rect,
    pub(super) controls: Rect,
    pub(super) bar: Rect,
    /// Where the rail may grow, down from its top.
    pub(super) rail_area: Rect,
    pub(super) content: Rect,
    /// Where the bottom pill sits, centred under the content.
    pub(super) band: Rect,
}

impl Plan {
    pub(super) fn new(screen: Rect) -> Self {
        let left = screen.left() + MARGIN;
        let right = screen.right() - MARGIN;
        let top = screen.top() + MARGIN;
        let bottom = screen.bottom() - MARGIN;
        let inner_left = left + RAIL_W + GAP;
        let controls = Rect::from_min_max(pos2(right - CONTROLS_W, top), pos2(right, top + TABS_H));
        let tabs_row = Rect::from_min_max(
            pos2(inner_left, top),
            pos2((controls.left() - GAP).max(inner_left), top + TABS_H),
        );
        let bar_top = top + TABS_H + ROW_GAP;
        let bar = Rect::from_min_max(pos2(inner_left, bar_top), pos2(right, bar_top + BAR_H));
        let content_top = bar.bottom() + GAP;
        let band = Rect::from_min_max(pos2(inner_left, bottom - BAND_H), pos2(right, bottom));
        let content = Rect::from_min_max(
            pos2(inner_left, content_top),
            pos2(right, (band.top() - GAP).max(content_top)),
        );
        let rail_area = Rect::from_min_max(pos2(left, content_top), pos2(left + RAIL_W, bottom));
        Self {
            tabs_row,
            controls,
            bar,
            rail_area,
            content,
            band,
        }
    }
}

/// The rail: as tall as its icons want, but never past its area.
pub(super) fn rail(area: Rect, wanted: f32) -> Rect {
    Rect::from_min_size(
        area.min,
        egui::vec2(area.width(), wanted.min(area.height())),
    )
}

/// A pill `wanted` wide at the left of `row`, no wider than the row.
pub(super) fn from_left(row: Rect, wanted: f32) -> Rect {
    Rect::from_min_size(row.min, egui::vec2(wanted.min(row.width()), row.height()))
}

/// A pill `wanted` wide in the middle of `band`, no wider than it.
pub(super) fn centred(band: Rect, wanted: f32) -> Rect {
    Rect::from_center_size(
        band.center(),
        egui::vec2(wanted.min(band.width()), band.height()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    /// The smallest window, which must still fit every island apart.
    use crate::placement::MIN_SIZE;

    fn screen(w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(w, h))
    }

    fn apart(a: Rect, b: Rect) -> bool {
        a.right() <= b.left()
            || b.right() <= a.left()
            || a.bottom() <= b.top()
            || b.bottom() <= a.top()
    }

    #[test]
    fn islands_never_overlap_down_to_the_smallest_window() {
        for (w, h) in [MIN_SIZE, (1100.0, 700.0), (2560.0, 1400.0)] {
            let p = Plan::new(screen(w, h));
            let rail = rail(p.rail_area, 10_000.0);
            let all = [p.tabs_row, p.controls, p.bar, rail, p.content, p.band];
            for (i, a) in all.iter().enumerate() {
                assert!(
                    a.width() > 0.0 && a.height() > 0.0,
                    "{w}x{h}: island {i} is empty"
                );
                assert!(
                    screen(w, h).shrink(MARGIN - 0.01).contains_rect(*a),
                    "{w}x{h}: {i} off"
                );
                for b in &all[i + 1..] {
                    assert!(apart(*a, *b), "{w}x{h}: {a:?} and {b:?} overlap");
                }
            }
            // The tabs keep room for a few tabs at the smallest size.
            assert!(p.tabs_row.width() >= 300.0, "{w}x{h}");
            assert!(p.content.height() >= 200.0, "{w}x{h}");
        }
    }

    #[test]
    fn content_lines_up_with_the_bar_and_the_rail() {
        let p = Plan::new(screen(1100.0, 700.0));
        assert_eq!(p.content.left(), p.bar.left());
        assert_eq!(p.content.right(), p.bar.right());
        assert_eq!(p.rail_area.top(), p.content.top());
        assert_eq!(p.controls.right(), p.bar.right());
        assert_eq!(p.content.left() - p.rail_area.right(), GAP);
    }

    #[test]
    fn sized_pills_fit_their_area() {
        let row = Rect::from_min_size(pos2(80.0, 12.0), egui::vec2(400.0, 40.0));
        assert_eq!(from_left(row, 120.0).width(), 120.0);
        assert_eq!(from_left(row, 900.0), row);
        let band = Rect::from_min_size(pos2(80.0, 600.0), egui::vec2(800.0, 44.0));
        let c = centred(band, 300.0);
        assert_eq!(c.center(), band.center());
        assert_eq!(centred(band, 2000.0).width(), 800.0);
        let area = Rect::from_min_size(pos2(12.0, 120.0), egui::vec2(56.0, 500.0));
        assert_eq!(rail(area, 300.0).height(), 300.0);
        assert_eq!(rail(area, 3000.0).height(), 500.0);
    }
}
