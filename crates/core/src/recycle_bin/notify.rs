//! Knowing the bin changed, without polling.
//!
//! The bin is a folder per drive and user, `X:\$Recycle.Bin\<SID>`, and every delete, restore and
//! empty adds or removes files directly in it. So the folder watcher the file list already uses
//! serves: watch those folders and re-list when one is reported. `SHChangeNotifyRegister` would
//! need a message window of its own on the UI thread, and Files' window procedure is shared with
//! the shell menu hook.

use std::path::PathBuf;
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// DRIVE_FIXED. Removable drives get no bin.
const DRIVE_FIXED: u32 = 3;

/// The bin folders that exist now, one per fixed drive that has had something deleted on it.
/// A drive's folder appears with its first deletion, so ask again when listing.
pub fn watch_dirs() -> Vec<PathBuf> {
    let Some(sid) = user_sid() else { return Vec::new() };
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| (b'A' + i) as char)
        .filter(|l| unsafe { GetDriveTypeW(&HSTRING::from(format!("{l}:\\"))) } == DRIVE_FIXED)
        .map(|l| bin_dir(l, &sid))
        .filter(|p| p.is_dir())
        .collect()
}

fn bin_dir(drive: char, sid: &str) -> PathBuf {
    PathBuf::from(format!("{drive}:\\$Recycle.Bin\\{sid}"))
}

/// The running user's SID as text, like `S-1-5-21-...`.
fn user_sid() -> Option<String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;
        let mut len = 0u32;
        // The first call only reports the size.
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        let got = GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr().cast()), len, &mut len);
        let _ = CloseHandle(token);
        got.ok()?;
        let user = &*(buf.as_ptr().cast::<TOKEN_USER>());
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut text).ok()?;
        let sid = text.to_string().ok();
        let _ = LocalFree(Some(HLOCAL(text.0.cast())));
        sid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bin_folder_path() {
        assert_eq!(bin_dir('D', "S-1-5-21-1"), PathBuf::from(r"D:\$Recycle.Bin\S-1-5-21-1"));
    }

    #[test]
    fn the_user_has_a_sid() {
        let sid = user_sid().expect("sid");
        assert!(sid.starts_with("S-1-"), "{sid}");
    }
}
