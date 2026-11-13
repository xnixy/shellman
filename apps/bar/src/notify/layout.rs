//! Where things go in the notifications window. Pure arithmetic in points, tested without
//! windows; the only outside input is how tall a wrapped body is, which `Measure` answers.
//!
//! The window is a column hanging under the top bar's clock island on the primary monitor,
//! centred on it, from the island's lower edge to the bottom of the work area; transparent and
//! click-through except on its islands. The islands (one per toast, and in the list a pill with
//! do-not-disturb and "Clear all") stack down from the top, newest nearest the clock, `EDGE` in
//! from the window's sides so their shadows fit.

use egui::{pos2, vec2, Rect};
use shellman_core::notify::Toast;

/// A toast island's width (the mockup's 360) and corner radius (its `.r`, 22).
pub const CARD_W: f32 = 360.0;
pub const RADIUS: f32 = 22.0;
/// Room around the islands inside the window, for their shadows.
pub const EDGE: f32 = 16.0;
/// From the clock island's lower edge to the first island under it.
pub const UNDER: f32 = 10.0;
pub const COLUMN_W: f32 = CARD_W + 2.0 * EDGE;
pub const PAD_X: f32 = 16.0;
pub const PAD_Y: f32 = 14.0;
pub const HEADER_H: f32 = 18.0;
const TITLE_GAP: f32 = 8.0;
pub const TITLE_H: f32 = 19.0;
const BODY_GAP: f32 = 2.0;
/// One body line at the body font; a body is cut after `MAX_BODY_LINES`.
pub const LINE_H: f32 = 18.0;
pub const MAX_BODY_LINES: f32 = 3.0;
pub const SMALL_H: f32 = 16.0;
const ROW_GAP: f32 = 12.0;
pub const BUTTON_H: f32 = 28.0;
pub const REPLY_H: f32 = 32.0;
/// Between islands, and between one app's group and the next in the list.
pub const GAP: f32 = 10.0;
pub const GROUP_GAP: f32 = 16.0;
pub const CONTROLS_H: f32 = 40.0;
/// The list shows this many of one app's toasts; the rest are counted on the last.
pub const PER_APP: usize = 3;

/// How tall `text` is, wrapped to `width` points, at the body font.
pub trait Measure {
    fn height(&self, text: &str, width: f32) -> f32;
}

/// The pieces of one toast island, relative to its top-left corner.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Parts {
    pub size: egui::Vec2,
    pub header: Rect,
    pub title: Option<Rect>,
    pub body: Option<Rect>,
    pub small: Option<Rect>,
    pub buttons: Option<Rect>,
    pub reply: Option<Rect>,
}

/// The body's text as drawn: its lines one under another.
pub fn body_text(toast: &Toast) -> String {
    toast.body.join("\n")
}

/// Lay one toast out. `small` is the line under the body (attribution, or "2 more").
pub fn card(toast: &Toast, small: bool, measure: &dyn Measure) -> Parts {
    let inner = CARD_W - 2.0 * PAD_X;
    let row = |y: f32, h: f32| Rect::from_min_size(pos2(PAD_X, y), vec2(inner, h));
    let mut y = PAD_Y;
    let header = row(y, HEADER_H);
    y += HEADER_H;
    let mut parts = Parts {
        size: egui::Vec2::ZERO,
        header,
        title: None,
        body: None,
        small: None,
        buttons: None,
        reply: None,
    };
    if !toast.title.is_empty() {
        y += TITLE_GAP;
        parts.title = Some(row(y, TITLE_H));
        y += TITLE_H;
    }
    let body = body_text(toast);
    if !body.is_empty() {
        y += BODY_GAP;
        let h = measure.height(&body, inner).clamp(LINE_H, LINE_H * MAX_BODY_LINES);
        parts.body = Some(row(y, h));
        y += h;
    }
    if small {
        y += BODY_GAP;
        parts.small = Some(row(y, SMALL_H));
        y += SMALL_H;
    }
    if !toast.actions.is_empty() {
        y += ROW_GAP;
        parts.buttons = Some(row(y, BUTTON_H));
        y += BUTTON_H;
    }
    if toast.has_reply() {
        y += ROW_GAP;
        parts.reply = Some(row(y, REPLY_H));
        y += REPLY_H;
    }
    parts.size = vec2(CARD_W, y + PAD_Y);
    parts
}

/// Stack islands of `heights` down from `top` (the first highest), `gaps[i]` above island `i`
/// (from the second on), centred in a window `width` wide; islands that would reach below
/// `bottom` are left out. Returns each island's rectangle, as many as fit.
pub fn stack(heights: &[f32], gaps: &[f32], top: f32, bottom: f32, width: f32) -> Vec<Rect> {
    let left = ((width - CARD_W) / 2.0).round();
    let mut out = Vec::with_capacity(heights.len());
    let mut y = top;
    for (i, h) in heights.iter().enumerate() {
        if i > 0 {
            y += gaps.get(i).copied().unwrap_or(GAP);
        }
        if y + h > bottom {
            break;
        }
        out.push(Rect::from_min_size(pos2(left, y), vec2(CARD_W, *h)));
        y += h;
    }
    out
}

/// The window's rectangle in physical pixels (`[left, top, right, bottom]`): `COLUMN_W` points
/// wide (at `scale` pixels per point), centred on `centre_x` but kept inside the work area
/// (`work`, pixels), from `top` (the clock island's lower edge) to the work area's bottom.
pub fn window_px(centre_x: i32, top: i32, work: [i32; 4], scale: f32) -> [i32; 4] {
    let w = ((COLUMN_W * scale).round() as i32).min(work[2] - work[0]);
    let left = (centre_x - w / 2).clamp(work[0], work[2] - w);
    [left, top, left + w, work[3].max(top)]
}

/// One monitor as the window needs it, in physical pixels (signed: monitors left of or above the
/// primary have negative origins): its rectangle, its work area, and its pixels per point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub rect: [i32; 4],
    pub work: [i32; 4],
    pub scale: f32,
}

/// Where a clock island hangs from, in screen pixels: the centre of the island and its lower
/// edge. `origin` is the bar window's client origin on the screen (signed), the island's
/// numbers are in points of that bar's window, `ppp` that monitor's pixels per point.
pub fn clock_anchor(origin: (i32, i32), centre_x: f32, bottom: f32, ppp: f32) -> (i32, i32) {
    (origin.0 + (centre_x * ppp).round() as i32, origin.1 + (bottom * ppp).round() as i32)
}

/// The monitor an anchor belongs to: the one holding the island's last pixel row (just above its
/// lower edge), else the nearest one. None without monitors.
pub fn area_of(anchor: (i32, i32), areas: &[Area]) -> Option<Area> {
    let (x, y) = (anchor.0, anchor.1 - 1);
    let inside = |a: &Area| x >= a.rect[0] && x < a.rect[2] && y >= a.rect[1] && y < a.rect[3];
    let distance = |a: &Area| {
        let dx = (a.rect[0] - x).max(x - (a.rect[2] - 1)).max(0) as i64;
        let dy = (a.rect[1] - y).max(y - (a.rect[3] - 1)).max(0) as i64;
        dx * dx + dy * dy
    };
    areas.iter().find(|a| inside(a)).or_else(|| areas.iter().min_by_key(|a| distance(a))).copied()
}

/// The window for an anchor on `area`: as wide as the column is at that monitor's scale, kept in
/// its work area, from the anchor down to the work area's bottom. None when that leaves nothing
/// (a zero or negative size must never reach the compositor).
pub fn window_for(anchor: (i32, i32), area: &Area) -> Option<[i32; 4]> {
    let top = anchor.1.clamp(area.work[1], area.work[3]);
    let r = window_px(anchor.0, top, area.work, area.scale);
    (r[2] > r[0] && r[3] - r[1] >= MIN_H).then_some(r)
}

/// The least height worth a window, in pixels.
const MIN_H: i32 = 40;

/// "now", "4 min", "2 h", "3 d": how long ago, as the header shows it.
pub fn ago(secs: u64) -> String {
    match secs {
        0..60 => "now".into(),
        60..3600 => format!("{} min", secs / 60),
        3600..86400 => format!("{} h", secs / 3600),
        _ => format!("{} d", secs / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellman_core::notify::source::sample;
    use shellman_core::notify::{Action, Activation, Reply};

    /// Seven points per character, wrapped by width.
    struct Chars;
    impl Measure for Chars {
        fn height(&self, text: &str, width: f32) -> f32 {
            let per_line = (width / 7.0).floor() as usize;
            text.lines().map(|l| l.chars().count().div_ceil(per_line).max(1)).sum::<usize>() as f32 * LINE_H
        }
    }

    #[test]
    fn a_plain_toast_is_header_title_and_body() {
        let parts = card(&sample(1, "a", "Title", 1), false, &Chars);
        assert_eq!(parts.size.x, CARD_W);
        assert_eq!(parts.size.y, PAD_Y + HEADER_H + 8.0 + TITLE_H + 2.0 + LINE_H + PAD_Y);
        assert!(parts.buttons.is_none() && parts.reply.is_none());
        let body = parts.body.unwrap();
        assert_eq!(body.left(), PAD_X);
        assert_eq!(body.width(), CARD_W - 2.0 * PAD_X);
    }

    #[test]
    fn a_long_body_is_cut_at_three_lines() {
        let mut t = sample(1, "a", "Title", 1);
        t.body = vec!["word ".repeat(200)];
        assert_eq!(card(&t, false, &Chars).body.unwrap().height(), 3.0 * LINE_H);
    }

    #[test]
    fn buttons_and_reply_add_their_rows() {
        let mut t = sample(1, "a", "Title", 1);
        let plain = card(&t, false, &Chars).size.y;
        let send = Action { label: "Send".into(), args: "r".into(), kind: Activation::Background, input: Some("r".into()) };
        t.actions = vec![Action { input: None, label: "OK".into(), ..send.clone() }];
        t.reply = Some(Reply { id: "r".into(), placeholder: "Reply".into(), send });
        let parts = card(&t, true, &Chars);
        assert_eq!(parts.size.y, plain + 2.0 + SMALL_H + 12.0 + BUTTON_H + 12.0 + REPLY_H);
        assert!(parts.reply.unwrap().top() > parts.buttons.unwrap().bottom());
    }

    #[test]
    fn islands_stack_down_from_the_top_until_the_bottom() {
        let rects = stack(&[100.0, 50.0, 80.0], &[0.0, 10.0, 16.0], 10.0, 200.0, COLUMN_W);
        assert_eq!(rects.len(), 2);
        assert_eq!((rects[0].top(), rects[0].bottom()), (10.0, 110.0));
        assert_eq!((rects[1].top(), rects[1].bottom()), (120.0, 170.0));
        assert_eq!(rects[0].left(), EDGE);
        assert_eq!(rects[0].right(), COLUMN_W - EDGE);
    }

    #[test]
    fn the_window_hangs_centred_under_the_clock() {
        // 150%: 392 points are 588 pixels, centred on the clock at 1920.
        assert_eq!(window_px(1920, 78, [0, 90, 3840, 2031], 1.5), [1626, 78, 2214, 2031]);
        // Kept inside the work area, and never wider than it.
        assert_eq!(window_px(3800, 78, [0, 90, 3840, 2031], 1.5), [3252, 78, 3840, 2031]);
        assert_eq!(window_px(150, 0, [0, 0, 300, 500], 1.0), [0, 0, 300, 500]);
    }

    /// The setup: primary 4K at 100%, a 4K at 150% to its left, a 4K at 300% to its right.
    fn setup() -> [Area; 3] {
        let area = |l: i32, scale: f32| Area { rect: [l, 0, l + 3840, 2160], work: [l, 80, l + 3840, 2100], scale };
        [area(0, 1.0), area(-3840, 1.5), area(3840, 3.0)]
    }

    #[test]
    fn the_anchor_is_in_signed_screen_pixels() {
        // A bar at x = -3840 (150%): the clock centre at 600 pt is 900 px in.
        assert_eq!(clock_anchor((-3840, 0), 600.0, 50.0, 1.5), (-3840 + 900, 75));
        assert_eq!(clock_anchor((3840, 0), 400.0, 20.0, 3.0), (3840 + 1200, 60));
        assert_eq!(clock_anchor((0, -2160), 100.0, 40.0, 1.0), (100, -2120));
    }

    #[test]
    fn the_anchor_picks_its_own_monitor() {
        let areas = setup();
        assert_eq!(area_of((1920, 78), &areas), Some(areas[0]));
        assert_eq!(area_of((-1920, 75), &areas), Some(areas[1]));
        assert_eq!(area_of((5760, 60), &areas), Some(areas[2]));
        // The lower edge on the next monitor's top row still belongs to the bar above it.
        assert_eq!(area_of((-3840, 1), &areas), Some(areas[1]));
        // Off every monitor: the nearest. None without monitors.
        assert_eq!(area_of((-9000, 50), &areas), Some(areas[1]));
        assert_eq!(area_of((0, 0), &[]), None);
    }

    #[test]
    fn the_window_is_sized_and_kept_by_the_monitor_it_hangs_on() {
        let areas = setup();
        // 100%: 392 px wide, centred on the clock.
        assert_eq!(window_for((1920, 78), &areas[0]), Some([1724, 80, 2116, 2100]));
        // 150%, left of the primary: 588 px, negative coordinates.
        assert_eq!(window_for((-1920, 78), &areas[1]), Some([-2214, 80, -1626, 2100]));
        // 300%: 1176 px; kept inside the work area at the monitor's edges.
        assert_eq!(window_for((5760, 78), &areas[2]), Some([5172, 80, 6348, 2100]));
        assert_eq!(window_for((7600, 78), &areas[2]), Some([6504, 80, 7680, 2100]));
        assert_eq!(window_for((-3800, 78), &areas[1]), Some([-3840, 80, -3252, 2100]));
    }

    #[test]
    fn no_work_area_left_means_no_window() {
        let flat = Area { rect: [0, 0, 100, 100], work: [0, 90, 100, 90], scale: 1.0 };
        assert_eq!(window_for((50, 90), &flat), None);
        let thin = Area { work: [0, 90, 100, 100], ..flat };
        assert_eq!(window_for((50, 90), &thin), None);
        let empty = Area { work: [0, 0, 0, 0], ..flat };
        assert_eq!(window_for((0, 0), &empty), None);
    }

    #[test]
    fn how_long_ago() {
        assert_eq!(ago(5), "now");
        assert_eq!(ago(240), "4 min");
        assert_eq!(ago(7200), "2 h");
        assert_eq!(ago(200_000), "2 d");
    }
}
