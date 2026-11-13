//! The small pill at the top right: how the folder is sorted (and whether hidden items show),
//! and the window buttons, drawn by Files since the window has no frame.

use super::actions::Action;
use super::tab::SortKey;
use egui::{Id, Rect, Ui, Vec2};
use shellman_ui::chips::{chip, window_buttons, Chip, CHIP_H};
use shellman_ui::menu::popup::{Anchor, Entry};
use shellman_ui::theme::glyph;

const PAD: f32 = 6.0;

/// The keys the sort menu offers, with their names.
const KEYS: [(SortKey, &str); 4] = [
    (SortKey::Name, "Name"),
    (SortKey::Modified, "Date modified"),
    (SortKey::Kind, "Type"),
    (SortKey::Size, "Size"),
];

impl super::FileMan {
    pub(super) fn window_controls(&mut self, ui: &mut Ui, pill: Rect) {
        let inner = pill.shrink(PAD);
        let sort = Rect::from_min_size(
            egui::pos2(inner.left() + 2.0, inner.center().y - CHIP_H / 2.0),
            Vec2::new(78.0, CHIP_H),
        );
        let resp = chip(
            ui,
            sort,
            Id::new("sort"),
            "Sort",
            Some(glyph::SORT_DOWN),
            Chip::Plain,
        )
        .on_hover_text("Sort, and hidden items");
        if resp.clicked() {
            let tab = &self.tabs[self.active];
            let entries = sort_entries(tab.sort, tab.ascending, self.settings.show_hidden);
            super::menu::request(Anchor::under(ui.ctx(), resp.rect), "sort", entries);
        }
        let buttons = Rect::from_min_max(egui::pos2(sort.right() + 6.0, inner.top()), inner.max);
        window_buttons(ui, buttons);
    }
}

/// The sort menu: the keys (picking the current one turns the order around), the order, and
/// whether hidden items show.
pub(super) fn sort_entries(sort: SortKey, ascending: bool, show_hidden: bool) -> Vec<Entry<Action>> {
    let mut e: Vec<Entry<Action>> =
        KEYS.iter().map(|&(key, name)| Entry::item(Action::SortBy(key), name).checked(sort == key)).collect();
    e.push(Entry::separator());
    for (asc, name) in [(true, "Ascending"), (false, "Descending")] {
        e.push(Entry::item(Action::SortOrder(asc), name).checked(ascending == asc));
    }
    e.push(Entry::separator());
    e.push(Entry::item(Action::ToggleHidden, "Hidden items").checked(show_hidden).hint("Ctrl+H"));
    e
}

/// What a pick in the sort menu does to the sort: (key, ascending).
pub(super) fn sort_by(now: (SortKey, bool), key: SortKey) -> (SortKey, bool) {
    if now.0 == key {
        (key, !now.1)
    } else {
        (key, key == SortKey::Name || key == SortKey::Kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sort_menu() {
        let e = sort_entries(SortKey::Size, false, true);
        let rows: Vec<(String, bool)> = e
            .iter()
            .map(|e| match e {
                Entry::Item(i) => (i.label.clone(), i.checked),
                _ => ("-".into(), false),
            })
            .collect();
        let want = [
            ("Name", false),
            ("Date modified", false),
            ("Type", false),
            ("Size", true),
            ("-", false),
            ("Ascending", false),
            ("Descending", true),
            ("-", false),
            ("Hidden items", true),
        ];
        assert_eq!(rows, want.map(|(l, c)| (l.to_string(), c)));
        let Entry::Item(hidden) = &e[8] else { panic!() };
        assert_eq!(hidden.hint, "Ctrl+H");
    }

    #[test]
    fn picking_the_current_key_turns_the_order_around() {
        assert_eq!(sort_by((SortKey::Name, true), SortKey::Name), (SortKey::Name, false));
        assert_eq!(sort_by((SortKey::Name, true), SortKey::Size), (SortKey::Size, false));
        assert_eq!(sort_by((SortKey::Size, false), SortKey::Kind), (SortKey::Kind, true));
        assert_eq!(sort_by((SortKey::Kind, true), SortKey::Modified), (SortKey::Modified, false));
    }
}