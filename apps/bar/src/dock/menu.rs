//! The dock's right-click menu: shellman's glass popup menu (`shellman_ui::menu::popup`), a
//! window of its own since the dock window is only as tall as its icons. It opens upwards from
//! the pointer; what is picked comes back to the dock's next frame as a [`Choice`].

use shellman_core::recycle_bin::{self, BinState};
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
    /// The first step: ask, in the menu itself.
    EmptyBin,
    /// The second: empty it for good.
    EmptyBinConfirmed,
    Cancel,
}

/// Which icon the menu is for.
pub enum Kind {
    /// The Launcher: just a way into the settings.
    Launcher,
    /// An app. `pinned` is `None` for icons that cannot be pinned or unpinned; `windows` is how
    /// many windows it has open.
    App { pinned: Option<bool>, windows: usize },
    /// The Recycle Bin, with what is in it (None while not known yet). Emptying is offered only
    /// when something is.
    Bin { state: Option<BinState> },
    /// The bin's second step: really empty it?
    ConfirmEmpty { state: BinState },
}

/// The menu's entries.
pub fn entries(kind: &Kind) -> Vec<Entry<Choice>> {
    match *kind {
        Kind::Launcher => vec![Entry::item(Choice::Settings, "Settings").icon(glyph::SETTINGS)],
        Kind::Bin { state } => {
            let what = match state {
                Some(s) if s.is_empty() => "Empty".to_owned(),
                Some(s) => recycle_bin::summary(s),
                None => "Looking…".to_owned(),
            };
            let can_empty = state.is_some_and(|s| !s.is_empty());
            vec![
                Entry::item(Choice::OpenBin, "Open").icon(glyph::OPEN_EXTERNAL),
                Entry::separator(),
                Entry::heading(what),
                Entry::item(Choice::EmptyBin, "Empty Recycle Bin…").icon(glyph::DELETE).enabled(can_empty).danger(),
            ]
        }
        Kind::ConfirmEmpty { state } => vec![
            Entry::heading(format!("Delete {} for good?", recycle_bin::summary(state))),
            Entry::item(Choice::EmptyBinConfirmed, "Empty Recycle Bin").icon(glyph::DELETE).danger(),
            Entry::item(Choice::Cancel, "Cancel"),
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
        let empty = Some(BinState::default());
        assert_eq!(
            ids(&entries(&Kind::Bin { state: empty })),
            [(Choice::OpenBin, true, "Open".into()), (Choice::EmptyBin, false, "Empty Recycle Bin…".into())]
        );
        // Not known yet: nothing to empty either.
        assert!(!ids(&entries(&Kind::Bin { state: None }))[1].1);
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

    fn headings(e: &[Entry<Choice>]) -> Vec<String> {
        e.iter().filter_map(|e| if let Entry::Heading(h) = e { Some(h.clone()) } else { None }).collect()
    }

    #[test]
    fn emptying_the_bin_says_how_much_and_asks_first() {
        let state = BinState { items: 3, bytes: 3 * 1024 * 1024 };
        let e = entries(&Kind::Bin { state: Some(state) });
        assert_eq!(headings(&e), ["3 items, 3.0 MB"]);
        let Entry::Item(first) = &e[3] else { panic!("an item") };
        assert!(first.danger && first.enabled && first.id == Choice::EmptyBin);
        // The first step only opens the second, which names what goes and is red too.
        let c = entries(&Kind::ConfirmEmpty { state });
        assert_eq!(headings(&c), ["Delete 3 items, 3.0 MB for good?"]);
        assert_eq!(ids(&c).iter().map(|i| i.0).collect::<Vec<_>>(), [Choice::EmptyBinConfirmed, Choice::Cancel]);
        let Entry::Item(sure) = &c[1] else { panic!("an item") };
        assert!(sure.danger);
        assert_eq!(headings(&entries(&Kind::Bin { state: Some(BinState::default()) })), ["Empty"]);
    }
}
