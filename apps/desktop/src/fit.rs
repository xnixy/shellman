//! Wallpaper fit modes as plain geometry: where the picture goes on a monitor, and how big it is
//! scaled. No Windows calls here, so every mode is unit-tested.
//!
//! The picture is scaled once to `scaled`, then only the `src` part of that is kept (a crop, for
//! fill, span, and centre on a small monitor). Its pixels are drawn at `dst` on the monitor; the
//! kept bitmap is always exactly `dst` in size, so painting is a plain blit (repeated for tile).

/// A rectangle in whole pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Area {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
}

/// How the picture is fitted to the monitor (the same set as Windows' Personalization page).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fit {
    Center,
    Tile,
    Stretch,
    /// The whole picture, kept in proportion, with bars of the background colour.
    Letterbox,
    /// Covers the whole monitor, kept in proportion, cropping what overflows.
    Fill,
    /// One picture across all monitors; each shows its part.
    Span,
}

/// What to draw on one monitor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Plan {
    /// Size to scale the whole picture to.
    pub scaled: (i32, i32),
    /// The part of the scaled picture to keep.
    pub src: Area,
    /// Where that part goes on the monitor.
    pub dst: Area,
    /// Repeat the kept bitmap over the whole monitor instead of drawing it once.
    pub tile: bool,
}

/// Plan for one monitor of size `mon`. `Span` here means a single monitor, which is just fill;
/// use [`plan_span`] when there are several.
pub fn plan(fit: Fit, img: (i32, i32), mon: (i32, i32)) -> Plan {
    let (iw, ih) = (img.0.max(1), img.1.max(1));
    let (mw, mh) = (mon.0.max(1), mon.1.max(1));
    let whole = Area::new(0, 0, mw, mh);
    match fit {
        Fit::Stretch => Plan { scaled: (mw, mh), src: whole, dst: whole, tile: false },
        Fit::Fill | Fit::Span => {
            let scaled = cover(iw, ih, mw, mh);
            let src = Area::new((scaled.0 - mw) / 2, (scaled.1 - mh) / 2, mw, mh);
            Plan { scaled, src, dst: whole, tile: false }
        }
        Fit::Letterbox => {
            let scaled = contain(iw, ih, mw, mh);
            let dst = Area::new((mw - scaled.0) / 2, (mh - scaled.1) / 2, scaled.0, scaled.1);
            Plan { scaled, src: Area::new(0, 0, scaled.0, scaled.1), dst, tile: false }
        }
        Fit::Center => {
            // Unscaled; a picture bigger than the monitor shows its middle.
            let (vw, vh) = (iw.min(mw), ih.min(mh));
            let src = Area::new((iw - vw) / 2, (ih - vh) / 2, vw, vh);
            let dst = Area::new((mw - vw) / 2, (mh - vh) / 2, vw, vh);
            Plan { scaled: (iw, ih), src, dst, tile: false }
        }
        Fit::Tile => Plan {
            scaled: (iw, ih),
            src: Area::new(0, 0, iw, ih),
            dst: Area::new(0, 0, iw, ih),
            tile: true,
        },
    }
}

/// Plan for one monitor when one picture spans all of them. `union` is the size of the
/// rectangle around every monitor; `mon` is this monitor's rectangle relative to its top-left.
pub fn plan_span(img: (i32, i32), union: (i32, i32), mon: Area) -> Plan {
    let (iw, ih) = (img.0.max(1), img.1.max(1));
    let (uw, uh) = (union.0.max(1), union.1.max(1));
    let scaled = cover(iw, ih, uw, uh);
    let src = Area::new((scaled.0 - uw) / 2 + mon.x, (scaled.1 - uh) / 2 + mon.y, mon.w, mon.h);
    Plan { scaled, src, dst: Area::new(0, 0, mon.w, mon.h), tile: false }
}

/// Smallest proportional size that covers `mw` x `mh`.
fn cover(iw: i32, ih: i32, mw: i32, mh: i32) -> (i32, i32) {
    let (iw, ih, mw, mh) = (iw as i64, ih as i64, mw as i64, mh as i64);
    if mw * ih >= mh * iw {
        (mw as i32, div_ceil(ih * mw, iw).max(mh) as i32)
    } else {
        (div_ceil(iw * mh, ih).max(mw) as i32, mh as i32)
    }
}

/// Largest proportional size that fits inside `mw` x `mh`.
fn contain(iw: i32, ih: i32, mw: i32, mh: i32) -> (i32, i32) {
    let (iw, ih, mw, mh) = (iw as i64, ih as i64, mw as i64, mh as i64);
    if mw * ih <= mh * iw {
        (mw as i32, ((ih * mw + iw / 2) / iw).clamp(1, mh) as i32)
    } else {
        (((iw * mh + ih / 2) / ih).clamp(1, mw) as i32, mh as i32)
    }
}

fn div_ceil(a: i64, b: i64) -> i64 {
    (a + b - 1) / b
}

#[cfg(test)]
mod tests {
    use super::*;

    const MON: (i32, i32) = (1920, 1080);

    #[test]
    fn stretch_ignores_proportions() {
        let p = plan(Fit::Stretch, (100, 100), MON);
        assert_eq!(p.scaled, MON);
        assert_eq!(p.dst, Area::new(0, 0, 1920, 1080));
        assert_eq!(p.src, p.dst);
    }

    #[test]
    fn fill_crops_the_overflow_evenly() {
        // A square picture on a wide monitor: width-limited, crops top and bottom.
        let p = plan(Fit::Fill, (1000, 1000), MON);
        assert_eq!(p.scaled, (1920, 1920));
        assert_eq!(p.src, Area::new(0, 420, 1920, 1080));
        assert_eq!(p.dst, Area::new(0, 0, 1920, 1080));
        // A tall-and-narrow monitor: height-limited, crops left and right.
        let p = plan(Fit::Fill, (2000, 1000), (1000, 1000));
        assert_eq!(p.scaled, (2000, 1000));
        assert_eq!(p.src, Area::new(500, 0, 1000, 1000));
    }

    #[test]
    fn fill_of_matching_aspect_is_exact() {
        let p = plan(Fit::Fill, (3840, 2160), MON);
        assert_eq!(p.scaled, MON);
        assert_eq!(p.src, Area::new(0, 0, 1920, 1080));
    }

    #[test]
    fn fit_letterboxes() {
        let p = plan(Fit::Letterbox, (1000, 1000), MON);
        assert_eq!(p.scaled, (1080, 1080));
        assert_eq!(p.dst, Area::new(420, 0, 1080, 1080));
        assert_eq!(p.src, Area::new(0, 0, 1080, 1080));
        // Wide picture on a tall monitor: bars above and below.
        let p = plan(Fit::Letterbox, (2000, 1000), (1000, 1000));
        assert_eq!(p.scaled, (1000, 500));
        assert_eq!(p.dst, Area::new(0, 250, 1000, 500));
    }

    #[test]
    fn center_small_picture_sits_in_the_middle() {
        let p = plan(Fit::Center, (800, 600), MON);
        assert_eq!(p.scaled, (800, 600));
        assert_eq!(p.src, Area::new(0, 0, 800, 600));
        assert_eq!(p.dst, Area::new(560, 240, 800, 600));
    }

    #[test]
    fn center_big_picture_shows_its_middle() {
        let p = plan(Fit::Center, (3000, 1080), MON);
        assert_eq!(p.src, Area::new(540, 0, 1920, 1080));
        assert_eq!(p.dst, Area::new(0, 0, 1920, 1080));
    }

    #[test]
    fn tile_repeats_the_unscaled_picture() {
        let p = plan(Fit::Tile, (64, 32), MON);
        assert!(p.tile);
        assert_eq!(p.scaled, (64, 32));
        assert_eq!(p.dst, Area::new(0, 0, 64, 32));
    }

    #[test]
    fn single_monitor_span_is_fill() {
        assert_eq!(plan(Fit::Span, (1000, 1000), MON), plan(Fit::Fill, (1000, 1000), MON));
    }

    #[test]
    fn span_gives_each_monitor_its_part() {
        // Two 1920x1080 monitors side by side, a 3840x1080 picture: each shows its half.
        let left = plan_span((3840, 1080), (3840, 1080), Area::new(0, 0, 1920, 1080));
        let right = plan_span((3840, 1080), (3840, 1080), Area::new(1920, 0, 1920, 1080));
        assert_eq!(left.scaled, (3840, 1080));
        assert_eq!(left.src, Area::new(0, 0, 1920, 1080));
        assert_eq!(right.src, Area::new(1920, 0, 1920, 1080));
        assert_eq!(right.dst, Area::new(0, 0, 1920, 1080));
    }

    #[test]
    fn span_crops_around_the_middle_of_the_union() {
        // A square picture over a 2000x1000 union: scaled to 2000x2000, middle band kept.
        let p = plan_span((500, 500), (2000, 1000), Area::new(1000, 0, 1000, 1000));
        assert_eq!(p.scaled, (2000, 2000));
        assert_eq!(p.src, Area::new(1000, 500, 1000, 1000));
    }

    #[test]
    fn degenerate_sizes_do_not_panic() {
        for fit in [Fit::Center, Fit::Tile, Fit::Stretch, Fit::Letterbox, Fit::Fill, Fit::Span] {
            let _ = plan(fit, (0, 0), (0, 0));
            let _ = plan(fit, (1, 5000), (7, 3));
        }
    }

    #[test]
    fn kept_bitmap_matches_the_destination() {
        for fit in [Fit::Center, Fit::Stretch, Fit::Letterbox, Fit::Fill] {
            for img in [(100, 3000), (3000, 100), (1920, 1080), (5, 5)] {
                let p = plan(fit, img, MON);
                assert_eq!((p.src.w, p.src.h), (p.dst.w, p.dst.h), "{fit:?} {img:?}");
                assert!(p.src.x >= 0 && p.src.y >= 0);
                assert!(p.src.x + p.src.w <= p.scaled.0 && p.src.y + p.src.h <= p.scaled.1);
            }
        }
    }
}
