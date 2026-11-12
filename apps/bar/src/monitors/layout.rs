//! Which bars go on which monitor, and where, in that monitor's pixels. Pure arithmetic over the
//! monitor list, so it is tested without windows.
//!
//! Bar sizes are in points; a monitor at 150% has 1.5 pixels per point. Each bar is sized with
//! its own monitor's scale, so a top bar is 32 pt tall on every monitor whatever the mix.
//!
//! Space is always measured from the monitor's edges (or the place Explorer grants), never from
//! the work area: the work area already has the bars' own space taken out, and measuring from it
//! would push each bar a band further in.

use super::Monitor;
use shellman_core::settings::bar::Docks;
use windows::Win32::Foundation::RECT;

/// Bar sizes, in points.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Sizes {
    pub top: f32,
    pub dock: f32,
    /// Space between the dock and the edge below it, and between the dock and maximized windows
    /// above it.
    pub gap: f32,
}

/// What one monitor gets.
#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub device: String,
    pub monitor: RECT,
    pub scale: f32,
    pub primary: bool,
    /// The top bar's window, which is also the space it reserves.
    pub top: RECT,
    /// The band the dock reserves along the bottom (dock height and a gap above and below), if
    /// this monitor has a dock.
    pub dock_band: Option<RECT>,
}

/// Points to whole pixels at `scale`.
pub fn px(points: f32, scale: f32) -> i32 {
    (points * scale).round() as i32
}

/// The top bar: the monitor's full width, flush with its top edge.
pub fn top_rect(monitor: RECT, scale: f32, sizes: &Sizes) -> RECT {
    RECT { bottom: monitor.top + px(sizes.top, scale), ..monitor }
}

/// The dock's band: the full width, ending at `bottom` (the monitor's bottom edge, unless
/// Explorer's taskbar sits there).
pub fn dock_band(monitor: RECT, bottom: i32, scale: f32, sizes: &Sizes) -> RECT {
    RECT {
        top: bottom - px(sizes.dock + 2.0 * sizes.gap, scale),
        bottom,
        ..monitor
    }
}

/// The dock window: `width` points wide, centred in its band, a gap above the band's bottom.
/// Never wider than the band.
pub fn dock_rect(band: RECT, width: f32, scale: f32, sizes: &Sizes) -> RECT {
    let w = px(width, scale).min(band.right - band.left);
    let left = band.left + (band.right - band.left - w) / 2;
    let bottom = band.bottom - px(sizes.gap, scale);
    RECT {
        left,
        top: bottom - px(sizes.dock, scale),
        right: left + w,
        bottom,
    }
}

/// Every monitor gets a top bar; docks go on all of them or only the primary.
pub fn plan(monitors: &[Monitor], docks: Docks, sizes: &Sizes) -> Vec<Slot> {
    monitors
        .iter()
        .map(|m| {
            let has_dock = m.primary || docks == Docks::All;
            Slot {
                device: m.device.clone(),
                monitor: m.rect,
                scale: m.scale,
                primary: m.primary,
                top: top_rect(m.rect, m.scale, sizes),
                dock_band: has_dock.then(|| dock_band(m.rect, m.rect.bottom, m.scale, sizes)),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZES: Sizes = Sizes { top: 32.0, dock: 56.0, gap: 8.0 };

    const fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT { left, top, right, bottom }
    }

    fn monitor(device: &str, rect: RECT, scale: f32, primary: bool) -> Monitor {
        Monitor { device: device.into(), rect, scale, primary }
    }

    fn inside(inner: RECT, outer: RECT) -> bool {
        inner.left >= outer.left && inner.top >= outer.top && inner.right <= outer.right && inner.bottom <= outer.bottom
    }

    #[test]
    fn a_4k_monitor_at_150_percent() {
        let m = rect(0, 0, 3840, 2160);
        let slots = plan(&[monitor("A", m, 1.5, true)], Docks::All, &SIZES);
        assert_eq!(slots[0].top, rect(0, 0, 3840, 48));
        let band = slots[0].dock_band.unwrap();
        // 72 pt of band, ending at the screen's bottom edge, not above some work area.
        assert_eq!(band, rect(0, 2160 - 108, 3840, 2160));
        let dock = dock_rect(band, 400.0, 1.5, &SIZES);
        assert_eq!(dock, rect(1620, 2160 - 12 - 84, 2220, 2160 - 12));
        assert!(inside(dock, band));
        // The gap below and above the dock are equal: maximized windows stop just above it.
        assert_eq!(dock.top - band.top, band.bottom - dock.bottom);
    }

    #[test]
    fn mixed_scales_size_each_monitor_by_its_own() {
        // The primary at 150%, a 1080p monitor at 100% to its right.
        let slots = plan(
            &[
                monitor("A", rect(0, 0, 3840, 2160), 1.5, true),
                monitor("B", rect(3840, 0, 5760, 1080), 1.0, false),
            ],
            Docks::All,
            &SIZES,
        );
        assert_eq!(slots[1].top, rect(3840, 0, 5760, 32));
        assert_eq!(slots[1].dock_band, Some(rect(3840, 1080 - 72, 5760, 1080)));
        let dock = dock_rect(slots[1].dock_band.unwrap(), 400.0, 1.0, &SIZES);
        assert_eq!(dock, rect(4600, 1016, 5000, 1072));
    }

    #[test]
    fn monitors_left_of_and_above_the_primary_have_negative_coordinates() {
        let left = monitor("L", rect(-2560, 0, 0, 1440), 1.25, false);
        let above = monitor("U", rect(0, -1080, 1920, 0), 1.0, false);
        let slots = plan(&[monitor("P", rect(0, 0, 1920, 1080), 1.0, true), left, above], Docks::All, &SIZES);
        assert_eq!(slots[1].top, rect(-2560, 0, 0, 40));
        assert_eq!(slots[1].dock_band, Some(rect(-2560, 1440 - 90, 0, 1440)));
        let dock = dock_rect(slots[1].dock_band.unwrap(), 300.0, 1.25, &SIZES);
        assert_eq!(dock.right - dock.left, 375);
        // Centred on the monitor, to the pixel (an odd width cannot split evenly).
        assert!((dock.left + dock.right + 2560).abs() <= 1);
        assert!(inside(dock, slots[1].dock_band.unwrap()));
        assert_eq!(slots[2].top, rect(0, -1080, 1920, -1048));
        assert_eq!(slots[2].dock_band, Some(rect(0, -72, 1920, 0)));
    }

    #[test]
    fn docks_on_the_primary_only() {
        let slots = plan(
            &[
                monitor("B", rect(-1920, 0, 0, 1080), 1.0, false),
                monitor("A", rect(0, 0, 1920, 1080), 1.0, true),
            ],
            Docks::Primary,
            &SIZES,
        );
        assert_eq!(slots[0].dock_band, None);
        assert!(slots[1].dock_band.is_some());
        // Every monitor keeps its top bar.
        assert_eq!(slots[0].top, rect(-1920, 0, 0, 32));
    }

    #[test]
    fn a_band_above_explorers_taskbar() {
        // Explorer's 48 px taskbar at the bottom: the band ends where it begins.
        let m = rect(0, 0, 1920, 1080);
        let band = dock_band(m, 1080 - 48, 1.0, &SIZES);
        assert_eq!(band, rect(0, 960, 1920, 1032));
        assert_eq!(dock_rect(band, 200.0, 1.0, &SIZES).bottom, 1024);
    }

    #[test]
    fn a_dock_wider_than_its_monitor_is_cut_to_fit() {
        let band = rect(100, 0, 600, 72);
        let dock = dock_rect(band, 900.0, 1.0, &SIZES);
        assert_eq!((dock.left, dock.right), (100, 600));
    }
}
