//! The asking app, as Windows sees it: its window, which the picker is centred over and owned
//! by, and its process, which the picker watches so it never outlives it.

use crate::placement::Rect;
use windows::Win32::Foundation::{CloseHandle, HWND, RECT, WAIT_TIMEOUT};
use windows::Win32::System::Threading::{
    OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, GetWindowRect, IsIconic, IsWindow, SetWindowLongPtrW,
    GWLP_HWNDPARENT,
};

fn hwnd(h: i64) -> HWND {
    HWND(h as isize as *mut _)
}

/// The owner's rectangle in pixels, when it is a real window that is not minimized.
pub fn owner_rect(owner: i64) -> Option<Rect> {
    if owner == 0 {
        return None;
    }
    let w = hwnd(owner);
    unsafe {
        if !IsWindow(Some(w)).as_bool() || IsIconic(w).as_bool() {
            return None;
        }
        let mut r = RECT::default();
        GetWindowRect(w, &mut r).ok()?;
        Some(Rect { left: r.left, top: r.top, right: r.right, bottom: r.bottom })
    }
}

/// Make `owner` own the picker window: it then stays above the app's window, is minimized with
/// it, and has no taskbar button of its own, as a dialog does. (The app's side disables its
/// window meanwhile, which makes the picker modal from the user's point of view.)
pub fn set_owner(picker: isize, owner: i64) {
    if owner == 0 || picker == 0 {
        return;
    }
    unsafe {
        if IsWindow(Some(hwnd(owner))).as_bool() {
            SetWindowLongPtrW(HWND(picker as *mut _), GWLP_HWNDPARENT, owner as isize);
        }
    }
}

/// Let the asking process bring its window to the front as the picker closes, so the user lands
/// back where they were. (Its window is still disabled at that moment, so Windows would
/// otherwise activate some other window; only the foreground process may hand this right on.)
pub fn let_host_take_focus(pid: u32) {
    if pid != 0 {
        unsafe {
            let _ = AllowSetForegroundWindow(pid);
        }
    }
}

/// Whether the process `pid` is still running. True when it cannot be told (no rights to ask),
/// so a picker is never closed by mistake.
pub fn alive(pid: u32) -> bool {
    if pid == 0 {
        return true;
    }
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) else {
            // Gone, or not ours to ask about. A pid that no longer exists fails here too, so
            // tell the two apart by the error: access denied means it is there.
            return windows::core::Error::from_win32().code()
                == windows::Win32::Foundation::E_ACCESSDENIED;
        };
        let running = WaitForSingleObject(handle, 0) == WAIT_TIMEOUT;
        let _ = CloseHandle(handle);
        running
    }
}
