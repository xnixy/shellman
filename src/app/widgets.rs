//! Small drawing and selection helpers shared by the panes.

use crate::theme::{self, colors, icon_font};
use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat, TextWrapping};
use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Id, Rect, RichText, Sense, Stroke, Ui, Vec2,
    WidgetText,
};

/// A glyph and a label in one piece of text.
fn icon_text(g: char, text: &str, color: Color32, size: f32) -> WidgetText {
    let mut job = LayoutJob::default();
    job.append(
        &g.to_string(),
        0.0,
        TextFormat {
            font_id: icon_font(size),
            color,
            valign: Align::Center,
            ..Default::default()
        },
    );
    job.append(
        text,
        8.0,
        TextFormat {
            font_id: FontId::proportional(size + 1.0),
            color,
            valign: Align::Center,
            ..Default::default()
        },
    );
    WidgetText::LayoutJob(job)
}

/// Minimize, maximize, or close, for when Windows draws none over the glass.
pub(super) fn caption_button(ui: &mut Ui, g: char, close: bool, height: f32) -> egui::Response {
    let c = colors();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(46.0, height), Sense::click());
    let (fill, fg) = match (resp.hovered(), close) {
        (true, true) => (Color32::from_rgb(196, 43, 28), Color32::WHITE),
        (true, false) => (c.control_hover, c.text),
        _ => (Color32::TRANSPARENT, c.text),
    };
    ui.painter().rect_filled(rect, CornerRadius::ZERO, fill);
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, g, icon_font(10.0), fg);
    resp
}

/// A round, borderless icon button.
pub(super) fn round_button(ui: &mut Ui, g: char, enabled: bool, tip: &str) -> egui::Response {
    let c = colors();
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(34.0), sense);
    if enabled && resp.hovered() {
        ui.painter()
            .circle_filled(rect.center(), 16.0, c.control_hover);
    }
    let col = if enabled {
        c.text
    } else {
        theme::with_alpha(c.muted, 80)
    };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        g,
        icon_font(13.0),
        col,
    );
    resp.on_hover_text(tip)
}

/// Title-bar behavior for empty space: drag to move, double click to maximize or restore.
pub(super) fn window_drag(ui: &Ui, resp: &egui::Response) {
    if resp.double_clicked() {
        let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    } else if resp.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
}

pub(super) fn menu_item(ui: &mut Ui, g: char, label: &str, shortcut: &str) -> bool {
    let c = colors();
    let mut b =
        egui::Button::new(icon_text(g, label, c.text, 13.0)).min_size(Vec2::new(220.0, 0.0));
    if !shortcut.is_empty() {
        b = b.shortcut_text(RichText::new(shortcut).color(c.muted).size(12.0));
    }
    ui.add(b).clicked()
}

pub(super) fn hover_outline(ui: &Ui, resp: &egui::Response) {
    if resp.hovered() {
        ui.painter().rect_stroke(
            resp.rect,
            CornerRadius::same(14),
            Stroke::new(1.0_f32, theme::with_alpha(colors().accent, 170)),
            egui::StrokeKind::Inside,
        );
    }
}

/// One line of text in a box, cut with an ellipsis when it does not fit.
pub(super) fn draw_text(
    ui: &Ui,
    rect: Rect,
    text: &str,
    font: FontId,
    color: Color32,
    right: bool,
) {
    if rect.width() <= 4.0 {
        return;
    }
    let mut job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat {
            font_id: font,
            color,
            ..Default::default()
        },
    );
    job.wrap = TextWrapping::truncate_at_width(rect.width());
    let galley = ui.fonts(|f| f.layout_job(job));
    let x = if right {
        rect.right() - galley.size().x
    } else {
        rect.left()
    };
    let pos = egui::pos2(x, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(pos, galley, color);
}

pub(super) fn select_range(ctx: &egui::Context, id: Id, from: usize, to: usize) {
    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
        state.cursor.set_char_range(Some(CCursorRange::two(
            CCursor::new(from),
            CCursor::new(to),
        )));
        state.store(ctx, id);
    } else {
        let mut state = egui::text_edit::TextEditState::default();
        state.cursor.set_char_range(Some(CCursorRange::two(
            CCursor::new(from),
            CCursor::new(to),
        )));
        state.store(ctx, id);
    }
}

pub(super) fn select_all(ctx: &egui::Context, id: Id, len: usize) {
    select_range(ctx, id, 0, len);
}
