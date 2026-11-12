//! Ending the session in order: ask every child to close, give them a moment, then go. Never
//! kills anything: a child that is slow is left to Windows, which is what would happen anyway.

use crate::{log, proc::Proc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Foundation::{HANDLE, WAIT_TIMEOUT};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForMultipleObjects, WaitForSingleObject};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, PostMessageW, WM_CLOSE,
};

/// How long children get to close.
const GRACE_MS: u32 = 4000;

/// Set once the session is ending for good: nothing is restarted any more.
pub static STOPPING: AtomicBool = AtomicBool::new(false);
/// Set from WM_QUERYENDSESSION: Windows is about to end the session, so children that exit now
/// are not crashes. Cleared again if the end is cancelled.
pub static ENDING: AtomicBool = AtomicBool::new(false);

static PIDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// The supervisor keeps this list current: the processes to close on the way out.
pub fn publish(pids: Vec<u32>) {
    if let Ok(mut p) = PIDS.lock() {
        *p = pids;
    }
}

pub fn quiet() -> bool {
    STOPPING.load(Ordering::SeqCst) || ENDING.load(Ordering::SeqCst)
}

struct Ctx<'a> {
    pids: &'a [u32],
    posted: usize,
}

unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam.0 as *mut Ctx);
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if ctx.pids.contains(&pid) && PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)).is_ok() {
        ctx.posted += 1;
    }
    BOOL(1)
}

/// WM_CLOSE to every top-level window of these processes (hidden and cloaked ones too: the bar's
/// real windows are viewports of a cloaked host). Returns how many were asked.
pub fn close_windows(pids: &[u32]) -> usize {
    let mut ctx = Ctx { pids, posted: 0 };
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut ctx as *mut Ctx as isize));
    }
    ctx.posted
}

/// Signalled when the session is ending, so a thread that sleeps (the startup items) wakes at once.
fn stop_event() -> HANDLE {
    static EVENT: std::sync::OnceLock<isize> = std::sync::OnceLock::new();
    HANDLE(*EVENT.get_or_init(|| unsafe { CreateEventW(None, true, false, None).map_or(0, |h| h.0 as isize) }) as *mut _)
}

/// Sleep for `d`; false when the session ended before that.
pub fn sleep(d: std::time::Duration) -> bool {
    if STOPPING.load(Ordering::SeqCst) {
        return false;
    }
    unsafe { WaitForSingleObject(stop_event(), d.as_millis().min(u32::MAX as u128 - 1) as u32) == WAIT_TIMEOUT }
}

pub fn stop() {
    STOPPING.store(true, Ordering::SeqCst);
    unsafe {
        let _ = SetEvent(stop_event());
    }
}

pub fn run(why: &str) {
    stop();
    let pids = PIDS.lock().map(|p| p.clone()).unwrap_or_default();
    let posted = close_windows(&pids);
    log::line(&format!("{why}: closing {} apps ({posted} windows asked)", pids.len()));
    let procs: Vec<Proc> = pids.iter().filter_map(|&p| Proc::open(p)).collect();
    if !procs.is_empty() {
        let handles: Vec<_> = procs.iter().map(|p| p.handle).take(64).collect();
        unsafe {
            WaitForMultipleObjects(&handles, true, GRACE_MS);
        }
    }
    let left: Vec<u32> = procs.iter().filter(|p| p.exit_code().is_none()).map(|p| p.pid).collect();
    log::line(&format!("{why}: done, still running: {left:?}"));
}
