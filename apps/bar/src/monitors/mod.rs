//! The monitors, as Windows reports them in physical pixels (the bar is per-monitor DPI aware,
//! as every eframe app is), each with its own scale.

pub mod layout;

use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    /// Windows' name for it (`\\.\DISPLAY1`): stable while it stays plugged in, so bars keep
    /// their windows when monitors are re-arranged or rescaled.
    pub device: String,
    pub rect: RECT,
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
    out.sort_by_key(|m| (!m.primary, m.rect.left, m.rect.top));
    out
}
