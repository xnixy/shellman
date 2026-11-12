//! Bringing the chosen window to front.
//!
//! Windows only lets a process take the foreground in narrow cases, chiefly when it sent the last
//! input. The switcher's keys were swallowed before any app saw them, so its first request is
//! usually refused (the window would only flash on the taskbar). It then sends one press of a key
//! no keyboard has and no app uses, which makes it the last input's sender, and asks again.
//! Measured: the first request refused, the second granted, every time.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetForegroundWindow, IsIconic, IsWindow, SetForegroundWindow, ShowWindowAsync,
    SwitchToThisWindow, GA_ROOTOWNER, SW_RESTORE,
};

/// Restore `hwnd` if it is minimized, and bring it to front. Never waits on the window's app,
/// so a hung app can't hang the switcher.
pub fn bring_to_front(hwnd: isize) {
    let hwnd = HWND(hwnd as *mut _);
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return;
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindowAsync(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
        if in_front(hwnd) {
            return;
        }
        crate::keys::send_mask();
        let _ = SetForegroundWindow(hwnd);
        if in_front(hwnd) {
            return;
        }
        // Last, the call Windows keeps for switchers (it, too, may be refused).
        SwitchToThisWindow(hwnd, true);
    }
}

/// Whether `hwnd` (or a window it owns, such as its dialog) is in front.
fn in_front(hwnd: HWND) -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        !fg.is_invalid() && (fg == hwnd || GetAncestor(fg, GA_ROOTOWNER) == hwnd)
    }
}
