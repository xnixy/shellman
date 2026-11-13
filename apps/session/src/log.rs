//! The session's log: one line per decision, so a shell that misbehaves can be understood after
//! the fact. `%LOCALAPPDATA%\shellman\session.log`, rotated past 64 KB. With
//! `SHELLMAN_CONFIG_DIR` set (tests) it is `<folder>-session.log` beside that folder instead.

use std::io::Write;
use std::path::PathBuf;
use windows::Win32::System::SystemInformation::GetLocalTime;

const CAP: u64 = 64 * 1024;

fn path() -> Option<PathBuf> {
    match std::env::var_os("SHELLMAN_CONFIG_DIR").filter(|d| !d.is_empty()) {
        Some(mut dir) => {
            dir.push("-session.log");
            Some(PathBuf::from(dir))
        }
        None => dirs::data_local_dir().map(|d| d.join("shellman").join("session.log")),
    }
}

pub fn line(text: &str) {
    // Unit tests never write a log: without SHELLMAN_CONFIG_DIR that would be the live session's.
    if cfg!(test) {
        return;
    }
    let Some(path) = path() else { return };
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > CAP) {
        let _ = std::fs::rename(&path, path.with_extension("old.log"));
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let t = unsafe { GetLocalTime() };
    let stamp = format!(
        "{}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds
    );
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{stamp} [{}] {text}", std::process::id());
    }
}
