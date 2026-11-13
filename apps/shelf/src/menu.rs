//! The desktop's right-click menu, shown at the point the desktop process reports, on whichever
//! monitor was clicked.
//!
//! It is a plain Windows popup, the component the dock's right-click menu uses: Windows sizes it
//! to its entries, keeps it on the monitor, opens the submenu, and closes it on Esc or a click
//! anywhere else. A first version drew shellman's own menu on glass in a window as large as the
//! monitor; opening it repeatedly crashed the compositor (`dwm.exe`, 18:00:02 on 12 Nov), and a
//! monitor-sized window also made the bar step aside as for a fullscreen app. The menu's entries
//! are plain data (`entries`), so a glass menu can come back on top of them once it is safe.

use windows::core::HSTRING;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetForegroundWindow, PostMessageW, SetForegroundWindow,
    TrackPopupMenuEx, HMENU, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, TPM_LEFTALIGN, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TPM_TOPALIGN, WM_NULL,
};

/// What the menu can do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    NewFolder,
    NewText,
    NewNote,
    Paste,
    OpenInFiles,
    Terminal,
    Wallpaper,
    DesktopSettings,
    DisplaySettings,
}

impl Action {
    const ALL: [Action; 9] = [
        Action::NewFolder,
        Action::NewText,
        Action::NewNote,
        Action::Paste,
        Action::OpenInFiles,
        Action::Terminal,
        Action::Wallpaper,
        Action::DesktopSettings,
        Action::DisplaySettings,
    ];

    /// The command id Windows hands back (never 0, which means nothing was picked).
    fn id(self) -> usize {
        Self::ALL.iter().position(|a| *a == self).map_or(0, |i| i + 1)
    }

    fn from_id(id: usize) -> Option<Action> {
        id.checked_sub(1).and_then(|i| Self::ALL.get(i).copied())
    }
}

/// One line of the menu.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    Item { label: &'static str, action: Action, enabled: bool },
    Sub { label: &'static str, entries: Vec<Entry> },
    Separator,
}

fn item(label: &'static str, action: Action) -> Entry {
    Entry::Item { label, action, enabled: true }
}

/// The menu's entries. Paste is greyed out when there is nothing to paste.
pub fn entries(can_paste: bool) -> Vec<Entry> {
    vec![
        Entry::Sub {
            label: "&New",
            entries: vec![
                item("&Folder", Action::NewFolder),
                item("&Text document", Action::NewText),
                item("&Markdown note", Action::NewNote),
            ],
        },
        Entry::Item { label: "&Paste", action: Action::Paste, enabled: can_paste },
        Entry::Separator,
        item("Open in &Files", Action::OpenInFiles),
        item("Open &Terminal here", Action::Terminal),
        Entry::Separator,
        item("Change &wallpaper…", Action::Wallpaper),
        item("&Desktop settings", Action::DesktopSettings),
        item("Displa&y settings", Action::DisplaySettings),
    ]
}

unsafe fn build(entries: &[Entry]) -> Option<HMENU> {
    let menu = CreatePopupMenu().ok()?;
    for e in entries {
        match e {
            Entry::Item { label, action, enabled } => {
                let flags = if *enabled { MF_STRING } else { MF_STRING | MF_GRAYED };
                let _ = AppendMenuW(menu, flags, action.id(), &HSTRING::from(*label));
            }
            Entry::Sub { label, entries } => {
                if let Some(sub) = build(entries) {
                    // The submenu is destroyed with its parent.
                    let _ = AppendMenuW(menu, MF_STRING | MF_POPUP, sub.0 as usize, &HSTRING::from(*label));
                }
            }
            Entry::Separator => {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
            }
        }
    }
    Some(menu)
}

/// Show the menu at `at` (screen pixels), owned by the shelf window `owner`. Blocks until it
/// closes; returns what was picked.
pub fn show(owner: isize, at: (i32, i32), can_paste: bool) -> Option<Action> {
    let hwnd = HWND(owner as *mut _);
    unsafe {
        let menu = build(&entries(can_paste))?;
        // A menu closes on a click elsewhere only if its owner is in front, so it borrows the
        // foreground (the desktop let this process take it) and gives it back after, unless the
        // click that closed it went to another window, which then stays in front.
        let before = GetForegroundWindow();
        let _ = SetForegroundWindow(hwnd);
        let picked = TrackPopupMenuEx(
            menu,
            (TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_TOPALIGN | TPM_LEFTALIGN).0,
            at.0,
            at.1,
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        if !before.is_invalid() && before != hwnd && GetForegroundWindow() == hwnd {
            let _ = SetForegroundWindow(before);
        }
        Action::from_id(picked.0 as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(entries: &[Entry]) -> Vec<String> {
        entries
            .iter()
            .filter_map(|e| match e {
                Entry::Item { label, .. } | Entry::Sub { label, .. } => Some(label.replace('&', "")),
                Entry::Separator => None,
            })
            .collect()
    }

    #[test]
    fn the_menu_has_the_designs_entries_in_order() {
        let e = entries(true);
        assert_eq!(
            labels(&e),
            ["New", "Paste", "Open in Files", "Open Terminal here", "Change wallpaper…", "Desktop settings", "Display settings"]
        );
        let Entry::Sub { entries: sub, .. } = &e[0] else { panic!("New opens a submenu") };
        assert_eq!(labels(sub), ["Folder", "Text document", "Markdown note"]);
    }

    #[test]
    fn paste_is_greyed_out_with_nothing_to_paste() {
        let paste = |can| {
            entries(can).into_iter().find_map(|e| match e {
                Entry::Item { action: Action::Paste, enabled, .. } => Some(enabled),
                _ => None,
            })
        };
        assert_eq!(paste(true), Some(true));
        assert_eq!(paste(false), Some(false));
    }

    #[test]
    fn command_ids_round_trip_and_zero_is_nothing() {
        for a in Action::ALL {
            assert_ne!(a.id(), 0);
            assert_eq!(Action::from_id(a.id()), Some(a));
        }
        assert_eq!(Action::from_id(0), None);
        assert_eq!(Action::from_id(99), None);
    }
}
