//! The modal dialogs: confirm-recycle, and the Recycle Bin's two confirmations.

use super::actions::Dialog;
use egui::{Id, Key, RichText};
use shellman_core::fsops::Op;
use shellman_core::recycle_bin;
use shellman_ui::theme::{self, colors};

/// One confirmation in the shared style: a title, a muted line, a red button and Cancel. True to
/// go ahead, false to cancel, nothing while it is still open. Enter confirms only when
/// `enter_confirms`; what cannot be undone is not left to a stray keypress.
fn confirm(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    note: &str,
    button: &str,
    enter_confirms: bool,
) -> Option<bool> {
    let c = colors();
    let modal = egui::Modal::new(Id::new(id)).frame(theme::dialog()).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.label(RichText::new(title).size(16.0).strong().color(c.text));
        ui.label(theme::muted(note));
        ui.add_space(12.0);
        let mut result = None;
        ui.horizontal(|ui| {
            if ui.add(theme::danger_button(button)).clicked()
                || (enter_confirms && ui.input(|i| i.key_pressed(Key::Enter)))
            {
                result = Some(true);
            }
            if ui.button("Cancel").clicked() {
                result = Some(false);
            }
        });
        result
    });
    modal.inner.or(modal.should_close().then_some(false))
}

impl super::FileMan {
    pub(super) fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        let mut keep = true;
        match &dialog {
            Dialog::Recycle(paths) => {
                let paths = paths.clone();
                let title = if paths.len() == 1 {
                    format!(
                        "Move \"{}\" to the Recycle Bin?",
                        paths[0].file_name().unwrap_or_default().to_string_lossy()
                    )
                } else {
                    format!("Move {} items to the Recycle Bin?", paths.len())
                };
                let note = "You can restore them from the Recycle Bin.";
                if let Some(yes) = confirm(ctx, "recycle", &title, note, "Move to Recycle Bin", true) {
                    keep = false;
                    if yes {
                        self.run_op(Op::Recycle, "Delete", paths, None);
                    }
                }
            }
            Dialog::BinDelete(handles, names) => {
                let title = super::bin::delete_title(names);
                let note = "This can't be undone.";
                if let Some(yes) = confirm(ctx, "bin-delete", &title, note, "Delete permanently", false) {
                    keep = false;
                    if yes {
                        self.bin_delete(handles.clone());
                    }
                }
            }
            Dialog::BinEmpty(state) => {
                let note = format!("{}. This can't be undone.", recycle_bin::summary(*state));
                if let Some(yes) = confirm(ctx, "bin-empty", "Empty the Recycle Bin?", &note, "Empty Recycle Bin", false) {
                    keep = false;
                    if yes {
                        self.bin_empty();
                    }
                }
            }
        }
        if keep {
            self.dialog = Some(dialog);
        }
    }
}
