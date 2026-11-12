//! Appearance: the accent color and the glass amount every shellman app shares.

use crate::page;
use crate::state::State;
use shellman_ui::look_controls;
use shellman_ui::theme;

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "Appearance", "The look every shellman app shares. Changes apply at once.");

    page::label(ui, "Accent color");
    if let Some(choice) = look_controls::accent_picker(ui, st.look.accent) {
        st.look.accent = choice;
        st.apply_look(ui.ctx());
        st.save_look();
    }

    page::gap(ui);
    page::label(ui, "Glass");
    let resp = look_controls::glass_slider(ui, &mut st.look.glass, st.glass);
    if resp.changed() {
        theme::set_glass(ui.ctx(), st.look.glass);
    }
    // Shared once the slider is let go, not on every step of a drag.
    if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
        st.save_look();
    }
}
