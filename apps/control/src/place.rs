//! Where the panel goes: hanging from the top-right corner of a monitor, just under the top
//! bar, as tall as its content.

/// The panel's width, in points.
pub const WIDTH: f32 = 360.0;
/// The panel's corner radius, in points.
pub const RADIUS: f32 = 22.0;
/// The top bar's height, and the gap the panel keeps from it and from the screen's right edge.
const BAR: f32 = 32.0;
const GAP: f32 = 8.0;
/// Never shorter than this, so an empty first frame still shows a panel.
const MIN_HEIGHT: f32 = 120.0;

/// A rectangle in physical pixels: left, top, width, height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Px {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// The panel on a monitor whose full rectangle is `(left, top, right, bottom)` in pixels, at
/// `scale` pixels per point, for content `height` points tall. A panel taller than the screen
/// stops at the bottom gap.
pub fn panel_rect(monitor: (i32, i32, i32, i32), scale: f32, height: f32) -> Px {
    let (left, top, right, bottom) = monitor;
    let px = |pt: f32| (pt * scale).round() as i32;
    let w = px(WIDTH).min(right - left - 2 * px(GAP));
    let y = top + px(BAR + GAP);
    let h = px(height.max(MIN_HEIGHT)).min(bottom - y - px(GAP)).max(1);
    let x = right - px(GAP) - w;
    Px { x, y, w, h }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hangs_from_the_top_right_corner() {
        let r = panel_rect((0, 0, 1920, 1080), 1.0, 500.0);
        assert_eq!(r, Px { x: 1920 - 8 - 360, y: 40, w: 360, h: 500 });
    }

    #[test]
    fn scales_on_a_monitor_left_of_the_primary() {
        // A 4K monitor at 150%, placed left of the primary.
        let r = panel_rect((-3840, 0, 0, 2160), 1.5, 500.0);
        assert_eq!(r, Px { x: -12 - 540, y: 60, w: 540, h: 750 });
    }

    #[test]
    fn stays_on_short_screens() {
        let r = panel_rect((0, 0, 1280, 600), 2.0, 600.0);
        assert_eq!(r.y, 80);
        assert_eq!(r.y + r.h, 600 - 16);
    }

    #[test]
    fn empty_content_still_shows_a_panel() {
        let r = panel_rect((0, 0, 1920, 1080), 1.0, 0.0);
        assert_eq!(r.h, MIN_HEIGHT as i32);
    }
}
