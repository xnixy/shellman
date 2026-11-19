//! The modal dialogs: confirm-recycle, the warning before a delete that cannot go to the Recycle
//! Bin, and the Recycle Bin's two confirmations. Each is a piece of the window's glass of its own
//! (see `island`), floating over the window's sheet.

use super::actions::Dialog;
use egui::{Id, Key, RichText};
use shellman_core::fsops::Op;
use shellman_core::recycle_bin;
use shellman_ui::glass::{content_frame, Island, Style};
use shellman_ui::theme::{self, colors};
use std::path::PathBuf;

/// The dialogs' corner radius.
const RADIUS: f32 = shellman_ui::theme::shape::DIALOG;

/// Where the open dialog is, for the glass panel.
fn island_id() -> Id {
    Id::new("files-dialog-island")
}

/// The open dialog as a piece of glass, when one is open this frame.
pub(super) fn island(ctx: &egui::Context) -> Option<Island> {
    ctx.data_mut(|d| d.remove_temp::<Option<egui::Rect>>(island_id()))
        .flatten()
        .map(|r| Island::new(r, RADIUS))
}

/// Which key answers a confirmation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Enter {
    /// Enter confirms.
    Confirms,
    /// Enter does nothing: what cannot be undone is not left to a stray keypress.
    Nothing,
    /// Enter cancels: Cancel is the default.
    Cancels,
}

/// One confirmation in the shared style: a title, a muted line, a red button and Cancel, on
/// glass. True to go ahead, false to cancel, nothing while it is still open.
///
/// The window's glass lies under everything Files draws, so under the dialog there is also the
/// folder's list: the dialog is filled with the glass's own colour, opaque (as the sheet looks
/// when Windows composes no glass), so nothing shows through it, and its piece of glass gives it
/// a shadow and rim.
fn confirm(ctx: &egui::Context, composed: bool, id: &str, title: &str, note: &str, button: &str, enter: Enter) -> Option<bool> {
    let c = colors();
    let style = Style::from_theme(RADIUS);
    let solid = content_frame(false, &style);
    let frame = solid
        .fill(theme::with_alpha(solid.fill, 255))
        .corner_radius(RADIUS as u8)
        .inner_margin(egui::Margin::same(22));
    let modal = egui::Modal::new(Id::new(id))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(400.0);
            ui.label(RichText::new(title).size(16.0).strong().color(c.text));
            ui.add_space(4.0);
            ui.label(RichText::new(note).color(c.text));
            ui.add_space(14.0);
            let mut result = None;
            ui.horizontal(|ui| {
                if ui.add(theme::danger_button(button)).clicked() {
                    result = Some(true);
                }
                let cancel = ui.button("Cancel");
                if enter == Enter::Cancels && !ui.memory(|m| m.focused().is_some()) {
                    cancel.request_focus();
                }
                if cancel.clicked() {
                    result = Some(false);
                }
            });
            match enter {
                Enter::Confirms if ui.input(|i| i.key_pressed(Key::Enter)) => result = result.or(Some(true)),
                Enter::Cancels if ui.input(|i| i.key_pressed(Key::Enter)) => result = result.or(Some(false)),
                _ => {}
            }
            result
        });
    if composed {
        ctx.data_mut(|d| d.insert_temp(island_id(), Some(modal.response.rect)));
    }
    modal.inner.or(modal.should_close().then_some(false))
}

/// What the warning before a permanent delete says: its title, its line, and its button.
pub(super) fn permanent_text(paths: &[PathBuf], doomed: &[PathBuf]) -> (String, String, &'static str) {
    let name = |p: &PathBuf| p.file_name().unwrap_or(p.as_os_str()).to_string_lossy().into_owned();
    let (n, all) = (doomed.len(), paths.len());
    let title = if n == 1 {
        format!("Delete \"{}\" permanently?", name(&doomed[0]))
    } else {
        format!("Delete {n} items permanently?")
    };
    let why = "They can't go to the Recycle Bin: a network or removable drive, a bin that is turned off, or too big for it.";
    let note = if n == all {
        let what = if n == 1 { "It will be deleted permanently" } else { "They will be deleted permanently" };
        let why = if n == 1 { why.replacen("They", "It", 1) } else { why.to_owned() };
        format!("{why} {what}. This can't be undone.")
    } else {
        let rest = all - n;
        let rest = if rest == 1 { "The other item goes".to_owned() } else { format!("The other {rest} go") };
        format!("{n} of the {all} items will be deleted permanently. {why} {rest} to the Recycle Bin. This can't be undone.")
    };
    (title, note, "Delete permanently")
}

impl super::FileMan {
    pub(super) fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        let composed = self.panel.as_ref().is_some_and(|p| p.composed());
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
                if let Some(yes) = confirm(ctx, composed, "recycle", &title, note, "Move to Recycle Bin", Enter::Confirms) {
                    keep = false;
                    if yes {
                        self.run_op(Op::Recycle, "Delete", paths, None);
                    }
                }
            }
            Dialog::Permanent { paths, doomed } => {
                let (title, note, button) = permanent_text(paths, doomed);
                if let Some(yes) = confirm(ctx, composed, "permanent", &title, &note, button, Enter::Cancels) {
                    keep = false;
                    if yes {
                        self.run_op(Op::RecycleOrDelete, "Delete", paths.clone(), None);
                    }
                }
            }
            Dialog::BinDelete(handles, names) => {
                let title = super::bin::delete_title(names);
                let note = "This can't be undone.";
                if let Some(yes) = confirm(ctx, composed, "bin-delete", &title, note, "Delete permanently", Enter::Nothing) {
                    keep = false;
                    if yes {
                        self.bin_delete(handles.clone());
                    }
                }
            }
            Dialog::BinEmpty(state) => {
                let note = format!("{}. This can't be undone.", recycle_bin::summary(*state));
                if let Some(yes) = confirm(ctx, composed, "bin-empty", "Empty the Recycle Bin?", &note, "Empty Recycle Bin", Enter::Nothing) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn p(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn the_permanent_warning_says_what_is_lost() {
        let one = p(&[r"\\nas\s\report.docx"]);
        let (title, note, button) = permanent_text(&one, &one);
        assert_eq!(title, "Delete \"report.docx\" permanently?");
        assert!(note.starts_with("It can't go to the Recycle Bin"), "{note}");
        assert!(note.contains("It will be deleted permanently"), "{note}");
        assert_eq!(button, "Delete permanently");

        let all = p(&[r"E:\a", r"E:\b", r"C:\c"]);
        let (title, note, _) = permanent_text(&all, &all[..2]);
        assert_eq!(title, "Delete 2 items permanently?");
        assert!(note.starts_with("2 of the 3 items will be deleted permanently."), "{note}");
        assert!(note.contains("The other item goes to the Recycle Bin"), "{note}");

        let (_, note, _) = permanent_text(&all, &all);
        assert!(note.contains("They will be deleted permanently"), "{note}");
    }
}
