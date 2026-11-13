//! What is read about a process by opening it: once when it is first seen (path, friendly name,
//! command line, whether it can be ended), and on asking (the details row).
//!
//! Only `PROCESS_QUERY_LIMITED_INFORMATION` is used to read; whether `PROCESS_TERMINATE` would be
//! granted is asked when the process is first seen, and the handle is closed at once.

use super::{Access, Details, Key};
use std::path::PathBuf;
use std::sync::Arc;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, SYSTEMTIME};
use windows::Win32::Security::{
    GetTokenInformation, LookupAccountSidW, TokenElevation, TokenUser, SID_NAME_USE, TOKEN_ELEVATION, TOKEN_QUERY,
    TOKEN_USER,
};
use windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE;
use windows::Win32::System::Threading::{
    GetProcessTimes, IsWow64Process2, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
};
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

/// A process handle, closed when dropped.
pub(super) struct Handle(pub HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Open `key`'s process with `rights`, and only if it is still the same process (it started when
/// the key says): a PID freed and reused meanwhile is a different process.
pub(super) fn open(key: Key, rights: windows::Win32::System::Threading::PROCESS_ACCESS_RIGHTS) -> Option<Handle> {
    let h = Handle(unsafe { OpenProcess(rights | PROCESS_QUERY_LIMITED_INFORMATION, false, key.pid) }.ok()?);
    (created(&h)? == key.created).then_some(h)
}

/// Why a process could not be opened for `key`.
pub(super) enum OpenError {
    /// Windows refused: it runs elevated, or as another user or the system.
    Denied,
    /// No such process now, or one that reuses the PID: `key` has already ended.
    Gone,
    Other(String),
}

/// Like [`open`], saying why when it fails.
pub(super) fn open_checked(
    key: Key,
    rights: windows::Win32::System::Threading::PROCESS_ACCESS_RIGHTS,
) -> Result<Handle, OpenError> {
    use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER};
    let opened = unsafe { OpenProcess(rights | PROCESS_QUERY_LIMITED_INFORMATION, false, key.pid) };
    let h = match opened {
        Ok(h) => Handle(h),
        Err(e) if e.code() == ERROR_ACCESS_DENIED.to_hresult() => return Err(OpenError::Denied),
        Err(e) if e.code() == ERROR_INVALID_PARAMETER.to_hresult() => return Err(OpenError::Gone),
        Err(e) => return Err(OpenError::Other(e.message())),
    };
    match created(&h) {
        Some(c) if c == key.created => Ok(h),
        Some(_) => Err(OpenError::Gone),
        None => Err(OpenError::Other("Its start time could not be read.".into())),
    }
}

fn created(h: &Handle) -> Option<i64> {
    let (mut c, mut e, mut k, mut u) = Default::default();
    unsafe { GetProcessTimes(h.0, &mut c, &mut e, &mut k, &mut u) }.ok()?;
    Some(filetime(c))
}

fn filetime(f: FILETIME) -> i64 {
    ((f.dwHighDateTime as i64) << 32) | f.dwLowDateTime as i64
}

/// What is read once per process.
#[derive(Clone, Debug, Default)]
pub struct Static {
    pub exe: Option<Arc<std::path::Path>>,
    pub description: Arc<str>,
    pub command_line: Arc<str>,
    pub access: Access,
}

/// Read a newly seen process. `name` is its image name, for when nothing else can be read.
pub fn read_static(key: Key, name: &str) -> Static {
    let stem = name.strip_suffix(".exe").or_else(|| name.strip_suffix(".EXE")).unwrap_or(name);
    let mut out = Static { description: stem.into(), command_line: "".into(), ..Default::default() };
    let (h, access) = match open(key, PROCESS_TERMINATE) {
        Some(h) => (Some(h), Access::Full),
        None => match open(key, Default::default()) {
            Some(h) => (Some(h), Access::ReadOnly),
            None => (None, Access::None),
        },
    };
    out.access = access;
    let Some(h) = h else {
        return out;
    };
    if let Some(path) = image_path(&h) {
        let id = path.to_string_lossy().to_lowercase();
        out.description = crate::system::app_name::display_name(&id, Some(&path)).into();
        out.exe = Some(path.into());
    }
    out.command_line = command_line(&h).unwrap_or_default().into();
    out
}

pub(super) fn image_path(h: &Handle) -> Option<PathBuf> {
    let mut buf = vec![0u16; 1024];
    let mut len = buf.len() as u32;
    unsafe { QueryFullProcessImageNameW(h.0, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }.ok()?;
    Some(PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
}

/// The command line, through `ProcessCommandLineInformation` (works with the limited right).
fn command_line(h: &Handle) -> Option<String> {
    const PROCESS_COMMAND_LINE_INFORMATION: u32 = 60;
    let mut buf = vec![0u64; 512];
    for _ in 0..3 {
        let mut needed = 0u32;
        let bytes = (buf.len() * 8) as u32;
        let status = unsafe {
            super::nt::NtQueryInformationProcess(h.0 .0, PROCESS_COMMAND_LINE_INFORMATION, buf.as_mut_ptr().cast(), bytes, &mut needed)
        };
        if status >= 0 {
            // A UNICODE_STRING (length, max length, pointer) whose text follows it in the buffer.
            let length = (buf[0] & 0xFFFF) as usize / 2;
            let ptr = buf[1] as *const u16;
            if ptr.is_null() || length == 0 {
                return None;
            }
            // SAFETY: the text lies inside `buf`, `length` units long.
            let s = unsafe { std::slice::from_raw_parts(ptr, length) };
            return Some(String::from_utf16_lossy(s));
        }
        if needed as usize <= bytes as usize {
            return None;
        }
        buf = vec![0u64; needed as usize / 8 + 1];
    }
    None
}

/// The details row: who runs it, elevated or not, its architecture and when it started.
pub fn details(key: Key) -> Details {
    let mut d = Details { started: local_time(key.created), ..Default::default() };
    let Some(h) = open(key, Default::default()) else {
        d.denied = Some("Windows does not let this window open the process (it is protected or runs as the system).".into());
        return d;
    };
    d.architecture = architecture(&h);
    let mut token = HANDLE::default();
    if unsafe { OpenProcessToken(h.0, TOKEN_QUERY, &mut token) }.is_ok() {
        let token = Handle(token);
        d.user = user(&token);
        d.elevated = elevated(&token);
    } else {
        d.denied = Some("Its owner and rights cannot be read without administrator rights.".into());
    }
    d
}

fn architecture(h: &Handle) -> Option<&'static str> {
    let mut process = IMAGE_FILE_MACHINE::default();
    let mut native = IMAGE_FILE_MACHINE::default();
    unsafe { IsWow64Process2(h.0, &mut process, Some(&mut native)) }.ok()?;
    let machine = if process.0 == 0 { native.0 } else { process.0 };
    Some(match machine {
        0x8664 => "x64",
        0x014C => "x86",
        0xAA64 => "ARM64",
        0x01C4 => "ARM",
        _ => "unknown",
    })
}

fn user(token: &Handle) -> Option<String> {
    let mut needed = 0u32;
    unsafe {
        let _ = GetTokenInformation(token.0, TokenUser, None, 0, &mut needed);
    }
    let mut buf = vec![0u64; needed as usize / 8 + 1];
    unsafe { GetTokenInformation(token.0, TokenUser, Some(buf.as_mut_ptr().cast()), needed, &mut needed) }.ok()?;
    let sid = unsafe { &*(buf.as_ptr() as *const TOKEN_USER) }.User.Sid;
    let mut name = [0u16; 256];
    let mut domain = [0u16; 256];
    let (mut nl, mut dl) = (name.len() as u32, domain.len() as u32);
    let mut kind = SID_NAME_USE::default();
    unsafe {
        LookupAccountSidW(None, sid, Some(PWSTR(name.as_mut_ptr())), &mut nl, Some(PWSTR(domain.as_mut_ptr())), &mut dl, &mut kind)
    }
    .ok()?;
    let name = String::from_utf16_lossy(&name[..nl as usize]);
    let domain = String::from_utf16_lossy(&domain[..dl as usize]);
    Some(if domain.is_empty() { name } else { format!("{domain}\\{name}") })
}

fn elevated(token: &Handle) -> Option<bool> {
    let mut e = TOKEN_ELEVATION::default();
    let mut len = 0u32;
    unsafe {
        GetTokenInformation(token.0, TokenElevation, Some((&raw mut e).cast()), size_of::<TOKEN_ELEVATION>() as u32, &mut len)
    }
    .ok()?;
    Some(e.TokenIsElevated != 0)
}

/// A FILETIME as local `YYYY-MM-DD HH:MM:SS`, or empty when it is not a time.
pub fn local_time(t: i64) -> String {
    if t <= 0 {
        return String::new();
    }
    let ft = FILETIME { dwLowDateTime: t as u32, dwHighDateTime: (t >> 32) as u32 };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    unsafe {
        if FileTimeToSystemTime(&ft, &mut utc).is_err() || SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_err() {
            return String::new();
        }
    }
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        local.wYear, local.wMonth, local.wDay, local.wHour, local.wMinute, local.wSecond
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me() -> Key {
        let mut r = super::super::nt::Reader::default();
        let p = r.processes().into_iter().find(|p| p.pid == std::process::id()).unwrap();
        Key { pid: p.pid, created: p.created }
    }

    #[test]
    fn reads_its_own_process() {
        let key = me();
        let s = read_static(key, "x.exe");
        assert_eq!(s.access, Access::Full);
        assert!(s.exe.is_some());
        assert!(!s.command_line.is_empty());
        let d = details(key);
        assert!(d.user.is_some() && d.elevated.is_some() && d.architecture.is_some());
        assert!(d.started.starts_with("20"), "{}", d.started);
    }

    #[test]
    fn a_wrong_start_time_is_another_process() {
        let mut key = me();
        key.created += 1;
        assert!(open(key, Default::default()).is_none());
        assert_eq!(read_static(key, "gone.exe").access, Access::None);
        assert_eq!(&*read_static(key, "gone.exe").description, "gone");
    }

    #[test]
    fn no_time_is_empty() {
        assert_eq!(local_time(0), "");
    }
}
