//! The arrangement of monitors as rectangles in the desktop's pixel space: the rules Windows
//! keeps (no overlap, no gaps, the primary at 0,0) and the snapping a drag needs. Pure, so it is
//! tested without a screen.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    /// Sharing any area (touching edges do not count).
    pub fn overlaps(&self, o: &Rect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }
    /// Sharing a stretch of edge, as two monitors side by side do. A shared corner is not enough:
    /// Windows refuses it.
    pub fn adjoins(&self, o: &Rect) -> bool {
        let across = |a0: i32, a1: i32, b0: i32, b1: i32| a0 < b1 && b0 < a1;
        let sides = (self.right() == o.x || o.right() == self.x) && across(self.y, self.bottom(), o.y, o.bottom());
        let tops = (self.bottom() == o.y || o.bottom() == self.y) && across(self.x, self.right(), o.x, o.right());
        sides || tops
    }
}

/// Shift everything so the primary monitor's corner is at 0,0.
pub fn normalise(rects: &mut [Rect], primary: usize) {
    let Some(p) = rects.get(primary).copied() else { return };
    for r in rects.iter_mut() {
        r.x -= p.x;
        r.y -= p.y;
    }
}

/// Whether every monitor can be reached from the first through shared edges.
pub fn connected(rects: &[Rect]) -> bool {
    if rects.is_empty() {
        return true;
    }
    let mut seen = vec![false; rects.len()];
    let mut stack = vec![0usize];
    seen[0] = true;
    while let Some(i) = stack.pop() {
        for j in 0..rects.len() {
            if !seen[j] && rects[i].adjoins(&rects[j]) {
                seen[j] = true;
                stack.push(j);
            }
        }
    }
    seen.into_iter().all(|s| s)
}

/// Whether an arrangement is one Windows accepts: nothing overlaps and nothing is cut off.
pub fn valid(rects: &[Rect]) -> bool {
    let clear = rects.iter().enumerate().all(|(i, a)| rects.iter().skip(i + 1).all(|b| !a.overlaps(b)));
    clear && connected(rects)
}

/// Where monitor `moving` lands when dragged so its corner wants `want`: the nearest place that
/// sits against another monitor's edge, aligned to that edge when within `snap` pixels of it,
/// overlapping nothing and leaving no monitor cut off. None when nowhere is allowed.
pub fn place(rects: &[Rect], moving: usize, want: (i32, i32), snap: i32) -> Option<(i32, i32)> {
    let m = *rects.get(moving)?;
    let mut best: Option<((i32, i32), i64)> = None;
    for (i, o) in rects.iter().enumerate() {
        if i == moving {
            continue;
        }
        // Across a side the other axis may slide while a stretch of edge is still shared.
        let slide_y = |y: i32| {
            let lo = o.y - m.h + 1;
            let hi = o.bottom() - 1;
            let mut y = y.clamp(lo, hi);
            for aligned in [o.y, o.bottom() - m.h, o.y - m.h, o.bottom()] {
                if aligned >= lo && aligned <= hi && (aligned - y).abs() <= snap {
                    y = aligned;
                    break;
                }
            }
            y
        };
        let slide_x = |x: i32| {
            let lo = o.x - m.w + 1;
            let hi = o.right() - 1;
            let mut x = x.clamp(lo, hi);
            for aligned in [o.x, o.right() - m.w] {
                if (aligned - x).abs() <= snap {
                    x = aligned;
                    break;
                }
            }
            x
        };
        let candidates = [
            (o.right(), slide_y(want.1)),
            (o.x - m.w, slide_y(want.1)),
            (slide_x(want.0), o.bottom()),
            (slide_x(want.0), o.y - m.h),
        ];
        for (x, y) in candidates {
            let mut test = rects.to_vec();
            test[moving] = Rect::new(x, y, m.w, m.h);
            if !valid(&test) {
                continue;
            }
            let (dx, dy) = ((x - want.0) as i64, (y - want.1) as i64);
            let d = dx * dx + dy * dy;
            if best.is_none_or(|(_, b)| d < b) {
                best = Some(((x, y), d));
            }
        }
    }
    best.map(|(p, _)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> Vec<Rect> {
        vec![Rect::new(0, 0, 1920, 1080), Rect::new(1920, 0, 1920, 1080)]
    }

    #[test]
    fn touching_is_not_overlap_but_corners_do_not_join() {
        let a = Rect::new(0, 0, 100, 100);
        assert!(!a.overlaps(&Rect::new(100, 0, 100, 100)));
        assert!(a.adjoins(&Rect::new(100, 50, 100, 100)));
        assert!(!a.adjoins(&Rect::new(100, 100, 100, 100)));
        assert!(a.overlaps(&Rect::new(99, 99, 100, 100)));
    }

    #[test]
    fn primary_goes_to_the_origin() {
        let mut r = vec![Rect::new(-1920, 0, 1920, 1080), Rect::new(0, 0, 1920, 1080), Rect::new(5, 1080, 1920, 1080)];
        normalise(&mut r, 1);
        assert_eq!(r[0], Rect::new(-1920, 0, 1920, 1080));
        assert_eq!(r[1].x, 0);
        let mut r = vec![Rect::new(-1920, 0, 1920, 1080), Rect::new(0, 0, 1920, 1080)];
        normalise(&mut r, 0);
        assert_eq!((r[0].x, r[1].x), (0, 1920));
    }

    #[test]
    fn a_drop_on_top_of_another_lands_against_its_edge() {
        let r = pair();
        let (x, y) = place(&r, 1, (1000, 10), 40).unwrap();
        let mut t = r.clone();
        t[1] = Rect::new(x, y, 1920, 1080);
        assert!(valid(&t));
    }

    #[test]
    fn a_near_miss_snaps_to_align() {
        let r = pair();
        assert_eq!(place(&r, 1, (1930, 20), 40), Some((1920, 0)));
        assert_eq!(place(&r, 1, (1930, 300), 40), Some((1920, 300)));
    }

    #[test]
    fn a_gap_is_closed() {
        let r = pair();
        assert_eq!(place(&r, 1, (2500, 0), 40), Some((1920, 0)));
    }

    #[test]
    fn below_and_above_work_and_a_lone_monitor_has_nowhere_to_go() {
        let r = pair();
        assert_eq!(place(&r, 1, (50, 1200), 40), Some((50, 1080)));
        assert_eq!(place(&r[..1], 0, (5, 5), 40), None);
    }

    #[test]
    fn a_middle_monitor_may_not_cut_the_others_off() {
        let r = vec![Rect::new(0, 0, 100, 100), Rect::new(100, 0, 100, 100), Rect::new(200, 0, 100, 100)];
        if let Some((x, y)) = place(&r, 1, (200, 100), 10) {
            let mut t = r.clone();
            t[1] = Rect::new(x, y, 100, 100);
            assert!(valid(&t));
        }
    }

    #[test]
    fn the_odd_arrangement_of_this_machine_is_valid_as_it_is() {
        // Two screens side by side and a third below the first, offset by 3746 px.
        let r = vec![Rect::new(0, 0, 3840, 2160), Rect::new(-3840, 0, 3840, 2160), Rect::new(3746, 2160, 3840, 2160)];
        assert!(valid(&r));
    }
}
