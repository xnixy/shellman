//! Whether this process runs with administrator rights (an elevated token).

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

pub fn is_elevated() -> bool {
    // SAFETY: the token handle is closed; the buffer is a TOKEN_ELEVATION of the size passed.
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut info = TOKEN_ELEVATION::default();
        let mut got = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut info as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut got,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && info.TokenIsElevated != 0
    }
}
