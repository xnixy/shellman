//! The clipboard's text, for a right-click paste (egui only hands over the clipboard on its own
//! paste keys), and opening a link with whatever Windows opens it with.

use windows::core::HSTRING;
use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// The clipboard's text, or None when it holds none (or another program has it open).
pub fn text() -> Option<String> {
    unsafe {
        // Another program may hold it a moment: try a few times.
        let mut open = false;
        for _ in 0..5 {
            if OpenClipboard(None).is_ok() {
                open = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        if !open {
            return None;
        }
        let text = (|| {
            let h: HANDLE = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u16;
            if p.is_null() {
                return None;
            }
            let max = GlobalSize(g) / 2;
            let s = std::slice::from_raw_parts(p, max);
            let len = s.iter().position(|&c| c == 0).unwrap_or(max);
            let t = String::from_utf16_lossy(&s[..len]);
            let _ = GlobalUnlock(g);
            Some(t)
        })();
        let _ = CloseClipboard();
        text
    }
}

/// Open `uri` (a link a program printed) with its handler. Only schemes a link should have.
pub fn open(uri: &str) {
    let lower = uri.to_ascii_lowercase();
    if !["http://", "https://", "file://", "ftp://", "mailto:"].iter().any(|s| lower.starts_with(s)) {
        return;
    }
    unsafe {
        ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(uri), None, None, SW_SHOWNORMAL);
    }
}
