//! The dock window's behaviour and outline.
//!
//! Windows paints a glass backdrop over the whole window rectangle, rounded only by its own
//! window corners; a window region does not clip it (tried and measured). So the dock's pill uses
//! exactly Windows' corner radius, which keeps real blur and smooth, anti-aliased corners.

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, STYLESTRUCT, WM_STYLECHANGING,
    WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

/// Windows 11's rounded window corner radius, in points.
pub const SYSTEM_RADIUS: f32 = 8.0;

/// Clicking the dock must not take focus from the app in front, or "minimize the app in front"
/// would always see the dock instead. The dock draws its own hairline, so Windows' window border
/// (which follows the accent color) is turned off.
///
/// The window library re-applies its own styles on resizes, so the styles are enforced as they
/// change, in a subclass, rather than set once.
pub fn never_activate(hwnd: isize) {
    let hwnd = HWND(hwnd as *mut _);
    unsafe {
        // DWMWA_COLOR_NONE.
        let none: u32 = 0xFFFF_FFFE;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, (&raw const none).cast(), 4);
        let _ = SetWindowSubclass(hwnd, Some(dock_proc), 1, 0);
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, fix_ex_style(ex as u32) as isize);
    }
}

/// Never "show on the taskbar" (the library marks every window so and hides it another way,
/// which other docks can't see), never activated, and a tool window.
fn fix_ex_style(ex: u32) -> u32 {
    (ex & !WS_EX_APPWINDOW.0) | WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0
}

unsafe extern "system" fn dock_proc(
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
        WM_STYLECHANGING if wparam.0 as i32 == GWL_EXSTYLE.0 => {
            let change = &mut *(lparam.0 as *mut STYLESTRUCT);
            change.styleNew = fix_ex_style(change.styleNew);
            DefSubclassProc(hwnd, msg, wparam, lparam)
        }
        _ => DefSubclassProc(hwnd, msg, wparam, lparam),
    }
}
