//! The monitors as Windows reports them, in physical pixels (for a per-monitor DPI aware
//! process, as every shellman app is), each with its work area and its own scale.
//!
//! The bar keeps a list of its own (`apps/bar/src/monitors`, the same enumeration without the
//! work area); apps that need the monitors from now on use this one.

use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    /// Windows' name for it (`\\.\DISPLAY1`): stable while it stays plugged in, so windows can
    /// stay with their monitor when monitors are re-arranged or rescaled.
    pub device: String,
    /// The whole monitor.
    pub rect: RECT,
    /// The monitor less the bars' (and any other app bar's) reserved space.
    pub work: RECT,
    /// Pixels per point: 1.5 at 150%.
    pub scale: f32,
    pub primary: bool,
}

/// MONITORINFOF_PRIMARY.
const PRIMARY: u32 = 1;

/// Every monitor now, the primary first, then left to right.
pub fn all() -> Vec<Monitor> {
    unsafe extern "system" fn visit(monitor: HMONITOR, _: HDC, _: *mut RECT, out: LPARAM) -> BOOL {
        let out = &mut *(out.0 as *mut Vec<Monitor>);
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(monitor, &mut info.monitorInfo).as_bool() {
            let (mut dpi, mut _y) = (96u32, 96u32);
            if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut _y).is_err() || dpi == 0 {
                dpi = 96;
            }
            let len = info.szDevice.iter().position(|&c| c == 0).unwrap_or(info.szDevice.len());
            out.push(Monitor {
                device: String::from_utf16_lossy(&info.szDevice[..len]),
                rect: info.monitorInfo.rcMonitor,
                work: info.monitorInfo.rcWork,
                scale: dpi as f32 / 96.0,
                primary: info.monitorInfo.dwFlags & PRIMARY != 0,
            });
        }
        BOOL(1)
    }
    let mut out: Vec<Monitor> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(visit), LPARAM((&raw mut out) as isize));
    }
    sort(&mut out);
    out
}

/// The primary first, then left to right, then top to bottom.
pub fn sort(list: &mut [Monitor]) {
    list.sort_by_key(|m| (!m.primary, m.rect.left, m.rect.top));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(device: &str, left: i32, primary: bool) -> Monitor {
        let rect = RECT { left, top: 0, right: left + 3840, bottom: 2160 };
        Monitor { device: device.into(), rect, work: rect, scale: 1.5, primary }
    }

    #[test]
    fn the_primary_comes_first_then_left_to_right() {
        let mut list = vec![mon("C", 3840, false), mon("A", 0, true), mon("B", -3840, false)];
        sort(&mut list);
        let order: Vec<&str> = list.iter().map(|m| m.device.as_str()).collect();
        assert_eq!(order, ["A", "B", "C"]);
    }

    #[test]
    fn this_machine_has_a_monitor_with_a_sane_scale() {
        // Runs against the real display set-up: at least the list is well-formed.
        for m in all() {
            assert!(m.scale >= 1.0 && m.scale.is_finite(), "{m:?}");
        }
    }
}
