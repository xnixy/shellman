//! Starting the helper elevated and waiting for it. Blocks until the UAC prompt is answered and
//! the helper has ended, so call it from a worker thread, never the UI thread.

use super::ops::Op;
use super::{outcome_of_exit, outcome_of_launch_error, Outcome};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

const HELPER: &str = "shellman-elevate.exe";

/// Run `op` in the helper as administrator (the standard UAC prompt) and wait for it.
pub fn run(op: &Op) -> Outcome {
    let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.to_path_buf())) else {
        return Outcome::HelperMissing;
    };
    run_from(&dir, op)
}

/// `run`, with the helper in `dir` (the uninstaller runs from a copy of itself elsewhere).
pub fn run_from(dir: &std::path::Path, op: &Op) -> Outcome {
    let exe = dir.join(HELPER);
    if !exe.is_file() {
        return Outcome::HelperMissing;
    }
    let file: Vec<u16> = exe.to_string_lossy().encode_utf16().chain(Some(0)).collect();
    // Every word was validated when the Op was made; the line quotes only names with spaces.
    let params: Vec<u16> = op.command_line().encode_utf16().chain(Some(0)).collect();
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    // SAFETY: `info` and the strings it points to outlive the calls; the process handle it
    // returns is closed here.
    unsafe {
        if ShellExecuteExW(&mut info).is_err() {
            return outcome_of_launch_error(GetLastError().0);
        }
        let process = info.hProcess;
        if process.is_invalid() {
            return Outcome::Failed("The administrator helper did not start.".into());
        }
        let outcome = if WaitForSingleObject(process, INFINITE) == WAIT_OBJECT_0 {
            let mut code = 0u32;
            match GetExitCodeProcess(process, &mut code) {
                Ok(()) => outcome_of_exit(code),
                Err(_) => Outcome::Failed("The administrator helper's result could not be read.".into()),
            }
        } else {
            Outcome::Failed("The administrator helper did not finish.".into())
        };
        let _ = CloseHandle(process);
        outcome
    }
}
