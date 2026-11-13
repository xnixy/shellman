//! Where the islands go, from the window's size alone: pure geometry, tested without a window.
//!
//! ```text
//!  [rail]    [ tabs pill ]                         [ sort | - [] x ]
//!  [ .. ]    [ < > ^ o  Home / Projects / shellman      search  ##  ]
//!  [ .. ]    content (cards straight on the desktop, or a list on an island)
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
/// The band along the bottom that holds the selection pill (or the item count).
pub(super) const BAND_H: f32 = 44.0;
/// The window buttons' pill: Sort, then minimize, maximize and close.
pub(super) const CONTROLS_W: f32 = 212.0;
/// The details island, when it shows (the design's 336 of 1600).
pub(super) const DETAILS_W: f32 = 300.0;
/// Narrower than this, the content keeps its room and the details island does not show.
const DETAILS_MIN_CONTENT: f32 = 400.0;

/// The fixed islands of one frame, and the areas the sized-to-fit ones may use.
#[derive(Debug, Clone, Copy)]
pub(super) struct Plan {
    /// Where the tabs pill may grow, from its left end.
    pub(super) tabs_row: Rect,
    pub(super) controls: Rect,
    pub(super) bar: Rect,
    /// The rail's column, the window's whole height.
    pub(super) rail_area: Rect,
    pub(super) content: Rect,
    /// Where the bottom pill sits, centred under the content.
    pub(super) band: Rect,
    /// The details island on the right, when it shows.
    pub(super) details: Option<Rect>,
}

impl Plan {
    /// The plan for a window of `screen`, with the rail `rail_w` wide.
    pub(super) fn new(screen: Rect, details: bool, rail_w: f32) -> Self {
        let left = screen.left() + MARGIN;
        let right = screen.right() - MARGIN;
        let top = screen.top() + MARGIN;
        let bottom = screen.bottom() - MARGIN;
        let inner_left = left + rail_w + GAP;
        let controls = Rect::from_min_max(pos2(right - CONTROLS_W, top), pos2(right, top + TABS_H));
        let tabs_row = Rect::from_min_max(
            pos2(inner_left, top),
            pos2((controls.left() - GAP).max(inner_left), top + TABS_H),
        );
        let bar_top = top + TABS_H + ROW_GAP;
        let bar = Rect::from_min_max(pos2(inner_left, bar_top), pos2(right, bar_top + BAR_H));
        let content_top = bar.bottom() + GAP;
        let band = Rect::from_min_max(pos2(inner_left, bottom - BAND_H), pos2(right, bottom));
        let content_bottom = (band.top() - GAP).max(content_top);
        let room = right - inner_left;
        let details = (details && room - DETAILS_W - GAP >= DETAILS_MIN_CONTENT).then(|| {
            Rect::from_min_max(pos2(right - DETAILS_W, content_top), pos2(right, content_bottom))
        });
        let content_right = details.map_or(right, |d| d.left() - GAP);
        let content = Rect::from_min_max(
            pos2(inner_left, content_top),
            pos2(content_right, content_bottom),
        );
        // The rail runs the window's whole height, level with the tabs at the top.
        let rail_area = Rect::from_min_max(pos2(left, top), pos2(left + rail_w, bottom));
        Self {
            tabs_row,
            controls,
            bar,
            rail_area,
            content,
            band,
            details,
        }
    }
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
    /// The rail folded to icons (it is wider with its names; see `shellman_ui::rail::width`).
    const RAIL_W: f32 = shellman_ui::rail::FOLDED_W;

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
        // The smallest window always folds the rail; just wide enough to show names, the
        // widest rail the names may make.
        let widest = 270.0;
        for (w, h, rail_w) in [
            (MIN_SIZE.0, MIN_SIZE.1, RAIL_W),
            (shellman_ui::rail::NARROW_BELOW, MIN_SIZE.1, widest),
            (1100.0, 700.0, 180.0),
            (2560.0, 1400.0, widest),
        ] {
            let p = Plan::new(screen(w, h), false, rail_w);
            let all = [p.tabs_row, p.controls, p.bar, p.rail_area, p.content, p.band];
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
            assert!(p.tabs_row.width() >= 250.0, "{w}x{h}");
            assert!(p.content.height() >= 200.0, "{w}x{h}");
        }
    }

    #[test]
    fn content_lines_up_with_the_bar_and_the_rail() {
        for rail_w in [RAIL_W, 200.0] {
            let p = Plan::new(screen(1100.0, 700.0), false, rail_w);
            assert_eq!(p.content.left(), p.bar.left());
            assert_eq!(p.content.right(), p.bar.right());
            assert_eq!(p.rail_area.top(), p.tabs_row.top());
            assert_eq!(p.rail_area.bottom(), p.band.bottom());
            assert_eq!(p.controls.right(), p.bar.right());
            assert_eq!(p.content.left() - p.rail_area.right(), GAP);
            assert_eq!(p.tabs_row.left(), p.content.left());
        }
    }

    #[test]
    fn the_details_island_takes_the_right_and_never_overlaps() {
        let p = Plan::new(screen(1400.0, 800.0), true, RAIL_W);
        let d = p.details.expect("room for it");
        assert_eq!(d.right(), p.bar.right());
        assert_eq!(d.top(), p.content.top());
        assert_eq!(d.bottom(), p.content.bottom());
        assert_eq!(d.left() - p.content.right(), GAP);
        assert!(apart(d, p.band) && apart(d, p.content));
        // Too narrow: the content keeps its room.
        let (w, _) = MIN_SIZE;
        assert!(Plan::new(screen(w, 600.0), true, RAIL_W).details.is_none());
        assert!(Plan::new(screen(1400.0, 800.0), false, RAIL_W).details.is_none());
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
    }
}
