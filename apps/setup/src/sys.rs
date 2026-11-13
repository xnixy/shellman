//! The Win32 side: processes, windows, version resources, registry reads, the lock, local time.
//! Read-only except for the lock. Nothing here creates a window or sends input.

use windows::core::{BOOL, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM};
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible};

/// `(pid, image path)` of every process in this logon session whose image we may query.
pub fn processes() -> Vec<(u32, String)> {
    let mut mine = 0;
    unsafe {
        let _ = ProcessIdToSessionId(GetCurrentProcessId(), &mut mine);
    }
    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut entry).is_ok();
        while ok {
            let pid = entry.th32ProcessID;
            let mut s = u32::MAX;
            let _ = ProcessIdToSessionId(pid, &mut s);
            if s == mine {
                if let Some(image) = image_of(pid) {
                    out.push((pid, image));
                }
            }
            ok = Process32NextW(snap, &mut entry).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

fn image_of(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let got = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        got.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

struct Titles<'a> {
    pids: &'a [u32],
    found: Vec<(u32, String)>,
}

unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam.0 as *mut Titles);
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if ctx.pids.contains(&pid) && IsWindowVisible(hwnd).as_bool() {
        let mut buf = [0u16; 256];
        let n = GetWindowTextW(hwnd, &mut buf);
        if n > 0 {
            ctx.found.push((pid, String::from_utf16_lossy(&buf[..n as usize])));
        }
    }
    BOOL(1)
}

/// Titles of the visible top-level windows of these processes (only reads their titles).
pub fn window_titles(pids: &[u32]) -> Vec<(u32, String)> {
    let mut ctx = Titles { pids, found: Vec::new() };
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut ctx as *mut Titles as isize));
    }
    ctx.found
}

/// A string value under HKCU (`None`: missing, or not a string).
pub fn hkcu_string(key: &str, value: &str) -> Option<String> {
    let mut buf = vec![0u16; 2048];
    let mut size = (buf.len() * 2) as u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            &HSTRING::from(value),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
    };
    if r.0 != 0 {
        return None;
    }
    let chars = (size as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..chars.min(buf.len())]))
}

/// The `ProductVersion` string of an exe or dll (its build stamp), read without running it.
pub fn file_stamp(path: &std::path::Path) -> Option<String> {
    let wide = HSTRING::from(path.as_os_str());
    unsafe {
        let size = GetFileVersionInfoSizeW(&wide, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(&wide, None, size, data.as_mut_ptr() as *mut _).ok()?;
        let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        // The language/codepage of the first translation, else US English / Unicode.
        let mut lang = "040904b0".to_string();
        if VerQueryValueW(data.as_ptr() as *const _, &HSTRING::from("\\VarFileInfo\\Translation"), &mut ptr, &mut len).as_bool() && len >= 4 {
            let t = std::slice::from_raw_parts(ptr as *const u16, 2);
            lang = format!("{:04x}{:04x}", t[0], t[1]);
        }
        let q = HSTRING::from(format!("\\StringFileInfo\\{lang}\\ProductVersion"));
        if !VerQueryValueW(data.as_ptr() as *const _, &q, &mut ptr, &mut len).as_bool() || len == 0 {
            return None;
        }
        let s = std::slice::from_raw_parts(ptr as *const u16, len as usize);
        let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
        Some(String::from_utf16_lossy(&s[..end])).filter(|s| !s.is_empty())
    }
}

/// `HHmmss` of the local time: the stamp in `name.oldHHmmss.exe`.
pub fn hhmmss() -> String {
    let t = unsafe { GetLocalTime() };
    format!("{:02}{:02}{:02}", t.wHour, t.wMinute, t.wSecond)
}

/// A named lock so two setups on one install cannot interleave. Held until dropped.
pub struct Lock(HANDLE);

impl Drop for Lock {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// The lock's name for an install folder: one per folder, so tests on temp folders never wait
/// for each other or for the real install.
pub fn lock_name(prefix: &std::path::Path) -> String {
    let norm = prefix.to_string_lossy().replace('/', "\\").to_ascii_lowercase();
    let mut h: u64 = 0xcbf29ce484222325;
    for b in norm.trim_end_matches('\\').bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    format!("Local\\shellman-setup-{h:016x}")
}

pub fn lock(prefix: &std::path::Path) -> Result<Lock, String> {
    let name = lock_name(prefix);
    let h = unsafe { CreateMutexW(None, false, PCWSTR(HSTRING::from(&name).as_ptr())) }.map_err(|e| format!("cannot create the lock: {e}"))?;
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            let _ = CloseHandle(h);
        }
        return Err("another shellman-setup is working on this install; wait for it to finish".into());
    }
    Ok(Lock(h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn locks_are_per_folder() {
        assert_eq!(lock_name(Path::new(r"C:\A\b")), lock_name(Path::new(r"c:/a/B\")));
        assert_ne!(lock_name(Path::new(r"C:\A\b")), lock_name(Path::new(r"C:\A\c")));
    }

    #[test]
    fn a_second_lock_on_the_same_folder_is_refused() {
        let p = Path::new(r"C:\no\such\setup-lock-test");
        let a = lock(p).unwrap();
        assert!(lock(p).is_err());
        drop(a);
        assert!(lock(p).is_ok());
    }

    #[test]
    fn this_process_is_listed() {
        let me = unsafe { GetCurrentProcessId() };
        assert!(processes().iter().any(|(p, _)| *p == me));
    }

    #[test]
    fn reads_a_version_stamp_from_an_exe_that_has_one() {
        // This test exe carries no stamp of ours, but the call must not fail badly on any file.
        let _ = file_stamp(&std::env::current_exe().unwrap());
        assert_eq!(file_stamp(Path::new(r"C:\no\such.exe")), None);
    }
}
