//! The row the Storage page is made of: a name, a line of detail, a usage bar and an "Open in
//! Files" button at its end. It draws and reports; the page decides what is shown.

use super::sound_rows::ROW_H;
use egui::{Align2, CornerRadius, FontId, Rect, Response, Sense, Ui, Vec2};
use shellman_ui::theme::{self, colors, glyph, icon_font};
use shellman_ui::widgets::draw_text;

/// What the bar shows.
pub struct Usage {
    /// 0 to 1.
    pub fraction: f32,
    /// Drawn in the warning colour (a nearly full drive).
    pub warn: bool,
}

/// One row. `detail` goes at the right of the name; `usage` is the bar under them (none while
/// nothing is known yet). True when "Open in Files" was pressed.
pub fn row(ui: &mut Ui, name: &str, detail: &str, usage: Option<Usage>, can_open: bool) -> bool {
    let c = colors();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H + 8.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(10), c.control);
    let open = Rect::from_min_max(egui::pos2(rect.right() - 44.0, rect.top()), rect.right_bottom());
    let text_right = if can_open { open.left() } else { rect.right() - 14.0 };
    let top = Rect::from_min_max(egui::pos2(rect.left() + 14.0, rect.top() + 8.0), egui::pos2(text_right, rect.top() + 30.0));
    let half = top.width() * 0.5;
    let name_rect = Rect::from_min_size(top.min, Vec2::new(half, top.height()));
    let detail_rect = Rect::from_min_max(egui::pos2(top.left() + half, top.top()), top.right_bottom());
    draw_text(ui, name_rect, name, FontId::proportional(14.0), c.text, false);
    draw_text(ui, detail_rect, detail, FontId::proportional(12.5), c.muted, true);
    let bar = Rect::from_min_max(egui::pos2(rect.left() + 14.0, rect.bottom() - 18.0), egui::pos2(text_right, rect.bottom() - 10.0));
    ui.painter().rect_filled(bar, CornerRadius::same(4), theme::with_alpha(c.text, 28));
    if let Some(u) = usage {
        let w = bar.width() * u.fraction.clamp(0.0, 1.0);
        // A sliver stays visible for anything that is not empty.
        let w = if u.fraction > 0.0 { w.max(4.0) } else { 0.0 };
        if w > 0.0 {
            let fill = Rect::from_min_size(bar.min, Vec2::new(w, bar.height()));
            ui.painter().rect_filled(fill, CornerRadius::same(4), if u.warn { c.danger } else { c.accent_fill });
        }
    }
    if !can_open {
        return false;
    }
    let resp: Response = ui.interact(open, ui.id().with(("open", name)), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(open.shrink2(Vec2::new(4.0, 8.0)), CornerRadius::same(8), c.control_hover);
    }
    ui.painter().text(
        open.center(),
        Align2::CENTER_CENTER,
        glyph::OPEN_EXTERNAL,
        icon_font(14.0),
        if resp.hovered() { c.text } else { c.muted },
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Open in Files").clicked()
}
