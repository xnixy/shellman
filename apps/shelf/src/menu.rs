//! The desktop's right-click menu, shown at the point the desktop process reports, on whichever
//! monitor was clicked.
//!
//! It is shellman's glass popup menu (`shellman_ui::menu::popup`), the one the dock's right-click
//! menu uses: one small window per process, made once and then only placed, shown and put away
//! (a first version on a monitor-sized window, its glass made per click, crashed the compositor
//! on 12 Nov). The entries are plain data (`entries`); the pick comes back as an [`Action`].
//! "Edit desktop…" starts arrange mode (`crate::arrange`): the shelf and the widgets can be
//! dragged about. "Desktop settings" opens Settings on the Desktop page.

use shellman_ui::menu::popup;
use shellman_ui::theme::glyph;

/// What the menu can do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    NewFolder,
    NewText,
    NewNote,
    Paste,
    OpenInFiles,
    Terminal,
    EditDesktop,
    Wallpaper,
    DesktopSettings,
    DisplaySettings,
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
        item("&Edit desktop…", Action::EditDesktop),
        item("Change &wallpaper…", Action::Wallpaper),
        item("&Desktop settings", Action::DesktopSettings),
        item("Displa&y settings", Action::DisplaySettings),
    ]
}

/// The glyph beside each entry.
fn icon(action: Action) -> char {
    match action {
        Action::NewFolder => glyph::NEW_FOLDER,
        Action::NewText => glyph::FILE,
        Action::NewNote => glyph::DOC,
        Action::Paste => glyph::PASTE,
        Action::OpenInFiles => glyph::FOLDER,
        // Segoe's "CommandPrompt".
        Action::Terminal => glyph::APP,
        Action::EditDesktop => glyph::VIEW_GRID,
        Action::Wallpaper => glyph::IMAGE,
        Action::DesktopSettings => glyph::SETTINGS,
        Action::DisplaySettings => glyph::DESKTOP,
    }
}

/// The entries as the glass popup menu takes them. No shortcut hints: none of these has a
/// shortcut on the desktop yet (the mockup's "Ctrl V" and "Ctrl Alt T" do not exist).
pub fn popup_entries(entries: &[Entry]) -> Vec<popup::Entry<Action>> {
    entries
        .iter()
        .map(|e| match e {
            Entry::Item { label, action, enabled } => {
                popup::Entry::item(*action, *label).icon(icon(*action)).enabled(*enabled)
            }
            Entry::Sub { label, entries } => popup::Entry::submenu(*label, popup_entries(entries)).icon(glyph::ADD),
            Entry::Separator => popup::Entry::separator(),
        })
        .collect()
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
            ["New", "Paste", "Open in Files", "Open Terminal here", "Edit desktop…", "Change wallpaper…", "Desktop settings", "Display settings"]
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
    fn the_popup_keeps_the_entries_order_and_state() {
        let p = popup_entries(&entries(false));
        assert_eq!(p.len(), 10);
        let popup::Entry::Submenu { entries: sub, .. } = &p[0] else { panic!("New opens a submenu") };
        assert_eq!(sub.len(), 3);
        let popup::Entry::Item(paste) = &p[1] else { panic!("Paste is an item") };
        assert_eq!((paste.id, paste.enabled, paste.hint.as_str()), (Action::Paste, false, ""));
        assert!(matches!(p[2], popup::Entry::Separator));
        let popup::Entry::Item(edit) = &p[6] else { panic!("an item") };
        assert_eq!(edit.id, Action::EditDesktop);
        let popup::Entry::Item(last) = &p[9] else { panic!("an item") };
        assert_eq!(last.id, Action::DisplaySettings);
    }
}
