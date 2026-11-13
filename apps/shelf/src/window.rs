//! The shelf windows' place and manners: the process's first window (the primary monitor's
//! shelf, which also hears the desktop) and one per other monitor, all alike.
//!
//! It lives at desktop level: above the wallpaper windows (and Windows' own `Progman`), below
//! every other window, never in Alt+Tab or the dock (a tool window that never activates), and
//! never taking the focus by being shown or clicked. Windows has no "desktop band" for an
//! ordinary process, so the window keeps its place itself: whenever anyone moves it in the
//! z-order (showing it, the window library resizing it), the move is redirected to just above
//! the topmost desktop window. Other windows coming forward never move it, so this costs nothing
//! at rest. While a stack is open the window is raised above everything (`set_raised`), and goes
//! back down when it closes.
//!
//! It also hears what the shelf reacts to without polling: the desktop's request for the menu
//! (a registered message from the desktop process), a check for a fullscreen app in front
//! (posted by the window watcher, and answered for every shelf window, each for its own
//! monitor), and monitor or work-area changes.

use shellman_core::winlist::fullscreen::foreground_covers_monitor_of;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::MutexGuard;
use std::sync::{Mutex, OnceLock};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_BORDER_COLOR};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetTopWindow, GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId,
    IsWindowVisible, PostMessageW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, GWL_STYLE, GW_HWNDNEXT,
    HWND_BOTTOM, HWND_NOTOPMOST, HWND_TOP, HWND_TOPMOST, SPI_SETWORKAREA, STYLESTRUCT, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, WINDOWPOS, WM_APP, WM_DISPLAYCHANGE, WM_DPICHANGED,
    WM_DWMCOMPOSITIONCHANGED, WM_MOUSEACTIVATE, WM_SETTINGCHANGE, WM_STYLECHANGING, WM_WINDOWPOSCHANGING,
    WS_CAPTION, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
    WS_SYSMENU, WS_THICKFRAME,
};

/// Posted by the window watcher: look at the window in front again.
const WM_CHECK: u32 = WM_APP + 1;
/// Classes that make up the desktop under the shelf.
const DESKTOP_CLASSES: [&str; 3] = ["ShellmanDesktop", "Progman", "WorkerW"];
/// The subclass id.
const SUBCLASS: usize = 0x5348_4c46;

static CTX: OnceLock<egui::Context> = OnceLock::new();
/// The first window, which the window watcher's checks are posted to.
static HWND_: AtomicIsize = AtomicIsize::new(0);
/// Every shelf window.
static SHELVES: Mutex<Vec<isize>> = Mutex::new(Vec::new());
/// The shelf windows above everything (a stack of theirs is open).
static RAISED: Mutex<Vec<isize>> = Mutex::new(Vec::new());
/// The shelf windows whose monitor a fullscreen app covers.
static COVERED: Mutex<Vec<isize>> = Mutex::new(Vec::new());
/// The monitors or work areas changed since the window was last placed.
static LAYOUT: AtomicBool = AtomicBool::new(true);
/// Where the desktop asked for its menu, not yet taken.
static MENU_AT: Mutex<Option<(i32, i32)>> = Mutex::new(None);

fn list(m: &'static Mutex<Vec<isize>>) -> MutexGuard<'static, Vec<isize>> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Give `hwnd` (the process's first shelf window, created hidden) its manners. Call once, on
/// the UI thread.
pub fn prepare(hwnd: isize, ctx: &egui::Context) {
    let _ = CTX.set(ctx.clone());
    HWND_.store(hwnd, Ordering::Release);
    prepare_other(hwnd);
}

/// Give another monitor's shelf window the same manners. Call once per window, on the UI thread.
pub fn prepare_other(hwnd: isize) {
    {
        let mut all = list(&SHELVES);
        if !all.contains(&hwnd) {
            all.push(hwnd);
        }
    }
    let h = HWND(hwnd as *mut _);
    unsafe {
        // DWMWA_COLOR_NONE: the glass draws the edge.
        let none: u32 = 0xFFFF_FFFE;
        let _ = DwmSetWindowAttribute(h, DWMWA_BORDER_COLOR, (&raw const none).cast(), 4);
        let _ = SetWindowSubclass(h, Some(proc), SUBCLASS, 0);
        fix_styles(h);
    }
    extend_frame(h);
}

/// The window library re-applies its own styles, so they are enforced as they change too.
pub unsafe fn fix_styles(h: HWND) {
    let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
    SetWindowLongPtrW(h, GWL_EXSTYLE, fix_ex_style(ex) as isize);
    let style = GetWindowLongPtrW(h, GWL_STYLE) as u32;
    SetWindowLongPtrW(h, GWL_STYLE, fix_style(style) as isize);
    let flags = SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE;
    let _ = SetWindowPos(h, None, 0, 0, 0, 0, flags);
}

/// Never on a taskbar, never activated by a click, a tool window (so never in Alt+Tab).
pub fn fix_ex_style(ex: u32) -> u32 {
    (ex & !WS_EX_APPWINDOW.0) | WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0
}

/// No caption, system menu or sizing frame, which Windows would draw ghosts of over glass.
pub fn fix_style(style: u32) -> u32 {
    style & !(WS_CAPTION.0 | WS_SYSMENU.0 | WS_THICKFRAME.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0)
}

/// Let the window's transparent pixels show what is under it (its glass).
pub fn extend_frame(h: HWND) {
    let margins = MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 };
    unsafe {
        let _ = DwmExtendFrameIntoClientArea(h, &margins);
    }
}

/// Put the window exactly on `rect` (physical pixels), at desktop level. Moving onto a monitor
/// with another scale makes the window library resize it by the ratio, so the rectangle is
/// checked after the move and set once more.
pub fn place(hwnd: isize, rect: RECT) {
    let h = HWND(hwnd as *mut _);
    for _ in 0..2 {
        let mut now = RECT::default();
        if unsafe { GetWindowRect(h, &mut now) }.is_ok() && now == rect {
            return;
        }
        unsafe {
            let flags = SWP_NOACTIVATE | SWP_NOOWNERZORDER;
            let _ = SetWindowPos(h, None, rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top, flags | SWP_NOZORDER);
        }
    }
}

/// A shelf window is about to close (its monitor went away): forget it.
pub fn forget(hwnd: isize) {
    for l in [&SHELVES, &RAISED, &COVERED] {
        list(l).retain(|h| *h != hwnd);
    }
    unsafe {
        let _ = RemoveWindowSubclass(HWND(hwnd as *mut _), Some(proc), SUBCLASS);
    }
}

fn raised(hwnd: isize) -> bool {
    list(&RAISED).contains(&hwnd)
}

/// Raise the window above everything (a stack is open), or put it back at desktop level.
pub fn set_raised(hwnd: isize, on: bool) {
    // The lock is let go before the window moves: the move comes back through `proc`.
    {
        let mut up = list(&RAISED);
        if up.contains(&hwnd) == on {
            return;
        }
        if on {
            up.push(hwnd);
        } else {
            up.retain(|h| *h != hwnd);
        }
    }
    let h = HWND(hwnd as *mut _);
    let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER;
    unsafe {
        if on {
            let _ = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
        } else {
            let _ = SetWindowPos(h, Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags);
            // The redirect in `proc` takes it on down to the desktop.
            let _ = SetWindowPos(h, Some(HWND_TOP), 0, 0, 0, 0, flags);
        }
    }
}

/// Take the desktop's latest request for its menu.
pub fn take_menu() -> Option<(i32, i32)> {
    MENU_AT.lock().ok()?.take()
}

/// Ask for the menu as if the desktop had (for `--menu` at start).
pub fn ask_menu(at: (i32, i32)) {
    if let Ok(mut m) = MENU_AT.lock() {
        *m = Some(at);
    }
}

/// Whether a fullscreen app covers the monitor of the shelf window `hwnd`.
pub fn covered(hwnd: isize) -> bool {
    list(&COVERED).contains(&hwnd)
}

/// Look at the window in front for every shelf window; true when that changed for any.
fn check_all() -> bool {
    let all = list(&SHELVES).clone();
    let now: Vec<isize> = all.into_iter().filter(|h| foreground_covers_monitor_of(*h)).collect();
    let mut covered = list(&COVERED);
    let changed = *covered != now;
    *covered = now;
    changed
}

/// Have the windows placed again (the settings moved or resized the shelf).
pub fn ask_layout() {
    LAYOUT.store(true, Ordering::Release);
}

/// True once after the monitors or work areas changed (and at the start).
pub fn take_layout() -> bool {
    LAYOUT.swap(false, Ordering::AcqRel)
}

/// Ask for a fullscreen check; safe from any thread (the window watcher calls it).
pub fn request_check() {
    let h = HWND_.load(Ordering::Acquire);
    if h != 0 {
        unsafe {
            let _ = PostMessageW(Some(HWND(h as *mut _)), WM_CHECK, WPARAM(0), LPARAM(0));
        }
    }
}

fn repaint() {
    if let Some(ctx) = CTX.get() {
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

fn class_of(h: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(h, &mut buf) }.max(0) as usize;
    String::from_utf16_lossy(&buf[..n])
}

fn ours(h: HWND) -> bool {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    pid == std::process::id()
}

/// What to put the shelf under to sit right above the desktop: the window just above the
/// topmost visible desktop window (shellman's wallpaper, Windows' `Progman`), not counting the
/// shelf's own windows. `HWND_TOP` when that is an always-on-top window or there is none,
/// `HWND_BOTTOM` when there is no desktop window.
///
/// Walked from the top down: other processes keep windows of their own under the wallpaper
/// (an "Open With" helper was found there), so the first non-desktop window from the bottom
/// is not the place.
fn desktop_slot() -> HWND {
    unsafe {
        let mut w = GetTopWindow(None).ok();
        let mut above: Option<HWND> = None;
        // A bounded walk: the z-order may change under it.
        for _ in 0..8192 {
            let Some(h) = w else { break };
            if !ours(h) {
                if IsWindowVisible(h).as_bool() && DESKTOP_CLASSES.contains(&class_of(h).as_str()) {
                    return match above {
                        Some(a) if GetWindowLongPtrW(a, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 == 0 => a,
                        _ => HWND_TOP,
                    };
                }
                above = Some(h);
            }
            w = GetWindow(h, GW_HWNDNEXT).ok();
        }
        // No desktop at all: the very bottom is the next best place.
        HWND_BOTTOM
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, _data: usize) -> LRESULT {
    const MA_NOACTIVATE: isize = 3;
    match msg {
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE),
        WM_WINDOWPOSCHANGING => {
            let pos = &mut *(lparam.0 as *mut WINDOWPOS);
            if pos.flags & SWP_NOZORDER == Default::default() && !raised(hwnd.0 as isize) {
                pos.hwndInsertAfter = desktop_slot();
            }
        }
        WM_STYLECHANGING if wparam.0 as i32 == GWL_EXSTYLE.0 => {
            let change = &mut *(lparam.0 as *mut STYLESTRUCT);
            change.styleNew = fix_ex_style(change.styleNew);
        }
        WM_STYLECHANGING if wparam.0 as i32 == GWL_STYLE.0 => {
            let change = &mut *(lparam.0 as *mut STYLESTRUCT);
            change.styleNew = fix_style(change.styleNew);
        }
        WM_CHECK => {
            if check_all() {
                repaint();
            }
            return LRESULT(0);
        }
        WM_DISPLAYCHANGE => {
            LAYOUT.store(true, Ordering::Release);
            repaint();
        }
        WM_SETTINGCHANGE if wparam.0 == SPI_SETWORKAREA.0 as usize => {
            LAYOUT.store(true, Ordering::Release);
            repaint();
        }
        WM_DPICHANGED => {
            let r = DefSubclassProc(hwnd, msg, wparam, lparam);
            LAYOUT.store(true, Ordering::Release);
            repaint();
            return r;
        }
        WM_DWMCOMPOSITIONCHANGED => extend_frame(hwnd),
        m if m != 0 && m == shellman_core::desktop_menu::message() => {
            if let Ok(mut at) = MENU_AT.lock() {
                *at = Some(shellman_core::desktop_menu::unpack(wparam, lparam));
            }
            repaint();
            return LRESULT(0);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_keep_it_out_of_the_way() {
        let ex = fix_ex_style(WS_EX_APPWINDOW.0);
        assert_eq!(ex & WS_EX_APPWINDOW.0, 0);
        assert_ne!(ex & WS_EX_NOACTIVATE.0, 0);
        assert_ne!(ex & WS_EX_TOOLWINDOW.0, 0);
        assert_eq!(fix_style(WS_CAPTION.0 | WS_THICKFRAME.0 | 0x1) & !0x1, 0);
    }
}
