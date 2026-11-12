//! The real `Runner`: starts an uninstaller the way Windows does and waits for it to end, and
//! asks the package manager to remove a Store app. Both block, so they run on a worker thread.

use super::act::{Progress, Runner};
use super::cmdline::Launch;
use super::packaged;
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
use windows::Win32::System::Threading::{CreateProcessW, WaitForSingleObject, CREATE_UNICODE_ENVIRONMENT, INFINITE, PROCESS_INFORMATION, STARTUPINFOW};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// "The requested operation requires elevation": the program wants administrator rights, which
/// only a ShellExecute `runas` can ask for.
const ERROR_ELEVATION_REQUIRED: u32 = 740;
/// The user said no at the UAC prompt.
const ERROR_CANCELLED: u32 = 1223;

pub struct Real;

fn wait_and_close(process: HANDLE) {
    // SAFETY: `process` is a handle this module opened and closes here, once.
    unsafe {
        let _ = WaitForSingleObject(process, INFINITE);
        let _ = CloseHandle(process);
    }
}

/// Start the program as it is; Err carries the Win32 error code.
fn create(launch: &Launch) -> Result<HANDLE, u32> {
    let mut line: Vec<u16> = launch.command_line().encode_utf16().chain(Some(0)).collect();
    let si = STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, ..Default::default() };
    let mut pi = PROCESS_INFORMATION::default();
    // SAFETY: `line` is a writable, NUL-ended buffer that outlives the call; the thread handle is
    // closed here and the process handle is returned to the caller to close.
    unsafe {
        CreateProcessW(PCWSTR::null(), Some(PWSTR(line.as_mut_ptr())), None, None, false, CREATE_UNICODE_ENVIRONMENT, None, PCWSTR::null(), &si, &mut pi)
            .map_err(|_| GetLastError().0)?;
        let _ = CloseHandle(pi.hThread);
    }
    Ok(pi.hProcess)
}

/// Start the program through the shell with the `runas` verb (the UAC prompt); Err carries the
/// Win32 error code.
fn create_elevated(launch: &Launch) -> Result<HANDLE, u32> {
    let file = HSTRING::from(&launch.exe);
    let params = HSTRING::from(&launch.args);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: windows::core::w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: if launch.args.is_empty() { PCWSTR::null() } else { PCWSTR(params.as_ptr()) },
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: `info` and the strings it points to outlive the call.
    unsafe { ShellExecuteExW(&mut info).map_err(|_| GetLastError().0)? };
    Ok(info.hProcess)
}

impl Runner for Real {
    fn launch(&self, launch: &Launch) -> Result<String, String> {
        let started = match create(launch) {
            Err(ERROR_ELEVATION_REQUIRED) => create_elevated(launch),
            other => other,
        };
        match started {
            Ok(process) => {
                if !process.is_invalid() {
                    wait_and_close(process);
                }
                Ok(String::new())
            }
            // Saying no to the prompt is the user's answer, not a failure.
            Err(ERROR_CANCELLED) => Ok(String::new()),
            Err(code) => Err(format!("Windows could not start {} (error {code}).", launch.exe)),
        }
    }

    fn remove_package(&self, full_name: &str, progress: Progress) -> Result<String, String> {
        packaged::remove(full_name, progress).map(|()| String::new())
    }
}
