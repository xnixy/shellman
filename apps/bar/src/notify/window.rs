//! The Win32 side of the notifications window: where the primary monitor's work area is, and
//! showing, hiding and (for typing a reply) focusing the window.

use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    IsWindowVisible, SetForegroundWindow, ShowWindow, SW_HIDE, SW_SHOWNOACTIVATE,
};

/// The primary monitor's work area (`[left, top, right, bottom]`, physical pixels: the screen
/// less the top bar's and the dock's reserved space) and its scale.
pub fn primary_work_area() -> ([i32; 4], f32) {
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    let (mut dpi, mut _y) = (96u32, 96u32);
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let _ = GetMonitorInfoW(monitor, &mut info);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut _y).is_err() || dpi == 0 {
            dpi = 96;
        }
    }
    let w = info.rcWork;
    ([w.left, w.top, w.right, w.bottom], dpi as f32 / 96.0)
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

/// Take the keyboard: the user clicked into a reply box. The window is never activated by a
/// click (it must not take focus from the app in front just by appearing or being clicked), so
/// it asks for it, which Windows allows the process the user just clicked.
pub fn take_focus(hwnd: isize) {
    unsafe {
        let _ = SetForegroundWindow(HWND(hwnd as *mut _));
    }
}
