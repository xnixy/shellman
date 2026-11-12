//! The Win32 side of startup items: expanding `%VARS%`, finding the Startup folders, knowing
//! whether the session is elevated, and starting an item.

use super::items::{FolderFile, Item, Origin};
use super::runner::Launcher;
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows::Win32::System::Threading::{
    CreateProcessW, GetCurrentProcess, OpenProcessToken, CREATE_NEW_PROCESS_GROUP, PROCESS_INFORMATION, STARTUPINFOW,
};
use windows::Win32::UI::Shell::{
    FOLDERID_CommonStartup, FOLDERID_Startup, SHGetKnownFolderPath, ShellExecuteExW, KF_FLAG_DEFAULT,
    SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

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

pub fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut e = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(token, TokenElevation, Some(&mut e as *mut _ as *mut _), size_of::<TOKEN_ELEVATION>() as u32, &mut len);
        let _ = CloseHandle(token);
        ok.is_ok() && e.TokenIsElevated != 0
    }
}

/// The files in the per-user and the common Startup folders, per-user last (Explorer's order is
/// common first).
pub fn startup_files() -> Vec<FolderFile> {
    let mut out = Vec::new();
    for (common, id) in [(true, &FOLDERID_CommonStartup), (false, &FOLDERID_Startup)] {
        let dir = unsafe {
            match SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) {
                Ok(p) => {
                    let s = p.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(p.0 as *const _));
                    s
                }
                Err(_) => continue,
            }
        };
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut files: Vec<_> = rd.flatten().filter(|e| e.path().is_file()).map(|e| e.path()).collect();
        files.sort();
        out.extend(files.into_iter().map(|path| FolderFile { common, path }));
    }
    out
}

/// Starts items for real. Never used by `--dry-run`.
pub struct Real;

fn create_process(command: &str) -> bool {
    let mut line: Vec<u16> = command.encode_utf16().chain([0]).collect();
    let si = STARTUPINFOW { cb: size_of::<STARTUPINFOW>() as u32, ..Default::default() };
    let mut pi = PROCESS_INFORMATION::default();
    let ok = unsafe {
        CreateProcessW(PCWSTR::null(), Some(PWSTR(line.as_mut_ptr())), None, None, false, CREATE_NEW_PROCESS_GROUP, None, PCWSTR::null(), &si, &mut pi)
    }
    .is_ok();
    if ok {
        unsafe {
            let _ = CloseHandle(pi.hProcess);
            let _ = CloseHandle(pi.hThread);
        }
    }
    ok
}

fn shell_execute(file: &str) -> Result<(), String> {
    let file = HSTRING::from(file);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpFile: PCWSTR(file.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info) }.map_err(|e| e.to_string())
}

impl Launcher for Real {
    fn launch(&self, item: &Item) -> Result<(), String> {
        match &item.origin {
            // Shortcuts and documents go through the shell, which knows what to do with them.
            Origin::Folder { .. } => shell_execute(&item.command),
            // A command line starts as it is; a value that is just a path with spaces and no
            // quotes does not parse as one, so that falls back to the shell.
            Origin::Run { .. } => {
                if create_process(&item.command) { Ok(()) } else { shell_execute(&item.command) }
            }
        }
    }
}
