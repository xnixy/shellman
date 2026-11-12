//! Opening a page of Windows Settings (an `ms-settings:` link).
//!
//! Settings is a modern (immersive) app: it opens only while something hosts the immersive shell,
//! which is Explorer, or Windows' own host that shellman keeps running without it (see
//! `shellman_core::app_host`). Without either, every way in fails (measured on Windows 11 26300):
//! ShellExecute finds no handler for the link, WinRT's `Launcher` answers false, and activating
//! the Settings app directly times out after ~45 s. So `available` asks whether the host is
//! there, and the panel offers the links only then; the link is opened off the UI thread, and if
//! Settings opens it takes the focus and the panel closes on its own.
//!
//! A page with an older Control Panel equivalent that opens without Explorer has it as a
//! fallback (`fallback`), offered when Settings cannot open.

use windows::core::HSTRING;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// Open `uri` (such as `ms-settings:bluetooth`) from a thread of its own; its classic
/// equivalent instead when Settings cannot open now.
pub fn open(uri: &'static str) {
    let target = match fallback(uri) {
        Some(applet) if !available() => applet,
        _ => uri,
    };
    std::thread::spawn(move || {
        // SAFETY: COM initialised on this thread first; the strings outlive the call.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(target), None, None, SW_SHOWNORMAL);
        }
    });
}

/// Whether Settings can open right now: Explorer or Windows' host for modern apps is running.
/// A couple of window lookups, no waiting, so it is asked at every showing.
pub fn available() -> bool {
    shellman_core::app_host::modern_apps_available()
}

/// Whether `uri` can be opened now, by Settings or its fallback.
pub fn can_open(uri: &str, settings: bool) -> bool {
    settings || fallback(uri).is_some()
}

/// The Control Panel applet for a Settings page, where one opens without Explorer. Only these:
/// `ncpa.cpl`, `control.exe` and Power Options start Explorer, and `desk.cpl` opens Settings.
pub fn fallback(uri: &str) -> Option<&'static str> {
    match uri {
        "ms-settings:bluetooth" => Some("bthprops.cpl"),
        "ms-settings:sound" => Some("mmsys.cpl"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_pages_with_a_safe_applet_fall_back() {
        assert_eq!(fallback("ms-settings:bluetooth"), Some("bthprops.cpl"));
        assert_eq!(fallback("ms-settings:network-wifi"), None, "ncpa.cpl would start Explorer");
        assert!(can_open("ms-settings:bluetooth", false));
        assert!(!can_open("ms-settings:quiethours", false));
        assert!(can_open("ms-settings:quiethours", true));
    }
}
