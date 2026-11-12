//! The monitors' rectangles in virtual-screen pixels (real pixels: the process is per-monitor DPI
//! aware).

use crate::fit::Area;
use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};

/// Every attached monitor, whole rectangle (not the work area).
pub fn list() -> Vec<Area> {
    unsafe extern "system" fn visit(_: HMONITOR, _: HDC, rect: *mut RECT, out: LPARAM) -> BOOL {
        let out = &mut *(out.0 as *mut Vec<Area>);
        let r = *rect;
        out.push(Area::new(r.left, r.top, r.right - r.left, r.bottom - r.top));
        BOOL(1)
    }
    let mut found: Vec<Area> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(visit), LPARAM((&raw mut found) as isize));
    }
    found
}

/// The rectangle around all monitors.
pub fn union(monitors: &[Area]) -> Area {
    let Some(first) = monitors.first() else {
        return Area::new(0, 0, 0, 0);
    };
    let (mut l, mut t, mut r, mut b) = (first.x, first.y, first.x + first.w, first.y + first.h);
    for m in monitors {
        l = l.min(m.x);
        t = t.min(m.y);
        r = r.max(m.x + m.w);
        b = b.max(m.y + m.h);
    }
    Area::new(l, t, r - l, b - t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_covers_every_monitor() {
        let u = union(&[Area::new(0, 0, 1920, 1080), Area::new(-1280, 100, 1280, 1024)]);
        assert_eq!(u, Area::new(-1280, 0, 3200, 1124));
    }
}
