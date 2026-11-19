//! The sidebar's pieces, shared by Files (places) and the settings app (sections).

use crate::theme::{self, colors, icon_font, shape};
use crate::widgets::draw_text;
use egui::{Align2, FontId, Rect, Sense, Ui, Vec2};

/// A row in a sidebar. The open one gets Glasshouse's soft field fill, not the accent, so the
/// sidebar stays quiet next to the page.
pub fn nav_row(ui: &mut Ui, g: char, label: &str, active: bool) -> egui::Response {
    nav_row_with(ui, label, active, false, |p, slot, active| {
        let c = colors();
        p.text(
            egui::pos2(slot.left(), slot.center().y),
            Align2::LEFT_CENTER,
            g,
            icon_font(14.0),
            if active { c.text } else { c.muted },
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
        ui.allocate_exact_size(Vec2::new(ui.available_width(), shape::NAV_ROW_H), Sense::click());
    let p = ui.painter();
    let round = shape::corner(shape::NAV_ROW);
    if c.high_contrast {
        theme::contrast_outline(ui, rect, shape::NAV_ROW, resp.hovered(), active);
    } else if active {
        p.rect_filled(rect, round, c.field);
    } else if resp.hovered() {
        p.rect_filled(rect, round, c.row_hover);
    }
    let slot = Rect::from_center_size(
        egui::pos2(rect.left() + 14.0 + 8.0, rect.center().y),
        Vec2::splat(16.0),
    );
    icon(p, slot, active);
    let text_rect = Rect::from_min_max(egui::pos2(rect.left() + 40.0, rect.top()), rect.max);
    let text = if dim {
        theme::with_alpha(c.text, 90)
    } else {
        c.text
    };
    draw_text(ui, text_rect, label, FontId::proportional(13.5), text, false);
    resp
}

/// A small heading over a group of sidebar rows: upper case and spaced out, as Glasshouse's
/// group titles.
pub fn section_label(ui: &mut Ui, text: &str) {
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.label(theme::group_heading(text));
    });
    ui.add_space(4.0);
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
