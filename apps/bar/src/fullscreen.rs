//! Bars step aside for fullscreen apps, monitor by monitor: when the window in front covers a
//! monitor, that monitor's bars are hidden, and they come back when it stops (it leaves
//! fullscreen, closes, or another window comes to the front). Bars on other monitors stay: for
//! them the window in front is on another monitor, so it covers nothing of theirs.
//!
//! The reserved space (app bar or work area) is left alone: a fullscreen window ignores it, and
//! changing it would make every other window jump. Hiding rather than lowering also keeps the
//! bars out of a game's way.
//!
//! No polling: the window watcher's events ask for a check. A game often shows an ordinary window
//! first and then takes the whole screen by dropping its frame, without moving or resizing, which
//! no event reports; so a change in front is followed by a few more checks over the next five
//! seconds ([`FOLLOW_UP_MS`]), and then none until the next change. A hidden window gets no repaints, so
//! the check is not made in a frame but by a message posted to the bar's host window (which is
//! never hidden), whose handler calls `check` on the UI thread. That is what brings bars back.
//!
//! Visibility is decided from scratch on each check (each bar window should be shown unless its
//! monitor is covered), so newly created bars and monitors need no state carried over.

use shellman_core::winlist::fullscreen::foreground_covers_monitor_of;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};
use std::sync::Mutex;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    IsWindowVisible, KillTimer, PostMessageW, SetTimer, ShowWindow, SW_HIDE, SW_SHOWNOACTIVATE, WM_APP,
};

/// Posted to the host window: look at the front window again.
pub const WM_CHECK: u32 = WM_APP + 1;

static HOST: AtomicIsize = AtomicIsize::new(0);
/// Each monitor's bar windows; the first one (its top bar) stands for the monitor.
static GROUPS: Mutex<Vec<Vec<isize>>> = Mutex::new(Vec::new());
/// A check is already posted and not yet run; a burst of events needs only one.
static PENDING: AtomicBool = AtomicBool::new(false);
/// The host window's timer for the follow-up checks.
pub const FOLLOW_UP_TIMER: usize = 0x4655;
/// When the follow-up checks come, in milliseconds after a change in front.
pub const FOLLOW_UP_MS: [u32; 4] = [300, 1000, 2500, 5000];
/// The next follow-up check, as an index into [`FOLLOW_UP_MS`]; past the end: none due.
static NEXT: AtomicUsize = AtomicUsize::new(usize::MAX);

/// Remember the window that receives check requests.
pub fn start(host: isize) {
    HOST.store(host, Ordering::Release);
}

/// The bar windows now, one list per monitor, each placed and ready to show. Applied at once.
pub fn set_groups(groups: Vec<Vec<isize>>) {
    *GROUPS.lock().unwrap() = groups;
    check();
}

/// Ask for a check. Safe from any thread, so the window watcher can call it.
pub fn request_check() {
    let host = HOST.load(Ordering::Acquire);
    if host != 0 && !PENDING.swap(true, Ordering::AcqRel) {
        let posted = unsafe { PostMessageW(Some(HWND(host as *mut _)), WM_CHECK, WPARAM(0), LPARAM(0)) };
        if posted.is_err() {
            PENDING.store(false, Ordering::Release);
        }
    }
}

/// The window in front changed or moved: check now, and a few more times shortly (see the
/// module's notes). Runs on the UI thread, from the posted check.
fn follow_up() {
    let host = HOST.load(Ordering::Acquire);
    if host == 0 {
        return;
    }
    NEXT.store(0, Ordering::Release);
    unsafe { SetTimer(Some(HWND(host as *mut _)), FOLLOW_UP_TIMER, FOLLOW_UP_MS[0], None) };
}

/// The follow-up timer fired: check, and set the next one or stop.
pub fn on_timer() {
    let host = HWND(HOST.load(Ordering::Acquire) as *mut _);
    let i = NEXT.load(Ordering::Acquire);
    unsafe {
        let _ = KillTimer(Some(host), FOLLOW_UP_TIMER);
    }
    decide();
    if let Some(&at) = FOLLOW_UP_MS.get(i + 1) {
        NEXT.store(i + 1, Ordering::Release);
        unsafe { SetTimer(Some(host), FOLLOW_UP_TIMER, at - FOLLOW_UP_MS[i], None) };
    } else {
        NEXT.store(usize::MAX, Ordering::Release);
    }
}

/// Decide and apply, for every monitor, and look again shortly. Runs on the UI thread.
pub fn check() {
    PENDING.store(false, Ordering::Release);
    decide();
    follow_up();
}

/// Decide and apply, for every monitor.
fn decide() {
    // Copied out so no lock is held while windows are shown (which sends them messages).
    let groups = GROUPS.lock().unwrap().clone();
    for group in groups {
        let Some(&probe) = group.first() else { continue };
        let visible = !foreground_covers_monitor_of(probe);
        for hwnd in group {
            show(hwnd, visible);
        }
    }
    // The preview panel is not part of a monitor's group (it comes and goes); it steps aside too.
    crate::preview::step_aside();
}

fn show(hwnd: isize, visible: bool) {
    let hwnd = HWND(hwnd as *mut _);
    unsafe {
        if IsWindowVisible(hwnd).as_bool() != visible {
            let _ = ShowWindow(hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
    }
}
