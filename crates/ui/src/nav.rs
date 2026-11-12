//! The sidebar's pieces, shared by Files (places) and the settings app (sections).

use crate::theme::{self, colors, icon_font};
use crate::widgets::draw_text;
use egui::{Align2, CornerRadius, FontId, Rect, RichText, Sense, Ui, Vec2};

/// A row in a sidebar. The open one gets a soft lift and a small accent bar rather than a loud
/// accent fill, so the sidebar stays quiet next to the page.
pub fn nav_row(ui: &mut Ui, g: char, label: &str, active: bool) -> egui::Response {
    nav_row_with(ui, label, active, false, |p, slot, active| {
        let c = colors();
        p.text(
            egui::pos2(slot.left(), slot.center().y),
            Align2::LEFT_CENTER,
            g,
            icon_font(14.0),
            if active { c.accent_text } else { c.muted },
        );
    })
}

/// A sidebar row whose icon is drawn by the caller into a 16 point square (an app's own icon, a
/// bitmap). `dim` fades the row, for a place that is not there. The glyph rows use this too.
pub fn nav_row_with(
    ui: &mut Ui,
    label: &str,
    active: bool,
    dim: bool,
    icon: impl FnOnce(&egui::Painter, Rect, bool),
) -> egui::Response {
    let c = colors();
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
    let p = ui.painter();
    if active {
        p.rect_filled(rect, CornerRadius::same(8), theme::with_alpha(c.text, 18));
        let bar = Rect::from_center_size(
            egui::pos2(rect.left() + 4.0 + 1.5, rect.center().y),
            Vec2::new(3.0, 16.0),
        );
        p.rect_filled(bar, CornerRadius::same(2), c.accent_fill);
    } else if resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(8), theme::with_alpha(c.text, 10));
    }
    let slot = Rect::from_center_size(
        egui::pos2(rect.left() + 12.0 + 8.0, rect.center().y),
        Vec2::splat(16.0),
    );
    icon(p, slot, active);
    let text_rect = Rect::from_min_max(egui::pos2(rect.left() + 36.0, rect.top()), rect.max);
    let text = if dim {
        theme::with_alpha(c.text, 90)
    } else {
        c.text
    };
    draw_text(ui, text_rect, label, FontId::proportional(13.5), text, false);
    resp
}

/// A small heading over a group of sidebar rows.
pub fn section_label(ui: &mut Ui, text: &str) {
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(RichText::new(text).size(11.0).color(colors().muted));
    });
    ui.add_space(2.0);
}

/// The sidebar's top: it is title bar too, with the app's name and an accent dot. Dragging it
/// moves the window.
pub fn sidebar_title(ui: &mut Ui, height: f32, name: &str) {
    let c = colors();
    let (bar, drag) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        Sense::click_and_drag(),
    );
    crate::widgets::window_drag(ui, &drag);
    ui.painter()
        .circle_filled(egui::pos2(bar.left() + 14.0, bar.center().y), 3.5, c.accent_fill);
    ui.painter().text(
        egui::pos2(bar.left() + 26.0, bar.center().y),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(12.5),
        c.muted,
    );
}
