#![windows_subsystem = "windows"]
//! A throwaway child for testing the session (never part of the shell). Copy it under another
//! name per app, so each has its own exe path. Modes, as arguments:
//!
//! - `wait`: runs until killed.
//! - `exit CODE [MS]`: exits with CODE after MS milliseconds (default 0).
//! - `window`: a hidden window that ends with exit code 0 when it gets WM_CLOSE.
//!
//! Each start is appended to the file named by CHILD_LOG as `<unix ms> <pid> <exe name> <args>`.

use std::io::Write;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostQuitMessage, RegisterClassW, MSG,
    WM_CLOSE, WNDCLASSW, WS_POPUP,
};

unsafe extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if m == WM_CLOSE {
        PostQuitMessage(0);
        return LRESULT(0);
    }
    DefWindowProcW(h, m, w, l)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(path) = std::env::var_os("CHILD_LOG") {
        let name = std::env::current_exe().ok().and_then(|e| e.file_name().map(|n| n.to_string_lossy().into_owned()));
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let session = std::env::var("SHELLMAN_SESSION").unwrap_or_default();
            let _ = writeln!(f, "{ms} {} {} {} session={session}", std::process::id(), name.unwrap_or_default(), args.join(" "));
        }
    }
    match args.first().map(String::as_str) {
        Some("exit") => {
            let code = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            let ms = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
            std::thread::sleep(std::time::Duration::from_millis(ms));
            std::process::exit(code);
        }
        Some("window") => unsafe {
            let class = w!("ShellmanTestChild");
            let wc = WNDCLASSW { lpfnWndProc: Some(proc), lpszClassName: class, ..Default::default() };
            RegisterClassW(&wc);
            let _ = CreateWindowExW(Default::default(), class, w!("child"), WS_POPUP, 0, 0, 0, 0, None, None, None, None);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                DispatchMessageW(&msg);
            }
        },
        _ => loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        },
    }
}
