//! Opening a page of Windows Settings (an `ms-settings:` link).
//!
//! Next to Explorer this simply works. With Explorer stopped it does not, and nothing here can
//! change that (measured on Windows 11 26300): ShellExecute finds no handler for the link ("no
//! association"), WinRT's `Launcher` answers false, and even activating the Settings app directly
//! through the activation manager times out after 50 s (0x80040900), since immersive apps need
//! what Explorer hosts. So the link is tried once, off the UI thread, and the panel stays open:
//! if Settings does open, it takes the focus and the panel closes on its own.
//!
//! The panel does not offer the links at all when `available` says no.

use windows::core::{w, HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, MAX_PATH};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// Open `uri` (such as `ms-settings:bluetooth`) from a thread of its own.
pub fn open(uri: &'static str) {
    std::thread::spawn(move || {
        // SAFETY: COM initialised on this thread first; the strings outlive the call.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(uri), None, None, SW_SHOWNORMAL);
        }
    });
}

/// Whether Explorer is the shell right now, which is what services `ms-settings:` links: the
/// window of class `Shell_TrayWnd` (every shell has one, shellman's own included) belongs to a
/// process called `explorer.exe`. A few Win32 calls, no waiting, so it is asked at every
/// showing; a link that still fails is just a click that does nothing.
pub fn available() -> bool {
    // SAFETY: plain queries; the process handle is closed before returning.
    unsafe {
        let Ok(tray) = FindWindowW(w!("Shell_TrayWnd"), None) else {
            return false;
        };
        let mut pid = 0u32;
        GetWindowThreadProcessId(tray, Some(&mut pid));
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut buf = [0u16; MAX_PATH as usize];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(process);
        ok && String::from_utf16_lossy(&buf[..len as usize]).to_lowercase().ends_with("\\explorer.exe")
    }
}
