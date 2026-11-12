//! Shell operations: open, and recycle/copy/move through the Windows shell.

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{SHFileOperationW, ShellExecuteW, SHFILEOPSTRUCTW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// Open a file with its default app, or run it.
pub fn open(path: &Path) {
    let p = HSTRING::from(path.as_os_str());
    unsafe {
        ShellExecuteW(None, &HSTRING::from("open"), &p, None, None, SW_SHOWNORMAL);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Recycle,
    Copy,
    /// Copy into the folder the items are already in, as "name - Copy".
    Duplicate,
    Move,
}

/// Runs a shell file operation. Blocks, so call it from a worker thread. Ok(false) when the user
/// cancelled part of it.
pub fn shell_op(hwnd: isize, op: Op, from: &[PathBuf], to: Option<&Path>) -> Result<bool, String> {
    // Both lists are NUL-separated and end with a double NUL.
    let mut src: Vec<u16> = Vec::new();
    for p in from {
        src.extend(p.as_os_str().encode_wide());
        src.push(0);
    }
    src.push(0);
    let dst: Option<Vec<u16>> = to.map(|t| {
        let mut v: Vec<u16> = t.as_os_str().encode_wide().collect();
        v.extend([0, 0]);
        v
    });
    // FO_MOVE = 1, FO_COPY = 2, FO_DELETE = 3. FOF_ALLOWUNDO = 0x40, FOF_NOCONFIRMATION = 0x10.
    let (func, flags) = match op {
        Op::Move => (1, 0x40),
        Op::Copy => (2, 0x40),
        // FOF_RENAMEONCOLLISION = 0x8.
        Op::Duplicate => (2, 0x40 | 0x8),
        Op::Recycle => (3, 0x40 | 0x10),
    };
    let mut info = SHFILEOPSTRUCTW {
        hwnd: HWND(hwnd as *mut _),
        wFunc: func,
        pFrom: PCWSTR(src.as_ptr()),
        pTo: dst.as_ref().map_or(PCWSTR::null(), |d| PCWSTR(d.as_ptr())),
        fFlags: flags,
        ..Default::default()
    };
    let code = unsafe { SHFileOperationW(&mut info) };
    if code != 0 {
        return Err(format!("Windows could not finish (error {code:#x})"));
    }
    Ok(!info.fAnyOperationsAborted.as_bool())
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[allow(dead_code)]
pub fn wide_path(p: &Path) -> Vec<u16> {
    wide(&p.to_string_lossy())
}
