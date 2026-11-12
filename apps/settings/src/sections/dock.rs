//! Dock: the icon size, and the pinned apps with their order.

use crate::page;
use crate::state::State;
use egui::{Align, CornerRadius, FontId, Layout, Sense, UiBuilder, Vec2};
use shellman_core::settings::dock::IconSize;
use shellman_ui::theme::{self, colors, glyph, icon_font};
use shellman_ui::widgets::{draw_text, round_button};

const ROW_H: f32 = 48.0;
const ICON: f32 = 28.0;
/// Segoe Fluent's down arrow, which the shared glyph list has no name for.
const DOWN: char = '\u{E74B}';

/// What a row's buttons asked for.
enum Edit {
    Up(usize),
    Down(usize),
    Unpin(usize),
}

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "Dock", "The icons at the bottom of the screen.");

    page::label(ui, "Icon size");
    let options = IconSize::ALL.map(|s| (s, s.label()));
    if page::segmented(ui, &mut st.dock.icon_size, &options) {
        st.save_dock();
    }

    page::gap(ui);
    page::label(ui, "Pinned apps");
    if st.dock.pins.is_empty() {
        ui.label(theme::muted(
            "Nothing is pinned. Right-click an app in the dock and choose Pin to dock.",
        ));
        return;
    }
    let mut edit = None;
    ui.spacing_mut().item_spacing.y = 4.0;
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

/// One pin: its icon, its name, and move up, move down and unpin.
fn pin_row(ui: &mut egui::Ui, st: &mut State, i: usize) -> Option<Edit> {
    let c = colors();
    let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::hover());
    ui.painter().rect_filled(row, CornerRadius::same(10), c.control);

    let icon = egui::Rect::from_center_size(
        egui::pos2(row.left() + 14.0 + ICON / 2.0, row.center().y),
        Vec2::splat(ICON),
    );
    let image = st
        .pin_icon(i)
        .and_then(|p| shellman_ui::app_icon::get(ui.ctx(), &p, true, &mut st.thumbs));
    match image {
        Some((id, size)) => {
            let scale = (ICON / size.x).min(ICON / size.y);
            egui::Image::from_texture((id, size))
                .paint_at(ui, egui::Rect::from_center_size(icon.center(), size * scale));
        }
        None => {
            ui.painter()
                .text(icon.center(), egui::Align2::CENTER_CENTER, glyph::APP, icon_font(18.0), c.muted);
        }
    }

    // The buttons take the right end; the name gets what is left.
    let buttons = egui::Rect::from_min_max(egui::pos2(row.right() - 3.0 * 36.0 - 8.0, row.top()), row.max);
    let name = egui::Rect::from_min_max(
        egui::pos2(icon.right() + 12.0, row.top()),
        egui::pos2(buttons.left() - 8.0, row.bottom()),
    );
    let label = st.dock_names.get(i).map_or("", String::as_str);
    draw_text(ui, name, label, FontId::proportional(14.0), c.text, false);

    let mut edit = None;
    let last = st.dock.pins.len() - 1;
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(buttons)
            .layout(Layout::right_to_left(Align::Center)),
    );
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
}
