//! Bringing to the front what a shell command opened (the Properties sheet, a dialog). A command
//! run while Files was not the foreground window opens its window behind others, or only flashes
//! its taskbar button; the windows that appear are found and raised once.

use std::collections::HashSet;
use std::time::{Duration, Instant};
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    SetForegroundWindow, ShowWindow, SW_RESTORE,
};

/// The visible top-level windows of this process.
pub(super) fn visible() -> HashSet<isize> {
    unsafe extern "system" fn visit(h: HWND, out: LPARAM) -> BOOL {
        let set = unsafe { &mut *(out.0 as *mut HashSet<isize>) };
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        if pid == unsafe { GetCurrentProcessId() } && unsafe { IsWindowVisible(h) }.as_bool() {
            set.insert(h.0 as isize);
        }
        BOOL(1)
    }
    let mut set = HashSet::new();
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM(&raw mut set as isize));
    }
    set
}

/// Raises any window of this process that is not in `seen` (and adds it), if it is not in front
/// already. Call it repeatedly while a command may still be opening something.
pub(super) fn raise_new(seen: &mut HashSet<isize>) {
    for h in visible() {
        if !seen.insert(h) {
            continue;
        }
        front(HWND(h as *mut _));
    }
}

/// Makes `h` the foreground window, by the means Windows allows a process that just got input.
fn front(h: HWND) {
    unsafe {
        if GetForegroundWindow() == h {
            return;
        }
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        crate::foreground::send_mask();
        let fg = GetForegroundWindow();
        let fg_thread = GetWindowThreadProcessId(fg, None);
        let me = GetCurrentThreadId();
        let attached = fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, true).as_bool();
        let _ = BringWindowToTop(h);
        let _ = SetForegroundWindow(h);
        if attached {
            let _ = AttachThreadInput(me, fg_thread, false);
        }
    }
}

/// For a command run from the calling thread: watches for a few seconds from a helper thread
/// and raises what opens.
pub fn watch_for_windows() {
    let mut seen = visible();
    let _ = std::thread::Builder::new().name("raise-sheet".into()).spawn(move || {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(4) {
            std::thread::sleep(Duration::from_millis(60));
            raise_new(&mut seen);
        }
    });
}
