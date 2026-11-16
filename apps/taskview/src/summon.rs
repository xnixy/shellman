//! Appearing on request, one Task View at a time; the launcher's way (`apps/launcher/src/summon.rs`).
//!
//! The process stays resident with its window out of sight (cloaked). Win+Tab (the switcher's
//! hook, `taskviewkey`) or running `shellman-taskview.exe` again signals it through a named event;
//! it then covers the monitor of the window in front, taking the foreground and the keyboard, or
//! goes away if it was showing. `SHELLMAN_INSTANCE` is added to the names, so a test copy runs
//! beside the real one without the two finding each other.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::Arc;
use windows::core::HSTRING;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::Threading::{CreateEventW, CreateMutexW, SetEvent, WaitForSingleObject, INFINITE};
use shellman_ui::glass;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, GetCursorPos, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, IsIconic,
    IsWindowVisible, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, ASFW_ANY, GWL_EXSTYLE,
    GWL_STYLE, HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNOACTIVATE,
    WS_CAPTION, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
};

pub enum Claim {
    Resident(Instance),
    Signalled,
}

/// The resident Task View's end of the signal.
pub struct Instance {
    event: HANDLE,
    _mutex: HANDLE,
}

/// What the window and the signal thread tell each other.
#[derive(Default)]
pub struct Shared {
    hide_requested: AtomicBool,
    /// Counts showings, so the window notices a new one.
    shown: AtomicU32,
    showing: AtomicBool,
    /// The app window that was in front when it was summoned (0: none), read before Task View
    /// took the foreground.
    front: AtomicIsize,
}

impl Shared {
    pub fn mark_hidden(&self) {
        self.showing.store(false, Ordering::Release);
    }

    pub fn is_showing(&self) -> bool {
        self.showing.load(Ordering::Acquire)
    }

    pub fn take_hide_request(&self) -> bool {
        self.hide_requested.swap(false, Ordering::AcqRel)
    }

    pub fn shown_count(&self) -> u32 {
        self.shown.load(Ordering::Acquire)
    }

    pub fn front(&self) -> Option<isize> {
        Some(self.front.load(Ordering::Acquire)).filter(|&h| h != 0)
    }
}

/// The name of one of Task View's kernel objects. The switcher's hook signals
/// `Local\shellman-taskview-summon<instance>`.
fn name(what: &str) -> HSTRING {
    let instance = std::env::var("SHELLMAN_INSTANCE").unwrap_or_default();
    HSTRING::from(format!("Local\\shellman-taskview-{what}{instance}"))
}

/// Become the resident Task View, or signal the one that is (see the launcher's `claim`).
pub fn claim(background: bool) -> Claim {
    unsafe {
        let Ok(event) = CreateEventW(None, false, false, &name("summon")) else {
            return Claim::Resident(Instance { event: HANDLE::default(), _mutex: HANDLE::default() });
        };
        let mutex = CreateMutexW(None, false, &name("resident"));
        if mutex.is_ok() && GetLastError() == ERROR_ALREADY_EXISTS {
            if !background {
                let _ = AllowSetForegroundWindow(ASFW_ANY);
                let _ = SetEvent(event);
            }
            return Claim::Signalled;
        }
        Claim::Resident(Instance { event, _mutex: mutex.unwrap_or_default() })
    }
}

impl Instance {
    pub fn summon(&self) {
        if !self.event.is_invalid() {
            unsafe {
                let _ = SetEvent(self.event);
            }
        }
    }

    /// Serve summons for `hwnd` from a thread of its own, until the process ends: each one
    /// toggles Task View.
    pub fn serve(&self, hwnd: isize, ctx: egui::Context, shared: Arc<Shared>) {
        if self.event.is_invalid() {
            return;
        }
        let event = self.event.0 as isize;
        std::thread::spawn(move || {
            let event = HANDLE(event as *mut _);
            let hwnd = HWND(hwnd as *mut _);
            while unsafe { WaitForSingleObject(event, INFINITE) }.0 == 0 {
                if shared.showing.load(Ordering::Acquire) {
                    shared.hide_requested.store(true, Ordering::Release);
                } else {
                    // Before showing: once shown, Task View is the window in front.
                    shared.front.store(shellman_core::winlist::foreground().unwrap_or(0), Ordering::Release);
                    shared.showing.store(true, Ordering::Release);
                    shared.shown.fetch_add(1, Ordering::AcqRel);
                    show(hwnd);
                }
                ctx.request_repaint();
            }
        });
    }
}

pub fn is_visible(hwnd: isize) -> bool {
    unsafe { IsWindowVisible(HWND(hwnd as *mut _)) }.as_bool() && !glass::is_cloaked(hwnd)
}

/// Resident and out of sight, and a tool window (never listed as a running app, so never in its
/// own list). Called once, before the window library first shows it.
pub fn prepare(hwnd: isize) {
    glass::cloak(hwnd, true);
    edge_to_edge(hwnd);
    // The frame comes off now, while out of sight, not at the first showing (see `show`).
    let h = HWND(hwnd as *mut _);
    strip_frame(h);
    unsafe {
        let _ = SetWindowPos(h, None, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER);
    }
    tool_window(HWND(hwnd as *mut _));
    shellman_ui::park::shrink(hwnd);
}

/// No Windows corners or border: the overlay runs edge to edge. Again after `paint_frame`, which
/// sets a border colour.
pub fn edge_to_edge(hwnd: isize) {
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND};
    let set = |attribute, value: u32| unsafe {
        let _ = DwmSetWindowAttribute(HWND(hwnd as *mut _), attribute, (&raw const value).cast(), 4);
    };
    set(DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND.0 as u32);
    // DWMWA_COLOR_NONE.
    set(DWMWA_BORDER_COLOR, 0xFFFF_FFFE);
}

/// Out of sight: cloaked, not hidden (a hidden window gets no paint messages, and the window
/// library then waits on a whole core for a frame it asked for). Hiding and showing again
/// without activation hands the keyboard back.
pub fn hide(hwnd: isize) {
    glass::cloak(hwnd, true);
    shellman_ui::park::shrink(hwnd);
    let hwnd = HWND(hwnd as *mut _);
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
}

/// Cover the whole monitor (the top bar and the dock too) and take the keyboard.
fn show(hwnd: HWND) {
    unsafe {
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(front_monitor(), &mut info).as_bool() {
            return;
        }
        // One pixel short of the monitor's height. Covering it exactly, this Windows (26300)
        // drew a light title strip over the top on the first showing; so did taking the frame
        // off at the first showing. Measured on cold starts, the strip was there for most of the
        // first showing with either alone, and gone with both this and `prepare`'s. (It also
        // keeps the overlay off the bar's full-screen check.)
        let mut r = info.rcMonitor;
        r.bottom -= 1;
        let (x, y, w, h) = (r.left, r.top, r.right - r.left, r.bottom - r.top);
        glass::cloak(hwnd.0 as isize, true);
        strip_frame(hwnd);
        tool_window(hwnd);
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_SHOWWINDOW | SWP_FRAMECHANGED);
        // Onto a monitor of another scale the window library resizes the window to keep its size
        // in points, so the place is set once more.
        let mut now = RECT::default();
        if GetWindowRect(hwnd, &mut now).is_ok() && now != r {
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE);
        }
        glass::cloak(hwnd.0 as isize, false);
        take_foreground(hwnd);
    }
}

fn tool_window(hwnd: HWND) {
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let tool = (ex & !WS_EX_APPWINDOW.0) | WS_EX_TOOLWINDOW.0;
        if tool != ex {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, tool as isize);
        }
    }
}

/// The foreground and the keyboard; when refused, one press of a key no keyboard has (which makes
/// this process the last input's sender) and again.
fn take_foreground(hwnd: HWND) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };
    unsafe {
        let _ = SetForegroundWindow(hwnd);
        if GetForegroundWindow() == hwnd {
            return;
        }
        let key = |flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0xE8), dwFlags: flags, ..Default::default() } },
        };
        SendInput(&[key(Default::default()), key(KEYEVENTF_KEYUP)], size_of::<INPUT>() as i32);
        let _ = SetForegroundWindow(hwnd);
    }
}

fn strip_frame(hwnd: HWND) {
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        // A bare pop-up: no caption, sizing frame or system menu.
        let bare = (style & !(WS_CAPTION.0 | WS_SYSMENU.0 | WS_THICKFRAME.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0)) | WS_POPUP.0;
        SetWindowLongPtrW(hwnd, GWL_STYLE, bare as isize);
    }
}

/// The monitor of the window in front, whichever it is (the desktop's too: Win+Tab is a key, so
/// that is where the user is); with nothing in front, or a minimized window, the one under the
/// pointer. A test copy may name a point instead: `SHELLMAN_TEST_AT=x,y` (or `@file`).
unsafe fn front_monitor() -> HMONITOR {
    if let Some(at) = test_point() {
        return MonitorFromPoint(at, MONITOR_DEFAULTTONEAREST);
    }
    let front = GetForegroundWindow();
    if !front.is_invalid() && !IsIconic(front).as_bool() {
        return MonitorFromWindow(front, MONITOR_DEFAULTTONEAREST);
    }
    let mut at = POINT::default();
    let _ = GetCursorPos(&mut at);
    MonitorFromPoint(at, MONITOR_DEFAULTTONEAREST)
}

fn test_point() -> Option<POINT> {
    std::env::var("SHELLMAN_INSTANCE").ok().filter(|i| !i.is_empty())?;
    let mut at = std::env::var("SHELLMAN_TEST_AT").ok()?;
    // `@file`: the point is read from that file at each showing, so a test can move one copy
    // between monitors.
    if let Some(file) = at.strip_prefix('@') {
        at = std::fs::read_to_string(file).ok()?;
    }
    let (x, y) = at.split_once(',')?;
    Some(POINT { x: x.trim().parse().ok()?, y: y.trim().parse().ok()? })
}
