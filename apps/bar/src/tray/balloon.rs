//! Balloons: tray apps' older way to notify (`Shell_NotifyIcon` with `NIF_INFO`). Explorer turns
//! them into toasts; without Explorer they come to shellman's tray and were dropped. Now each one
//! becomes a notification of shellman's own (it never reaches Windows' store, so it lives only
//! in this bar process): it pops up and is listed like any toast, and a click on it tells the app
//! (`NIN_BALLOONUSERCLICK`), as Explorer would.

use shellman_core::notify::{Activation, Toast};
use std::sync::Mutex;
use std::time::SystemTime;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_USER};

/// The message a version-4 app gets when its balloon is clicked (`NIN_BALLOONUSERCLICK`).
const NIN_BALLOONUSERCLICK: u32 = WM_USER + 5;
/// Balloons' ids, above any id the platform's store hands out in practice.
pub const FIRST_ID: u32 = 0xF000_0000;

/// One balloon, as the app sent it.
#[derive(Clone, Debug, PartialEq)]
pub struct Balloon {
    pub hwnd: isize,
    pub uid: u32,
    pub pid: u32,
    pub callback: u32,
    pub version: u32,
    /// The icon's tip, else the program's name: who it is from.
    pub app: String,
    pub title: String,
    pub text: String,
}

static QUEUE: Mutex<Vec<Balloon>> = Mutex::new(Vec::new());

/// A balloon came (on the tray's thread).
pub fn push(b: Balloon) {
    if let Ok(mut q) = QUEUE.lock() {
        q.push(b);
    }
    crate::host::request_frame();
}

/// The balloons that came since the last call.
pub fn take() -> Vec<Balloon> {
    QUEUE.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
}

/// The notification a balloon shows as. The tip's first line names the app.
pub fn to_toast(b: &Balloon, id: u32, time: SystemTime) -> Toast {
    let app = b.app.lines().next().unwrap_or("").trim();
    let app = if app.is_empty() { "App" } else { app };
    let (title, body) = match (b.title.trim(), b.text.trim()) {
        ("", text) => (text.to_owned(), Vec::new()),
        (title, "") => (title.to_owned(), Vec::new()),
        (title, text) => (title.to_owned(), text.lines().map(str::to_owned).collect()),
    };
    Toast {
        id,
        // No AppUserModelID: the icon falls back to the generic app glyph.
        app_id: String::new(),
        app_name: app.to_owned(),
        app_icon: None,
        title,
        body,
        attribution: None,
        time,
        launch: String::new(),
        launch_kind: Activation::Foreground,
        actions: Vec::new(),
        reply: None,
    }
}

/// The message posted to the app when its balloon is clicked: (wParam, lParam).
pub fn click_message(version: u32, uid: u32) -> (usize, isize) {
    if version >= 4 {
        (0, (NIN_BALLOONUSERCLICK | (uid & 0xFFFF) << 16) as isize)
    } else {
        (uid as usize, NIN_BALLOONUSERCLICK as isize)
    }
}

/// Tell the app its balloon was clicked (it usually opens its window).
pub fn clicked(b: &Balloon) {
    if b.callback == 0 {
        return;
    }
    shellman_core::foreground::allow_foreground(b.pid);
    let (w, l) = click_message(b.version, b.uid);
    unsafe {
        let _ = PostMessageW(Some(HWND(b.hwnd as *mut _)), b.callback, WPARAM(w), LPARAM(l));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn balloon(title: &str, text: &str) -> Balloon {
        Balloon { hwnd: 1, uid: 7, pid: 2, callback: 0x8001, version: 4, app: "Backup\nidle".into(), title: title.into(), text: text.into() }
    }

    #[test]
    fn a_balloon_reads_as_a_notification() {
        let t = to_toast(&balloon("Backup done", "12 files\nin 3 s"), FIRST_ID, SystemTime::UNIX_EPOCH);
        assert_eq!((t.app_name.as_str(), t.title.as_str()), ("Backup", "Backup done"));
        assert_eq!(t.body, ["12 files", "in 3 s"]);
        assert!(t.app_id.is_empty() && t.actions.is_empty());
        // Text alone becomes the title; an empty tip still names something.
        let t = to_toast(&Balloon { app: String::new(), ..balloon("", "Disk almost full") }, FIRST_ID + 1, SystemTime::UNIX_EPOCH);
        assert_eq!((t.app_name.as_str(), t.title.as_str()), ("App", "Disk almost full"));
        assert!(t.body.is_empty());
    }

    #[test]
    fn the_click_is_told_the_way_the_app_asked() {
        assert_eq!(click_message(4, 7), (0, (0x405 | (7 << 16)) as isize));
        assert_eq!(click_message(0, 7), (7, 0x405));
    }
}
