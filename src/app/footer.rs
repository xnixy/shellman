//! The footer under the list: item count, selection, and messages.

use super::tab::Loc;
use super::FOOTER_H;
use crate::fsops::{self, Op};
use crate::theme::colors;
use egui::{Align, Layout, RichText, Sense, Ui, Vec2};

const TOAST_SECS: f64 = 3.0;

impl super::FileMan {
    /// Item count, selection, and messages, along the bottom of the sheet.
    pub(super) fn footer(&mut self, ui: &mut Ui) {
        let c = colors();
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), FOOTER_H), Sense::hover());
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(Vec2::new(14.0, 0.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        let ui = &mut child;
        let tab = &self.tabs[self.active];
        ui.horizontal(|ui| {
            let mut text = match &tab.loc {
                Loc::Pc => format!("{} drives", self.drives.len()),
                Loc::Dir(_) => {
                    let n = tab.view.len();
                    format!("{n} item{}", if n == 1 { "" } else { "s" })
                }
            };
            if !tab.selected.is_empty() {
                let (count, bytes) = tab.selection_stats();
                text += &format!("   ·   {count} selected");
                if bytes > 0 {
                    text += &format!("  ({})", fsops::format_size(bytes));
                }
            }
            if tab.loading {
                text += "   ·   Loading…";
            }
            ui.label(RichText::new(text).size(11.5).color(c.muted));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let now = ui.input(|i| i.time);
                if let Some((msg, at)) = &self.toast {
                    if now - at < TOAST_SECS {
                        ui.label(RichText::new(msg).size(11.5).color(c.accent_text));
                        ui.ctx()
                            .request_repaint_after(std::time::Duration::from_millis(250));
                    }
                }
                if let Some((op, paths)) = &self.clipboard {
                    let verb = if *op == Op::Move { "cut" } else { "copied" };
                    ui.label(
                        RichText::new(format!("{} {verb}  ·  ", paths.len()))
                            .size(11.5)
                            .color(c.muted),
                    );
                }
            });
        });
    }
}
