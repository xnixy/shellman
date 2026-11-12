//! A handle on a process of ours to wait on: one we started, or one we adopted.

use crate::apps::App;
use std::os::windows::process::CommandExt;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
};

const STILL_ACTIVE: u32 = 259;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Set in every child's environment: "a session runs you". The bar then leaves the residents to
/// the session instead of starting them itself.
pub const MARK: &str = "SHELLMAN_SESSION";

pub struct Proc {
    pub pid: u32,
    pub handle: HANDLE,
}

impl Drop for Proc {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

impl Proc {
    /// Start an app. Not tied to the session by a job: if the session dies, they live on, and the
    /// next session adopts them.
    pub fn spawn(app: &App) -> Result<Proc, String> {
        let mut cmd = std::process::Command::new(&app.exe);
        cmd.args(&app.args)
            .env(MARK, "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        if let Some(dir) = app.exe.parent().filter(|d| d.is_dir()) {
            cmd.current_dir(dir);
        }
        let child = cmd.spawn().map_err(|e| e.to_string())?;
        let pid = child.id();
        // The std handle is dropped with `child`; ours is opened by pid, like an adopted one.
        Proc::open(pid).ok_or_else(|| "started, but its handle could not be opened".to_string())
    }

    pub fn open(pid: u32) -> Option<Proc> {
        let handle =
            unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
        Some(Proc { pid, handle })
    }

    /// The exit code, or `None` while it still runs.
    pub fn exit_code(&self) -> Option<u32> {
        let mut code = 0u32;
        unsafe { GetExitCodeProcess(self.handle, &mut code) }.ok()?;
        (code != STILL_ACTIVE).then_some(code)
    }
}
