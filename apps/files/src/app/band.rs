//! The band along the bottom. While something is selected, a pill there says how much and offers
//! what can be done to it: Copy, Rename (one item) and Delete; in the Recycle Bin, Restore and
//! Delete for good. Otherwise a small muted pill counts the items. Messages ("Copy done") show in
//! a pill of their own at the right.

use super::actions::Action;
use super::tab::Loc;
use egui::{Align2, Color32, FontId, Id, Rect, Ui, Vec2};
use shellman_core::fsops::{self, Op};
use shellman_ui::chips::{chip, chip_width, Chip, CHIP_H};
use shellman_ui::theme::{colors, glyph, semibold_font};

const TOAST_SECS: f64 = 3.0;
/// Inside the pill, at each end, and between its parts.
const PAD: f32 = 8.0;
const LEAD: f32 = 18.0;
const SPACE: f32 = 8.0;

/// One button of the selection pill.
struct Button {
    label: &'static str,
    icon: char,
    kind: Chip,
    action: Action,
}

/// The buttons the selection pill offers for `count` items, in the bin or elsewhere.
fn buttons(bin: bool, count: usize) -> Vec<Button> {
    let b = |label, icon, kind, action| Button {
        label,
        icon,
        kind,
        action,
    };
    if bin {
        return vec![
            b("Restore", glyph::UNDO, Chip::Soft, Action::BinRestore),
            b(
                "Delete for good",
                glyph::DELETE,
                Chip::Danger,
                Action::BinDelete,
            ),
        ];
    }
    let mut list = vec![b("Copy", glyph::COPY, Chip::Soft, Action::Clip(Op::Copy))];
    if count == 1 {
        list.push(b("Rename", glyph::RENAME, Chip::Soft, Action::Rename));
    }
    list.push(b("Delete", glyph::DELETE, Chip::Danger, Action::Recycle));
    list
}

fn text_width(ui: &Ui, text: &str, font: FontId) -> f32 {
    ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font, Color32::WHITE))
        .size()
        .x
        .ceil()
}

impl super::FileMan {
    /// The message pill ("Copy done") at the right of `band`, while there is a message; it is
    /// added to `extra`.
    pub(super) fn toast_pill(&mut self, ui: &mut Ui, band: Rect, extra: &mut Vec<Rect>) {
        let c = colors();
        let now = ui.input(|i| i.time);
        if let Some((msg, at)) = &self.toast {
            if now - at < TOAST_SECS {
                let w = text_width(ui, msg, FontId::proportional(12.5)) + LEAD * 2.0;
                let r = Rect::from_min_max(
                    egui::pos2((band.right() - w).max(band.left()), band.top() + 6.0),
                    egui::pos2(band.right(), band.bottom() - 6.0),
                );
                ui.painter().text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    msg,
                    FontId::proportional(12.5),
                    c.accent_text,
                );
                extra.push(r);
                let left = TOAST_SECS - (now - at);
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_secs_f64(left.max(0.05)));
            }
        }
    }

    /// Draw the band in `band`; returns the main pill, if one shows.
    pub(super) fn band(&mut self, ui: &mut Ui, band: Rect, actions: &mut Vec<Action>) -> Option<Rect> {
        let c = colors();

        let tab = &self.tabs[self.active];
        if tab.selected.is_empty() || tab.loc == Loc::Pc {
            // Just the count, small and muted.
            let mut text = match &tab.loc {
                Loc::Pc => format!("{} drives", self.drives.len()),
                Loc::Dir(_) | Loc::Bin => {
                    let n = tab.view.len();
                    format!("{n} item{}", if n == 1 { "" } else { "s" })
                }
            };
            if tab.loading {
                text += "  ·  Loading…";
            }
            if let Some((op, paths)) = &self.clipboard {
                let verb = if *op == Op::Move { "cut" } else { "copied" };
                text += &format!("  ·  {} {verb}", paths.len());
            }
            let font = FontId::proportional(12.0);
            let w = text_width(ui, &text, font.clone()) + LEAD * 2.0;
            let pill = Rect::from_center_size(band.center(), Vec2::new(w.min(band.width()), 30.0));
            ui.painter()
                .text(pill.center(), Align2::CENTER_CENTER, text, font, c.muted);
            return Some(pill);
        }

        let (count, bytes) = tab.selection_stats();
        let bin = tab.loc == Loc::Bin;
        let label = format!("{count} selected");
        let size = (bytes > 0).then(|| fsops::format_size(bytes));
        let list = buttons(bin, count);
        let label_w = text_width(ui, &label, semibold_font(13.5));
        let size_w = size.as_ref().map_or(0.0, |s| {
            text_width(ui, s, FontId::proportional(13.0)) + SPACE
        });
        let widths: Vec<f32> = list
            .iter()
            .map(|b| chip_width(ui, b.label, true, b.kind))
            .collect();
        let wanted = LEAD
            + label_w
            + SPACE
            + size_w
            + SPACE
            + widths.iter().sum::<f32>()
            + SPACE * (widths.len() as f32 - 1.0)
            + PAD;
        let pill = super::plan::centred(band, wanted);
        let mut x = pill.left() + LEAD;
        let y = pill.center().y;
        let p = ui.painter();
        p.text(
            egui::pos2(x, y),
            Align2::LEFT_CENTER,
            &label,
            semibold_font(13.5),
            c.text,
        );
        x += label_w + SPACE;
        if let Some(s) = size {
            p.text(
                egui::pos2(x, y),
                Align2::LEFT_CENTER,
                s,
                FontId::proportional(13.0),
                c.muted,
            );
            x += size_w;
        }
        x += SPACE;
        for (b, w) in list.into_iter().zip(widths) {
            let r = Rect::from_min_size(egui::pos2(x, y - CHIP_H / 2.0), Vec2::new(w, CHIP_H));
            if chip(
                ui,
                r,
                Id::new(("band", b.label)),
                b.label,
                Some(b.icon),
                b.kind,
            )
            .clicked()
            {
                actions.push(b.action);
            }
            x += w + SPACE;
        }
        Some(pill)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(bin: bool, count: usize) -> Vec<&'static str> {
        buttons(bin, count).iter().map(|b| b.label).collect()
    }

    #[test]
    fn rename_only_for_one_item_and_the_bin_has_its_own() {
        assert_eq!(labels(false, 1), ["Copy", "Rename", "Delete"]);
        assert_eq!(labels(false, 3), ["Copy", "Delete"]);
        assert_eq!(labels(true, 2), ["Restore", "Delete for good"]);
        assert!(buttons(false, 2)
            .last()
            .is_some_and(|b| b.kind == Chip::Danger));
    }
}
