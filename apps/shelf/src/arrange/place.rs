//! Where the shelf and the widgets go on a monitor once Edit desktop… has moved them, how a
//! dragged piece snaps, and how saved places stay valid when the monitors change. Pure, so it is
//! tested without windows.
//!
//! Everything here is in points from the monitor's top-left ("monitor points"); the work area is
//! the space between the top bar and the dock in those points. A saved place is a [`Spot`] (or a
//! [`Pin`] for the shelf, which keeps to its edge and moves only up and down): whole points from
//! an edge or the middle of the work area, so it means the same on any size, scale or bar layout,
//! and it is always cut back into the work area when it is turned into a rectangle. A widget that
//! would then lie over the shelf or the other widget is moved to the nearest free side of it.

use crate::geometry::{self, EDGE_GAP, SHADOW, SHELF_W};
use crate::widgets::place::{RIGHT, TOP, WIDTH};
use egui::{pos2, vec2, Pos2, Rect};
use shellman_core::monitors::Monitor;
use shellman_core::settings::desktop::{Anchor, Arrangement, Edge, Pin, Spot};
use shellman_ui::glass::guard;
use windows::Win32::Foundation::RECT;

/// Between pieces when they snap beside each other (the widgets' gap).
pub const GAP: f32 = 16.0;
/// From the work area's sides and its top and bottom to a widget snapped there.
pub const MARGIN_X: f32 = RIGHT;
pub const MARGIN_Y: f32 = TOP;
/// How near (points) a dragged edge must come to a line to snap to it.
pub const SNAP: f32 = 12.0;
/// The key of the primary monitor's arrangement: the primary keeps its place whatever Windows
/// calls it today (docking a laptop renames displays).
pub const PRIMARY: &str = "primary";

/// The arrangement's key for a monitor.
pub fn key(m: &Monitor) -> String {
    if m.primary {
        PRIMARY.to_owned()
    } else {
        m.device.clone()
    }
}

/// A name for the monitor people can read: "main display", "display 2".
pub fn display_name(m: &Monitor) -> String {
    if m.primary {
        return "main display".to_owned();
    }
    let digits: String = m.device.chars().rev().take_while(char::is_ascii_digit).collect::<Vec<_>>().into_iter().rev().collect();
    if digits.is_empty() {
        "another display".to_owned()
    } else {
        format!("display {digits}")
    }
}

/// The monitor's work area in monitor points.
pub fn work(m: &Monitor) -> Rect {
    let s = if m.scale.is_finite() && m.scale > 0.0 { m.scale } else { 1.0 };
    Rect::from_min_max(
        pos2((m.work.left - m.rect.left) as f32 / s, (m.work.top - m.rect.top) as f32 / s),
        pos2((m.work.right - m.rect.left) as f32 / s, (m.work.bottom - m.rect.top) as f32 / s),
    )
}

/// Where a piece `size` long starts on an axis from `lo` to `hi`, pinned by `pin`; always inside
/// (at `lo` when it does not fit).
pub fn start(pin: Pin, lo: f32, hi: f32, size: f32) -> f32 {
    let by = pin.by as f32;
    let at = match pin.from {
        Anchor::Start => lo + by,
        Anchor::End => hi - by - size,
        Anchor::Middle => (lo + hi) / 2.0 + by - size / 2.0,
    };
    clamp_start(at, lo, hi, size)
}

fn clamp_start(at: f32, lo: f32, hi: f32, size: f32) -> f32 {
    if !at.is_finite() {
        return lo;
    }
    at.clamp(lo, (hi - size).max(lo))
}

/// The pin for a piece starting at `at`: from the edge it is nearer to, or from the middle when
/// its centre is in the middle third.
pub fn pin_for(at: f32, lo: f32, hi: f32, size: f32) -> Pin {
    let at = clamp_start(at, lo, hi, size);
    let len = (hi - lo).max(1.0);
    let centre = (at + size / 2.0 - lo) / len;
    if centre < 1.0 / 3.0 {
        Pin { from: Anchor::Start, by: (at - lo).round() as i32 }
    } else if centre > 2.0 / 3.0 {
        Pin { from: Anchor::End, by: (hi - at - size).round() as i32 }
    } else {
        Pin { from: Anchor::Middle, by: (at + size / 2.0 - (lo + hi) / 2.0).round() as i32 }
    }
}

/// The spot for a widget at `r` in the work area.
pub fn spot_for(r: Rect, work: Rect) -> Spot {
    Spot { x: pin_for(r.left(), work.left(), work.right(), r.width()), y: pin_for(r.top(), work.top(), work.bottom(), r.height()) }
}

/// The rectangle of a widget `size` big at `spot`, inside the work area.
pub fn at_spot(spot: Spot, work: Rect, size: egui::Vec2) -> Rect {
    let x = start(spot.x, work.left(), work.right(), size.x);
    let y = start(spot.y, work.top(), work.bottom(), size.y);
    Rect::from_min_size(pos2(x, y), size)
}

/// Today's place by the design: top right, 40 pt from the right, 16 pt under the top bar.
pub fn default_today() -> Spot {
    Spot { x: Pin { from: Anchor::End, by: MARGIN_X as i32 }, y: Pin { from: Anchor::Start, by: MARGIN_Y as i32 } }
}

/// The shelf's place by the design: centred between the bars.
pub fn default_shelf() -> Pin {
    Pin { from: Anchor::Middle, by: 0 }
}

/// The shelf's top (monitor points) for a shelf `h` tall.
pub fn shelf_top(work: Rect, h: f32, pin: Option<Pin>) -> f32 {
    start(pin.unwrap_or_else(default_shelf), work.top(), work.bottom(), h)
}

/// The shelf's left side on `edge`.
pub fn shelf_left(work: Rect, edge: Edge) -> f32 {
    match edge {
        Edge::Left => work.left() + EDGE_GAP,
        Edge::Right => work.right() - EDGE_GAP - SHELF_W,
    }
}

/// The shelf with `stacks` stacks.
pub fn shelf_rect(work: Rect, edge: Edge, stacks: usize, pin: Option<Pin>) -> Rect {
    let h = geometry::shelf_height(stacks);
    Rect::from_min_size(pos2(shelf_left(work, edge), shelf_top(work, h, pin)), vec2(SHELF_W, h))
}

/// The two widgets' rectangles on a monitor, for a Today `today_h` tall (None: not shown) and
/// Now playing `playing_h` tall (None: not shown), with the shelf at `shelf` (if this monitor
/// has one). Always inside the work area; neither lies over the shelf or the other when there is
/// room beside them.
pub fn widgets(work: Rect, shelf: Option<Rect>, a: &Arrangement, today_h: Option<f32>, playing_h: Option<f32>) -> (Option<Rect>, Option<Rect>) {
    let avoid_shelf: Vec<Rect> = shelf.into_iter().collect();
    let today_spot = a.today.unwrap_or_else(default_today);
    let today = today_h.map(|h| separate(at_spot(today_spot, work, vec2(WIDTH, h)), &avoid_shelf, work));
    let playing = playing_h.map(|h| {
        let size = vec2(WIDTH, h);
        match a.playing {
            Some(spot) => {
                let mut avoid = avoid_shelf.clone();
                avoid.extend(today);
                separate(at_spot(spot, work, size), &avoid, work)
            }
            // Under Today (in Today's place when Today is off), above it when there is no room.
            None => match today {
                Some(t) => under(t, size, work),
                None => separate(at_spot(today_spot, work, size), &avoid_shelf, work),
            },
        }
    });
    (today, playing)
}

/// A widget `size` big right under `t` (above it when it does not fit below).
pub fn under(t: Rect, size: egui::Vec2, work: Rect) -> Rect {
    let below = Rect::from_min_size(pos2(t.left(), t.bottom() + GAP), size);
    if below.bottom() <= work.bottom() + 0.01 {
        return below;
    }
    let above = Rect::from_min_size(pos2(t.left(), t.top() - GAP - size.y), size);
    if above.top() >= work.top() - 0.01 {
        return above;
    }
    clamp(below, work)
}

/// Whether Now playing at `p` sits right under Today at `t`, as it does by default (so dropping
/// it there ties it to Today again).
pub fn is_under(p: Rect, t: Rect) -> bool {
    (p.left() - t.left()).abs() < 0.5 && (p.top() - (t.bottom() + GAP)).abs() < 0.5
}

/// `r` moved inside the work area.
pub fn clamp(r: Rect, work: Rect) -> Rect {
    let x = clamp_start(r.left(), work.left(), work.right(), r.width());
    let y = clamp_start(r.top(), work.top(), work.bottom(), r.height());
    Rect::from_min_size(pos2(x, y), r.size())
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.intersects(b) && a.intersect(b).area() > 0.5
}

/// `r` inside the work area and, when it lies over any of `avoid`, moved to the nearest place
/// beside one of them (below, above, right, left) that is free and inside; left over them when
/// there is no such place.
pub fn separate(r: Rect, avoid: &[Rect], work: Rect) -> Rect {
    let r = clamp(r, work);
    if !avoid.iter().any(|a| overlaps(r, *a)) {
        return r;
    }
    let mut best: Option<(f32, Rect)> = None;
    for a in avoid {
        let tries = [
            pos2(r.left(), a.bottom() + GAP),
            pos2(r.left(), a.top() - GAP - r.height()),
            pos2(a.right() + GAP, r.top()),
            pos2(a.left() - GAP - r.width(), r.top()),
        ];
        for p in tries {
            let c = Rect::from_min_size(p, r.size());
            let inside = c.left() >= work.left() - 0.01 && c.right() <= work.right() + 0.01 && c.top() >= work.top() - 0.01 && c.bottom() <= work.bottom() + 0.01;
            if !inside || avoid.iter().any(|o| overlaps(c, *o)) {
                continue;
            }
            let d = (c.min - r.min).length();
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, c));
            }
        }
    }
    best.map_or(r, |(_, c)| c)
}

/// The nearest of `targets` (a position for the moving side, and the shift that gets it there)
/// within `SNAP`.
fn nearest(shifts: impl Iterator<Item = f32>) -> Option<f32> {
    shifts.filter(|d| d.abs() <= SNAP).min_by(|a, b| a.abs().total_cmp(&b.abs()))
}

/// A widget dragged to `r`, snapped on each axis to the work area's sides (at the margins) and
/// middle and to the edges of `others` (lined up with them or `GAP` beside them), then kept
/// inside.
pub fn snap_widget(r: Rect, work: Rect, others: &[Rect]) -> Rect {
    let mut xs = vec![work.left() + MARGIN_X - r.left(), work.right() - MARGIN_X - r.right(), work.center().x - r.center().x];
    let mut ys = vec![work.top() + MARGIN_Y - r.top(), work.bottom() - MARGIN_Y - r.bottom(), work.center().y - r.center().y];
    for o in others {
        xs.extend([o.left() - r.left(), o.right() - r.right(), o.right() + GAP - r.left(), o.left() - GAP - r.right()]);
        ys.extend([o.top() - r.top(), o.bottom() - r.bottom(), o.bottom() + GAP - r.top(), o.top() - GAP - r.bottom()]);
    }
    let dx = nearest(xs.into_iter()).unwrap_or(0.0);
    let dy = nearest(ys.into_iter()).unwrap_or(0.0);
    clamp(r.translate(vec2(dx, dy)), work)
}

/// The shelf's top when dragged to `top` (it is `h` tall): snapped to the middle, the margins at
/// the top and bottom of the work area, and the tops, bottoms and middles of `others`; inside.
pub fn snap_shelf(top: f32, h: f32, work: Rect, others: &[Rect]) -> f32 {
    let mut ys = vec![work.center().y - h / 2.0 - top, work.top() + MARGIN_Y - top, work.bottom() - MARGIN_Y - h - top];
    for o in others {
        ys.extend([o.top() - top, o.bottom() - h - top, o.center().y - h / 2.0 - top]);
    }
    let dy = nearest(ys.into_iter()).unwrap_or(0.0);
    clamp_start(top + dy, work.top(), work.bottom(), h)
}

/// Which edge a shelf dragged with the pointer at `x` (monitor points) belongs on: the half of
/// the work area the pointer is in.
pub fn edge_at(x: f32, work: Rect) -> Edge {
    if x < work.center().x {
        Edge::Left
    } else {
        Edge::Right
    }
}

/// Where one widget's window goes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct WidgetPlacement {
    /// The window, in physical screen pixels.
    pub rect: RECT,
    /// The widget's top-left in the window's points.
    pub origin: Pos2,
    pub scale: f32,
}

/// The window for a widget at `island` (monitor points, as tall as it is now) that can grow to
/// `max_h`: the island and its shadow, as tall as the widget can grow, moved up when that would
/// pass the monitor's bottom, cut to the monitor; never the monitor's size. None when the window
/// would not be a sane size.
pub fn widget_window(m: &Monitor, island: Rect, max_h: f32) -> Option<WidgetPlacement> {
    if !crate::displays::usable(m) {
        return None;
    }
    let s = m.scale;
    let mon_h = (m.rect.bottom - m.rect.top) as f32 / s;
    let mon_w = (m.rect.right - m.rect.left) as f32 / s;
    let tall = max_h.max(island.height()) + 2.0 * SHADOW;
    let mut top = island.top() - SHADOW;
    if top + tall > mon_h {
        top = (mon_h - tall).min(island.top()).max(0.0);
    }
    let top = top.max(0.0);
    let bottom = (top + tall).min(mon_h);
    let left = (island.left() - SHADOW).max(0.0);
    let right = (island.right() + SHADOW).min(mon_w);
    let px = |v: f32| (v * s).round() as i32;
    let rect = RECT { left: m.rect.left + px(left), top: m.rect.top + px(top), right: m.rect.left + px(right), bottom: m.rect.top + px(bottom) };
    let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
    if guard::window((w, h)) != Some((w, h)) || w * 2 > m.rect.right - m.rect.left || w < 24 || h < 24 {
        return None;
    }
    // Where the island lands in the window, from the rounded window corner.
    let origin = pos2(island.left() - (rect.left - m.rect.left) as f32 / s, island.top() - (rect.top - m.rect.top) as f32 / s);
    Some(WidgetPlacement { rect, origin, scale: s })
}

/// Words for where a piece is, for screen readers: "40 pt from the right, 16 pt from the top",
/// "centred".
pub fn describe(r: Rect, work: Rect) -> String {
    let x = pin_for(r.left(), work.left(), work.right(), r.width());
    let y = pin_for(r.top(), work.top(), work.bottom(), r.height());
    let axis = |p: Pin, start: &str, end: &str, before: &str, after: &str| match p.from {
        Anchor::Start => format!("{} pt from the {start}", p.by),
        Anchor::End => format!("{} pt from the {end}", p.by),
        Anchor::Middle if p.by == 0 => "centred".to_owned(),
        Anchor::Middle if p.by < 0 => format!("{} pt {before} the middle", -p.by),
        Anchor::Middle => format!("{} pt {after} the middle", p.by),
    };
    format!("{}, {}", axis(x, "left", "right", "left of", "right of"), axis(y, "top", "bottom", "above", "below"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::place::{playing_height, today_height, today_max, TodayShape, MAX_ROWS};

    fn rect(left: i32, top: i32, w: i32, h: i32) -> RECT {
        RECT { left, top, right: left + w, bottom: top + h }
    }

    fn mon(device: &str, left: i32, top: i32, w: i32, h: i32, scale: f32, primary: bool) -> Monitor {
        let r = rect(left, top, w, h);
        let work = RECT { top: r.top + (60.0 * scale) as i32, bottom: r.bottom - (102.0 * scale) as i32, ..r };
        Monitor { device: device.into(), rect: r, work, scale, primary }
    }

    fn today_h() -> f32 {
        today_height(TodayShape { rows: 2, heading: false, note: 0 })
    }

    fn inside(r: Rect, work: Rect) -> bool {
        work.expand(0.01).contains_rect(r)
    }

    #[test]
    fn pins_measure_from_the_nearer_edge_and_round_trip() {
        let (lo, hi) = (0.0, 1000.0);
        assert_eq!(pin_for(40.0, lo, hi, 100.0), Pin { from: Anchor::Start, by: 40 });
        assert_eq!(pin_for(860.0, lo, hi, 100.0), Pin { from: Anchor::End, by: 40 });
        assert_eq!(pin_for(450.0, lo, hi, 100.0), Pin { from: Anchor::Middle, by: 0 });
        for at in [0.0, 40.0, 333.0, 450.0, 700.0, 900.0] {
            assert!((start(pin_for(at, lo, hi, 100.0), lo, hi, 100.0) - at).abs() < 0.51, "{at}");
        }
        // Too big for the axis: at its start.
        assert_eq!(start(Pin { from: Anchor::End, by: 10 }, 0.0, 50.0, 100.0), 0.0);
    }

    #[test]
    fn a_widget_on_the_right_stays_on_the_right_at_another_size() {
        let big = work(&mon("A", 0, 0, 3840, 2160, 1.5, true));
        let small = work(&mon("A", 0, 0, 1920, 1080, 1.0, true));
        let size = vec2(WIDTH, 200.0);
        let r = at_spot(default_today(), big, size);
        assert!((big.right() - r.right() - MARGIN_X).abs() < 0.01);
        let spot = spot_for(Rect::from_min_size(pos2(big.right() - 100.0 - WIDTH, big.bottom() - 30.0 - 200.0), size), big);
        let moved = at_spot(spot, small, size);
        assert!((small.right() - moved.right() - 100.0).abs() < 0.51);
        assert!((small.bottom() - moved.bottom() - 30.0).abs() < 0.51);
    }

    #[test]
    fn saved_places_are_cut_back_into_a_smaller_work_area() {
        let w = work(&mon("A", 0, 0, 1280, 800, 1.0, true));
        let far = Spot { x: Pin { from: Anchor::Start, by: 5000 }, y: Pin { from: Anchor::Start, by: -900 } };
        let r = at_spot(far, w, vec2(WIDTH, 300.0));
        assert!(inside(r, w), "{r:?} in {w:?}");
        let pin = Some(Pin { from: Anchor::End, by: -400 });
        let s = shelf_rect(w, Edge::Left, 3, pin);
        assert!(s.top() >= w.top() && s.bottom() <= w.bottom().max(w.top() + s.height()));
    }

    #[test]
    fn defaults_are_the_designs_stack() {
        let w = work(&mon("A", 0, 0, 3840, 2160, 1.5, true));
        let shelf = shelf_rect(w, Edge::Left, 3, None);
        assert!((shelf.center().y - w.center().y).abs() < 0.01);
        assert_eq!(shelf.left(), w.left() + EDGE_GAP);
        let (t, p) = widgets(w, Some(shelf), &Arrangement::default(), Some(today_h()), Some(playing_height()));
        let (t, p) = (t.unwrap(), p.unwrap());
        assert!((w.right() - t.right() - MARGIN_X).abs() < 0.01);
        assert!((t.top() - w.top() - MARGIN_Y).abs() < 0.01);
        assert!(is_under(p, t));
        // Today off: Now playing takes its place.
        let (none, p) = widgets(w, None, &Arrangement::default(), None, Some(playing_height()));
        assert!(none.is_none());
        assert!((p.unwrap().top() - w.top() - MARGIN_Y).abs() < 0.01);
    }

    #[test]
    fn now_playing_under_today_moves_above_when_there_is_no_room() {
        let w = work(&mon("A", 0, 0, 1920, 1080, 1.0, true));
        let a = Arrangement { today: Some(Spot { x: Pin { from: Anchor::End, by: 40 }, y: Pin { from: Anchor::End, by: 16 } }), ..Default::default() };
        let (t, p) = widgets(w, None, &a, Some(today_h()), Some(playing_height()));
        let (t, p) = (t.unwrap(), p.unwrap());
        assert!(p.bottom() <= t.top() && inside(p, w), "{p:?} {t:?}");
    }

    #[test]
    fn a_widget_over_the_shelf_or_the_other_widget_moves_beside_it() {
        let w = work(&mon("A", 0, 0, 3840, 2160, 1.5, true));
        let shelf = shelf_rect(w, Edge::Left, 3, None);
        let on_shelf = spot_for(Rect::from_min_size(shelf.min, vec2(WIDTH, 200.0)), w);
        let a = Arrangement { today: Some(on_shelf), playing: Some(on_shelf), ..Default::default() };
        let (t, p) = widgets(w, Some(shelf), &a, Some(200.0), Some(playing_height()));
        let (t, p) = (t.unwrap(), p.unwrap());
        assert!(!overlaps(t, shelf) && !overlaps(p, shelf) && !overlaps(p, t), "{t:?} {p:?} {shelf:?}");
        assert!(inside(t, w) && inside(p, w));
        // Moved the short way: right of the shelf, not to the other side of the screen.
        assert!(t.left() < shelf.right() + GAP + 1.0);
    }

    #[test]
    fn dragging_snaps_to_margins_middles_and_other_pieces() {
        let w = Rect::from_min_max(pos2(0.0, 40.0), pos2(2560.0, 1372.0));
        let size = vec2(WIDTH, 200.0);
        // 7 pt short of the right margin: snapped onto it.
        let r = Rect::from_min_size(pos2(w.right() - MARGIN_X - WIDTH - 7.0, 500.0), size);
        let s = snap_widget(r, w, &[]);
        assert!((w.right() - s.right() - MARGIN_X).abs() < 0.01);
        assert_eq!(s.top(), 500.0);
        // Near the middle: centred.
        let r = Rect::from_min_size(pos2(w.center().x - WIDTH / 2.0 + 9.0, w.center().y - 100.0 - 5.0), size);
        let s = snap_widget(r, w, &[]);
        assert!((s.center() - w.center()).length() < 0.01);
        // Beside another piece: its gap and its top.
        let other = Rect::from_min_size(pos2(800.0, 300.0), vec2(WIDTH, 150.0));
        let r = Rect::from_min_size(pos2(other.right() + GAP + 6.0, other.top() - 4.0), size);
        let s = snap_widget(r, w, &[other]);
        assert_eq!(s.min, pos2(other.right() + GAP, other.top()));
        // Under it: lined up with its left and a gap below.
        let r = Rect::from_min_size(pos2(other.left() + 5.0, other.bottom() + GAP - 8.0), size);
        assert_eq!(snap_widget(r, w, &[other]).min, pos2(other.left(), other.bottom() + GAP));
        // Far from everything: left where it was dropped.
        let r = Rect::from_min_size(pos2(1300.0, 700.0), size);
        assert_eq!(snap_widget(r, w, &[other]), r);
        // Dragged off the screen: kept inside.
        let r = Rect::from_min_size(pos2(-300.0, 5000.0), size);
        assert!(inside(snap_widget(r, w, &[]), w));
    }

    #[test]
    fn the_shelf_snaps_up_and_down_and_picks_its_edge_by_half() {
        let w = Rect::from_min_max(pos2(0.0, 40.0), pos2(2560.0, 1372.0));
        let h = geometry::shelf_height(3);
        let centred = w.center().y - h / 2.0;
        assert_eq!(snap_shelf(centred + 10.0, h, w, &[]), centred);
        assert_eq!(snap_shelf(w.top() + MARGIN_Y + 3.0, h, w, &[]), w.top() + MARGIN_Y);
        let today = Rect::from_min_size(pos2(1800.0, 300.0), vec2(WIDTH, 250.0));
        assert_eq!(snap_shelf(296.0, h, w, &[today]), 300.0);
        assert_eq!(snap_shelf(-500.0, h, w, &[]), w.top());
        assert_eq!(edge_at(100.0, w), Edge::Left);
        assert_eq!(edge_at(2000.0, w), Edge::Right);
    }

    #[test]
    fn widget_windows_hold_the_widget_at_its_tallest_and_stay_on_the_monitor() {
        for m in [mon("A", -3840, 0, 3840, 2160, 1.5, false), mon("B", 3746, 2160, 3840, 2160, 3.0, false), mon("C", -1920, -1080, 1920, 1080, 1.0, true)] {
            let w = work(&m);
            let tallest = today_height(TodayShape { rows: MAX_ROWS, heading: true, note: 0 });
            for spot in [default_today(), Spot { x: Pin { from: Anchor::Start, by: 0 }, y: Pin { from: Anchor::End, by: 0 } }] {
                let island = at_spot(spot, w, vec2(WIDTH, today_h()));
                let p = widget_window(&m, island, today_max()).expect("a window");
                let r = p.rect;
                assert!(r.left >= m.rect.left && r.right <= m.rect.right && r.top >= m.rect.top && r.bottom <= m.rect.bottom, "{r:?}");
                assert!((r.right - r.left) * 2 <= m.rect.right - m.rect.left);
                let win = Rect::from_min_size(Pos2::ZERO, vec2((r.right - r.left) as f32 / p.scale, (r.bottom - r.top) as f32 / p.scale));
                let grown = Rect::from_min_size(p.origin, vec2(WIDTH, tallest));
                // At the bottom of the screen the widget grows up, not out of its window.
                if spot.y.from == Anchor::Start {
                    assert!(win.expand(1.0).contains_rect(grown), "{grown:?} in {win:?}");
                }
                assert!(win.expand(1.0).contains_rect(Rect::from_min_size(p.origin, island.size())));
                // The island lands where it was put on the monitor.
                let on_monitor = r.left as f32 / p.scale + p.origin.x - m.rect.left as f32 / p.scale;
                assert!((on_monitor - island.left()).abs() < 1.0);
            }
        }
        assert_eq!(widget_window(&mon("D", 0, 0, 1280, 800, 2.0, true), Rect::from_min_size(pos2(0.0, 30.0), vec2(WIDTH, 100.0)), 300.0), None);
    }

    #[test]
    fn keys_and_names_follow_the_monitor() {
        let p = mon(r"\\.\DISPLAY4", 0, 0, 1920, 1080, 1.0, true);
        let o = mon(r"\\.\DISPLAY12", 0, 0, 1920, 1080, 1.0, false);
        assert_eq!(key(&p), PRIMARY);
        assert_eq!(key(&o), r"\\.\DISPLAY12");
        assert_eq!(display_name(&p), "main display");
        assert_eq!(display_name(&o), "display 12");
    }

    #[test]
    fn descriptions_say_which_edge() {
        let w = Rect::from_min_max(pos2(0.0, 0.0), pos2(1000.0, 1000.0));
        let r = Rect::from_min_size(pos2(1000.0 - 40.0 - 300.0, 16.0), vec2(300.0, 100.0));
        assert_eq!(describe(r, w), "40 pt from the right, 16 pt from the top");
        let c = Rect::from_center_size(w.center(), vec2(300.0, 100.0));
        assert_eq!(describe(c, w), "centred, centred");
    }
}
