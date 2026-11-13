//! Where the widgets go: a fixed stack top right of a monitor's work area, under the top bar
//! (the design's "Shelf + Widgets": 372 pt wide, 40 pt from the right edge, Today above Now
//! playing). Each monitor that shows widgets gets one small window sized to the tallest stack
//! the widgets can make plus their shadows, worked out once per monitor and scale, never per
//! frame and never the monitor's size. Pure, so it is tested without windows.

use crate::displays::usable;
use crate::geometry::SHADOW;
use egui::{pos2, vec2, Rect};
use shellman_core::monitors::Monitor;
use shellman_ui::glass::guard;
use windows::Win32::Foundation::RECT;

/// The widgets' width.
pub const WIDTH: f32 = 372.0;
/// From the work area's right edge to the widgets.
pub const RIGHT: f32 = 40.0;
/// From the work area's top (the top bar's lower edge) to the first widget.
pub const TOP: f32 = 16.0;
/// Between one widget and the next.
pub const GAP: f32 = 16.0;
pub const RADIUS: f32 = 22.0;

/// Today: padding (top, sides, bottom).
pub const PAD_TOP: f32 = 22.0;
pub const PAD_X: f32 = 24.0;
pub const PAD_BOTTOM: f32 = 12.0;
/// The date block: weekday, the day's number, the month.
pub const HEADER_H: f32 = 116.0;
/// Between the date and the first row.
pub const ROWS_GAP: f32 = 16.0;
pub const ROW_H: f32 = 65.0;
/// The "Tomorrow" heading over tomorrow's rows.
pub const HEADING_H: f32 = 34.0;
/// A note in place of rows ("Sign-in failed" over a line saying what to do), and one of a single
/// line ("Nothing on today or tomorrow").
pub const NOTE_H: f32 = 65.0;
pub const NOTE_ONE_H: f32 = 46.0;
/// The most event rows Today lists.
pub const MAX_ROWS: usize = 3;

/// Now playing: padding and the art's side.
pub const PLAY_PAD: f32 = 12.0;
pub const ART: f32 = 54.0;

/// What Today shows below its date, for its height.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TodayShape {
    pub rows: usize,
    pub heading: bool,
    /// The lines of a note standing in for rows: 0 (none), 1 or 2.
    pub note: u8,
}

pub fn today_height(s: TodayShape) -> f32 {
    let mut h = PAD_TOP + HEADER_H + ROWS_GAP + s.rows.min(MAX_ROWS) as f32 * ROW_H + PAD_BOTTOM;
    if s.heading {
        h += HEADING_H;
    }
    h += match s.note {
        0 => 0.0,
        1 => NOTE_ONE_H,
        _ => NOTE_H,
    };
    h
}

pub fn playing_height() -> f32 {
    2.0 * PLAY_PAD + ART
}

/// The tallest the stack can be: Today with a heading and every row (a note only stands in for
/// rows, so it is shorter), then Now playing.
pub fn max_stack() -> f32 {
    let rows = today_height(TodayShape { rows: MAX_ROWS, heading: true, note: 0 });
    let note = today_height(TodayShape { rows: 0, heading: true, note: 2 });
    rows.max(note) + GAP + playing_height()
}

/// Where one monitor's widget window goes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Placement {
    /// The window, in physical screen pixels.
    pub rect: RECT,
    /// The widgets' column (top-left and width) in the window's points.
    pub column: Rect,
    pub scale: f32,
}

/// The window for monitor `m`, or None when the monitor is not fit for one (a monitor in the
/// middle of a change, or too small to leave most of it free).
pub fn place(m: &Monitor) -> Option<Placement> {
    if !usable(m) {
        return None;
    }
    let scale = m.scale;
    let px = |pt: f32| (pt * scale).round() as i32;
    // The column's top-left on the monitor, in pixels.
    let col_right = m.work.right - px(RIGHT);
    let col_left = col_right - px(WIDTH);
    let col_top = m.work.top + px(TOP);
    let left = (col_left - px(SHADOW)).max(m.rect.left);
    let right = (col_right + px(SHADOW)).min(m.rect.right);
    let top = (col_top - px(SHADOW)).max(m.rect.top);
    let bottom = (col_top + px(max_stack() + SHADOW)).min(m.work.bottom).min(m.rect.bottom);
    let (w, h) = (right - left, bottom - top);
    let mon_w = m.rect.right - m.rect.left;
    // Never the monitor's size: at most half its width, and the guard takes the size unchanged.
    if guard::window((w, h)) != Some((w, h)) || w * 2 > mon_w || col_left < m.work.left || h < px(max_stack()) / 2 {
        return None;
    }
    let column = Rect::from_min_size(
        pos2((col_left - left) as f32 / scale, (col_top - top) as f32 / scale),
        vec2(WIDTH, max_stack()),
    );
    Some(Placement { rect: RECT { left, top, right, bottom }, column, scale })
}

/// The islands' rectangles for a Today of `today` (None: off) and Now playing when `playing`,
/// stacked down the column.
pub fn stack(column: Rect, today: Option<TodayShape>, playing: bool) -> (Option<Rect>, Option<Rect>) {
    let mut y = column.top();
    let t = today.map(|s| {
        let r = Rect::from_min_size(pos2(column.left(), y), vec2(WIDTH, today_height(s)));
        y = r.bottom() + GAP;
        r
    });
    let p = playing.then(|| Rect::from_min_size(pos2(column.left(), y), vec2(WIDTH, playing_height())));
    (t, p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, w: i32, h: i32) -> RECT {
        RECT { left, top, right: left + w, bottom: top + h }
    }

    fn mon(left: i32, top: i32, w: i32, h: i32, scale: f32) -> Monitor {
        let r = rect(left, top, w, h);
        let work = RECT { top: r.top + (60.0 * scale) as i32, bottom: r.bottom - (102.0 * scale) as i32, ..r };
        Monitor { device: "x".into(), rect: r, work, scale, primary: false }
    }

    /// The window sits top right inside its monitor, is far smaller than it, and the whole
    /// stack, at its tallest, fits inside the window.
    fn check(m: &Monitor) -> Placement {
        let p = place(m).expect("widgets");
        let r = p.rect;
        assert!(r.left >= m.rect.left && r.right <= m.rect.right && r.top >= m.rect.top && r.bottom <= m.rect.bottom, "{r:?}");
        assert!((r.right - r.left) * 2 < m.rect.right - m.rect.left, "{r:?}");
        assert!((r.bottom - r.top) < m.rect.bottom - m.rect.top);
        let window = Rect::from_min_size(pos2(0.0, 0.0), vec2((r.right - r.left) as f32 / p.scale, (r.bottom - r.top) as f32 / p.scale));
        let (t, n) = stack(p.column, Some(TodayShape { rows: MAX_ROWS, heading: true, note: 0 }), true);
        assert!(window.expand(0.5).contains_rect(t.unwrap()), "{t:?} in {window:?}");
        assert!(window.expand(0.5).contains_rect(n.unwrap()), "{n:?} in {window:?}");
        // 40 pt from the work area's right, 16 pt under its top.
        let right_on_monitor = r.left as f32 + p.column.right() * p.scale;
        assert!((m.work.right as f32 - right_on_monitor - RIGHT * p.scale).abs() <= 1.0);
        let top_on_monitor = r.top as f32 + p.column.top() * p.scale;
        assert!((top_on_monitor - m.work.top as f32 - TOP * p.scale).abs() <= 1.0);
        p
    }

    #[test]
    fn left_of_the_primary_at_150_percent() {
        let p = check(&mon(-3840, 0, 3840, 2160, 1.5));
        assert!(p.rect.right <= 0 && p.rect.left > -3840 + 1920);
    }

    #[test]
    fn at_100_and_300_percent() {
        check(&mon(0, 0, 1920, 1080, 1.0));
        let p = check(&mon(3746, 2160, 3840, 2160, 3.0));
        assert!(p.rect.top >= 2160);
        // Above the primary, at a negative origin.
        check(&mon(-1920, -1080, 1920, 1080, 1.0));
    }

    #[test]
    fn no_window_on_a_monitor_too_small_or_in_a_change() {
        assert_eq!(place(&mon(0, 0, 1280, 800, 2.0)), None);
        let mut bad = mon(0, 0, 3840, 2160, 1.5);
        bad.rect = rect(0, 0, 0, 0);
        assert_eq!(place(&bad), None);
        bad = mon(0, 0, 3840, 2160, f32::NAN);
        assert_eq!(place(&bad), None);
    }

    #[test]
    fn heights_follow_the_content() {
        let none = today_height(TodayShape { rows: 0, heading: false, note: 2 });
        let two = today_height(TodayShape { rows: 2, heading: false, note: 0 });
        assert_eq!(two - (none - NOTE_H), 2.0 * ROW_H);
        let (t, p) = stack(Rect::from_min_size(pos2(10.0, 20.0), vec2(WIDTH, max_stack())), None, true);
        assert!(t.is_none());
        assert_eq!(p.unwrap().top(), 20.0);
    }
}
