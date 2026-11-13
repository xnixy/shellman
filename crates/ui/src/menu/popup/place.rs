//! Where a popup menu goes, in physical screen pixels: beside the point it was opened at, kept
//! in the monitor's work area and flipped at its edges, and its submenu beside it. Also the
//! guards on every size the window and its glass are given. Pure arithmetic, tested on its own.
//!
//! Sizes matter beyond looks: DWM fails fast (`dwmcore`, E_INVALIDARG from `CreateTexture2D`)
//! when a composition surface gets a size of 0, a negative one or one above 16384, and glass
//! asks for a texture of its window's size plus its shadow's room. So nothing is sized before
//! it passes [`guard`].

/// A rectangle in physical pixels: left, top, right, bottom (right and bottom outside).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Px {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Px {
    pub fn new(left: i32, top: i32, w: i32, h: i32) -> Self {
        Self { left, top, right: left + w, bottom: top + h }
    }

    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    pub fn union(&self, o: &Px) -> Px {
        Px {
            left: self.left.min(o.left),
            top: self.top.min(o.top),
            right: self.right.max(o.right),
            bottom: self.bottom.max(o.bottom),
        }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

/// The smallest side a menu piece or window is ever given, in pixels.
pub const MIN_PX: i32 = 24;
/// The largest, whatever the monitor: well under composition's 16384 even with the shadow's room.
pub const MAX_PX: i32 = 8192;

/// A size in points at `scale` pixels per point, as whole pixels, if it is fit to give a window:
/// finite, at least [`MIN_PX`] on each side, and no larger than `limit` (the work area) nor
/// [`MAX_PX`]. A size too large is cut to fit; one that is not a number, or too small, is
/// refused.
pub fn guard(w_pt: f32, h_pt: f32, scale: f32, limit: (i32, i32)) -> Option<(i32, i32)> {
    if !(scale.is_finite() && scale > 0.0 && w_pt.is_finite() && h_pt.is_finite()) {
        return None;
    }
    let (w, h) = ((w_pt * scale).ceil(), (h_pt * scale).ceil());
    if w < MIN_PX as f32 || h < MIN_PX as f32 {
        return None;
    }
    let cap = |v: f32, lim: i32| v.min(lim.clamp(MIN_PX, MAX_PX) as f32) as i32;
    Some((cap(w, limit.0), cap(h, limit.1)))
}

/// Whether a rectangle is fit to give a window or a piece of glass (see [`guard`]).
pub fn sane(r: &Px, work: &Px) -> bool {
    let (w, h) = (r.width(), r.height());
    w >= MIN_PX && h >= MIN_PX && w <= work.width().clamp(MIN_PX, MAX_PX) && h <= work.height().clamp(MIN_PX, MAX_PX)
}

/// Keep `start..start+len` inside `lo..hi` (at `lo` when it does not fit at all).
fn clamp_span(start: i32, len: i32, lo: i32, hi: i32) -> i32 {
    start.min(hi - len).max(lo)
}

/// The menu itself, `size` pixels, opened at the point `at` in the work area `work`.
///
/// Its top-left corner goes at the point (its bottom-left with `up`, for a menu opened from a
/// dock at the screen's bottom). Where it would cross the work area's right edge it opens to
/// the left of the point instead, and where it would cross the bottom (or, upwards, the top) it
/// flips to the other side of the point; what still does not fit is pushed inside.
pub fn place_menu(at: (i32, i32), size: (i32, i32), up: bool, work: Px) -> Px {
    let (w, h) = size;
    let mut x = at.0;
    if x + w > work.right {
        x = at.0 - w;
    }
    let mut y = if up { at.1 - h } else { at.1 };
    if !up && y + h > work.bottom {
        y = at.1 - h;
    } else if up && y < work.top {
        y = at.1;
    }
    Px::new(clamp_span(x, w, work.left, work.right), clamp_span(y, h, work.top, work.bottom), w, h)
}

/// The submenu, `size` pixels, beside `menu`: right of it, `gap` pixels apart, with its first row
/// level with the row that opened it (`row_top`, minus the submenu's own padding `pad`); left of
/// it when there is no room on the right; pushed up when it would cross the bottom.
pub fn place_submenu(menu: Px, row_top: i32, pad: i32, size: (i32, i32), gap: i32, work: Px) -> Px {
    let (w, h) = size;
    let right = menu.right + gap;
    let x = if right + w <= work.right || menu.left - gap - w < work.left {
        right
    } else {
        menu.left - gap - w
    };
    let y = row_top - pad;
    Px::new(clamp_span(x, w, work.left, work.right), clamp_span(y, h, work.top, work.bottom), w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three monitors here: left (x -3840, 300%), primary (0, 150%), right (x 3840, 150%),
    /// each 3840 x 2160, with a top bar and a dock taken out of the work area.
    fn work(left: i32) -> Px {
        Px { left, top: 60, right: left + 3840, bottom: 2160 - 140 }
    }

    #[test]
    fn opens_down_and_right_of_the_point() {
        let r = place_menu((500, 400), (450, 600), false, work(0));
        assert_eq!(r, Px::new(500, 400, 450, 600));
    }

    #[test]
    fn flips_left_and_up_at_the_right_and_bottom_edges() {
        let w = work(0);
        let r = place_menu((3700, 1900), (450, 600), false, w);
        assert_eq!((r.left, r.top), (3700 - 450, 1900 - 600));
        // On the monitor left of the primary, all coordinates negative.
        let w = work(-3840);
        let r = place_menu((-150, 1950), (900, 1200), false, w);
        assert_eq!((r.right, r.bottom), (-150, 1950));
        assert!(r.left >= w.left);
    }

    #[test]
    fn upwards_from_a_dock_and_down_when_no_room_above() {
        let w = work(3840);
        let r = place_menu((5000, 2030), (450, 300), true, w);
        // The point is below the work area (on the dock): the menu ends above the dock.
        assert_eq!(r.bottom, w.bottom);
        assert_eq!(r.left, 5000);
        let r = place_menu((5000, 100), (450, 300), true, w);
        assert_eq!(r.top, 100);
    }

    #[test]
    fn a_menu_taller_than_the_work_area_starts_at_its_top() {
        let w = work(0);
        let r = place_menu((10, 1000), (400, 5000), false, w);
        assert_eq!(r.top, w.top);
        assert_eq!(r.left, 10);
    }

    #[test]
    fn submenu_goes_right_else_left_and_stays_on_the_monitor() {
        let w = work(0);
        let menu = Px::new(500, 400, 450, 600);
        let s = place_submenu(menu, 420, 12, (360, 300), 9, w);
        assert_eq!((s.left, s.top), (950 + 9, 408));
        // Near the right edge: on the left.
        let menu = Px::new(3300, 400, 450, 600);
        let s = place_submenu(menu, 420, 12, (360, 300), 9, w);
        assert_eq!(s.right, 3300 - 9);
        // Near the bottom: pushed up.
        let menu = Px::new(500, 1500, 450, 500);
        let s = place_submenu(menu, 1950, 12, (360, 300), 9, w);
        assert_eq!(s.bottom, w.bottom);
        // On the leftmost monitor at 300 %: no room on either side keeps it right, inside.
        let w = work(-3840);
        let menu = Px::new(-3840, 100, 3500, 600);
        let s = place_submenu(menu, 200, 36, (1080, 900), 27, w);
        assert!(s.left >= w.left && s.right <= w.right);
    }

    #[test]
    fn guard_refuses_nonsense_and_caps_at_the_work_area() {
        let lim = (3840, 2020);
        assert_eq!(guard(300.0, 200.0, 1.5, lim), Some((450, 300)));
        assert_eq!(guard(300.0, 200.0, 3.0, lim), Some((900, 600)));
        assert_eq!(guard(f32::NAN, 200.0, 1.5, lim), None);
        assert_eq!(guard(300.0, f32::INFINITY, 1.5, lim), None);
        assert_eq!(guard(300.0, 200.0, 0.0, lim), None);
        assert_eq!(guard(-300.0, 200.0, 1.5, lim), None);
        assert_eq!(guard(10.0, 10.0, 1.0, lim), None);
        assert_eq!(guard(0.0, 0.0, 1.5, lim), None);
        // Larger than the monitor: cut to the work area, never more.
        assert_eq!(guard(5000.0, 3000.0, 1.5, lim), Some((3840, 2020)));
        // A nonsense work area still never gives more than MAX_PX or less than MIN_PX.
        assert_eq!(guard(50000.0, 200.0, 1.0, (100000, 100000)), Some((MAX_PX, 200)));
    }

    #[test]
    fn sane_rectangles() {
        let w = work(0);
        assert!(sane(&Px::new(0, 0, 450, 600), &w));
        assert!(!sane(&Px::new(0, 0, 0, 600), &w));
        assert!(!sane(&Px::new(0, 0, -5, 600), &w));
        assert!(!sane(&Px::new(0, 0, 4032, 2352), &w));
        assert!(!sane(&Px::new(0, 0, 10, 10), &w));
    }
}
