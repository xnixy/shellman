//! The session's Win32 side of startup items: knowing whether it is elevated, and starting an
//! item. (Expanding `%VARS%`, finding the Startup folders and packaged tasks are shared, in core.)

use super::items::{Item, Origin};
use super::packaged_win;
use super::runner::Launcher;
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::{
    CreateProcessW, GetCurrentProcess, GetCurrentProcessId, OpenProcessToken, CREATE_NEW_PROCESS_GROUP,
    PROCESS_INFORMATION, STARTUPINFOW,
};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// COM for this thread (the package manager and the startup-task classes need it).
pub fn init_com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

/// The logon session this process runs in (the `SessionInfo\<n>` of Windows' marker).
pub fn session_id() -> u32 {
    let mut id = 0u32;
    unsafe {
        let _ = ProcessIdToSessionId(GetCurrentProcessId(), &mut id);
    }
    id
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
            // As the shell does it, so the app runs with its package identity and sees a
            // startup-task activation. A UWP task needs the immersive shell, which is there only
            // while Explorer or the modern-app host runs; without it activation hangs ~45 s.
            Origin::Packaged { full_trust, executable, task_id, .. } => {
                if !full_trust && !shellman_core::app_host::modern_apps_available() {
                    return Err("a UWP task, and modern apps cannot open now (the modern-apps setting is off)".into());
                }
                packaged_win::start(executable, task_id).map(|_pid| ())
            }
        }
    }
}
