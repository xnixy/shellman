//! Paths and the shell's items: what an app hands the dialog (a folder as an `IShellItem`) and
//! what it gets back (the chosen files as items, or an array of them), and strings the app frees.

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::System::Com::{
    CoTaskMemAlloc, CoTaskMemFree, CreateBindCtx, BIND_OPTS, STGM_CREATE,
};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    IShellItem, IShellItemArray, SHCreateItemFromParsingName, SHCreateShellItemArrayFromIDLists,
    SHParseDisplayName, SIGDN_FILESYSPATH,
};
use windows_core::Result;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn wide_path(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// A string the app passed in, as Rust text ("" for null).
pub fn text(p: &PCWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { p.to_string().unwrap_or_default() }
}

/// `s` in memory the caller frees with `CoTaskMemFree`, as `GetFileName` hands back.
pub fn task_string(s: &str) -> PWSTR {
    let w = wide(s);
    unsafe {
        let p = CoTaskMemAlloc(w.len() * 2) as *mut u16;
        if p.is_null() {
            return PWSTR::null();
        }
        std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
        PWSTR(p)
    }
}

/// The file system path of an item, if it has one (a library or This PC does not).
pub fn path_of(item: &IShellItem) -> Option<PathBuf> {
    unsafe {
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s.map(PathBuf::from)
    }
}

/// An item for `path`. A file that does not exist yet (a name to save under) is made all the
/// same, as the real save dialog does: the bind context's `STGM_CREATE` lets the parse through.
pub fn item(path: &Path) -> Result<IShellItem> {
    let w = wide_path(path);
    unsafe {
        let bind = CreateBindCtx(0)?;
        let opts = BIND_OPTS {
            cbStruct: size_of::<BIND_OPTS>() as u32,
            grfMode: STGM_CREATE.0,
            ..Default::default()
        };
        bind.SetBindOptions(&opts)?;
        SHCreateItemFromParsingName(PCWSTR(w.as_ptr()), &bind)
    }
}

/// An array of items for `paths`.
pub fn array(paths: &[PathBuf]) -> Result<IShellItemArray> {
    let mut lists: Vec<*mut ITEMIDLIST> = Vec::with_capacity(paths.len());
    let result = (|| unsafe {
        let bind = CreateBindCtx(0)?;
        let opts = BIND_OPTS {
            cbStruct: size_of::<BIND_OPTS>() as u32,
            grfMode: STGM_CREATE.0,
            ..Default::default()
        };
        bind.SetBindOptions(&opts)?;
        for p in paths {
            let w = wide_path(p);
            let mut list = std::ptr::null_mut();
            SHParseDisplayName(PCWSTR(w.as_ptr()), &bind, &mut list, 0, None)?;
            lists.push(list);
        }
        let consts: Vec<*const ITEMIDLIST> = lists.iter().map(|&l| l as *const _).collect();
        SHCreateShellItemArrayFromIDLists(&consts)
    })();
    for l in lists {
        unsafe { CoTaskMemFree(Some(l as *const _)) };
    }
    result
}
