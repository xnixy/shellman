//! About: the name, the version, where the settings live, and the host for Windows' own apps.

use crate::page;
use crate::state::State;
use shellman_ui::theme::{self, colors};
use shellman_ui::widgets::switch;

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "About", "shellman replaces everything Explorer does, in one style.");

    ui.label(
        egui::RichText::new("shellman Settings")
            .size(15.0)
            .strong()
            .color(colors().text),
    );
    ui.label(theme::muted(format!("Version {}", env!("CARGO_PKG_VERSION"))));

    page::gap(ui);
    page::label(ui, "Windows apps");
    if switch(ui, &mut st.shell.modern_apps, "Keep Settings, Calculator and the Store working").changed() {
        st.save_shell();
    }
    // Without Explorer these apps only open while Windows' own host for them runs; the desktop
    // starts and watches it (`app_host`). One window lookup, so asked every frame.
    let now = if shellman_core::app_host::modern_apps_available() { "They can open now." } else { "They cannot open right now." };
    ui.label(theme::muted(format!("Runs Windows' own host for them while Explorer is not running. {now}")).size(12.0));

    page::gap(ui);
    page::label(ui, "Settings folder");
    match shellman_core::config::config_dir() {
        Some(dir) => {
            ui.label(egui::RichText::new(dir.display().to_string()).color(colors().text));
            ui.add_space(8.0);
            if ui.button("Open settings folder").clicked() {
                // It is created by the first save; make sure there is something to open.
                let _ = std::fs::create_dir_all(&dir);
                // In Files, not whatever the system opens folders with (Explorer).
                if !shellman_core::sibling::start_with("files", &[&dir.display().to_string()]) {
                    shellman_core::fsops::open(&dir);
                }
            }
        }
        None => {
            ui.label(theme::muted("Windows did not say where."));
        }
    }
}
