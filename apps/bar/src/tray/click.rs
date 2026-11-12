//! Telling an app its tray icon was clicked, in the message format the app asked for.

use super::TrayIcon;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, GetCursorPos, IsWindow, PostMessageW, WM_CONTEXTMENU,
    WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_USER,
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

impl TrayIcon {
    pub fn click(&self, click: Click) {
        if self.callback == 0 {
            return;
        }
        // The app usually opens a menu or its window; let it come to the front.
        unsafe {
            let _ = AllowSetForegroundWindow(self.pid);
        }
        let events: &[u32] = match (click, self.version >= 4) {
            (Click::Left, true) => &[WM_LBUTTONDOWN, WM_LBUTTONUP, NIN_SELECT],
            (Click::Left, false) => &[WM_LBUTTONDOWN, WM_LBUTTONUP],
            (Click::Double, _) => &[WM_LBUTTONDBLCLK, WM_LBUTTONUP],
            (Click::Right, true) => &[WM_RBUTTONDOWN, WM_RBUTTONUP, WM_CONTEXTMENU],
            (Click::Right, false) => &[WM_RBUTTONDOWN, WM_RBUTTONUP],
        };
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        for &event in events {
            let (wparam, lparam) = if self.version >= 4 {
                // Version 4: the anchor point in wParam, the event and icon ID in lParam.
                let anchor = (pt.x as u16 as usize) | ((pt.y as u16 as usize) << 16);
                (anchor, (event as usize & 0xFFFF) | ((self.uid as usize & 0xFFFF) << 16))
            } else {
                (self.uid as usize, event as usize)
            };
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(self.hwnd as *mut _)),
                    self.callback,
                    WPARAM(wparam),
                    LPARAM(lparam as isize),
                );
            }
        }
    }
}
