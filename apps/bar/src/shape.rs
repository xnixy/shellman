//! The bar windows' behaviour and outline.
//!
//! Windows paints a glass backdrop over the whole window rectangle, rounded only by its own
//! window corners; a window region does not clip it (tried and measured). So the dock's pill uses
//! exactly Windows' corner radius, which keeps real blur and smooth, anti-aliased corners.

pub mod surface;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
};
use windows::core::BOOL;
use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_NCACTIVATE, EnumWindows, GetWindowTextW, GetWindowThreadProcessId};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, GWL_STYLE, STYLESTRUCT,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WM_DPICHANGED, WM_STYLECHANGING,
    WS_CAPTION, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_MAXIMIZEBOX,
    WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
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
        // A new window starts inactive; say it is active now (the subclass keeps it so).
        SendMessageW(hwnd, WM_NCACTIVATE, Some(WPARAM(1)), Some(LPARAM(-1)));
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, fix_ex_style(ex as u32) as isize);
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        SetWindowLongPtrW(hwnd, GWL_STYLE, fix_style(style as u32) as isize);
        let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

/// Never "show on the taskbar" (the library marks every window so and hides it another way,
/// which other docks can't see), never activated, and a tool window.
fn fix_ex_style(ex: u32) -> u32 {
    (ex & !WS_EX_APPWINDOW.0) | WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0
}

/// No caption, system menu, or sizing frame: over glass, Windows would otherwise draw a ghost
/// close button into the bar.
fn fix_style(style: u32) -> u32 {
    style & !(WS_CAPTION.0 | WS_SYSMENU.0 | WS_THICKFRAME.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0)
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
        // These windows are never active, and Windows swaps an inactive window's acrylic for a
        // flat colour. Telling the frame it is active keeps the real backdrop.
        WM_NCACTIVATE => DefSubclassProc(hwnd, msg, WPARAM(1), lparam),
        // The monitor's scale changed, or the window was moved onto one with another scale: the
        // window library resizes it by the scale, and the bars are then laid out again.
        WM_DPICHANGED => {
            let result = DefSubclassProc(hwnd, msg, wparam, lparam);
            crate::host::request_layout();
            result
        }
        crate::appbar::CALLBACK => {
            if wparam.0 == crate::appbar::POS_CHANGED {
                crate::host::request_requery();
            }
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

/// Square corners, for a bar that runs flush along a screen edge.
pub fn square_corners(hwnd: isize) {
    let value = DWMWCP_DONOTROUND.0;
    unsafe {
        let _ = DwmSetWindowAttribute(
            HWND(hwnd as *mut _),
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&raw const value).cast(),
            4,
        );
    }
}

/// This process's window with the given title. egui creates extra windows (the top bar) without
/// handing out their handles, so they are found by title.
pub fn own_window(title: &str) -> Option<isize> {
    struct Search<'a> {
        title: &'a str,
        pid: u32,
        found: Option<isize>,
    }
    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let search = &mut *(lparam.0 as *mut Search);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == search.pid {
            let mut buf = [0u16; 128];
            let n = GetWindowTextW(hwnd, &mut buf).max(0) as usize;
            if String::from_utf16_lossy(&buf[..n]) == search.title {
                search.found = Some(hwnd.0 as isize);
                return BOOL(0);
            }
        }
        BOOL(1)
    }
    let mut search = Search {
        title,
        pid: std::process::id(),
        found: None,
    };
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM((&raw mut search) as isize));
    }
    search.found
}
