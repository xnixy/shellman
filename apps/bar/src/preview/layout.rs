//! The preview panel's geometry, as pure arithmetic: how big each window's tile is, how the tiles
//! are arranged, and where the panel goes on the monitor. Tested without windows.
//!
//! Tiles are in points (the panel's own units, relative to its top-left); the panel's rectangle on
//! the screen is in the monitor's physical pixels, like every bar window.

use egui::{pos2, vec2, Rect};
use windows::Win32::Foundation::RECT;

/// Most windows one panel shows.
pub const MAX_WINDOWS: usize = 6;
/// Space inside a tile around its picture and title.
const PAD: f32 = 8.0;
/// Space between the panel's edge and its tiles, and between tiles.
const PANEL_PAD: f32 = 6.0;
const GAP: f32 = 4.0;
/// Every picture is this tall; its width follows the window's shape, within limits.
const PREVIEW_H: f32 = 108.0;
const MIN_W: f32 = 128.0;
const MAX_W: f32 = 240.0;
/// A window with no known shape (minimized) gets a 16:10 plate.
const DEFAULT_ASPECT: f32 = 1.6;
const LABEL_H: f32 = 22.0;
const LABEL_GAP: f32 = 4.0;
/// The close button's size, at the right end of the title row.
pub const CLOSE: f32 = 18.0;
/// Space kept free at both sides of the monitor when the panel is wider than the monitor allows.
const SIDE_MARGIN: f32 = 12.0;
/// Space between the panel and the dock below it, in points.
pub const DOCK_GAP: f32 = 6.0;

/// One window's tile, relative to the panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tile {
    pub outer: Rect,
    /// Where the live picture (or the icon plate) goes.
    pub preview: Rect,
    /// The row under the picture: the app's small icon, the title, then the close button.
    pub label: Rect,
    pub close: Rect,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Layout {
    pub w: f32,
    pub h: f32,
    pub tiles: Vec<Tile>,
}

/// The picture box for a window of the given pixel size (None when it has none to show): always
/// `PREVIEW_H` tall, as wide as its shape asks within limits. The live picture is then fitted
/// inside, keeping the window's shape.
pub fn preview_size(source: Option<(f32, f32)>) -> (f32, f32) {
    let aspect = match source {
        Some((w, h)) if w > 0.0 && h > 0.0 => w / h,
        _ => DEFAULT_ASPECT,
    };
    ((PREVIEW_H * aspect).clamp(MIN_W, MAX_W), PREVIEW_H)
}

fn tile_height() -> f32 {
    PAD + PREVIEW_H + LABEL_GAP + LABEL_H + PAD
}

/// The widest the panel may be on a monitor `monitor_w` pixels wide at `scale`, in points.
pub fn max_width(monitor_w: i32, scale: f32) -> f32 {
    monitor_w as f32 / scale - 2.0 * SIDE_MARGIN
}

/// Lay tiles out left to right, starting a new row when the next would pass `max_width`; each
/// row is centred in the panel.
pub fn arrange(previews: &[(f32, f32)], max_width: f32) -> Layout {
    if previews.is_empty() {
        return Layout::default();
    }
    let room = max_width - 2.0 * PANEL_PAD;
    let mut rows: Vec<Vec<usize>> = vec![Vec::new()];
    let mut row_w = 0.0_f32;
    for (i, p) in previews.iter().enumerate() {
        let w = p.0 + 2.0 * PAD;
        let row = rows.last_mut().unwrap();
        if !row.is_empty() && row_w + GAP + w > room {
            rows.push(vec![i]);
            row_w = w;
        } else {
            row_w += if row.is_empty() { w } else { GAP + w };
            row.push(i);
        }
    }
    let row_width = |row: &[usize]| {
        row.iter().map(|&i| previews[i].0 + 2.0 * PAD).sum::<f32>() + GAP * (row.len() - 1) as f32
    };
    let widest = rows.iter().map(|r| row_width(r)).fold(0.0, f32::max);
    let w = widest + 2.0 * PANEL_PAD;
    let th = tile_height();
    let h = rows.len() as f32 * th + (rows.len() - 1) as f32 * GAP + 2.0 * PANEL_PAD;
    let mut tiles = vec![Tile::default(); previews.len()];
    for (r, row) in rows.iter().enumerate() {
        let mut x = (w - row_width(row)) / 2.0;
        let y = PANEL_PAD + r as f32 * (th + GAP);
        for &i in row {
            let pw = previews[i].0;
            let outer = Rect::from_min_size(pos2(x, y), vec2(pw + 2.0 * PAD, th));
            let label = Rect::from_min_size(
                outer.min + vec2(PAD, PAD + PREVIEW_H + LABEL_GAP),
                vec2(pw, LABEL_H),
            );
            tiles[i] = Tile {
                outer,
                preview: Rect::from_min_size(outer.min + vec2(PAD, PAD), vec2(pw, PREVIEW_H)),
                label,
                close: Rect::from_center_size(pos2(label.right() - CLOSE / 2.0, label.center().y), vec2(CLOSE, CLOSE)),
            };
            x += outer.width() + GAP;
        }
    }
    Layout { w, h, tiles }
}

impl Default for Tile {
    fn default() -> Self {
        Self { outer: Rect::ZERO, preview: Rect::ZERO, label: Rect::ZERO, close: Rect::ZERO }
    }
}

/// The panel's size in whole pixels at `scale`, never rounded down so nothing is cut off.
pub fn size_px(layout: &Layout, scale: f32) -> (i32, i32) {
    ((layout.w * scale).ceil() as i32, (layout.h * scale).ceil() as i32)
}

/// The panel's rectangle: centred on `center_x` (the hovered icon), its bottom edge at `bottom`
/// (just above the dock), kept inside `monitor`. A panel wider than the monitor is centred on it.
/// `edge` is the space kept free at the monitor's sides.
pub fn panel_rect(center_x: i32, bottom: i32, size: (i32, i32), monitor: RECT, edge: i32) -> RECT {
    let (w, h) = size;
    let free = monitor.right - monitor.left;
    let left = if w + 2 * edge >= free {
        monitor.left + (free - w) / 2
    } else {
        (center_x - w / 2).clamp(monitor.left + edge, monitor.right - edge - w)
    };
    let top = (bottom - h).max(monitor.top);
    RECT { left, top, right: left + w, bottom: top + h }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT { left, top, right, bottom }
    }

    #[test]
    fn a_picture_keeps_the_windows_shape_within_limits() {
        assert_eq!(preview_size(Some((1920.0, 1080.0))), (PREVIEW_H * 16.0 / 9.0, PREVIEW_H));
        // Very wide and very tall windows stop at the limits (the picture is then fitted inside).
        assert_eq!(preview_size(Some((4000.0, 400.0))).0, MAX_W);
        assert_eq!(preview_size(Some((300.0, 900.0))).0, MIN_W);
        // Unknown shape: the default plate.
        assert_eq!(preview_size(None), (PREVIEW_H * DEFAULT_ASPECT, PREVIEW_H));
        assert_eq!(preview_size(Some((0.0, 0.0))), preview_size(None));
    }

    #[test]
    fn one_row_is_centred_and_tiles_do_not_overlap() {
        let previews = [(200.0, PREVIEW_H), (150.0, PREVIEW_H), (128.0, PREVIEW_H)];
        let l = arrange(&previews, 2000.0);
        assert_eq!(l.tiles.len(), 3);
        assert_eq!(l.h, tile_height() + 2.0 * PANEL_PAD);
        assert_eq!(l.w, 200.0 + 150.0 + 128.0 + 6.0 * PAD + 2.0 * GAP + 2.0 * PANEL_PAD);
        assert_eq!(l.tiles[0].outer.min, pos2(PANEL_PAD, PANEL_PAD));
        for pair in l.tiles.windows(2) {
            assert_eq!(pair[1].outer.min.x, pair[0].outer.max.x + GAP);
        }
        // Each picture sits inside its tile, the title row below it, the close button at its end.
        let t = l.tiles[1];
        assert!(t.outer.contains_rect(t.preview) && t.outer.contains_rect(t.label));
        assert_eq!(t.preview.size(), vec2(150.0, PREVIEW_H));
        assert!(t.label.min.y >= t.preview.max.y);
        assert_eq!(t.close.max.x, t.label.max.x);
        assert!(t.label.contains_rect(t.close));
    }

    #[test]
    fn tiles_wrap_into_centred_rows_when_too_wide() {
        let previews = [(240.0, PREVIEW_H); 5];
        // Room for two tiles (2 * 256 + gap) but not three.
        let l = arrange(&previews, 2.0 * 256.0 + GAP + 2.0 * PANEL_PAD + 1.0);
        let rows: Vec<f32> = l.tiles.iter().map(|t| t.outer.min.y).collect();
        assert_eq!(rows[0], rows[1]);
        assert!(rows[2] > rows[1] && rows[2] == rows[3] && rows[4] > rows[3]);
        assert_eq!(l.h, 3.0 * tile_height() + 2.0 * GAP + 2.0 * PANEL_PAD);
        // The lone tile in the last row is centred.
        let last = l.tiles[4].outer;
        assert!(((last.min.x + last.max.x) / 2.0 - l.w / 2.0).abs() < 0.01);
        // Nothing sticks out of the panel.
        let all = Rect::from_min_size(pos2(0.0, 0.0), vec2(l.w, l.h));
        assert!(l.tiles.iter().all(|t| all.contains_rect(t.outer)));
    }

    #[test]
    fn no_windows_no_panel() {
        assert_eq!(arrange(&[], 1000.0), Layout::default());
    }

    #[test]
    fn size_is_rounded_up_to_whole_pixels() {
        let l = Layout { w: 100.3, h: 50.1, tiles: Vec::new() };
        assert_eq!(size_px(&l, 1.5), (151, 76));
        assert_eq!(size_px(&l, 1.0), (101, 51));
    }

    #[test]
    fn the_widest_panel_is_the_monitor_less_its_margins() {
        assert_eq!(max_width(3840, 3.0), 1280.0 - 2.0 * SIDE_MARGIN);
        assert_eq!(max_width(3840, 1.5), 2560.0 - 2.0 * SIDE_MARGIN);
    }

    #[test]
    fn the_panel_is_centred_on_the_icon_above_the_dock() {
        let monitor = rect(0, 0, 3840, 2160);
        let r = panel_rect(1920, 2000, (600, 240), monitor, 12);
        assert_eq!(r, rect(1620, 1760, 2220, 2000));
    }

    #[test]
    fn the_panel_is_pushed_in_at_the_monitors_edges() {
        let monitor = rect(0, 0, 3840, 2160);
        let left = panel_rect(100, 2000, (600, 240), monitor, 12);
        assert_eq!((left.left, left.right), (12, 612));
        let right = panel_rect(3800, 2000, (600, 240), monitor, 12);
        assert_eq!((right.left, right.right), (3840 - 12 - 600, 3840 - 12));
    }

    #[test]
    fn a_monitor_with_negative_coordinates() {
        // Left of the primary, and above it: nothing assumes the origin is a corner.
        let left = rect(-2560, -300, 0, 1140);
        let r = panel_rect(-2500, 1100, (500, 200), left, 10);
        assert_eq!(r, rect(-2550, 900, -2050, 1100));
        let r = panel_rect(-20, 1100, (500, 200), left, 10);
        assert_eq!((r.left, r.right), (-510, -10));
        let above = rect(0, -1080, 1920, 0);
        let r = panel_rect(960, -50, (400, 100), above, 10);
        assert_eq!(r, rect(760, -150, 1160, -50));
        // Taller than the room above the dock: stays on the monitor.
        let r = panel_rect(960, -1000, (400, 300), above, 10);
        assert_eq!((r.top, r.bottom), (-1080, -780));
    }

    #[test]
    fn a_panel_wider_than_the_monitor_is_centred_on_it() {
        let monitor = rect(-1000, 0, 0, 800);
        let r = panel_rect(-900, 700, (1200, 100), monitor, 12);
        assert_eq!((r.left, r.right), (-1100, 100));
        assert_eq!(r.left + r.right, monitor.left + monitor.right);
    }
}
