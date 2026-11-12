//! The switcher window's behaviour: a glass panel that never takes focus (the app in front keeps
//! it, and its keys come from the hook), has no taskbar button or caption, stays above other
//! windows, and is shown and hidden directly rather than through the window library, which would
//! activate it.
//!
//! The same treatment as the dock's (`apps/bar/src/shape.rs`), plus three states:
//! - hidden between uses: Windows sends a hidden window no paint messages, so the UI sleeps;
//! - awake: on, but cloaked (the compositor leaves it out, and clicks pass it by), so the UI runs
//!   while Alt+Tab is held but before the panel should be seen;
//! - shown.

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_CLOAK};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, RedrawWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
    RDW_INTERNALPAINT,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, GetWindowRect, IsWindowVisible, PostMessageW, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, GWL_EXSTYLE, SW_SHOWNOACTIVATE, WM_APP,
    GWL_STYLE, HWND_TOPMOST, STYLESTRUCT, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE, WM_STYLECHANGING, WS_CAPTION,
    WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_MAXIMIZEBOX,
    WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
};
/// Posted by the keyboard hook so the hidden window turns on and its UI runs.
const WM_WAKE: u32 = WM_APP + 7;

/// Wake the UI from any thread, without waiting for it.
pub fn post_wake(hwnd: isize) {
    unsafe {
        let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_WAKE, WPARAM(0), LPARAM(0));
    }
}

fn cloak(hwnd: HWND, on: bool) {
    let value = u32::from(on);
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_CLOAK, (&raw const value).cast(), 4);
    }
}

/// Turn the hidden window on, cloaked, so it gets paint messages again.
fn wake(hwnd: HWND) {

    unsafe {
        if IsWindowVisible(hwnd).as_bool() {
            return;
        }
        cloak(hwnd, true);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        // Showing does not always ask for a paint (the window keeps its last image), and a frame
        // asked for while hidden was dropped.
        let _ = RedrawWindow(Some(hwnd), None, None, RDW_INTERNALPAINT);
    }
}

/// Set up the window. It starts cloaked: the window library shows it once after its first frame,
/// which the switcher undoes on the next, and cloaked that one frame is never seen.
pub fn never_activate(hwnd: isize) {
    let hwnd = HWND(hwnd as *mut _);
    cloak(hwnd, true);
    unsafe {
        // DWMWA_COLOR_NONE: the panel draws its own hairline.
        let none: u32 = 0xFFFF_FFFE;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, (&raw const none).cast(), 4);
        let _ = SetWindowSubclass(hwnd, Some(panel_proc), 1, 0);
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, fix_ex_style(ex as u32) as isize);
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        SetWindowLongPtrW(hwnd, GWL_STYLE, fix_style(style as u32) as isize);
        let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

fn fix_ex_style(ex: u32) -> u32 {
    (ex & !WS_EX_APPWINDOW.0) | WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0
}

fn fix_style(style: u32) -> u32 {
    style & !(WS_CAPTION.0 | WS_SYSMENU.0 | WS_THICKFRAME.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0)
}

unsafe extern "system" fn panel_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    const WM_MOUSEACTIVATE: u32 = 0x0021;
    const MA_NOACTIVATE: isize = 3;
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE),
        WM_WAKE => {
            wake(hwnd);
            LRESULT(0)
        }
        WM_STYLECHANGING if wparam.0 as i32 == GWL_EXSTYLE.0 => {
            let change = &mut *(lparam.0 as *mut STYLESTRUCT);
            change.styleNew = fix_ex_style(change.styleNew);
            DefSubclassProc(hwnd, msg, wparam, lparam)
        }
        WM_STYLECHANGING if wparam.0 as i32 == GWL_STYLE.0 => {
            let change = &mut *(lparam.0 as *mut STYLESTRUCT);
            change.styleNew = fix_style(change.styleNew);
            DefSubclassProc(hwnd, msg, wparam, lparam)
        }
        _ => DefSubclassProc(hwnd, msg, wparam, lparam),
    }
}

/// A monitor's work area in physical pixels, and its pixels per point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub work: RECT,
    pub scale: f32,
}

impl Screen {
    /// The monitor most of `hwnd` is on; the primary one for no window.
    pub fn of(hwnd: isize) -> Self {
        unsafe {
            let monitor = MonitorFromWindow(HWND(hwnd as *mut _), MONITOR_DEFAULTTOPRIMARY);
            let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(monitor, &mut info);
            let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
            let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
            Self { work: info.rcWork, scale: dpi_x.max(96) as f32 / 96.0 }
        }
    }

    /// The work area's size in points.
    pub fn size_points(&self) -> (f32, f32) {
        (
            (self.work.right - self.work.left) as f32 / self.scale,
            (self.work.bottom - self.work.top) as f32 / self.scale,
        )
    }

    /// A rectangle of `size` pixels, centred in the work area.
    pub fn centred(&self, size: (i32, i32)) -> RECT {
        let left = (self.work.left + self.work.right - size.0) / 2;
        let top = (self.work.top + self.work.bottom - size.1) / 2;
        RECT { left, top, right: left + size.0, bottom: top + size.1 }
    }
}

/// Place the panel and show it above every window, without activating it. Moving onto a monitor
/// of another scale makes the window library resize the window to keep its size in points, so
/// the size is set again once the move is done. It is placed while still cloaked, so it is never
/// seen where it was.
pub fn show_at(hwnd: isize, r: RECT) {
    let hwnd = HWND(hwnd as *mut _);
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            cloak(hwnd, true);
        }
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), r.left, r.top, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        let mut now = RECT::default();
        if GetWindowRect(hwnd, &mut now).is_ok() && now != r {
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), r.left, r.top, w, h, SWP_NOACTIVATE);
        }
    }
    cloak(hwnd, false);
}

pub fn hide(hwnd: isize) {
    unsafe {
        let _ = ShowWindow(HWND(hwnd as *mut _), SW_HIDE);
    }
}

pub fn is_on(hwnd: isize) -> bool {
    unsafe { IsWindowVisible(HWND(hwnd as *mut _)) }.as_bool()
}
