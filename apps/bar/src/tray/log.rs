//! Optional trace of tray traffic: set `SHELLMAN_TRAY_LOG=<file>` before starting the bar and
//! every icon change and click is appended there, with the owner's process name, icon version,
//! what was sent, and whether the owner runs elevated (which blocks a medium-integrity bar).

use std::io::Write;
use std::sync::OnceLock;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{
    OpenProcess, OpenProcessToken, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

fn path() -> Option<&'static str> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(|| std::env::var("SHELLMAN_TRAY_LOG").ok().filter(|p| !p.is_empty()))
        .as_deref()
}

pub fn enabled() -> bool {
    path().is_some()
}

pub fn line(text: &str) {
    let Some(path) = path() else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{text}");
    }
}

pub fn process_name(pid: u32) -> String {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return "?".into();
        };
        let mut buf = [0u16; 520];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        if ok.is_err() {
            return "?".into();
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        full.rsplit('\\').next().unwrap_or(&full).to_string()
    }
}

/// "yes", "no", or "unknown" when the process cannot be inspected (often a protected one).
pub fn elevated(pid: u32) -> &'static str {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return "unknown";
        };
        let mut token = HANDLE::default();
        let mut answer = "unknown";
        if OpenProcessToken(h, TOKEN_QUERY, &mut token).is_ok() {
            let mut e = TOKEN_ELEVATION::default();
            let mut got = 0u32;
            if GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut e as *mut _ as *mut _),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut got,
            )
            .is_ok()
            {
                answer = if e.TokenIsElevated != 0 { "yes" } else { "no" };
            }
            let _ = CloseHandle(token);
        }
        let _ = CloseHandle(h);
        answer
    }
}
