//! About: the name, the version, and where the settings live.

use crate::page;
use shellman_ui::theme::{self, colors};

pub fn show(ui: &mut egui::Ui) {
    page::heading(ui, "About", "shellman replaces everything Explorer does, in one style.");

    ui.label(
        egui::RichText::new("shellman Settings")
            .size(15.0)
            .strong()
            .color(colors().text),
    );
    ui.label(theme::muted(format!("Version {}", env!("CARGO_PKG_VERSION"))));

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
