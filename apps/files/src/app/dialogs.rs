//! The modal dialogs: confirm-recycle.

use super::actions::Dialog;
use egui::{Id, Key, RichText};
use shellman_core::fsops::Op;
use shellman_ui::theme::{self, colors};

impl super::FileMan {
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
        }
        if keep {
            self.dialog = Some(dialog);
        }
    }
}
