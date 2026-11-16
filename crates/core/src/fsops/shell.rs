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
    let result = unsafe { ShellExecuteW(None, &HSTRING::from("open"), &p, None, None, SW_SHOWNORMAL) };
    // Above 32 is success. A Store app (a dock pin) is refused when no Explorer runs.
    if result.0 as usize <= 32 {
        crate::apps_index::activate::retry(&path.to_string_lossy());
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    /// To the Recycle Bin. Should an item be unable to go there (see `recycle::unrecyclable`),
    /// Windows asks before deleting it for good rather than doing so silently.
    Recycle,
    /// To the Recycle Bin what can go there, and deleted for good what cannot, without asking
    /// again: only after the user agreed to exactly that.
    RecycleOrDelete,
    Copy,
    /// Copy into the folder the items are already in, as "name - Copy".
    Duplicate,
    Move,
}

/// Runs a shell file operation. Blocks, so call it from a worker thread. Ok(false) when the user
/// cancelled part of it.
pub fn shell_op(hwnd: isize, op: Op, from: &[PathBuf], to: Option<&Path>) -> Result<bool, String> {
    shell_op_mapped(hwnd, op, from, to).map(|(done, _)| done)
}

/// `shell_op`, also saying which items arrived under another name than their own (a copy beside
/// the original, or "keep both" in a conflict): (from, to) for each.
pub fn shell_op_mapped(
    hwnd: isize,
    op: Op,
    from: &[PathBuf],
    to: Option<&Path>,
) -> Result<(bool, Vec<(PathBuf, PathBuf)>), String> {
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
        Op::Recycle => (3, recycle_flags(false)),
        Op::RecycleOrDelete => (3, recycle_flags(true)),
    };
    // FOF_WANTMAPPINGHANDLE = 0x20: the names given to items renamed on arrival.
    let flags = if func == 3 { flags } else { flags | 0x20 };
    let mut info = SHFILEOPSTRUCTW {
        hwnd: HWND(hwnd as *mut _),
        wFunc: func,
        pFrom: PCWSTR(src.as_ptr()),
        pTo: dst.as_ref().map_or(PCWSTR::null(), |d| PCWSTR(d.as_ptr())),
        fFlags: flags,
        ..Default::default()
    };
    // The top bar names the work from this mark while Windows' dialog shows its progress.
    let work = match op {
        Op::Move => super::busy::Work::Move,
        Op::Copy | Op::Duplicate => super::busy::Work::Copy,
        Op::Recycle | Op::RecycleOrDelete => super::busy::Work::Delete,
    };
    let _busy = super::busy::start(hwnd, work);
    let code = unsafe { SHFileOperationW(&mut info) };
    let renamed = unsafe { take_mappings(info.hNameMappings) };
    if code != 0 {
        return Err(format!("Windows could not finish (error {code:#x})"));
    }
    Ok((!info.fAnyOperationsAborted.as_bool(), renamed))
}

/// A delete's flags: undo (the bin) and no "are you sure" of the shell's, as Files asks itself.
/// FOF_WANTNUKEWARNING (0x4000) makes the shell ask before deleting for good what cannot go to
/// the bin; without it that happens silently. It is left out only once the user agreed to it.
fn recycle_flags(may_delete: bool) -> u16 {
    let flags = 0x40 | 0x10;
    if may_delete {
        flags
    } else {
        flags | 0x4000
    }
}

/// The renames in a name-mapping handle, which is then freed. The handle points at a count and
/// an array of `SHNAMEMAPPINGW`.
unsafe fn take_mappings(handle: *mut core::ffi::c_void) -> Vec<(PathBuf, PathBuf)> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::UI::Shell::{SHFreeNameMappings, SHNAMEMAPPINGW};
    #[repr(C)]
    struct Mappings {
        count: u32,
        list: *const SHNAMEMAPPINGW,
    }
    if handle.is_null() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let m = &*(handle as *const Mappings);
    if !m.list.is_null() {
        for i in 0..m.count as usize {
            let e = &*m.list.add(i);
            // The counts are not reliably with or without the NUL; the strings end with one.
            let text = |p: windows::core::PWSTR| {
                PathBuf::from(std::ffi::OsString::from_wide(std::slice::from_raw_parts(p.0, p.len())))
            };
            if !e.pszOldPath.is_null() && !e.pszNewPath.is_null() {
                out.push((text(e.pszOldPath), text(e.pszNewPath)));
            }
        }
    }
    SHFreeNameMappings(Some(windows::Win32::Foundation::HANDLE(handle)));
    out
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn wide_path(p: &Path) -> Vec<u16> {
    wide(&p.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::recycle_flags;

    #[test]
    fn a_recycle_never_deletes_for_good_unasked() {
        const ALLOWUNDO: u16 = 0x40;
        const NUKE_WARNING: u16 = 0x4000;
        assert_eq!(recycle_flags(false) & (ALLOWUNDO | NUKE_WARNING), ALLOWUNDO | NUKE_WARNING);
        // Once the user agreed to a permanent delete, the shell does not ask a second time.
        assert_eq!(recycle_flags(true) & (ALLOWUNDO | NUKE_WARNING), ALLOWUNDO);
    }
}
