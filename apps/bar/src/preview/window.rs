//! The Win32 side of the preview panel: where a dock window and its monitor are, and showing
//! and hiding the panel's own window (cloaking goes through `glass::cloak`, which takes its glass
//! along).

use super::hover::Anchor;
use super::layout::DOCK_GAP;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, GetWindowRect, IsWindowVisible, ShowWindow, SW_HIDE, SW_SHOWNOACTIVATE,
};

/// Where the cursor is, in physical pixels.
pub fn cursor() -> Option<(i32, i32)> {
    let mut p = POINT::default();
    unsafe { GetCursorPos(&mut p).ok().map(|_| (p.x, p.y)) }
}

/// A window's rectangle, if it still exists.
pub fn rect_of(hwnd: isize) -> Option<RECT> {
    let mut r = RECT::default();
    unsafe { GetWindowRect(HWND(hwnd as *mut _), &mut r).ok().map(|_| r) }
}

/// Exists and is shown (cloaked windows count as shown).
pub fn is_shown(hwnd: isize) -> bool {
    unsafe { IsWindowVisible(HWND(hwnd as *mut _)).as_bool() }
}

pub fn foreground() -> isize {
    unsafe { GetForegroundWindow().0 as isize }
}

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
