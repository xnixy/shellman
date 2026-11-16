//! Where the shelf's pieces go, in points of the shelf window. Pure, so it is tested without a
//! window. The sizes are the design's (`docs/design/desktop-concepts.html`, "Shelf + Widgets").
//!
//! The shelf is a slim island on one edge of the primary monitor: the drop zone, then a stack
//! per folder. A stack fans open along an arc whose centre lies off the screen beyond the shelf,
//! so its items lie on a gentle curve bulging away from the edge, and the last pill opens the
//! folder in Files.

use egui::{pos2, vec2, Pos2, Rect};
use shellman_core::settings::desktop::Edge;

/// From the screen's edge to the shelf.
pub const EDGE_GAP: f32 = 20.0;
pub const SHELF_W: f32 = 104.0;
pub const PAD: f32 = 10.0;
pub const RADIUS: f32 = 30.0;
pub const DROP_H: f32 = 84.0;
pub const DROP_RADIUS: f32 = 20.0;
pub const STACK_H: f32 = 118.0;
pub const STACK_RADIUS: f32 = 18.0;
/// The box the three cards of a stack sit in.
pub const CARDS: f32 = 52.0;

/// The arc's centre lies this far beyond the shelf's outer side.
pub const ARC_OFFSET: f32 = 170.0;
pub const ARC_RADIUS: f32 = 440.0;
/// Degrees between one pill and the next.
pub const ARC_STEP: f32 = 10.5;
pub const PILL_W: f32 = 330.0;
pub const ITEM_H: f32 = 62.0;
pub const LAST_H: f32 = 50.0;
/// How many of a folder's newest items its fan shows.
pub const FAN_ITEMS: usize = 6;
/// Room around the islands for their shadows.
pub const SHADOW: f32 = 40.0;
/// Kept free between the fan and the top bar or the dock.
const AREA_GAP: f32 = 8.0;

/// The shelf's height with `stacks` stacks.
pub fn shelf_height(stacks: usize) -> f32 {
    2.0 * PAD + DROP_H + stacks as f32 * STACK_H
}

/// From the shelf's outer side to the far edge of the widest-reaching pill.
fn reach() -> f32 {
    -ARC_OFFSET + ARC_RADIUS + PILL_W
}

/// The shelf window's width: the shelf, the fan beside it, and the shadows.
pub fn window_width() -> f32 {
    EDGE_GAP + reach() + SHADOW
}

/// The shelf and its parts.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub shelf: Rect,
    pub drop: Rect,
    pub stacks: Vec<Rect>,
}

/// The shelf in a window `window` (points, its whole client area), on `edge`, vertically
/// centred in `area` (as it is until Edit desktop… moves it) (the space between the top bar and the dock, in the window's points).
#[cfg(test)]
pub fn layout(window: Rect, area: Rect, edge: Edge, stacks: usize) -> Layout {
    let h = shelf_height(stacks);
    let y = (area.center().y - h / 2.0).clamp(area.top(), (area.bottom() - h).max(area.top()));
    layout_at(window, y, edge, stacks)
}

/// The shelf in a window `window` on `edge`, its top at `y` (window points): where Edit
/// desktop… put it (`arrange::place::shelf_top`).
pub fn layout_at(window: Rect, y: f32, edge: Edge, stacks: usize) -> Layout {
    let h = shelf_height(stacks);
    let x = match edge {
        Edge::Left => window.left() + EDGE_GAP,
        Edge::Right => window.right() - EDGE_GAP - SHELF_W,
    };
    let shelf = Rect::from_min_size(pos2(x, y), vec2(SHELF_W, h));
    let inner = SHELF_W - 2.0 * PAD;
    let drop = Rect::from_min_size(pos2(x + PAD, y + PAD), vec2(inner, DROP_H));
    let stacks = (0..stacks)
        .map(|i| Rect::from_min_size(pos2(x + PAD, drop.bottom() + i as f32 * STACK_H), vec2(inner, STACK_H)))
        .collect();
    Layout { shelf, drop, stacks }
}

/// The pills of a fan of `items` items plus the "Open in Files" pill (last), for a stack whose
/// centre is at `anchor_y`, fully open. The arc is moved up or down to stay within `area`.
#[cfg(test)]
pub fn fan(shelf: Rect, anchor_y: f32, items: usize, edge: Edge, area: Rect) -> Vec<Rect> {
    fan_at(shelf, anchor_y, items, edge, area, 1.0)
}

/// The fan part of the way open: at `t` 0 the pills are bunched at the arc's middle on a
/// smaller arc, at 1 they are in place. `t` is already eased.
pub fn fan_at(shelf: Rect, anchor_y: f32, items: usize, edge: Edge, area: Rect, t: f32) -> Vec<Rect> {
    let slots = items + 1;
    let mid = (slots - 1) as f32 / 2.0;
    let height = |i: usize| if i + 1 == slots { LAST_H } else { ITEM_H };
    let angle = |i: usize| ((i as f32 - mid) * ARC_STEP).to_radians();
    // Where the arc's centre sits vertically: on the stack, unless that would push a pill past
    // the area; worked out for the open fan, so the fan does not drift as it opens.
    let above = (0..slots).map(|i| ARC_RADIUS * angle(i).sin() - height(i) / 2.0).fold(f32::MAX, f32::min);
    let below = (0..slots).map(|i| ARC_RADIUS * angle(i).sin() + height(i) / 2.0).fold(f32::MIN, f32::max);
    let (lo, hi) = (area.top() + AREA_GAP - above, area.bottom() - AREA_GAP - below);
    let cy = if lo <= hi { anchor_y.clamp(lo, hi) } else { lo };
    let t = t.clamp(0.0, 1.0);
    let radius = ARC_RADIUS * (0.86 + 0.14 * t);
    (0..slots)
        .map(|i| {
            let a = angle(i) * t;
            let (h, y) = (height(i), cy + radius * a.sin());
            match edge {
                Edge::Left => {
                    let x = shelf.left() - ARC_OFFSET + radius * a.cos();
                    Rect::from_min_size(pos2(x, y - h / 2.0), vec2(PILL_W, h))
                }
                Edge::Right => {
                    let x = shelf.right() + ARC_OFFSET - radius * a.cos();
                    Rect::from_min_max(pos2(x - PILL_W, y - h / 2.0), pos2(x, y + h / 2.0))
                }
            }
        })
        .collect()
}

/// Room around the fan's path: between its ends the pills swing a little past both.
const SWEPT_PAD: f32 = 8.0;

/// The part of the window a fan moves in while it opens or shuts: what holds its pills when
/// shut (`from`) and when open (`to`), with a little room, in the window's pixels (left, top,
/// right, bottom). None without pills.
pub fn swept(from: &[Rect], to: &[Rect], scale: f32) -> Option<(i32, i32, i32, i32)> {
    let all = from.iter().chain(to).fold(Rect::NOTHING, |a, r| a.union(*r));
    if !all.is_positive() {
        return None;
    }
    let all = all.expand(SWEPT_PAD);
    Some((
        (all.left() * scale).floor() as i32,
        (all.top() * scale).floor() as i32,
        (all.right() * scale).ceil() as i32,
        (all.bottom() * scale).ceil() as i32,
    ))
}

/// The point of `p` (points in the window) in physical screen pixels, for a window whose client
/// area starts at `origin` (pixels) with `scale` pixels per point.
pub fn to_screen(p: Pos2, origin: (i32, i32), scale: f32) -> (i32, i32) {
    (origin.0 + (p.x * scale).round() as i32, origin.1 + (p.y * scale).round() as i32)
}

/// A rectangle in the window's points as screen pixels (left, top, right, bottom).
pub fn rect_to_screen(r: Rect, origin: (i32, i32), scale: f32) -> (i32, i32, i32, i32) {
    let (l, t) = to_screen(r.min, origin, scale);
    let (rr, b) = to_screen(r.max, origin, scale);
    (l, t, rr, b)
}

/// A point on the screen (pixels) in the window's points.
#[cfg(test)]
pub fn from_screen(p: (i32, i32), origin: (i32, i32), scale: f32) -> Pos2 {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    pos2((p.0 - origin.0) as f32 / scale, (p.1 - origin.1) as f32 / scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fans_path_holds_every_pill_on_the_way() {
        let shelf = Rect::from_min_size(pos2(8.0, 300.0), vec2(64.0, 200.0));
        let area = Rect::from_min_max(pos2(0.0, 64.0), pos2(800.0, 974.0));
        for edge in [Edge::Left, Edge::Right] {
            let at = |t: f32| fan_at(shelf, 400.0, 5, edge, area, t);
            let (l, t, r, b) = swept(&at(0.0), &at(1.0), 2.0).unwrap();
            let held = Rect::from_min_max(pos2(l as f32 / 2.0, t as f32 / 2.0), pos2(r as f32 / 2.0, b as f32 / 2.0));
            for step in 0..=20 {
                for pill in at(step as f32 / 20.0) {
                    assert!(held.contains_rect(pill), "{edge:?} at {step}: {pill:?} outside {held:?}");
                }
            }
        }
        assert_eq!(swept(&[], &[], 1.0), None);
    }

    fn window() -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(window_width(), 1080.0))
    }

    fn area() -> Rect {
        Rect::from_min_max(pos2(0.0, 64.0), pos2(window_width(), 974.0))
    }

    #[test]
    fn the_shelf_is_the_designs_size_and_centred_between_the_bars() {
        let l = layout(window(), area(), Edge::Left, 3);
        assert_eq!(l.shelf.left(), 20.0);
        assert_eq!(l.shelf.width(), 104.0);
        assert_eq!(l.shelf.height(), 2.0 * 10.0 + 84.0 + 3.0 * 118.0);
        assert!((l.shelf.center().y - area().center().y).abs() < 0.01);
        assert_eq!(l.drop, Rect::from_min_size(pos2(30.0, l.shelf.top() + 10.0), vec2(84.0, 84.0)));
        assert_eq!(l.stacks.len(), 3);
        assert_eq!(l.stacks[0].top(), l.drop.bottom());
        assert_eq!(l.stacks[2].bottom(), l.shelf.bottom() - 10.0);
    }

    #[test]
    fn on_the_right_edge_it_mirrors() {
        let l = layout(window(), area(), Edge::Right, 2);
        assert_eq!(l.shelf.right(), window_width() - 20.0);
        assert_eq!(l.stacks.len(), 2);
    }

    #[test]
    fn a_short_screen_keeps_the_shelf_at_the_top_of_the_area() {
        let small = Rect::from_min_max(pos2(0.0, 40.0), pos2(600.0, 300.0));
        let l = layout(window(), small, Edge::Left, 3);
        assert_eq!(l.shelf.top(), 40.0);
    }

    #[test]
    fn the_fan_follows_the_designs_arc() {
        let l = layout(window(), area(), Edge::Left, 3);
        let anchor = l.stacks[1].center().y;
        let pills = fan(l.shelf, anchor, FAN_ITEMS, Edge::Left, area());
        assert_eq!(pills.len(), 7);
        // The middle pill is the furthest out: the arc's radius beyond its centre.
        let mid = pills[3];
        assert!((mid.left() - (l.shelf.left() - ARC_OFFSET + ARC_RADIUS)).abs() < 0.01);
        assert!((mid.center().y - anchor).abs() < 0.01);
        // Symmetric, and every pill sits right of the shelf, never over it.
        assert!((pills[0].left() - pills[6].left()).abs() < 0.01);
        assert!(pills.iter().all(|p| p.left() > l.shelf.right()));
        assert!(pills.iter().all(|p| p.right() <= window_width() - SHADOW + 0.01));
        // Items are taller than the last pill, and they do not overlap.
        assert_eq!(pills[0].height(), ITEM_H);
        assert_eq!(pills[6].height(), LAST_H);
        for w in pills.windows(2) {
            assert!(w[0].bottom() <= w[1].top() + 0.01, "{w:?}");
        }
    }

    #[test]
    fn a_fan_near_the_bottom_moves_up_into_the_area() {
        let l = layout(window(), area(), Edge::Left, 3);
        let pills = fan(l.shelf, area().bottom(), FAN_ITEMS, Edge::Left, area());
        assert!(pills.last().unwrap().bottom() <= area().bottom());
        let pills = fan(l.shelf, area().top(), FAN_ITEMS, Edge::Left, area());
        assert!(pills[0].top() >= area().top());
    }

    #[test]
    fn fewer_items_stay_centred_and_the_right_edge_mirrors() {
        let l = layout(window(), area(), Edge::Right, 3);
        let anchor = l.stacks[0].center().y;
        let pills = fan(l.shelf, anchor, 2, Edge::Right, area());
        assert_eq!(pills.len(), 3);
        assert!((pills[1].right() - (l.shelf.right() + ARC_OFFSET - ARC_RADIUS)).abs() < 0.01);
        assert!(pills.iter().all(|p| p.right() < l.shelf.left()));
    }

    #[test]
    fn opening_starts_bunched_and_ends_in_place() {
        let l = layout(window(), area(), Edge::Left, 3);
        let anchor = l.stacks[1].center().y;
        let start = fan_at(l.shelf, anchor, 6, Edge::Left, area(), 0.0);
        let end = fan_at(l.shelf, anchor, 6, Edge::Left, area(), 1.0);
        assert_eq!(end, fan(l.shelf, anchor, 6, Edge::Left, area()));
        // Bunched: all at the middle's height, and closer to the shelf.
        assert!(start.iter().all(|p| (p.center().y - anchor).abs() < 0.01));
        assert!(start[3].left() < end[3].left());
    }

    #[test]
    fn screen_points_round_trip() {
        let origin = (-3840, 100);
        let p = pos2(120.5, 40.0);
        let px = to_screen(p, origin, 1.5);
        assert_eq!(px, (-3840 + 181, 160));
        let back = from_screen(px, origin, 1.5);
        assert!((back.x - p.x).abs() < 1.0 && (back.y - p.y).abs() < 1.0);
    }
}
