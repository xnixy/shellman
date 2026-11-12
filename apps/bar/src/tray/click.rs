//! Telling an app its tray icon was clicked, in the message format the app asked for.

use super::{log, TrayIcon};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, IsWindow, PostMessageW, WM_CONTEXTMENU, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_USER,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Click {
    Left,
    Double,
    Right,
}

/// NIN_SELECT: a version-4 app's "icon chosen" after a left click.
const NIN_SELECT: u32 = WM_USER;

pub fn alive(hwnd: isize) -> bool {
    unsafe { IsWindow(Some(HWND(hwnd as *mut _))).as_bool() }
}

/// The messages Explorer sends for `click`: (message, wParam, lParam), in order.
///
/// Version 0 and 3 apps get the icon ID in wParam and the mouse message in lParam. Version 4
/// apps (`NIM_SETVERSION`) get the anchor point (screen coordinates, one signed 16-bit value
/// each) in wParam, and the event in the low and the icon ID in the high word of lParam. Many
/// version 4 apps open their menu only on `WM_CONTEXTMENU` and act on a left click only on
/// `NIN_SELECT`.
pub fn sequence(version: u32, click: Click, uid: u32, anchor: (i32, i32)) -> Vec<(u32, usize, isize)> {
    let events: &[u32] = match (click, version >= 4) {
        (Click::Left, true) => &[WM_LBUTTONDOWN, WM_LBUTTONUP, NIN_SELECT],
        (Click::Left, false) => &[WM_LBUTTONDOWN, WM_LBUTTONUP],
        (Click::Double, _) => &[WM_LBUTTONDBLCLK, WM_LBUTTONUP],
        (Click::Right, true) => &[WM_RBUTTONDOWN, WM_RBUTTONUP, WM_CONTEXTMENU],
        (Click::Right, false) => &[WM_RBUTTONDOWN, WM_RBUTTONUP],
    };
    events
        .iter()
        .map(|&event| {
            if version >= 4 {
                let xy = (anchor.0 as u16 as usize) | ((anchor.1 as u16 as usize) << 16);
                (event, xy, ((event & 0xFFFF) | ((uid & 0xFFFF) << 16)) as isize)
            } else {
                (event, uid as usize, event as isize)
            }
        })
        .collect()
}

impl TrayIcon {
    pub fn click(&self, click: Click) {
        if self.callback == 0 {
            return;
        }
        let target = HWND(self.hwnd as *mut _);
        if !alive(self.hwnd) {
            log::line(&format!("click {click:?} {}: owner window is gone", self.describe()));
            return;
        }
        // The app usually opens a menu or its window and calls SetForegroundWindow so that the
        // menu closes when the user clicks away; that needs the grant first, every time.
        let (granted, needed_key) = shellman_core::foreground::allow_foreground(self.pid);
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        let messages = sequence(self.version, click, self.uid, (pt.x, pt.y));
        log::line(&format!(
            "click {click:?} {} anchor=({},{}) grant={granted} key_press={needed_key} elevated={}",
            self.describe(),
            pt.x,
            pt.y,
            log::elevated(self.pid)
        ));
        for (msg, wparam, lparam) in messages {
            let sent = unsafe {
                PostMessageW(Some(target), self.callback, WPARAM(wparam), LPARAM(lparam))
            };
            match sent {
                Ok(()) => log::line(&format!("  post {msg:#x} wparam={wparam:#x} lparam={lparam:#x}")),
                // Access denied means an elevated app, which a medium-integrity bar cannot reach.
                Err(e) => log::line(&format!(
                    "  post {msg:#x} FAILED {e} (access denied means an elevated owner)"
                )),
            }
        }
    }

    /// Owner process, version, flags: what a log line needs to name this icon.
    pub fn describe(&self) -> String {
        format!(
            "[{} pid={} uid={} guid={} version={} callback={:#x} tip={:?}]",
            log::process_name(self.pid),
            self.pid,
            self.uid,
            self.guid.is_some(),
            self.version,
            self.callback,
            self.tip
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UID: u32 = 7;

    #[test]
    fn legacy_left_sends_down_up() {
        for v in [0, 3] {
            assert_eq!(
                sequence(v, Click::Left, UID, (100, 20)),
                vec![
                    (WM_LBUTTONDOWN, 7, WM_LBUTTONDOWN as isize),
                    (WM_LBUTTONUP, 7, WM_LBUTTONUP as isize)
                ]
            );
        }
    }

    #[test]
    fn legacy_right_sends_down_up_and_no_context_menu() {
        let s = sequence(0, Click::Right, UID, (1, 2));
        assert_eq!(
            s,
            vec![
                (WM_RBUTTONDOWN, 7, WM_RBUTTONDOWN as isize),
                (WM_RBUTTONUP, 7, WM_RBUTTONUP as isize)
            ]
        );
    }

    #[test]
    fn legacy_double_click() {
        let s = sequence(3, Click::Double, UID, (1, 2));
        assert_eq!(s[0], (WM_LBUTTONDBLCLK, 7, WM_LBUTTONDBLCLK as isize));
        assert_eq!(s[1].0, WM_LBUTTONUP);
    }

    #[test]
    fn v4_right_ends_with_context_menu() {
        let s = sequence(4, Click::Right, UID, (3000, 40));
        let msgs: Vec<u32> = s.iter().map(|m| m.0).collect();
        assert_eq!(msgs, vec![WM_RBUTTONDOWN, WM_RBUTTONUP, WM_CONTEXTMENU]);
        for (event, wparam, lparam) in s {
            assert_eq!(wparam, 3000 | (40 << 16), "anchor packed as x, y");
            assert_eq!(lparam as u32 & 0xFFFF, event);
            assert_eq!(lparam as u32 >> 16, UID, "icon id in the high word");
        }
    }

    #[test]
    fn v4_left_ends_with_select() {
        let s = sequence(4, Click::Left, UID, (5, 6));
        let events: Vec<u32> = s.iter().map(|m| m.0).collect();
        assert_eq!(events, vec![WM_LBUTTONDOWN, WM_LBUTTONUP, NIN_SELECT]);
    }

    #[test]
    fn v4_double_click_and_negative_anchor() {
        let s = sequence(4, Click::Double, UID, (-100, 20));
        assert_eq!(s[0].0, WM_LBUTTONDBLCLK);
        // A monitor left of the primary: the signed 16-bit x survives the packing.
        assert_eq!(s[0].1 as u16 as i16, -100);
        assert_eq!((s[0].1 >> 16) as u16, 20);
    }
}
