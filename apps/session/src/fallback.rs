//! The Win32 half of the safety net: is the tray window up, is Shift held, is Explorer running,
//! and starting Explorer.

use crate::{find, log, proc::MARK};
use std::path::PathBuf;
use windows::core::w;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

/// Replaces explorer.exe, for tests only (a harmless program standing in for it).
pub const ENV: &str = "SHELLMAN_SESSION_FALLBACK";

pub fn shift_held() -> bool {
    // Reads the key state; it sends no input.
    unsafe { GetAsyncKeyState(VK_SHIFT.0 as i32) < 0 }
}

pub fn tray_up() -> bool {
    unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }.is_ok_and(|h| !h.is_invalid())
}

fn explorer_exe() -> PathBuf {
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\Windows"));
    root.join("explorer.exe")
}

/// Start Explorer as the shell, unless one runs already. In a test (the app list overridden) and
/// without `SHELLMAN_SESSION_FALLBACK`, only logs what it would have done.
pub fn start_explorer(test: bool) {
    let exe = match std::env::var_os(ENV).filter(|v| !v.is_empty()) {
        Some(p) => PathBuf::from(p),
        None if test => {
            log::line("fallback: test run, would start explorer.exe here");
            return;
        }
        None => explorer_exe(),
    };
    if !find::by_exe(&exe).is_empty() {
        log::line("fallback: it already runs, nothing to start");
        return;
    }
    // Without the mark, so Explorer and what it starts do not think a session owns them.
    match std::process::Command::new(&exe).env_remove(MARK).spawn() {
        Ok(c) => log::line(&format!("fallback: started {} as pid {}", exe.display(), c.id())),
        Err(e) => log::line(&format!("fallback: could not start {}: {e}", exe.display())),
    }
}
