//! Bars step aside for fullscreen apps: when the window in front covers the bars' monitor, both
//! bar windows are hidden, and they come back when it stops.
//!
//! The reserved space (app bar or work area) is left alone: a fullscreen window ignores it, and
//! changing it would make every other window jump. Hiding rather than lowering also keeps the
//! bars out of a game's way.
//!
//! No polling: the window watcher's events ask for a check. A hidden window gets no repaints, so
//! the check is not made in a frame but by a message posted to the dock window, which its
//! subclass (`shape.rs`) hands to `check` on the UI thread. That is what brings the bars back.

use shellman_core::winlist::fullscreen::foreground_covers_monitor_of;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, ShowWindow, SW_HIDE, SW_SHOWNOACTIVATE, WM_APP};

/// Posted to the dock window: look at the front window again.
pub const WM_CHECK: u32 = WM_APP + 1;

static DOCK: AtomicIsize = AtomicIsize::new(0);
static TOP: AtomicIsize = AtomicIsize::new(0);
/// The bars are hidden because a fullscreen app is in front.
static ASIDE: AtomicBool = AtomicBool::new(false);
/// A check is already posted and not yet run; a burst of events needs only one.
static PENDING: AtomicBool = AtomicBool::new(false);

/// Remember the dock window (whose monitor is the bars' monitor) and check once.
pub fn start(dock: isize) {
    DOCK.store(dock, Ordering::Release);
    request_check();
}

/// The top bar's window exists now (it is created a frame or two after the dock's).
pub fn set_top(top: isize) {
    if TOP.swap(top, Ordering::AcqRel) != top && ASIDE.load(Ordering::Acquire) {
        show(top, false);
    }
}

/// Ask for a check. Safe from any thread, so the window watcher can call it.
pub fn request_check() {
    let dock = DOCK.load(Ordering::Acquire);
    if dock != 0 && !PENDING.swap(true, Ordering::AcqRel) {
        let posted = unsafe { PostMessageW(Some(HWND(dock as *mut _)), WM_CHECK, WPARAM(0), LPARAM(0)) };
        if posted.is_err() {
            PENDING.store(false, Ordering::Release);
        }
    }
}

/// Decide and apply. Runs on the UI thread, from the dock window's message handler.
pub fn check() {
    PENDING.store(false, Ordering::Release);
    let dock = DOCK.load(Ordering::Acquire);
    let fullscreen = foreground_covers_monitor_of(dock);
    if ASIDE.swap(fullscreen, Ordering::AcqRel) != fullscreen {
        show(dock, !fullscreen);
        let top = TOP.load(Ordering::Acquire);
        if top != 0 {
            show(top, !fullscreen);
        }
    }
}

fn show(hwnd: isize, visible: bool) {
    unsafe {
        let _ = ShowWindow(HWND(hwnd as *mut _), if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
    }
}
