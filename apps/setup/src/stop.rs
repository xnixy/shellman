//! Closing shellman's processes for an uninstall, and telling whether shellman is the shell that
//! runs right now (then nothing is closed: the uninstall finishes at the next sign-in).

use crate::plan;
use std::path::Path;
use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId};

/// `(pid, image)` of every process in this logon session that runs a file from `prefix`, except
/// this one.
pub fn running_from(procs: &[(u32, String)], prefix: &Path) -> Vec<(u32, String)> {
    let me = unsafe { GetCurrentProcessId() };
    procs.iter().filter(|(pid, image)| *pid != me && plan::owner(image, prefix).is_some()).cloned().collect()
}

fn is_session(image: &str) -> bool {
    image.to_ascii_lowercase().ends_with("shellman-session.exe")
}

/// Whether Explorer's taskbar is up (then Explorer is the shell that runs now).
/// `SHELLMAN_SETUP_EXPLORER=none` stands in for "no taskbar", for tests.
pub fn explorer_is_shell(procs: &[(u32, String)]) -> bool {
    if std::env::var("SHELLMAN_SETUP_EXPLORER").is_ok_and(|v| v == "none") {
        return false;
    }
    let Ok(tray) = (unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }) else { return false };
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(tray, Some(&mut pid)) };
    procs.iter().any(|(p, image)| *p == pid && image.to_ascii_lowercase().ends_with("\\explorer.exe"))
}

/// Whether shellman is the shell that runs now: its session runs from `prefix` and Explorer has
/// no taskbar. Closing it then would leave the user with no shell at all.
pub fn shellman_is_live_shell(procs: &[(u32, String)], prefix: &Path) -> bool {
    running_from(procs, prefix).iter().any(|(_, i)| is_session(i)) && !explorer_is_shell(procs)
}

/// End the processes: the session first (it would start the others again), then the rest.
/// Returns the ones that are still running.
pub fn close(procs: &[(u32, String)]) -> Vec<(u32, String)> {
    let mut order: Vec<&(u32, String)> = procs.iter().filter(|(_, i)| is_session(i)).collect();
    order.extend(procs.iter().filter(|(_, i)| !is_session(i)));
    let mut left = Vec::new();
    for p in order {
        if !end(p.0) {
            left.push(p.clone());
        }
    }
    left
}

fn end(pid: u32) -> bool {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, pid) else {
            // Gone already, or not ours to end.
            return !crate::sys::processes().iter().any(|(p, _)| *p == pid);
        };
        let _ = TerminateProcess(h, 0);
        let done = WaitForSingleObject(h, 5000) == WAIT_OBJECT_0;
        let _ = CloseHandle(h);
        done
    }
}
