//! The bar windows' behaviour and outline.
//!
//! Their surfaces are `shellman_ui::glass` panels (kept in `surface`): Windows' own backdrop
//! paints the whole window rectangle and goes flat on a window that is never active, which these
//! never are, while the glass module's composed backdrop blurs at any radius.

pub mod surface;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::Graphics::Dwm::{DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_BORDER_COLOR};
use windows::core::BOOL;
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, GetWindowThreadProcessId};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, GWL_STYLE, STYLESTRUCT,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WM_DPICHANGED, WM_STYLECHANGING,
    WS_CAPTION, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_MAXIMIZEBOX,
    WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
};

/// Clicking the dock must not take focus from the app in front, or "minimize the app in front"
/// would always see the dock instead. Windows' window border (which follows the accent color) is
/// turned off: the glass draws the edge.
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

/// Let the window's own transparent pixels show what is under it (its glass). A window the
/// window library made transparent should already, but a bar window's GL surface showed black
/// there until DWM was told its frame covers the whole window.
pub fn see_through(hwnd: isize) {
    let margins = MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 };
    unsafe {
        let _ = DwmExtendFrameIntoClientArea(HWND(hwnd as *mut _), &margins);
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
