//! The Win32 side of the preview panel: where a dock window and its monitor are, and showing and
//! cloaking the panel's own window.

use super::hover::Anchor;
use super::layout::DOCK_GAP;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CLOAK};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, IsWindowVisible, ShowWindow, SW_HIDE, SW_SHOWNOACTIVATE,
};

/// Where the panel goes for an icon whose centre is `center_x` points from the left of the dock
/// window `dock`, over glass whose top is `top` points down from the window's top, drawn at `ppp`
/// pixels per point. In the dock's monitor's pixels.
pub fn anchor(dock: isize, center_x: f32, top: f32, ppp: f32) -> Anchor {
    let dock = HWND(dock as *mut _);
    let mut at = RECT::default();
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    let (mut dpi, mut _y) = (96u32, 96u32);
    unsafe {
        let _ = GetWindowRect(dock, &mut at);
        let monitor = MonitorFromWindow(dock, MONITOR_DEFAULTTONEAREST);
        let _ = GetMonitorInfoW(monitor, &mut info);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut _y).is_err() || dpi == 0 {
            dpi = 96;
        }
    }
    let scale = dpi as f32 / 96.0;
    Anchor {
        center_x: at.left + (center_x * ppp).round() as i32,
        bottom: at.top + (top * ppp).round() as i32 - (DOCK_GAP * scale).round() as i32,
        monitor: info.rcMonitor,
        scale,
    }
}

/// Cloaked, Windows keeps a window visible (so it gets frames) but never draws it.
pub fn cloak(hwnd: isize, on: bool) {
    let value = u32::from(on);
    unsafe {
        let _ = DwmSetWindowAttribute(HWND(hwnd as *mut _), DWMWA_CLOAK, (&raw const value).cast(), 4);
    }
}

/// Show without activating, if it is not shown already.
pub fn show(hwnd: isize) {
    let hwnd = HWND(hwnd as *mut _);
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    }
}

pub fn hide(hwnd: isize) {
    unsafe {
        let _ = ShowWindow(HWND(hwnd as *mut _), SW_HIDE);
    }
}
