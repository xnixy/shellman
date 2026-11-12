//! Bringing the chosen window to front.
//!
//! Windows only lets a process take the foreground in narrow cases (it received the last input,
//! or it owns the window in front), and the switcher's keys were swallowed before any app saw
//! them, so a plain request can be refused; the window then only flashes on the taskbar. When it
//! is, the switcher asks the way Windows' own Alt+Tab does, and as a last resort briefly shares
//! input state with the window in front, which makes it a foreground request from that thread.

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow,
    SetForegroundWindow, ShowWindowAsync, SwitchToThisWindow, GA_ROOTOWNER, SW_RESTORE,
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
        if SetForegroundWindow(hwnd).as_bool() && in_front(hwnd) {
            return;
        }
        // The function Alt+Tab is documented for; it also restores a minimized window.
        SwitchToThisWindow(hwnd, true);
        if in_front(hwnd) {
            return;
        }
        let front = GetForegroundWindow();
        let front_thread = GetWindowThreadProcessId(front, None);
        let me = GetCurrentThreadId();
        if front_thread != 0 && front_thread != me && AttachThreadInput(me, front_thread, true).as_bool() {
            let _ = SetForegroundWindow(hwnd);
            let _ = AttachThreadInput(me, front_thread, false);
        }
    }
}

/// Whether `hwnd` (or a window it owns, such as its dialog) is in front.
fn in_front(hwnd: HWND) -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        !fg.is_invalid() && (fg == hwnd || GetAncestor(fg, GA_ROOTOWNER) == hwnd)
    }
}
