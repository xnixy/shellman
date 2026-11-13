//! Whether DWM may draw transparency (blur) at all, and making it so without Microsoft's shell.
//!
//! DWM serves every blur path (the host backdrop brush, acrylic, Mica) as a flat colour unless a
//! session WNF state says transparency is allowed: uDWM's `CWindowList` subscribes to state name
//! `0x0F950324A3BC4035` (family IMSN, the immersive shell; well-known, session-scoped, not
//! persistent, most likely `WNF_IMSN_TRANSPARENCYPOLICY`) and takes `*(u32*)data != 0` as "allowed
//! by the system". The immersive shell (`twinui.pcshell.dll`, in Explorer or `ShellAppRuntime.exe`)
//! publishes it; without either the state has no data and every backdrop is flat. Its security
//! descriptor (`HKLM\SYSTEM\CurrentControlSet\Control\Notifications\0F950324A3BC4035`) lets
//! INTERACTIVE users subscribe and publish, so the session publishes it itself (found on
//! 10.0.26300; the kernel keeps the data until the logon session ends).
//!
//! Calls are `ntdll` exports looked up by name (the `windows` crate does not declare them), with
//! the prototypes of System Informer's `phnt` (`ntexapi.h`): `WNF_STATE_NAME` is a union over one
//! `ULONGLONG`, so the state name is that 64-bit value; `WNF_CHANGE_STAMP` and `LOGICAL` are
//! `ULONG`. Everything fails harmlessly: glass stays flat, as it was.
//!
//! Not handled: the real shell may also turn transparency off under energy saver (battery saver).
//! Which signal it follows is not known yet; shellman publishes only the user's setting.

use windows::core::{s, w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, MAX_PATH};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetShellWindow, GetWindowThreadProcessId};

/// The WNF state DWM reads for "transparency allowed by the system".
pub const STATE: u64 = 0x0F95_0324_A3BC_4035;

/// The user's setting: Settings, Personalization, Colors, "Transparency effects".
pub const SETTING_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
pub const SETTING_VALUE: &str = "EnableTransparency";

/// What the state holds now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Reading {
    /// The change stamp: how many times it was published this logon session.
    pub stamp: u32,
    /// None while nobody has published it (DWM then draws everything flat).
    pub allowed: Option<bool>,
}

type Query = unsafe extern "system" fn(*const u64, *const u8, *const u8, *mut u32, *mut u8, *mut u32) -> i32;
type Update = unsafe extern "system" fn(*const u64, *const u8, u32, *const u8, *const u8, u32, u32) -> i32;

fn ntdll(name: windows::core::PCSTR) -> Option<unsafe extern "system" fn() -> isize> {
    unsafe { GetProcAddress(GetModuleHandleW(w!("ntdll.dll")).ok()?, name) }
}

/// The state now, or the NTSTATUS the query failed with (also when ntdll lacks the call:
/// STATUS_PROCEDURE_NOT_FOUND).
pub fn read() -> Result<Reading, i32> {
    let f = ntdll(s!("NtQueryWnfStateData")).ok_or(0xC000_007Au32 as i32)?;
    let f: Query = unsafe { std::mem::transmute(f) };
    let name = STATE;
    let mut stamp = 0u32;
    let mut buf = [0u8; 16];
    let mut size = buf.len() as u32;
    let status = unsafe {
        f(&name, std::ptr::null(), std::ptr::null(), &mut stamp, buf.as_mut_ptr(), &mut size)
    };
    if status < 0 {
        return Err(status);
    }
    Ok(Reading { stamp, allowed: decode(&buf[..(size as usize).min(buf.len())]) })
}

/// The state, or None when it cannot be read or has no data.
pub fn query() -> Option<(u32, bool)> {
    read().ok().and_then(|r| r.allowed.map(|a| (r.stamp, a)))
}

/// What a published value says: a u32, non-zero for allowed (as uDWM reads it).
fn decode(data: &[u8]) -> Option<bool> {
    let bytes: [u8; 4] = data.get(..4)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes) != 0)
}

/// Publish the state for this logon session: 1 allows transparency, 0 turns it off for every
/// window. The NTSTATUS on failure.
pub fn publish(allowed: bool) -> Result<(), i32> {
    let f = ntdll(s!("NtUpdateWnfStateData")).ok_or(0xC000_007Au32 as i32)?;
    let f: Update = unsafe { std::mem::transmute(f) };
    let name = STATE;
    let value = u32::from(allowed).to_le_bytes();
    // No type id, the current session's scope, and no change-stamp check.
    let status = unsafe { f(&name, value.as_ptr(), 4, std::ptr::null(), std::ptr::null(), 0, 0) };
    if status < 0 { Err(status) } else { Ok(()) }
}

/// The user's transparency setting; on when it is missing or unreadable (Windows' default).
pub fn setting() -> bool {
    let mut value = 0u32;
    let mut size = 4u32;
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(SETTING_KEY),
            &HSTRING::from(SETTING_VALUE),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&mut size),
        )
    };
    read.is_err() || value != 0
}

/// The process of Windows' own immersive shell when one runs in this session (Explorer or
/// ShellAppRuntime owns the shell window, or the immersive shell's window exists): it owns this
/// state then, and shellman leaves it alone.
pub fn real_shell() -> Option<u32> {
    let owner = |hwnd: HWND| {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        (pid != 0).then_some(pid)
    };
    let shell = unsafe { GetShellWindow() };
    if !shell.is_invalid() {
        if let Some(pid) = owner(shell).filter(|&p| is_windows_shell(p)) {
            return Some(pid);
        }
    }
    let immersive = unsafe { FindWindowW(w!("ApplicationManager_ImmersiveShellWindow"), PCWSTR::null()) };
    immersive.ok().filter(|h| !h.is_invalid()).and_then(owner)
}

fn is_windows_shell(pid: u32) -> bool {
    let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return false;
    };
    let mut buf = [0u16; MAX_PATH as usize];
    let mut len = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len) }.is_ok();
    unsafe {
        let _ = windows::Win32::Foundation::CloseHandle(process);
    }
    let path = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
    ok && (path.ends_with("\\explorer.exe") || path.ends_with("\\shellappruntime.exe"))
}

/// What to do about the state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    /// Publish this value.
    Publish(bool),
    /// Leave it: it already says what the user wants.
    Matches,
    /// Leave it: Windows' own immersive shell runs and owns it.
    RealShell,
}

/// Publish only what differs from the user's setting (or when nothing was published), and never
/// while Windows' own shell runs. `state` is the state's value, None without data or unreadable.
pub fn decide(state: Option<bool>, wanted: bool, real_shell: bool) -> Act {
    if real_shell {
        Act::RealShell
    } else if state == Some(wanted) {
        Act::Matches
    } else {
        Act::Publish(wanted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_name_is_a_well_known_session_name_of_the_immersive_shell() {
        // Well-known names are stored XORed with this constant (phnt `WNF_STATE_NAME`).
        let plain = STATE ^ 0x41C6_4E6D_A3BC_0074;
        assert_eq!(plain & 0xF, 1, "version");
        assert_eq!((plain >> 4) & 0x3, 0, "lifetime: well-known");
        assert_eq!((plain >> 6) & 0xF, 1, "scope: session");
        assert_eq!((plain >> 10) & 0x1, 0, "not permanent");
        assert_eq!(&(plain >> 32).to_le_bytes()[..4], b"IMSN");
    }

    #[test]
    fn a_value_is_a_u32_non_zero_for_allowed() {
        assert_eq!(decode(&[1, 0, 0, 0]), Some(true));
        assert_eq!(decode(&[0, 0, 0, 0]), Some(false));
        assert_eq!(decode(&[0, 1, 0, 0]), Some(true));
        assert_eq!(decode(&[]), None);
        assert_eq!(decode(&[1, 0]), None);
    }

    #[test]
    fn publishes_only_what_differs_and_never_over_the_real_shell() {
        assert_eq!(decide(None, true, false), Act::Publish(true));
        assert_eq!(decide(Some(false), true, false), Act::Publish(true));
        assert_eq!(decide(Some(true), false, false), Act::Publish(false));
        assert_eq!(decide(Some(true), true, false), Act::Matches);
        assert_eq!(decide(Some(false), false, false), Act::Matches);
        assert_eq!(decide(None, true, true), Act::RealShell);
        assert_eq!(decide(Some(false), true, true), Act::RealShell);
    }
}
