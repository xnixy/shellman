//! The session's one window: hidden, but top-level, because Windows sends the end-of-session
//! messages to top-level windows only (a message-only window never hears them).

use crate::{log, shutdown};
use std::sync::atomic::Ordering;
use windows::core::{w, HSTRING};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, PostQuitMessage, RegisterClassW, CW_USEDEFAULT, WM_CLOSE,
    WM_COPYDATA, WM_DESTROY, WM_ENDSESSION, WM_QUERYENDSESSION, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // Never block a shutdown or logoff: say yes at once.
        WM_QUERYENDSESSION => {
            shutdown::ENDING.store(true, Ordering::SeqCst);
            LRESULT(1)
        }
        WM_ENDSESSION => {
            if wparam.0 != 0 {
                // Must finish before returning: the process may be ended right after.
                shutdown::run("end of session");
            } else {
                shutdown::ENDING.store(false, Ordering::SeqCst);
                log::line("end of session cancelled");
            }
            LRESULT(0)
        }
        // "Restart app X" from shellman-setup (`restart.rs`): answered at once, handled by the supervisor.
        WM_COPYDATA => {
            let cds = lparam.0 as *const COPYDATASTRUCT;
            if cds.is_null() || (*cds).dwData != crate::restart::MAGIC || (*cds).lpData.is_null() || (*cds).cbData > 256 {
                return LRESULT(0);
            }
            let data = std::slice::from_raw_parts((*cds).lpData as *const u8, (*cds).cbData as usize);
            LRESULT(crate::restart::on_request(data, shutdown::quiet()) as isize)
        }
        WM_CLOSE => {
            shutdown::run("closed");
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => {
            crate::transparency::on_message(msg, wparam, lparam);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
    }
}

/// The window's title: tells a test session (`SHELLMAN_INSTANCE`) from the real one, so a request
/// meant for one never reaches the other.
pub fn title(instance: &str) -> String {
    if instance.is_empty() {
        "shellman session".to_string()
    } else {
        format!("shellman session {instance}")
    }
}

pub fn create() -> Option<HWND> {
    unsafe {
        let hinst = GetModuleHandleW(None).ok()?.into();
        let class = w!("ShellmanSession");
        let wc = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: hinst, lpszClassName: class, ..Default::default() };
        RegisterClassW(&wc);
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class,
            &HSTRING::from(title(&std::env::var("SHELLMAN_INSTANCE").unwrap_or_default())),
            WS_POPUP,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            0,
            0,
            None,
            None,
            Some(hinst),
            None,
        )
        .ok()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn title_names_the_instance() {
        assert_eq!(super::title(""), "shellman session");
        assert_eq!(super::title("t1"), "shellman session t1");
    }
}
