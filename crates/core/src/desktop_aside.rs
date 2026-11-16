//! Telling shellman's wallpaper to step aside for Explorer's desktop, and to come back.
//!
//! When the session's safety net hands over to Explorer, the desktop process keeps running (it
//! is left alone like every app), and its wallpaper windows, one per monitor and as large as
//! it, lay over Explorer's desktop and taskbar: on 13 Nov Explorer's taskbars on two of three
//! monitors were not topmost and sat under them in the z-order, so nothing on them could be
//! clicked, until the wallpaper windows were hidden. So the session tells the desktop, with a
//! registered message to its controller window: `wparam` 1 hides the wallpaper windows (and
//! keeps new ones hidden), 0 shows them again.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, PostMessageW, RegisterWindowMessageW};

/// The class of the desktop process's controller window (one per desktop process).
pub const CONTROLLER_CLASS: PCWSTR = w!("ShellmanDesktopController");

/// The message the controller gets; `wparam` is 1 to step aside, 0 to come back.
pub fn message() -> u32 {
    static MESSAGE: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(w!("shellman-desktop-aside")) })
}

/// Tell every running desktop process. How many were told.
pub fn tell(aside: bool) -> usize {
    let mut told = 0;
    let mut after: Option<HWND> = None;
    // A handful at most (the real one, a frames-only copy, test copies); bounded all the same.
    for _ in 0..16 {
        let Ok(hwnd) = (unsafe { FindWindowExW(None, after, CONTROLLER_CLASS, None) }) else { break };
        if hwnd.is_invalid() {
            break;
        }
        if unsafe { PostMessageW(Some(hwnd), message(), WPARAM(aside as usize), LPARAM(0)) }.is_ok() {
            told += 1;
        }
        after = Some(hwnd);
    }
    told
}
