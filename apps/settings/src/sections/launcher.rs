//! Launcher: the "most used" history.

use crate::page;
use crate::state::State;

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "Launcher", "What the launcher remembers.");

    page::group(ui, "Most used", |ui| {
        let used = st.launcher.apps_used();
        let line = match used {
            0 => "Nothing yet: the launcher counts apps you start from it.".to_owned(),
            1 => "1 app in the history.".to_owned(),
            n => format!("{n} apps in the history."),
        };
        // The history is only the launcher's own count of its launches; nothing is read from Windows.
        let cleared = page::row(ui, "Clear history", &line, |ui| ui.add_enabled(used > 0, egui::Button::new("Clear history")).clicked());
        if cleared {
            st.launcher.clear_history();
            st.save_launcher();
        }
    });
}
