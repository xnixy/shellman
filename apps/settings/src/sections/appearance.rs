//! Appearance: the accent color, light or dark, how windows are drawn, the glass amount and the
//! text font every shellman app shares, and the Islands look for Firefox (`firefox.rs`).
//!
//! The Window choice and the glass amount are two dials: the choice says where the glass is
//! (under each island, one pane under the whole window, or none), the amount how much of it
//! shows through its tint. So the amount applies to Invisible and Frosted alike, and is greyed
//! out, saying why, when the window has no glass (No transparency, Remote Desktop, high contrast,
//! transparency effects off).

use crate::page;
use crate::state::State;
use shellman_ui::theme::window_style::{self, ThemeChoice, WindowStyle};
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

    page::group(ui, "Theme", |ui| theme_group(ui, st));
    let shown = window_style::shown(st.look.window, window_style::conditions());
    page::group(ui, "Window", |ui| window_group(ui, st, shown));

    page::group(ui, "Glass", |ui| {
        let detail = if shown.glass_applies {
            "How much of the glass shows through its tint, in Invisible and Frosted windows."
        } else {
            "Nothing to set: the window has no glass now."
        };
        ui.label(theme::muted(detail).size(11.5));
        ui.add_space(4.0);
        let resp = look_controls::glass_slider(ui, &mut st.look.glass, shown.glass_applies);
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
    page::group(ui, "Sidebars", |ui| sidebars(ui, st));
    page::group(ui, "Icon pack", |ui| super::icon_pack::group(ui, st));
    page::group(ui, "Window frames", frames_group);
    page::group(ui, "Firefox", |ui| super::firefox::group(ui, &mut st.firefox));
}

/// Light, dark, or as Windows is set. High contrast uses Windows' own colours whatever is chosen.
fn theme_group(ui: &mut egui::Ui, st: &mut State) {
    let options: Vec<(ThemeChoice, &str)> = ThemeChoice::ALL.iter().map(|t| (*t, t.label())).collect();
    let hc = theme::env::high_contrast();
    if let Some(choice) = look_controls::choice_chips(ui, st.look.theme, &options, !hc) {
        st.look.theme = choice;
        st.apply_look(ui.ctx());
        st.save_look();
    }
    let detail = if hc {
        "High contrast uses Windows' own colours."
    } else {
        "Light or dark, or as Windows is set."
    };
    ui.label(theme::muted(detail).size(11.5));
}

/// Invisible, Frosted or No transparency, as chips; forced to No transparency, and saying why,
/// in high contrast or with Windows' transparency effects off.
fn window_group(ui: &mut egui::Ui, st: &mut State, shown: window_style::Shown) {
    let options: Vec<(WindowStyle, &str)> = WindowStyle::ALL.iter().map(|s| (*s, s.label())).collect();
    if let Some(choice) = look_controls::choice_chips(ui, shown.lit, &options, shown.enabled) {
        st.look.window = choice;
        st.apply_look(ui.ctx());
        st.save_look();
    }
    ui.label(theme::muted(shown.detail).size(11.5));
}

/// How every app's sidebar starts: with names, or icons only. Choosing again also undoes what
/// each app's own fold button chose, so every open app follows at once.
fn sidebars(ui: &mut egui::Ui, st: &mut State) {
    let mut collapsed = st.look.rail_collapsed;
    if page::segmented(ui, &mut collapsed, &[(false, "Show names"), (true, "Icons only")]) {
        st.look.set_rail_default(collapsed);
        st.save_look();
    }
    ui.add_space(6.0);
    ui.label(
        theme::muted("The button at the foot of an app's sidebar changes that app only; choosing here sets every app. Narrow windows show icons only.")
            .size(12.0),
    );
    // Apps set their own way: one click puts them all back on the default.
    if st.look.rails != shellman_ui::appearance::Rails::default() {
        ui.add_space(4.0);
        if ui.link(egui::RichText::new("Set every app to this").size(12.5)).clicked() {
            st.look.set_rail_default(collapsed);
            st.save_look();
        }
    }
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
