//! For tests only: `SHELLMAN_ONLY_MONITOR=<x>,<y>` keeps a test copy of the shelf (with its own
//! `SHELLMAN_INSTANCE`) to the one monitor holding that point, taken as the primary, so it never
//! draws on the others (someone may be using them). Unset, as always outside tests, it changes
//! nothing.

use shellman_core::monitors::Monitor;

pub const VAR: &str = "SHELLMAN_ONLY_MONITOR";

/// The point, when the variable is set.
pub fn point() -> Option<(i32, i32)> {
    let v = std::env::var(VAR).ok()?;
    let (x, y) = v.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// The monitors, cut down to the one holding the point when the variable is set.
pub fn restrict(list: Vec<Monitor>) -> Vec<Monitor> {
    match point() {
        Some(p) => only(list, p),
        None => list,
    }
}

fn only(list: Vec<Monitor>, (x, y): (i32, i32)) -> Vec<Monitor> {
    list.into_iter()
        .filter(|m| m.rect.left <= x && x < m.rect.right && m.rect.top <= y && y < m.rect.bottom)
        .map(|mut m| {
            m.primary = true;
            m
        })
        .take(1)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::RECT;

    #[test]
    fn keeps_the_monitor_under_the_point_as_primary() {
        let mon = |d: &str, left: i32, primary: bool| {
            let rect = RECT { left, top: 0, right: left + 3840, bottom: 2160 };
            Monitor { device: d.into(), rect, work: rect, scale: 1.5, primary }
        };
        let list = vec![mon("A", 0, true), mon("B", -3840, false)];
        let got = only(list, (-3000, 100));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].device, "B");
        assert!(got[0].primary);
    }
}
