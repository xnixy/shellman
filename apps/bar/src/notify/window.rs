//! The Win32 side of the notifications window: the monitors' work areas, and
//! showing, hiding and (for typing a reply) focusing the window.

use super::layout::Area;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    IsWindowVisible, SetForegroundWindow, ShowWindow, SW_SHOWNOACTIVATE,
};

/// Every monitor with its work area (the screen less the top bar's and the dock's reserved
/// space) and its scale, in signed physical pixels; the primary first.
pub fn areas() -> Vec<Area> {
    unsafe extern "system" fn visit(monitor: HMONITOR, _: HDC, _: *mut RECT, out: LPARAM) -> BOOL {
        let out = &mut *(out.0 as *mut Vec<(bool, Area)>);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let (mut dpi, mut _y) = (96u32, 96u32);
            if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut _y).is_err() || dpi == 0 {
                dpi = 96;
            }
            let (m, w) = (info.rcMonitor, info.rcWork);
            out.push((
                info.dwFlags & 1 != 0,
                Area { rect: [m.left, m.top, m.right, m.bottom], work: [w.left, w.top, w.right, w.bottom], scale: dpi as f32 / 96.0 },
            ));
        }
        BOOL(1)
    }
    let mut out: Vec<(bool, Area)> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(visit), LPARAM((&raw mut out) as isize));
    }
    out.sort_by_key(|(primary, _)| !primary);
    out.into_iter().map(|(_, a)| a).collect()
}

/// The device name (`\\.\DISPLAY1`) of the monitor the window in front is on, if any.
pub fn foreground_device() -> Option<String> {
    use windows::Win32::Graphics::Gdi::{MonitorFromWindow, MONITORINFOEXW, MONITOR_DEFAULTTONULL};
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    unsafe {
        let front = GetForegroundWindow();
        if front.is_invalid() {
            return None;
        }
        let monitor = MonitorFromWindow(front, MONITOR_DEFAULTTONULL);
        if monitor.is_invalid() {
            return None;
        }
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        if !GetMonitorInfoW(monitor, &mut info.monitorInfo).as_bool() {
            return None;
        }
        let len = info.szDevice.iter().position(|&c| c == 0).unwrap_or(info.szDevice.len());
        Some(String::from_utf16_lossy(&info.szDevice[..len]))
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

/// Take the keyboard: the user clicked into a reply box. The window is never activated by a
/// click (it must not take focus from the app in front just by appearing or being clicked), so
/// it asks for it, which Windows allows the process the user just clicked.
pub fn take_focus(hwnd: isize) {
    unsafe {
        let _ = SetForegroundWindow(HWND(hwnd as *mut _));
    }
}
