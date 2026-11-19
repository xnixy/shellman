//! The list item the Storage page is made of: a name, a line of detail, a usage bar and an "Open in
//! Files" button at its end. It draws and reports; the page decides what is shown.

use crate::page;
use egui::{Align2, FontId, Rect, Response, Sense, Ui, Vec2};
use shellman_ui::theme::{self, colors, glyph, icon_font, shape};
use shellman_ui::widgets::draw_text;

/// The height of an item's content: the name line and the bar under it.
const HEIGHT: f32 = 36.0;

/// What the bar shows.
pub struct Usage {
    /// 0 to 1.
    pub fraction: f32,
    /// Drawn in the warning colour (a nearly full drive).
    pub warn: bool,
}

/// One item of the list. `detail` goes at the right of the name; `usage` is the bar under them
/// (none while nothing is known yet). True when "Open in Files" was pressed.
pub fn row(ui: &mut Ui, name: &str, detail: &str, usage: Option<Usage>, can_open: bool) -> bool {
    let c = colors();
    page::item(ui, false, |ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), HEIGHT), Sense::hover());
        let open = Rect::from_min_max(egui::pos2(rect.right() - 34.0, rect.top()), rect.right_bottom());
        let text_right = if can_open { open.left() } else { rect.right() };
        let top = Rect::from_min_max(rect.min, egui::pos2(text_right, rect.top() + 20.0));
        let half = top.width() * 0.5;
        let name_rect = Rect::from_min_size(top.min, Vec2::new(half, top.height()));
        let detail_rect = Rect::from_min_max(egui::pos2(top.left() + half, top.top()), top.right_bottom());
        draw_text(ui, name_rect, name, FontId::proportional(13.5), c.text, false);
        draw_text(ui, detail_rect, detail, FontId::proportional(12.5), c.muted, true);
        let bar = Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 8.0), egui::pos2(text_right, rect.bottom()));
        // Anything that is not empty keeps a dot of the bar.
        let (fraction, fill) = usage.map_or((0.0, c.accent_fill), |u| (u.fraction, if u.warn { c.danger } else { c.accent_fill }));
        page::thin_bar(ui, bar, fraction, fill);
        if !can_open {
            return false;
        }
        let resp: Response = ui.interact(open, ui.id().with(("open", name)), Sense::click());
        if resp.hovered() {
            let wash = open.shrink2(Vec2::new(2.0, 4.0));
            if c.high_contrast {
                theme::contrast_outline(ui, wash, wash.height() / 2.0, true, false);
            } else {
                ui.painter().rect_filled(wash, shape::pill(wash), c.control_hover);
            }
        }
        ui.painter().text(
            open.center(),
            Align2::CENTER_CENTER,
            glyph::OPEN_EXTERNAL,
            icon_font(14.0),
            if resp.hovered() { c.text } else { c.muted },
        );
        resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Open in Files").clicked()
    })
}
