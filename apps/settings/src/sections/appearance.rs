//! Appearance: the accent color, the glass amount and the text font every shellman app shares.

use crate::page;
use crate::state::State;
use shellman_ui::{font_picker, look_controls};
use shellman_ui::theme;
use shellman_ui::widgets::switch;

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "Appearance", "The look every shellman app shares. Changes apply at once.");

    page::group(ui, "Accent color", |ui| {
        if let Some(choice) = look_controls::accent_picker(ui, st.look.accent) {
            st.look.accent = choice;
            st.apply_look(ui.ctx());
            st.save_look();
        }
    });

    page::group(ui, "Glass", |ui| {
        let resp = look_controls::glass_slider(ui, &mut st.look.glass, st.glass);
        if resp.changed() {
            theme::set_glass(ui.ctx(), st.look.glass);
        }
        // Shared while it is dragged, so the other apps follow the slider, but not on every frame.
        let id = egui::Id::new("glass last shared");
        let now = ui.input(|i| i.time);
        let last: f64 = ui.data(|d| d.get_temp(id)).unwrap_or(f64::MIN);
        let due = resp.changed() && now - last >= 0.08;
        if resp.drag_stopped() || due {
            ui.data_mut(|d| d.insert_temp(id, now));
            st.save_look();
        }
    });

    page::group(ui, "Font", |ui| {
        if let Some(choice) = font_picker::font_picker(ui, st.look.font) {
            st.look.font = choice;
            st.apply_look(ui.ctx());
            st.save_look();
        }
        font_picker::preview(ui);
    });
    page::group(ui, "Icon pack", |ui| super::icon_pack::group(ui, st));
    page::group(ui, "Window frames", frames_group);
}

/// Other apps' title bars and borders in shellman's look (`frames.json`, read by the desktop).
/// Kept in the page's memory once read, and written as soon as a switch is flipped.
fn frames_group(ui: &mut egui::Ui) {
    let id = egui::Id::new("frames settings");
    let mut frames: shellman_core::frames::FramesSettings =
        ui.data(|d| d.get_temp(id)).unwrap_or_else(shellman_core::frames::FramesSettings::load);
    let mut changed = switch(ui, &mut frames.enabled, "Match other apps' window frames").changed();
    ui.add_enabled_ui(frames.enabled, |ui| {
        changed |= switch(ui, &mut frames.accent_border, "Accent border on the focused window").changed();
    });
    if changed {
        frames.save();
    }
    ui.data_mut(|d| d.insert_temp(id, frames));
}
