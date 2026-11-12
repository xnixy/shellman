//! Appearance: the accent color and the glass amount every shellman app shares.

use crate::page;
use crate::state::State;
use shellman_ui::look_controls;
use shellman_ui::theme;

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
}
