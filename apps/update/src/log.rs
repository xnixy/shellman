//! Lines in the session's log (`session.log`, the same file and rotation as
//! `apps/session/src/log.rs`), each starting `update:`; printed too when run from a terminal. One
//! check at a time: a named mutex (with `SHELLMAN_INSTANCE`, a test copy has its own).

use std::io::Write;
use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, GENERIC_WRITE, HANDLE};
use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows::Win32::System::Console::{AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::CreateMutexW;

const CAP: u64 = 64 * 1024;

pub fn line(text: &str) {
    println!("{text}");
    if cfg!(test) {
        return;
    }
    let Some(path) = shellman_update_core::state::log_path() else { return };
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > CAP) {
        let _ = std::fs::rename(&path, path.with_extension("old.log"));
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let t = unsafe { GetLocalTime() };
    let stamp = format!("{}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{stamp} [{}] update: {text}", std::process::id());
    }
}

/// Hold the one-check-at-a-time mutex; `None` when another check holds it.
pub fn claim() -> Option<HANDLE> {
    let m = unsafe { CreateMutexW(None, false, &HSTRING::from(shellman_update_core::state::mutex_name())) }.ok()?;
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return None;
    }
    Some(m)
}

/// Make `println!` reach the terminal this was started from (a Windows-subsystem exe has none).
pub fn attach_console() {
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        let has_out = GetStdHandle(STD_OUTPUT_HANDLE).is_ok_and(|h| !h.is_invalid() && !h.0.is_null());
        if !has_out {
            if let Ok(con) = CreateFileW(w!("CONOUT$"), GENERIC_WRITE.0, FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, None) {
                let _ = SetStdHandle(STD_OUTPUT_HANDLE, con);
                let _ = SetStdHandle(STD_ERROR_HANDLE, con);
            }
        }
    }
}
