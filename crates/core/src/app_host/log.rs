//! The supervisor's few lines: one per decision, so a host that keeps dying or never starts can
//! be seen after the fact. Kept under 64 KB.
//!
//! In `%LOCALAPPDATA%\shellman\app-host.log`, not beside the settings: the supervisor watches the
//! settings folder, and its own writes there would wake it. With `SHELLMAN_CONFIG_DIR` set (tests),
//! next to that folder as `<folder>-app-host.log`.

use std::io::Write;
use std::path::PathBuf;
use windows::Win32::System::SystemInformation::GetLocalTime;

const CAP: u64 = 64 * 1024;

fn path() -> Option<PathBuf> {
    match std::env::var_os(crate::config::DIR_OVERRIDE).filter(|d| !d.is_empty()) {
        Some(dir) => {
            let mut name = dir;
            name.push("-app-host.log");
            Some(PathBuf::from(name))
        }
        None => dirs::data_local_dir().map(|d| d.join("shellman").join("app-host.log")),
    }
}

pub fn line(text: &str) {
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
