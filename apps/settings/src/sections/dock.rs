//! Dock: the icon size, which monitors have one, and the pinned apps with their order.

use crate::page;
use crate::state::State;
use egui::{Align, FontId, Layout, Sense, UiBuilder, Vec2};
use shellman_core::settings::bar::Docks;
use shellman_core::settings::dock::IconSize;
use shellman_ui::theme::{self, colors, glyph, icon_font};
use shellman_ui::widgets::{draw_text, round_button};

/// The height of a pin item's content; the list adds its own padding around it.
const ROW_H: f32 = 36.0;
const ICON: f32 = 28.0;
/// Segoe Fluent's down arrow, which the shared glyph list has no name for.
const DOWN: char = '\u{E74B}';

/// What an item's buttons asked for.
enum Edit {
    Up(usize),
    Down(usize),
    Unpin(usize),
}

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "Dock", "The icons at the bottom of the screen.");

    page::group(ui, "Icons", |ui| {
        let options = IconSize::ALL.map(|s| (s, s.label()));
        if page::row(ui, "Icon size", "", |ui| page::segmented(ui, &mut st.dock.icon_size, &options)) {
            st.save_dock();
        }
        // Kept with the bar's settings, since it decides which bar windows each monitor gets.
        let options = Docks::ALL.map(|d| (d, d.label()));
        if page::row(ui, "Show the dock on", "", |ui| page::segmented(ui, &mut st.bar.docks, &options)) {
            st.save_bar();
        }
    });

    page::group(ui, "Pinned apps", |ui| pinned(ui, st));
}

fn pinned(ui: &mut egui::Ui, st: &mut State) {
    if st.dock.pins.is_empty() {
        page::row_with(ui, |ui| {
            ui.add(egui::Label::new(theme::muted("Nothing is pinned. Right-click an app in the dock and choose Pin to dock.").size(12.5)).wrap());
        });
        return;
    }
    let mut edit = None;
    for i in 0..st.dock.pins.len() {
        if let Some(e) = pin_row(ui, st, i) {
            edit = Some(e);
        }
    }
    if let Some(edit) = edit {
        apply(st, edit);
    }
}

/// Carry out one button press, and save.
fn apply(st: &mut State, edit: Edit) {
    let moved = match edit {
        Edit::Up(i) => st.dock.shift(i, false),
        Edit::Down(i) => st.dock.shift(i, true),
        Edit::Unpin(i) => {
            if i < st.dock.pins.len() {
                st.dock.pins.remove(i);
                true
            } else {
                false
            }
        }
    };
    if moved {
        st.rebuild_names();
        st.save_dock();
    }
}

/// One pin, an item of the list: its icon, its name, and move up, move down and unpin.
fn pin_row(ui: &mut egui::Ui, st: &mut State, i: usize) -> Option<Edit> {
    page::item(ui, false, |ui| {
        let c = colors();
        let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::hover());
        let icon = egui::Rect::from_center_size(egui::pos2(row.left() + ICON / 2.0, row.center().y), Vec2::splat(ICON));
        let image = st
            .pin_icon(i)
            .and_then(|p| shellman_ui::app_icon::get(ui.ctx(), &p, true, &mut st.thumbs));
        match image {
            Some((id, size)) => {
                let scale = (ICON / size.x).min(ICON / size.y);
                egui::Image::from_texture((id, size)).paint_at(ui, egui::Rect::from_center_size(icon.center(), size * scale));
            }
            None => {
                ui.painter().text(icon.center(), egui::Align2::CENTER_CENTER, glyph::APP, icon_font(18.0), c.muted);
            }
        }

        // The buttons take the right end; the name gets what is left.
        let buttons = egui::Rect::from_min_max(egui::pos2(row.right() - 3.0 * 36.0, row.top()), row.max);
        let name = egui::Rect::from_min_max(egui::pos2(icon.right() + 12.0, row.top()), egui::pos2(buttons.left() - 8.0, row.bottom()));
        let label = st.dock_names.get(i).map_or("", String::as_str);
        draw_text(ui, name, label, FontId::proportional(13.5), c.text, false);

        let mut edit = None;
        let last = st.dock.pins.len() - 1;
        let mut child = ui.new_child(UiBuilder::new().max_rect(buttons).layout(Layout::right_to_left(Align::Center)));
        child.spacing_mut().item_spacing.x = 4.0;
        if round_button(&mut child, glyph::CLOSE, true, "Unpin").clicked() {
            edit = Some(Edit::Unpin(i));
        }
        if round_button(&mut child, DOWN, i < last, "Move later").clicked() {
            edit = Some(Edit::Down(i));
        }
        if round_button(&mut child, glyph::UP, i > 0, "Move earlier").clicked() {
            edit = Some(Edit::Up(i));
        }
        edit
    })
}
