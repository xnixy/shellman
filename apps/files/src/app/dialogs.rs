//! The modal dialogs: settings and confirm-recycle.

use super::actions::Dialog;
use egui::{Align, Id, Key, Layout, RichText, Ui};
use shellman_core::fsops::Op;
use shellman_ui::look_controls;
use shellman_ui::theme::{self, colors};
use shellman_ui::widgets::switch;

impl super::FileMan {
    /// How much of the acrylic shows through, applied as the slider moves. Without system
    /// backdrops (Windows 10) there is no glass to show, so the slider is shown but disabled.
    fn glass_slider(&mut self, ui: &mut Ui) {
        ui.label(theme::muted("Glass").size(12.0));
        let resp = look_controls::glass_slider(ui, &mut self.settings.glass, self.glass);
        if resp.changed() {
            theme::set_glass(ui.ctx(), self.settings.glass);
            self.save_pending = true;
        }
        // Shared once the slider is let go, not on every step of a drag.
        if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
            self.share_look();
        }
    }

    pub(super) fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        let c = colors();
        let mut keep = true;
        match &dialog {
            Dialog::ConfirmRecycle(paths) => {
                let paths = paths.clone();
                let modal = egui::Modal::new(Id::new("recycle"))
                    .frame(theme::dialog())
                    .show(ctx, |ui| {
                        ui.set_width(380.0);
                        let title = if paths.len() == 1 {
                            format!(
                                "Move \"{}\" to the Recycle Bin?",
                                paths[0].file_name().unwrap_or_default().to_string_lossy()
                            )
                        } else {
                            format!("Move {} items to the Recycle Bin?", paths.len())
                        };
                        ui.label(RichText::new(title).size(16.0).strong().color(c.text));
                        ui.label(theme::muted("You can restore them from the Recycle Bin."));
                        ui.add_space(12.0);
                        let mut result = None;
                        ui.horizontal(|ui| {
                            if ui
                                .add(theme::danger_button("Move to Recycle Bin"))
                                .clicked()
                                || ui.input(|i| i.key_pressed(Key::Enter))
                            {
                                result = Some(true);
                            }
                            if ui.button("Cancel").clicked() {
                                result = Some(false);
                            }
                        });
                        result
                    });
                let answer = modal.inner.or(modal.should_close().then_some(false));
                if let Some(yes) = answer {
                    keep = false;
                    if yes {
                        self.run_op(Op::Recycle, "Delete", paths, None);
                    }
                }
            }
            Dialog::Settings => {
                let modal = egui::Modal::new(Id::new("settings"))
                    .frame(theme::dialog())
                    .show(ctx, |ui| {
                        ui.set_width(420.0);
                        ui.label(
                            RichText::new("Appearance")
                                .size(16.0)
                                .strong()
                                .color(c.text),
                        );
                        ui.add_space(8.0);
                        ui.label(theme::muted("Accent color").size(12.0));
                        ui.add_space(2.0);
                        if let Some(p) = look_controls::accent_picker(ui, self.settings.accent) {
                            self.set_accent(p);
                        }
                        ui.add_space(14.0);
                        self.glass_slider(ui);
                        ui.add_space(14.0);
                        if switch(ui, &mut self.settings.show_hidden, "Show hidden files").changed()
                        {
                            self.save_pending = true;
                            for t in &mut self.tabs {
                                t.dirty = true;
                            }
                        }
                        ui.add_space(14.0);
                        ui.horizontal(|ui| {
                            // Everything else shellman can be set up for lives in its own app.
                            if ui.button("All settings").clicked() {
                                shellman_core::sibling::start("settings");
                            }
                            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                                ui.add(theme::primary_button("Done")).clicked()
                            })
                            .inner
                        })
                        .inner
                    });
                if modal.inner || modal.should_close() {
                    keep = false;
                }
            }
        }
        if keep {
            self.dialog = Some(dialog);
        }
    }
}
