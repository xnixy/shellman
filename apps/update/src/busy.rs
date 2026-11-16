//! What a new release must not interrupt, read from the desktop: a fullscreen window in front
//! (`core::winlist::fullscreen`), and file work under way: Windows' copy/move/delete progress
//! dialog (class `OperationStatusWindow`, whoever started it) or a shellman window that says it
//! has file work (`core::fsops::busy`).

use shellman_update_core::schedule::Busy;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, IsWindowVisible};

pub fn now() -> Busy {
    Busy { fullscreen: shellman_core::winlist::fullscreen::foreground_is_fullscreen(), file_work: file_work() }
}

fn file_work() -> bool {
    unsafe extern "system" fn each(h: HWND, l: LPARAM) -> BOOL {
        let found = &mut *(l.0 as *mut bool);
        let mut buf = [0u16; 64];
        let n = GetClassNameW(h, &mut buf) as usize;
        let class = String::from_utf16_lossy(&buf[..n]);
        if (class == "OperationStatusWindow" && IsWindowVisible(h).as_bool()) || !shellman_core::fsops::busy::read(h.0 as isize).is_empty() {
            *found = true;
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut found = false;
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut found as *mut bool as isize));
    }
    found
}
