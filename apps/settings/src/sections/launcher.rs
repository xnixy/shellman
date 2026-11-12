//! Launcher: the "most used" history.

use crate::page;
use crate::state::State;
use shellman_ui::theme;

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "Launcher", "What the launcher remembers.");

    page::group(ui, "Most used", |ui| {
        let used = st.launcher.apps_used();
        let line = match used {
            0 => "Nothing yet: the launcher counts apps you start from it.".to_owned(),
            1 => "1 app in the history.".to_owned(),
            n => format!("{n} apps in the history."),
        };
        ui.label(theme::muted(line));
        ui.add_space(8.0);
        // The history is only the launcher's own count of its launches; nothing is read from Windows.
        if ui.add_enabled(used > 0, egui::Button::new("Clear history")).clicked() {
            st.launcher.clear_history();
            st.save_launcher();
        }
    });
}
