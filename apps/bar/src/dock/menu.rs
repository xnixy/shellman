//! The dock's right-click menu: shellman's glass popup menu (`shellman_ui::menu::popup`), a
//! window of its own since the dock window is only as tall as its icons. It opens upwards from
//! the pointer; what is picked comes back to the dock's next frame as a [`Choice`].

use shellman_ui::menu::popup::{Anchor, Entry};
use shellman_ui::theme::glyph;
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
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

/// The menu's entries: the same as the plain Windows menu offered before.
pub fn entries(kind: &Kind) -> Vec<Entry<Choice>> {
    match *kind {
        Kind::Launcher => vec![Entry::item(Choice::Settings, "Settings").icon(glyph::SETTINGS)],
        Kind::Bin { can_empty } => vec![
            Entry::item(Choice::OpenBin, "Open").icon(glyph::OPEN_EXTERNAL),
            Entry::item(Choice::EmptyBin, "Empty Recycle Bin").icon(glyph::DELETE).enabled(can_empty).danger(),
        ],
        Kind::App { pinned, windows } => {
            let mut e = Vec::new();
            if let Some(pinned) = pinned {
                e.push(if pinned {
                    Entry::item(Choice::TogglePin, "Unpin from dock").icon(glyph::UNPIN)
                } else {
                    Entry::item(Choice::TogglePin, "Pin to dock").icon(glyph::PIN)
                });
            }
            if windows > 0 {
                let text = if windows == 1 { "Close window" } else { "Close all windows" };
                e.push(Entry::item(Choice::Close, text).icon(glyph::CLOSE));
            }
            e
        }
    }
}

/// Where the menu opens: above the pointer.
pub fn anchor() -> Anchor {
    let mut at = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut at);
    }
    Anchor::above(at.x, at.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(e: &[Entry<Choice>]) -> Vec<(Choice, bool, String)> {
        e.iter()
            .filter_map(|e| match e {
                Entry::Item(i) => Some((i.id, i.enabled, i.label.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn each_icon_offers_what_it_did() {
        assert_eq!(ids(&entries(&Kind::Launcher)), [(Choice::Settings, true, "Settings".into())]);
        assert_eq!(
            ids(&entries(&Kind::Bin { can_empty: false })),
            [(Choice::OpenBin, true, "Open".into()), (Choice::EmptyBin, false, "Empty Recycle Bin".into())]
        );
        assert_eq!(
            ids(&entries(&Kind::App { pinned: Some(true), windows: 2 })),
            [(Choice::TogglePin, true, "Unpin from dock".into()), (Choice::Close, true, "Close all windows".into())]
        );
        assert_eq!(
            ids(&entries(&Kind::App { pinned: Some(false), windows: 1 })),
            [(Choice::TogglePin, true, "Pin to dock".into()), (Choice::Close, true, "Close window".into())]
        );
        // Nothing to offer: no menu.
        assert!(entries(&Kind::App { pinned: None, windows: 0 }).is_empty());
    }

    #[test]
    fn emptying_the_bin_is_marked_destructive() {
        let e = entries(&Kind::Bin { can_empty: true });
        let Entry::Item(empty) = &e[1] else { panic!("an item") };
        assert!(empty.danger && empty.enabled);
    }
}
