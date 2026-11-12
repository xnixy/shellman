//! The This PC page: the drives, as cards laid out in as many columns as fit.

use super::actions::Action;
use super::tab::Loc;
use egui::{Align2, CornerRadius, FontId, Id, Rect, ScrollArea, Sense, Ui, Vec2};
use shellman_core::fsops::{self, Drive};
use shellman_ui::theme::{self, colors, glyph, icon_font};
use shellman_ui::widgets::draw_text;

const CARD_MIN_W: f32 = 250.0;
const CARD_H: f32 = 76.0;
const GAP: f32 = 12.0;

impl super::FileMan {
    pub(super) fn pc_page(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let c = colors();
        ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(8.0);
            ui.label(theme::muted("Drives").size(13.0));
            ui.add_space(4.0);
            // Same column rule as the icons view: whole cards per line, stretched to fill it.
            // Wrapping layouts neither wrapped these cards nor top-aligned them.
            let width = ui.available_width();
            let cols = (((width + GAP) / (CARD_MIN_W + GAP)).floor() as usize).max(1);
            let card_w = (width + GAP) / cols as f32 - GAP;
            ui.spacing_mut().item_spacing.y = GAP;
            for (line, drives) in self.drives.chunks(cols).enumerate() {
                let (row, _) = ui.allocate_exact_size(Vec2::new(width, CARD_H), Sense::hover());
                for (col, d) in drives.iter().enumerate() {
                    let rect = Rect::from_min_size(
                        row.min + Vec2::new(col as f32 * (card_w + GAP), 0.0),
                        Vec2::new(card_w, CARD_H),
                    );
                    let resp = ui.interact(rect, Id::new(("drive", line, col)), Sense::click());
                    let fill = if resp.hovered() {
                        c.control_hover
                    } else {
                        c.control
                    };
                    ui.painter().rect_filled(rect, CornerRadius::same(12), fill);
                    drive_card(ui, rect, d);
                    if resp.clicked() {
                        actions.push(Action::Navigate(Loc::Dir(d.root.clone())));
                    }
                    if resp.middle_clicked() {
                        actions.push(Action::NewTab(Loc::Dir(d.root.clone())));
                    }
                }
            }
        });
    }
}

/// The inside of one drive card: icon and name, then a usage bar the width of the card and the
/// free space under it.
fn drive_card(ui: &Ui, rect: Rect, d: &Drive) {
    let c = colors();
    let inner = rect.shrink2(Vec2::new(16.0, 0.0));
    let title_y = if d.total > 0 {
        rect.top() + 24.0
    } else {
        rect.center().y
    };
    ui.painter().text(
        egui::pos2(inner.left() + 13.0, title_y),
        Align2::CENTER_CENTER,
        glyph::DRIVE,
        icon_font(26.0),
        c.accent_text,
    );
    let title = Rect::from_min_max(
        egui::pos2(inner.left() + 38.0, title_y - 10.0),
        egui::pos2(inner.right(), title_y + 10.0),
    );
    draw_text(
        ui,
        title,
        &d.title(),
        FontId::proportional(14.5),
        c.text,
        false,
    );
    if d.total == 0 {
        return;
    }
    let used = (1.0 - d.free as f32 / d.total as f32).clamp(0.0, 1.0);
    let bar = Rect::from_min_size(
        egui::pos2(inner.left(), rect.top() + 44.0),
        Vec2::new(inner.width(), 4.0),
    );
    let mut fill = bar;
    fill.set_width(bar.width() * used);
    let col = if used > 0.9 { c.danger } else { c.accent_fill };
    let p = ui.painter();
    p.rect_filled(bar, CornerRadius::same(2), c.track);
    p.rect_filled(fill, CornerRadius::same(2), col);
    let free = format!(
        "{} free of {}",
        fsops::format_size(d.free),
        fsops::format_size(d.total)
    );
    let line = Rect::from_min_max(
        egui::pos2(inner.left(), bar.bottom() + 4.0),
        egui::pos2(inner.right(), bar.bottom() + 20.0),
    );
    draw_text(ui, line, &free, FontId::proportional(12.0), c.muted, false);
}
