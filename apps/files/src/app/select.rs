//! How a click, a key or a dragged rectangle changes the selection, as plain functions on row
//! numbers, so every gesture is tested without a window. The tab turns rows into paths.
//!
//! - A plain click selects one item, which becomes the anchor and the cursor.
//! - Ctrl+click toggles one item and moves the anchor to it.
//! - Shift+click selects from the anchor to the item, instead of what was selected.
//! - Ctrl+Shift+click adds that range to what was selected.
//! - Ctrl+Space toggles the item under the cursor.
//! - A rectangle selects every item it touches; with Ctrl held, on top of what was selected.

use egui::{Pos2, Rect};
use std::collections::BTreeSet;

/// The keys held during a gesture.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(super) struct Mods {
    pub(super) ctrl: bool,
    pub(super) shift: bool,
}

/// The selection as rows of the view.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub(super) struct Sel {
    pub(super) rows: BTreeSet<usize>,
    pub(super) anchor: Option<usize>,
    pub(super) cursor: Option<usize>,
}

/// The selection after a click on `row` with `mods` held.
pub(super) fn click(sel: &Sel, row: usize, mods: Mods) -> Sel {
    let anchor = sel.anchor.unwrap_or(row);
    let range = || anchor.min(row)..=anchor.max(row);
    match (mods.ctrl, mods.shift) {
        (false, false) => Sel {
            rows: BTreeSet::from([row]),
            anchor: Some(row),
            cursor: Some(row),
        },
        (true, false) => {
            let mut rows = sel.rows.clone();
            if !rows.remove(&row) {
                rows.insert(row);
            }
            Sel {
                rows,
                anchor: Some(row),
                cursor: Some(row),
            }
        }
        (false, true) => Sel {
            rows: range().collect(),
            anchor: Some(anchor),
            cursor: Some(row),
        },
        (true, true) => {
            let mut rows = sel.rows.clone();
            rows.extend(range());
            Sel {
                rows,
                anchor: Some(anchor),
                cursor: Some(row),
            }
        }
    }
}

/// Ctrl+Space: toggle the item under the cursor.
pub(super) fn toggle_cursor(sel: &Sel) -> Sel {
    let Some(c) = sel.cursor else {
        return sel.clone();
    };
    let mut out = sel.clone();
    if !out.rows.remove(&c) {
        out.rows.insert(c);
    }
    out.anchor = Some(c);
    out
}

/// Where the items are, in the scrolled content's own coordinates (0,0 at the top left of the
/// first item, growing down as it scrolls).
#[derive(Clone, Copy, Debug)]
pub(super) struct Layout {
    pub(super) count: usize,
    /// Items per line: the grid's columns, or 1 for the list.
    pub(super) cols: usize,
    /// One item's size, and the distance from one item to the next across and down.
    pub(super) item: egui::Vec2,
    pub(super) step: egui::Vec2,
}

impl Layout {
    /// Item `i`'s rectangle.
    pub(super) fn rect(&self, i: usize) -> Rect {
        let cols = self.cols.max(1);
        let (line, col) = (i / cols, i % cols);
        Rect::from_min_size(
            egui::pos2(col as f32 * self.step.x, line as f32 * self.step.y),
            self.item,
        )
    }

    /// Every item the rectangle `band` touches, in order. Only the lines and columns it crosses
    /// are looked at, so a folder of any size costs the same.
    pub(super) fn hits(&self, band: Rect) -> Vec<usize> {
        let cols = self.cols.max(1);
        if self.count == 0 || band.max.y < 0.0 || band.max.x < 0.0 {
            return Vec::new();
        }
        let lines = self.count.div_ceil(cols);
        let first_line = ((band.min.y.max(0.0)) / self.step.y).floor() as usize;
        let last_line = ((band.max.y / self.step.y).floor() as usize).min(lines - 1);
        let first_col = ((band.min.x.max(0.0)) / self.step.x).floor() as usize;
        let last_col = ((band.max.x / self.step.x).floor() as usize).min(cols - 1);
        let mut out = Vec::new();
        for line in first_line..=last_line {
            for col in first_col..=last_col.max(first_col) {
                let i = line * cols + col;
                if col < cols && i < self.count && self.rect(i).intersects(band) {
                    out.push(i);
                }
            }
        }
        out
    }
}

/// The rectangle between where a drag began and where the pointer is, in content coordinates.
pub(super) fn band(from: Pos2, to: Pos2) -> Rect {
    Rect::from_two_pos(from, to)
}

/// How far to scroll this frame while a rectangle is dragged near (or past) the top or bottom
/// of the view: faster the further out, nothing in the middle. Negative scrolls up.
pub(super) fn edge_scroll(pointer_y: f32, top: f32, bottom: f32, dt: f32) -> f32 {
    const ZONE: f32 = 36.0;
    const SPEED: f32 = 14.0;
    let over = if pointer_y < top + ZONE {
        pointer_y - (top + ZONE)
    } else if pointer_y > bottom - ZONE {
        pointer_y - (bottom - ZONE)
    } else {
        return 0.0;
    };
    over.clamp(-160.0, 160.0) * SPEED * dt.clamp(0.0, 0.1)
}

/// The selection a rectangle makes: what it touches, plus what was selected before when Ctrl
/// was held as it began.
pub(super) fn with_band(before: &BTreeSet<usize>, hits: &[usize], add: bool) -> BTreeSet<usize> {
    let mut rows: BTreeSet<usize> = if add { before.clone() } else { BTreeSet::new() };
    rows.extend(hits.iter().copied());
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Mods = Mods {
        ctrl: false,
        shift: false,
    };
    const CTRL: Mods = Mods {
        ctrl: true,
        shift: false,
    };
    const SHIFT: Mods = Mods {
        ctrl: false,
        shift: true,
    };
    const BOTH: Mods = Mods {
        ctrl: true,
        shift: true,
    };

    fn rows(s: &Sel) -> Vec<usize> {
        s.rows.iter().copied().collect()
    }

    #[test]
    fn a_plain_click_selects_one() {
        let s = click(&Sel::default(), 4, NONE);
        assert_eq!(rows(&s), [4]);
        assert_eq!((s.anchor, s.cursor), (Some(4), Some(4)));
        let s = click(&s, 2, NONE);
        assert_eq!(rows(&s), [2]);
    }

    #[test]
    fn ctrl_click_toggles_and_moves_the_anchor() {
        let s = click(&Sel::default(), 1, NONE);
        let s = click(&s, 3, CTRL);
        let s = click(&s, 5, CTRL);
        assert_eq!(rows(&s), [1, 3, 5]);
        assert_eq!(s.anchor, Some(5));
        let s = click(&s, 3, CTRL);
        assert_eq!(rows(&s), [1, 5]);
        assert_eq!(s.cursor, Some(3));
    }

    #[test]
    fn shift_click_selects_the_range_from_the_anchor() {
        let s = click(&Sel::default(), 2, NONE);
        let s = click(&s, 6, SHIFT);
        assert_eq!(rows(&s), [2, 3, 4, 5, 6]);
        // The anchor stays, so a second Shift+click re-ranges from it.
        let s = click(&s, 0, SHIFT);
        assert_eq!(rows(&s), [0, 1, 2]);
        assert_eq!((s.anchor, s.cursor), (Some(2), Some(0)));
        // Without an anchor, the range is just the item.
        assert_eq!(rows(&click(&Sel::default(), 3, SHIFT)), [3]);
    }

    #[test]
    fn ctrl_shift_click_adds_a_range() {
        let s = click(&Sel::default(), 0, NONE);
        let s = click(&s, 5, CTRL);
        let s = click(&s, 7, BOTH);
        assert_eq!(rows(&s), [0, 5, 6, 7]);
    }

    #[test]
    fn ctrl_space_toggles_the_cursor() {
        let s = Sel {
            rows: BTreeSet::from([1]),
            anchor: Some(1),
            cursor: Some(4),
        };
        let t = toggle_cursor(&s);
        assert_eq!(rows(&t), [1, 4]);
        assert_eq!(rows(&toggle_cursor(&t)), [1]);
        assert_eq!(toggle_cursor(&Sel::default()), Sel::default());
    }

    fn grid() -> Layout {
        // Three columns of 100x80 cards, 10 apart; 8 items.
        Layout {
            count: 8,
            cols: 3,
            item: egui::vec2(100.0, 80.0),
            step: egui::vec2(110.0, 90.0),
        }
    }

    #[test]
    fn a_rectangle_selects_every_card_it_touches() {
        let g = grid();
        // Inside the first card only.
        assert_eq!(g.hits(band(egui::pos2(5.0, 5.0), egui::pos2(20.0, 20.0))), [0]);
        // Across the gap between cards 0 and 1 and down into the second line.
        let r = band(egui::pos2(90.0, 70.0), egui::pos2(115.0, 95.0));
        assert_eq!(g.hits(r), [0, 1, 3, 4]);
        // Only in a gap: nothing.
        assert!(g.hits(band(egui::pos2(101.0, 5.0), egui::pos2(108.0, 60.0))).is_empty());
        // Dragged from below and right, past the last item: the last line's items.
        let r = band(egui::pos2(500.0, 400.0), egui::pos2(0.0, 185.0));
        assert_eq!(g.hits(r), [6, 7]);
        // Above the content.
        assert!(g.hits(band(egui::pos2(0.0, -50.0), egui::pos2(10.0, -10.0))).is_empty());
    }

    #[test]
    fn a_rectangle_in_the_list_selects_rows() {
        let list = Layout {
            count: 100,
            cols: 1,
            item: egui::vec2(600.0, 32.0),
            step: egui::vec2(600.0, 32.0),
        };
        let r = band(egui::pos2(300.0, 40.0), egui::pos2(10.0, 130.0));
        assert_eq!(list.hits(r), [1, 2, 3, 4]);
    }

    #[test]
    fn ctrl_keeps_what_was_selected_under_a_rectangle() {
        let before = BTreeSet::from([9, 10]);
        assert_eq!(
            with_band(&before, &[1, 2], true),
            BTreeSet::from([1, 2, 9, 10])
        );
        assert_eq!(with_band(&before, &[1, 2], false), BTreeSet::from([1, 2]));
    }

    #[test]
    fn dragging_near_an_edge_scrolls_that_way() {
        assert_eq!(edge_scroll(300.0, 100.0, 600.0, 0.016), 0.0);
        assert!(edge_scroll(105.0, 100.0, 600.0, 0.016) < 0.0);
        assert!(edge_scroll(700.0, 100.0, 600.0, 0.016) > 0.0);
        // Further past the edge, faster.
        assert!(edge_scroll(800.0, 100.0, 600.0, 0.016) > edge_scroll(620.0, 100.0, 600.0, 0.016));
    }
}
