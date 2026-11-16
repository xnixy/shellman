//! Which monitors get a shelf, where each shelf window goes on its monitor, and how the windows
//! follow the monitors as they come, go and change. Pure, so it is tested without windows.
//!
//! Every shelf window is only as large as its island plus the room an open fan (and the glass's
//! shadow) needs: as wide as the fan reaches, as tall as the shelf and the furthest a fan can
//! open above and below it. Never the monitor's size. The size is worked out once per monitor,
//! scale, edge and number of stacks, not per fan: opening a stack only shows islands inside it.
//! A monitor that reports nonsense (a 0 or tiny rectangle, a work area outside it, a scale that
//! is not a number), as happens for a moment while displays change, gets no shelf until it
//! reports something sane; no window is made or sized for it.

use crate::geometry::{self, FAN_ITEMS, SHADOW};
use egui::{pos2, Rect};
use shellman_core::monitors::Monitor;
use shellman_core::settings::desktop::{Edge, Pin, ShelfOn};
use shellman_ui::glass::guard;
use windows::Win32::Foundation::RECT;

/// A monitor smaller than this on either side (pixels), or a work area this small, is taken as
/// a monitor in the middle of a change, not a real one.
const MIN_SIDE: i32 = 200;
/// Larger than any real monitor (pixels): anything beyond is nonsense.
const MAX_SIDE: i32 = 32768;
/// The scales a monitor can really have (pixels per point).
const SCALES: (f32, f32) = (0.5, 8.0);

fn width(r: &RECT) -> i32 {
    r.right.saturating_sub(r.left)
}

fn height(r: &RECT) -> i32 {
    r.bottom.saturating_sub(r.top)
}

/// Whether a monitor is fit to carry a shelf: a real size, a work area inside it, a real scale.
pub fn usable(m: &Monitor) -> bool {
    let sized = |r: &RECT| (MIN_SIDE..=MAX_SIDE).contains(&width(r)) && (MIN_SIDE..=MAX_SIDE).contains(&height(r));
    let w = &m.work;
    let inside = w.left >= m.rect.left && w.top >= m.rect.top && w.right <= m.rect.right && w.bottom <= m.rect.bottom;
    sized(&m.rect) && sized(w) && inside && m.scale.is_finite() && (SCALES.0..=SCALES.1).contains(&m.scale)
}

/// The monitors that get a shelf for the setting: every usable monitor, or the primary alone,
/// the primary first. A list without a primary (in the middle of a change) gives the primary's
/// shelf to no one.
pub fn targets(on: ShelfOn, monitors: &[Monitor]) -> Vec<Monitor> {
    let mut list: Vec<Monitor> = monitors
        .iter()
        .filter(|m| usable(m) && (on == ShelfOn::All || m.primary))
        .cloned()
        .collect();
    shellman_core::monitors::sort(&mut list);
    list
}

/// The process's first window carries the primary monitor's shelf; the other monitors' shelves
/// are windows of their own, by device name. Split the targets so.
pub fn split(targets: Vec<Monitor>) -> (Option<Monitor>, Vec<Monitor>) {
    let (primary, others): (Vec<Monitor>, Vec<Monitor>) = targets.into_iter().partition(|m| m.primary);
    (primary.into_iter().next(), others)
}

/// What to do with the other monitors' shelf windows: which to close (their monitor went away,
/// became the primary, or the setting no longer gives it a shelf) and which to open. A monitor
/// that is still there keeps its window, whatever moved or rescaled; it is only placed again.
#[derive(Debug, PartialEq, Eq, Default)]
pub struct Plan {
    pub close: Vec<String>,
    pub open: Vec<String>,
}

pub fn reconcile(have: &[String], want: &[String]) -> Plan {
    let mut plan = Plan::default();
    for d in have {
        if !want.contains(d) && !plan.close.contains(d) {
            plan.close.push(d.clone());
        }
    }
    for d in want {
        if !have.contains(d) && !plan.open.contains(d) {
            plan.open.push(d.clone());
        }
    }
    plan
}

/// Where one shelf window goes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Placement {
    /// The window, in physical screen pixels.
    pub rect: RECT,
    /// The space between the top bar and the dock, in the window's points (it may reach past the
    /// window: the window is cut to what the shelf and its fans need).
    pub area: Rect,
    /// The monitor's pixels per point.
    pub scale: f32,
    /// The shelf's top in the window's points.
    pub shelf_top: f32,
}

/// Everything the shelf and any fan of it may cover, in points from the monitor's top-left:
/// the shelf in a window as tall as the monitor, and every fan of every stack at any point of
/// opening, with any number of items.
fn reach(window: Rect, area: Rect, edge: Edge, stacks: usize, top: f32) -> Rect {
    let layout = geometry::layout_at(window, top, edge, stacks);
    let mut all = layout.shelf;
    for stack in &layout.stacks {
        for items in 0..=FAN_ITEMS {
            for t in [0.0, 0.5, 1.0] {
                for pill in geometry::fan_at(layout.shelf, stack.center().y, items, edge, area, t) {
                    all = all.union(pill);
                }
            }
        }
    }
    all
}

/// The shelf window for monitor `m` on `edge` with `stacks` stacks, the shelf at `pin` (None:
/// centred between the bars), or None when the monitor is not fit for one or the window would
/// not be a sane size (it never is the monitor's size, and every side passes the glass's guard
/// unchanged).
pub fn place(m: &Monitor, edge: Edge, stacks: usize, pin: Option<Pin>) -> Option<Placement> {
    if !usable(m) {
        return None;
    }
    let scale = m.scale;
    let pt = |px: i32| px as f32 / scale;
    // Points from the monitor's top-left.
    let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(geometry::window_width(), pt(height(&m.rect))));
    let area = Rect::from_min_max(
        pos2(0.0, pt(m.work.top - m.rect.top)),
        pos2(geometry::window_width(), pt(m.work.bottom - m.rect.top)),
    );
    let shelf_top = crate::arrange::place::shelf_top(area, geometry::shelf_height(stacks), pin);
    let need = reach(full, area, edge, stacks, shelf_top);
    let top_pt = (need.top() - SHADOW).max(0.0);
    let bottom_pt = (need.bottom() + SHADOW).min(full.bottom());
    let top = m.rect.top + (top_pt * scale).floor() as i32;
    let bottom = (m.rect.top + (bottom_pt * scale).ceil() as i32).min(m.rect.bottom);
    let w = (geometry::window_width() * scale).round() as i32;
    let h = bottom - top;
    // The glass guard must take the size as it is (no clamping), and the window must leave
    // most of the monitor free.
    if guard::window((w, h)) != Some((w, h)) || w >= width(&m.work) || w < 24 || h < 24 {
        return None;
    }
    let (left, right) = match edge {
        Edge::Left => (m.work.left, m.work.left + w),
        Edge::Right => (m.work.right - w, m.work.right),
    };
    let rect = RECT { left, top, right, bottom };
    // The area in the window's points: from the window's top, not the monitor's.
    let shift = pt(top - m.rect.top);
    let area = area.translate(egui::vec2(0.0, -shift));
    Some(Placement { rect, area, scale, shelf_top: shelf_top - shift })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, w: i32, h: i32) -> RECT {
        RECT { left, top, right: left + w, bottom: top + h }
    }

    /// A monitor with a top bar and dock reserved (in points) at `scale`.
    fn mon(device: &str, left: i32, top: i32, scale: f32, primary: bool) -> Monitor {
        let r = rect(left, top, 3840, 2160);
        let work = RECT { top: r.top + (60.0 * scale) as i32, bottom: r.bottom - (102.0 * scale) as i32, ..r };
        Monitor { device: device.into(), rect: r, work, scale, primary }
    }

    /// The user's set-up: the primary at 150% at the origin, one at 150% to its left, one at
    /// 300% to the right and lower.
    fn three() -> Vec<Monitor> {
        vec![
            mon(r"\\.\DISPLAY3", 3746, 2160, 3.0, false),
            mon(r"\\.\DISPLAY1", 0, 0, 1.5, true),
            mon(r"\\.\DISPLAY2", -3840, 0, 1.5, false),
        ]
    }

    fn devices(list: &[Monitor]) -> Vec<&str> {
        list.iter().map(|m| m.device.as_str()).collect()
    }

    #[test]
    fn all_displays_gives_every_monitor_a_shelf_primary_first() {
        let t = targets(ShelfOn::All, &three());
        assert_eq!(devices(&t), [r"\\.\DISPLAY1", r"\\.\DISPLAY2", r"\\.\DISPLAY3"]);
        let (primary, others) = split(t);
        assert_eq!(primary.unwrap().device, r"\\.\DISPLAY1");
        assert_eq!(devices(&others), [r"\\.\DISPLAY2", r"\\.\DISPLAY3"]);
    }

    #[test]
    fn primary_display_gives_only_the_primary_a_shelf() {
        let t = targets(ShelfOn::Primary, &three());
        assert_eq!(devices(&t), [r"\\.\DISPLAY1"]);
        let (primary, others) = split(t);
        assert!(primary.is_some() && others.is_empty());
    }

    #[test]
    fn monitors_in_the_middle_of_a_change_are_skipped() {
        let mut list = three();
        // A zero rectangle, a tiny one, a work area outside the monitor, a scale that is not one.
        list[0].rect = rect(3746, 2160, 0, 0);
        list[2].work = rect(-9000, 0, 3840, 2160);
        assert_eq!(devices(&targets(ShelfOn::All, &list)), [r"\\.\DISPLAY1"]);
        list[1].scale = f32::NAN;
        assert!(targets(ShelfOn::All, &list).is_empty());
        let mut tiny = three();
        tiny[1].rect = rect(0, 0, 50, 2160);
        tiny[1].work = tiny[1].rect;
        assert_eq!(devices(&targets(ShelfOn::All, &tiny)), [r"\\.\DISPLAY2", r"\\.\DISPLAY3"]);
        assert_eq!(place(&tiny[1], Edge::Left, 3, None), None);
        // No primary in the list: the primary's shelf goes to no one.
        let (primary, others) = split(targets(ShelfOn::All, &tiny));
        assert!(primary.is_none());
        assert_eq!(others.len(), 2);
    }

    #[test]
    fn windows_follow_the_monitors_by_name() {
        let have = vec![r"\\.\DISPLAY2".to_string(), r"\\.\DISPLAY3".to_string()];
        // Nothing changed (or a monitor only moved or rescaled): nothing opens or closes.
        assert_eq!(reconcile(&have, &have), Plan::default());
        // One unplugged, one plugged in.
        let want = vec![r"\\.\DISPLAY2".to_string(), r"\\.\DISPLAY4".to_string()];
        let plan = reconcile(&have, &want);
        assert_eq!(plan.close, [r"\\.\DISPLAY3"]);
        assert_eq!(plan.open, [r"\\.\DISPLAY4"]);
        // Primary only: every other window closes.
        assert_eq!(reconcile(&have, &[]).close, have);
        // Twice in a list never opens two windows.
        let twice = vec![r"\\.\DISPLAY5".to_string(), r"\\.\DISPLAY5".to_string()];
        assert_eq!(reconcile(&[], &twice).open, [r"\\.\DISPLAY5"]);
    }

    /// The window is on its monitor, on the edge, as wide as the shelf and its fan, far from
    /// the monitor's size, and the shelf and every fan fit inside it.
    fn check(m: &Monitor, edge: Edge) -> Placement {
        let p = place(m, edge, 3, None).expect("a shelf");
        let r = p.rect;
        assert!(r.left >= m.rect.left && r.right <= m.rect.right && r.top >= m.rect.top && r.bottom <= m.rect.bottom, "{r:?}");
        match edge {
            Edge::Left => assert_eq!(r.left, m.work.left),
            Edge::Right => assert_eq!(r.right, m.work.right),
        }
        assert_eq!(width(&r), (geometry::window_width() * m.scale).round() as i32);
        assert!(width(&r) < width(&m.rect), "{r:?}");
        assert!(height(&r) <= height(&m.rect));
        assert_eq!(p.scale, m.scale);
        // Lay out as the frame does, in the window's points.
        let window = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(geometry::window_width(), height(&r) as f32 / m.scale));
        let layout = geometry::layout_at(window, p.shelf_top, edge, 3);
        assert!((layout.shelf.top() - geometry::layout(window, p.area, edge, 3).shelf.top()).abs() < 0.01);
        assert!(window.contains_rect(layout.shelf), "{:?} in {window:?}", layout.shelf);
        for s in &layout.stacks {
            for pill in geometry::fan(layout.shelf, s.center().y, FAN_ITEMS, edge, p.area) {
                assert!(window.expand(0.01).contains_rect(pill), "{pill:?} in {window:?}");
            }
        }
        // The shelf is centred between the bars, as on a window as tall as the monitor.
        let top_bar = (m.work.top - m.rect.top) as f32 / m.scale;
        let dock = (m.work.bottom - m.rect.top) as f32 / m.scale;
        let shelf_on_monitor = layout.shelf.center().y + (r.top - m.rect.top) as f32 / m.scale;
        assert!((shelf_on_monitor - (top_bar + dock) / 2.0).abs() < 1.0);
        p
    }

    #[test]
    fn a_shelf_at_150_percent_left_of_the_primary() {
        let m = mon(r"\\.\DISPLAY2", -3840, 0, 1.5, false);
        let p = check(&m, Edge::Left);
        assert_eq!(p.rect.left, -3840);
        // Much less than the monitor's height at 150%.
        assert!(p.rect.bottom - p.rect.top < 2160 * 3 / 4, "{:?}", p.rect);
        check(&m, Edge::Right);
    }

    #[test]
    fn a_shelf_at_300_percent_lower_right() {
        let m = mon(r"\\.\DISPLAY3", 3746, 2160, 3.0, false);
        let p = check(&m, Edge::Left);
        assert_eq!(p.rect.left, 3746);
        assert!(p.rect.top >= 2160);
        check(&m, Edge::Right);
    }

    #[test]
    fn a_shelf_at_100_percent_and_on_a_monitor_above() {
        let m = Monitor { device: "x".into(), rect: rect(-1920, -1080, 1920, 1080), work: rect(-1920, -1020, 1920, 960), scale: 1.0, primary: false };
        let p = check(&m, Edge::Left);
        assert!(p.rect.top >= -1080 && p.rect.bottom <= 0);
        check(&m, Edge::Right);
    }

    #[test]
    fn a_shelf_moved_up_or_down_keeps_every_fan_in_its_window() {
        use shellman_core::settings::desktop::Anchor;
        let m = mon("P", 0, 0, 1.5, true);
        for pin in [Pin { from: Anchor::Start, by: 0 }, Pin { from: Anchor::End, by: 0 }, Pin { from: Anchor::Start, by: 5000 }, Pin { from: Anchor::Middle, by: -200 }] {
            for edge in [Edge::Left, Edge::Right] {
                let p = place(&m, edge, 3, Some(pin)).expect("a shelf");
                let r = p.rect;
                assert!(r.top >= m.rect.top && r.bottom <= m.rect.bottom);
                let window = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(geometry::window_width(), height(&r) as f32 / m.scale));
                let layout = geometry::layout_at(window, p.shelf_top, edge, 3);
                assert!(window.contains_rect(layout.shelf), "{pin:?}: {:?} in {window:?}", layout.shelf);
                // Inside the work area.
                assert!(layout.shelf.top() >= p.area.top() - 0.01 && layout.shelf.bottom() <= p.area.bottom() + 0.01, "{pin:?}");
                for s in &layout.stacks {
                    for pill in geometry::fan(layout.shelf, s.center().y, FAN_ITEMS, edge, p.area) {
                        assert!(window.expand(0.01).contains_rect(pill), "{pin:?}: {pill:?} in {window:?}");
                    }
                }
            }
        }
        // At the top of the work area: the window starts higher than when centred.
        let top = place(&m, Edge::Left, 3, Some(Pin { from: Anchor::Start, by: 0 })).unwrap();
        let mid = place(&m, Edge::Left, 3, None).unwrap();
        assert!(top.rect.top < mid.rect.top);
    }

    #[test]
    fn no_shelf_where_it_would_fill_the_monitor() {
        // 1280x800 at 200%: 640 points wide, less than the shelf and its fan need.
        let m = Monitor { device: "x".into(), rect: rect(0, 0, 1280, 800), work: rect(0, 0, 1280, 800), scale: 2.0, primary: true };
        assert_eq!(place(&m, Edge::Left, 3, None), None);
    }
}
