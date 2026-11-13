//! The Win32 side of the menu window: finding it, keeping it a borderless tool window, putting
//! it on a rectangle of physical pixels, showing it with the keyboard focus and hiding it.

use super::place::Px;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, RedrawWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST, RDW_INTERNALPAINT,
};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId,
    SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE,
    GWL_STYLE, HWND_TOPMOST, STYLESTRUCT, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER,
    SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNOACTIVATE, WM_DWMCOMPOSITIONCHANGED, WM_NCACTIVATE, WM_NCPAINT, WM_STYLECHANGING, WS_CAPTION,
    WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
};

fn h(hwnd: isize) -> HWND {
    HWND(hwnd as *mut _)
}

/// This process's window with the given title (egui does not hand out its windows' handles).
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
            let mut buf = [0u16; 64];
            let n = GetWindowTextW(hwnd, &mut buf).max(0) as usize;
            if String::from_utf16_lossy(&buf[..n]) == search.title {
                search.found = Some(hwnd.0 as isize);
                return BOOL(0);
            }
        }
        BOOL(1)
    }
    let mut search = Search { title, pid: std::process::id(), found: None };
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM((&raw mut search) as isize));
    }
    search.found
}

/// A tool window (never in Alt+Tab, the dock or the window list) without caption or frame, kept
/// so in a subclass (the window library puts its styles back now and then), whose transparent
/// pixels show the glass under it.
pub fn prepare(hwnd: isize) {
    let hwnd = h(hwnd);
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(menu_proc), SUBCLASS, 0);
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, fix_ex(ex) as isize);
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        SetWindowLongPtrW(hwnd, GWL_STYLE, fix_style(style) as isize);
        let flags = SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE;
        let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, flags);
    }
    extend_frame(hwnd);
}

const SUBCLASS: usize = 0x6d65_6e75;

fn fix_ex(ex: u32) -> u32 {
    (ex & !WS_EX_APPWINDOW.0) | WS_EX_TOOLWINDOW.0
}

/// A popup: an overlapped window (the window library's kind) always gets a caption, which DWM
/// then draws over the glass when it is active, title and all (seen at the first opens).
fn fix_style(style: u32) -> u32 {
    (style & !(WS_CAPTION.0 | WS_SYSMENU.0 | WS_THICKFRAME.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0)) | WS_POPUP.0
}

/// Tell DWM the frame covers the whole window, so its transparent pixels are see-through. DWM
/// forgets this when it restarts, so it is said again then.
fn extend_frame(hwnd: HWND) {
    let margins = MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 };
    unsafe {
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
    }
}

unsafe extern "system" fn menu_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        WM_STYLECHANGING if wparam.0 as i32 == GWL_EXSTYLE.0 => {
            let change = &mut *(lparam.0 as *mut STYLESTRUCT);
            change.styleNew = fix_ex(change.styleNew);
        }
        WM_STYLECHANGING if wparam.0 as i32 == GWL_STYLE.0 => {
            let change = &mut *(lparam.0 as *mut STYLESTRUCT);
            change.styleNew = fix_style(change.styleNew);
        }
        WM_DWMCOMPOSITIONCHANGED => extend_frame(hwnd),
        // Windows paints a caption (a tool window's, with the title) into a window whose frame is
        // extended over all of it when it becomes active, even without a caption style (seen at
        // the first opens): the window has no frame to paint, so none is painted.
        WM_NCACTIVATE => return LRESULT(1),
        WM_NCPAINT => return LRESULT(0),
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

/// The work area and scale (pixels per point) of the monitor at a point.
pub fn monitor_at(x: i32, y: i32) -> Option<(Px, f32)> {
    unsafe {
        let m = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(m, &mut info).as_bool() {
            return None;
        }
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let w = info.rcWork;
        Some((Px { left: w.left, top: w.top, right: w.right, bottom: w.bottom }, dx.max(96) as f32 / 96.0))
    }
}

pub fn rect_of(hwnd: isize) -> Option<Px> {
    let mut r = RECT::default();
    unsafe { GetWindowRect(h(hwnd), &mut r) }.ok()?;
    Some(Px { left: r.left, top: r.top, right: r.right, bottom: r.bottom })
}

/// Move and size the window to `r`, topmost, without activating it. Landing on a monitor with
/// another scale makes the window library resize it by the ratio of the scales, so the rectangle
/// is checked and set once more. Never changes whether it is shown.
pub fn put(hwnd: isize, r: Px) {
    for _ in 0..2 {
        if rect_of(hwnd) == Some(r) {
            return;
        }
        unsafe {
            let _ = SetWindowPos(
                h(hwnd),
                Some(HWND_TOPMOST),
                r.left,
                r.top,
                r.width(),
                r.height(),
                SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
        }
    }
}

/// Show the (already placed and sized) window on top and give it the keyboard focus, for Esc
/// and the arrows. Windows lets the process that got the click do so; if it refuses, one press
/// of an unused key makes this process the last input's sender and it is asked again.
pub fn show_focused(hwnd: isize) {
    let hwnd = h(hwnd);
    unsafe {
        let flags = SWP_SHOWWINDOW | SWP_NOMOVE | SWP_NOSIZE | SWP_NOOWNERZORDER;
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
        let _ = SetForegroundWindow(hwnd);
        if GetForegroundWindow() != hwnd {
            shellman_core::foreground::send_mask();
            let _ = SetForegroundWindow(hwnd);
        }
        // A frame asked for while it was hidden was dropped; ask again.
        let _ = RedrawWindow(Some(hwnd), None, None, RDW_INTERNALPAINT);
    }
}

/// Out of sight without being hidden: Windows sends a hidden window no paint messages, and the
/// window library then retries a frame asked for meanwhile without pause (the Control Center
/// found a core spinning so). So the window is cloaked (with its glass) by the caller, then
/// hidden, which hands the activation on, and shown again without activation. It keeps its size.
pub fn park(hwnd: isize) {
    unsafe {
        let _ = ShowWindow(h(hwnd), SW_HIDE);
        let _ = ShowWindow(h(hwnd), SW_SHOWNOACTIVATE);
    }
}

pub fn foreground() -> isize {
    unsafe { GetForegroundWindow().0 as isize }
}

pub fn set_foreground(hwnd: isize) {
    unsafe {
        let _ = SetForegroundWindow(h(hwnd));
    }
}
