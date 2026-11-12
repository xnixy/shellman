//! The Win32 side shared by the startup items: expanding `%VARS%`, finding the Startup folders,
//! and the current time as a FILETIME.

use super::items::FolderFile;
use windows::core::HSTRING;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows::Win32::UI::Shell::{FOLDERID_CommonStartup, FOLDERID_Startup, SHGetKnownFolderPath, KF_FLAG_DEFAULT};

pub fn expand(s: &str) -> String {
    let src = HSTRING::from(s);
    unsafe {
        let need = ExpandEnvironmentStringsW(&src, None);
        if need == 0 {
            return s.to_string();
        }
        let mut buf = vec![0u16; need as usize];
        let got = ExpandEnvironmentStringsW(&src, Some(&mut buf));
        String::from_utf16_lossy(&buf[..(got as usize).saturating_sub(1)])
    }
}

/// The two Startup folders themselves, as (common, path).
pub fn startup_dirs() -> Vec<(bool, std::path::PathBuf)> {
    let mut out = Vec::new();
    for (common, id) in [(true, &FOLDERID_CommonStartup), (false, &FOLDERID_Startup)] {
        unsafe {
            if let Ok(p) = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) {
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as *const _));
                out.push((common, s.into()));
            }
        }
    }
    out
}

/// The files in the per-user and the common Startup folders, per-user last (Explorer's order is
/// common first).
pub fn startup_files() -> Vec<FolderFile> {
    let mut out = Vec::new();
    for (common, dir) in startup_dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut files: Vec<_> = rd.flatten().filter(|e| e.path().is_file()).map(|e| e.path()).collect();
        files.sort();
        out.extend(files.into_iter().map(|path| FolderFile { common, path }));
    }
    out
}

/// Now, as a FILETIME (100 ns ticks since 1601), the stamp Windows writes when it disables an item.
pub fn now_filetime() -> u64 {
    let ft = unsafe { GetSystemTimeAsFileTime() };
    (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime)
}
