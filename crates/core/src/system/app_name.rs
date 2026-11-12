//! Friendly app names for the bar's foreground-app label.

use std::cell::RefCell;
use std::collections::HashMap;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::core::{w, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{IShellItem, SHCreateItemFromParsingName, SIGDN_NORMALDISPLAY};

thread_local! {
    /// Called on every foreground change, and the lookups touch the shell and the disk.
    static CACHE: RefCell<HashMap<(String, Option<PathBuf>), String>> = RefCell::new(HashMap::new());
}

/// Friendly name: shell display name for real AppUserModelIDs, else the exe's FileDescription,
/// else the exe's file stem, else `app_id`. Packaged apps resolve via step 1 to their Start name.
pub fn display_name(app_id: &str, exe: Option<&Path>) -> String {
    let key = (app_id.to_owned(), exe.map(Path::to_path_buf));
    if let Some(hit) = CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return hit;
    }
    let name = resolve(app_id, exe);
    CACHE.with(|c| c.borrow_mut().insert(key, name.clone()));
    name
}

fn resolve(app_id: &str, exe: Option<&Path>) -> String {
    let nonempty = |s: &String| !s.trim().is_empty();
    if !app_id.contains('\\') {
        if let Some(n) = shell_name(app_id).filter(nonempty) {
            return n;
        }
    }
    if let Some(exe) = exe {
        if let Some(n) = file_description(exe).filter(nonempty) {
            return n;
        }
        let stem = exe.file_stem().and_then(|s| s.to_str()).map(str::to_owned);
        if let Some(stem) = stem.filter(nonempty) {
            return stem;
        }
    }
    app_id.to_owned()
}

fn shell_name(app_id: &str) -> Option<String> {
    if app_id.is_empty() {
        return None;
    }
    // SAFETY: COM calls with valid arguments; the returned PWSTR is freed with CoTaskMemFree.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let path = HSTRING::from(format!("shell:AppsFolder\\{app_id}"));
        let item: IShellItem = SHCreateItemFromParsingName(&path, None).ok()?;
        let p: PWSTR = item.GetDisplayName(SIGDN_NORMALDISPLAY).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s
    }
}

fn file_description(exe: &Path) -> Option<String> {
    let wide: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
    let path = PCWSTR(wide.as_ptr());
    // SAFETY: `path` outlives the calls; `data` is sized by the API; query results point into `data`.
    unsafe {
        let size = GetFileVersionInfoSizeW(path, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(path, None, size, data.as_mut_ptr().cast()).ok()?;

        let mut p = std::ptr::null_mut();
        let mut len = 0u32;
        let key = w!("\\VarFileInfo\\Translation");
        if !VerQueryValueW(data.as_ptr().cast(), key, &mut p, &mut len).as_bool()
            || p.is_null()
            || len < 4
        {
            return None;
        }
        let tr = std::slice::from_raw_parts(p as *const u16, 2);
        let sub =
            HSTRING::from(format!("\\StringFileInfo\\{:04x}{:04x}\\FileDescription", tr[0], tr[1]));
        if !VerQueryValueW(data.as_ptr().cast(), &sub, &mut p, &mut len).as_bool()
            || p.is_null()
            || len == 0
        {
            return None;
        }
        // `len` counts UTF-16 units including the terminator.
        let s = std::slice::from_raw_parts(p as *const u16, len as usize);
        let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
        Some(String::from_utf16_lossy(&s[..end]).trim().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notepad_gets_friendly_name() {
        let exe = Path::new(r"C:\Windows\System32\notepad.exe");
        let id = exe.to_string_lossy().to_lowercase();
        let n = display_name(&id, Some(exe));
        assert!(!n.is_empty() && n != id, "{n}");
        assert_eq!(n, display_name(&id, Some(exe)), "cache must be stable");
    }

    #[test]
    fn falls_back_to_app_id() {
        assert_eq!(display_name("c:\\nope\\x", None), "c:\\nope\\x");
    }
}
