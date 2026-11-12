//! The small pill at the top right: how the folder is sorted (and whether hidden items show),
//! and the window buttons, drawn by Files since the window has no frame.

use super::actions::Action;
use super::tab::SortKey;
use egui::{Id, PopupCloseBehavior, Rect, Ui, Vec2};
use shellman_ui::chips::{chip, window_buttons, Chip, CHIP_H};
use shellman_ui::theme::glyph;
use shellman_ui::widgets::menu_item;

const PAD: f32 = 6.0;

/// The keys the sort menu offers, with their names.
const KEYS: [(SortKey, &str); 4] = [
    (SortKey::Name, "Name"),
    (SortKey::Modified, "Date modified"),
    (SortKey::Kind, "Type"),
    (SortKey::Size, "Size"),
];

impl super::FileMan {
    pub(super) fn window_controls(&mut self, ui: &mut Ui, pill: Rect, actions: &mut Vec<Action>) {
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
        let popup = Id::new("sort-menu");
        if resp.clicked() {
            ui.memory_mut(|m| m.toggle_popup(popup));
        }
        egui::popup::popup_below_widget(
            ui,
            popup,
            &resp,
            PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                self.sort_menu(ui, actions);
            },
        );
        let buttons = Rect::from_min_max(egui::pos2(sort.right() + 6.0, inner.top()), inner.max);
        window_buttons(ui, buttons);
    }

    fn sort_menu(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        ui.set_min_width(200.0);
        let tab = &mut self.tabs[self.active];
        for (key, name) in KEYS {
            let g = if tab.sort == key { glyph::CHECK } else { ' ' };
            if menu_item(ui, g, name, "") {
                if tab.sort == key {
                    tab.ascending = !tab.ascending;
                } else {
                    tab.sort = key;
                    tab.ascending = key == SortKey::Name || key == SortKey::Kind;
                }
                tab.dirty = true;
                ui.memory_mut(|m| m.close_popup());
            }
        }
        ui.separator();
        for (ascending, name) in [(true, "Ascending"), (false, "Descending")] {
            let g = if tab.ascending == ascending {
                glyph::CHECK
            } else {
                ' '
            };
            if menu_item(ui, g, name, "") {
                tab.ascending = ascending;
                tab.dirty = true;
                ui.memory_mut(|m| m.close_popup());
            }
        }
        ui.separator();
        let g = if self.settings.show_hidden {
            glyph::CHECK
        } else {
            ' '
        };
        if menu_item(ui, g, "Hidden items", "Ctrl+H") {
            actions.push(Action::ToggleHidden);
            ui.memory_mut(|m| m.close_popup());
        }
    }
}
