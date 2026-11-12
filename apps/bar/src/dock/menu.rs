//! The dock's right-click menu: a plain Windows popup, since the dock window is only as tall as
//! its icons and an in-window menu would be cut off.

use windows::core::HSTRING;
use windows::Win32::Foundation::{HWND, POINT, WPARAM, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, GetForegroundWindow, PostMessageW,
    SetForegroundWindow, TrackPopupMenuEx, MF_GRAYED, MF_STRING, TPM_BOTTOMALIGN, TPM_LEFTALIGN,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_NULL,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    Nothing,
    TogglePin,
    Close,
    Settings,
    OpenBin,
    EmptyBin,
}

/// Which icon the menu is for.
pub enum Kind {
    /// The Launcher: just a way into the settings.
    Launcher,
    /// An app. `pinned` is `None` for icons that cannot be pinned or unpinned; `windows` is how
    /// many windows it has open.
    App { pinned: Option<bool>, windows: usize },
    /// The Recycle Bin. Emptying is offered only when `can_empty`.
    Bin { can_empty: bool },
}

const PIN: usize = 1;
const CLOSE: usize = 2;
const SETTINGS: usize = 3;
const OPEN_BIN: usize = 4;
const EMPTY_BIN: usize = 5;

/// Show the menu at the pointer, opening upwards. Blocks until it closes.
pub fn show(hwnd: isize, kind: Kind) -> Choice {
    let hwnd = HWND(hwnd as *mut _);
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return Choice::Nothing;
        };
        match kind {
            Kind::Launcher => {
                let _ = AppendMenuW(menu, MF_STRING, SETTINGS, &HSTRING::from("Settings"));
            }
            Kind::Bin { can_empty } => {
                let _ = AppendMenuW(menu, MF_STRING, OPEN_BIN, &HSTRING::from("Open"));
                let state = if can_empty { MF_STRING } else { MF_STRING | MF_GRAYED };
                let _ = AppendMenuW(menu, state, EMPTY_BIN, &HSTRING::from("Empty Recycle Bin"));
            }
            Kind::App { pinned, windows } => {
                if let Some(pinned) = pinned {
                    let text = if pinned { "Unpin from dock" } else { "Pin to dock" };
                    let _ = AppendMenuW(menu, MF_STRING, PIN, &HSTRING::from(text));
                }
                if windows > 0 {
                    let text = if windows == 1 { "Close window" } else { "Close all windows" };
                    let _ = AppendMenuW(menu, MF_STRING, CLOSE, &HSTRING::from(text));
                }
            }
        }
        let mut at = POINT::default();
        let _ = GetCursorPos(&mut at);
        // A menu closes when you click elsewhere only if its owner is in front, so borrow the
        // foreground for its duration and give it back after, so the app you were in stays "in
        // front" for the dock's own marks and the top bar.
        let before = GetForegroundWindow();
        let _ = SetForegroundWindow(hwnd);
        let picked = TrackPopupMenuEx(
            menu,
            (TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_LEFTALIGN).0,
            at.x,
            at.y,
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        // Only if nothing took the foreground meanwhile: a click on another window to dismiss the
        // menu activates that window, which must stay in front.
        if !before.is_invalid() && before != hwnd && GetForegroundWindow() == hwnd {
            let _ = SetForegroundWindow(before);
        }
        match picked.0 as usize {
            PIN => Choice::TogglePin,
            CLOSE => Choice::Close,
            SETTINGS => Choice::Settings,
            OPEN_BIN => Choice::OpenBin,
            EMPTY_BIN => Choice::EmptyBin,
            _ => Choice::Nothing,
        }
    }
}
