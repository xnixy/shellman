//! Opening a page of Windows Settings (an `ms-settings:` link).
//!
//! Next to Explorer this simply works. With Explorer stopped it does not, and nothing here can
//! change that (measured on Windows 11 26300): ShellExecute finds no handler for the link ("no
//! association"), WinRT's `Launcher` answers false, and even activating the Settings app directly
//! through the activation manager times out after 50 s (0x80040900), since immersive apps need
//! what Explorer hosts. So the link is tried once, off the UI thread, and the panel stays open:
//! if Settings does open, it takes the focus and the panel closes on its own.

use windows::core::HSTRING;
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
