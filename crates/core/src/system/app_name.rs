//! Friendly app names for the bar's foreground-app label.

use std::collections::{HashMap, HashSet};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use windows::core::{w, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{IShellItem, SHCreateItemFromParsingName, SIGDN_NORMALDISPLAY};

type Key = (String, Option<PathBuf>);

/// Called on every foreground change, and the lookups touch the shell and the disk. Shared by
/// every thread, so a name a worker found serves the UI thread too.
static CACHE: LazyLock<Mutex<HashMap<Key, String>>> = LazyLock::new(Mutex::default);
/// Names a worker is looking up now.
static PENDING: LazyLock<Mutex<HashSet<Key>>> = LazyLock::new(Mutex::default);

/// Friendly name: shell display name for real AppUserModelIDs, else the exe's FileDescription,
/// else the exe's file stem, else `app_id`. Packaged apps resolve via step 1 to their Start name.
/// Blocks on the shell and the disk the first time (100 ms and more for an Apps-folder parse):
/// a UI thread wants [`display_name_soon`].
pub fn display_name(app_id: &str, exe: Option<&Path>) -> String {
    let key = (app_id.to_owned(), exe.map(Path::to_path_buf));
    if let Some(hit) = CACHE.lock().unwrap().get(&key).cloned() {
        return hit;
    }
    let name = resolve(app_id, exe);
    CACHE.lock().unwrap().insert(key, name.clone());
    name
}

/// The friendly name if it is known already; otherwise a quick stand-in (shellman's own name,
/// the exe's stem, or the id) while a worker thread looks it up, calling `ready` once it is
/// known (to repaint). Never blocks.
pub fn display_name_soon(app_id: &str, exe: Option<&Path>, ready: impl FnOnce() + Send + 'static) -> String {
    let key: Key = (app_id.to_owned(), exe.map(Path::to_path_buf));
    if let Some(hit) = CACHE.lock().unwrap().get(&key).cloned() {
        return hit;
    }
    if PENDING.lock().unwrap().insert(key.clone()) {
        std::thread::spawn(move || {
            let name = resolve(&key.0, key.1.as_deref());
            CACHE.lock().unwrap().insert(key.clone(), name);
            PENDING.lock().unwrap().remove(&key);
            ready();
        });
    }
    stand_in(app_id, exe)
}

/// What shows while the real name is looked up.
fn stand_in(app_id: &str, exe: Option<&Path>) -> String {
    if let Some(n) = exe.and_then(own_app_name) {
        return n.to_owned();
    }
    exe.and_then(|e| e.file_stem())
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(app_id)
        .to_owned()
}

/// Shellman's own apps are named by their exe (`shellman-files.exe`), which has no description
/// worth showing; they get their proper names.
fn own_app_name(exe: &Path) -> Option<&'static str> {
    let stem = exe.file_stem()?.to_str()?.to_ascii_lowercase();
    Some(match stem.strip_prefix("shellman-")? {
        "files" => "Files",
        "settings" => "Settings",
        "launcher" => "Launcher",
        "switcher" => "Switcher",
        "desktop" => "Desktop",
        "bar" => "Bar",
        "session" => "Shellman",
        "tasks" => "Task Manager",
        _ => return None,
    })
}

fn resolve(app_id: &str, exe: Option<&Path>) -> String {
    let nonempty = |s: &String| !s.trim().is_empty();
    if let Some(n) = exe.and_then(own_app_name) {
        return n.to_owned();
    }
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

pub fn file_description(exe: &Path) -> Option<String> {
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
    fn own_apps_get_proper_names() {
        let name = |p: &str| own_app_name(Path::new(p));
        assert_eq!(name(r"C:\x\target\debug\shellman-files.exe"), Some("Files"));
        assert_eq!(name(r"C:\x\Shellman-Settings.EXE"), Some("Settings"));
        assert_eq!(name(r"C:\x\shellman-unknown.exe"), None);
        assert_eq!(name(r"C:\x\files.exe"), None);
        let exe = Path::new(r"C:\x\shellman-files.exe");
        assert_eq!(display_name(r"c:\x\shellman-files.exe", Some(exe)), "Files");
    }

    #[test]
    fn soon_gives_a_stand_in_then_the_name() {
        let exe = Path::new(r"C:\Windows\System32\charmap.exe");
        let id = r"c:\windows\system32\charmap.exe";
        let (tx, rx) = std::sync::mpsc::channel();
        let first = display_name_soon(id, Some(exe), move || {
            let _ = tx.send(());
        });
        let _ = rx.recv_timeout(std::time::Duration::from_secs(10));
        let name = display_name(id, Some(exe));
        // The exe's stem for now, unless already known.
        assert!(first == "charmap" || first == name, "{first}");
        assert_eq!(display_name_soon(id, Some(exe), || {}), name);
        assert_eq!(stand_in("Some.App", None), "Some.App");
    }

    #[test]
    fn falls_back_to_app_id() {
        assert_eq!(display_name("c:\\nope\\x", None), "c:\\nope\\x");
    }
}
