//! Small drawing and selection helpers shared by the panes.

use crate::theme::{self, colors, icon_font};
use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat, TextWrapping};
use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Id, Rect, RichText, Sense, Ui, Vec2, WidgetText,
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
pub fn caption_button(ui: &mut Ui, g: char, close: bool, height: f32) -> egui::Response {
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
pub fn round_button(ui: &mut Ui, g: char, enabled: bool, tip: &str) -> egui::Response {
    let c = colors();
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(30.0), sense);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, tip));
    theme::focus_ring(ui, &resp, rect, 15.0);
    if enabled && resp.hovered() {
        ui.painter()
            .circle_filled(rect.center(), 15.0, c.control_hover);
    }
    let col = if enabled {
        c.text
    } else {
        theme::with_alpha(c.muted, 90)
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
pub fn window_drag(ui: &Ui, resp: &egui::Response) {
    if resp.double_clicked() {
        let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    } else if resp.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
}

pub fn menu_item(ui: &mut Ui, g: char, label: &str, shortcut: &str) -> bool {
    let c = colors();
    let mut b =
        egui::Button::new(icon_text(g, label, c.text, 13.0)).min_size(Vec2::new(220.0, 0.0));
    if !shortcut.is_empty() {
        b = b.shortcut_text(RichText::new(shortcut).color(c.muted).size(12.0));
    }
    ui.add(b).clicked()
}

/// A two-tone folder: a darker back with a tab, and a lighter front, so a folder reads at a
/// glance from plain rects instead of one flat glyph. Everything scales from `rect`, the icon's
/// bounding box, so the same drawing serves the grid's 62x46 and the list's 18x14.
pub fn folder_icon(
    painter: &egui::Painter,
    rect: Rect,
    accent: Color32,
    dim: impl Fn(Color32) -> Color32,
) {
    let (w, h) = (rect.width(), rect.height());
    let back = dim(theme::mix(accent, Color32::BLACK, 0.15));
    let front = dim(theme::mix(accent, Color32::WHITE, 0.25));
    let k = w / 62.0;
    let at = |x: f32, y: f32, ww: f32, hh: f32| {
        Rect::from_min_size(rect.min + Vec2::new(x, y), Vec2::new(ww, hh))
    };
    painter.rect_filled(
        at(0.0, 0.0, w * 0.45, h * 0.18),
        CornerRadius::same((5.0 * k).round() as u8),
        back,
    );
    painter.rect_filled(
        at(0.0, h * 0.15, w, h * 0.85),
        CornerRadius::same((7.0 * k).round() as u8),
        back,
    );
    let inset = w * 0.03;
    painter.rect_filled(
        at(inset, h * 0.2, w - inset * 2.0, h * 0.8),
        CornerRadius::same((7.0 * k).round() as u8),
        front,
    );
}

/// A toggle switch with its label to the right; the knob slides as it flips. Unlike a checkbox
/// it says "on" or "off" at a glance.
pub fn switch(ui: &mut Ui, on: &mut bool, label: &str) -> egui::Response {
    let c = colors();
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), FontId::proportional(13.5), c.text);
    let size = Vec2::new(36.0 + 10.0 + galley.size().x, 20.0_f32.max(galley.size().y));
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, *on, label));
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let track = Rect::from_min_size(egui::pos2(rect.left(), rect.center().y - 10.0), Vec2::new(36.0, 20.0));
    theme::focus_ring(ui, &resp, track, 10.0);
    paint_switch(ui, track, resp.id, *on, resp.hovered());
    let pos = egui::pos2(track.right() + 10.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(pos, galley, c.text);
    resp
}

/// A switch alone (its label is beside it in the row, see `theme::setting_row`); `name` is what a
/// screen reader calls it.
pub fn toggle(ui: &mut Ui, on: &mut bool, name: &str) -> egui::Response {
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(36.0, 20.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, *on, name));
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    theme::focus_ring(ui, &resp, rect, 10.0);
    paint_switch(ui, rect, resp.id, *on, resp.hovered());
    resp
}

/// The track turns to the accent as the knob slides over; the knob turns `on_accent` (a white
/// knob would vanish on a light accent). In high contrast: outlined, Windows' own pairs.
fn paint_switch(ui: &Ui, track: Rect, id: Id, on: bool, hovered: bool) {
    let c = colors();
    let t = ui.ctx().animate_bool_responsive(id, on);
    let p = ui.painter();
    let x = track.left() + 10.0 + 16.0 * t;
    let r = if hovered { 7.5 } else { 7.0 };
    if c.high_contrast {
        let (fill, knob) = if on { (c.accent_fill, c.on_accent) } else { (c.bg, c.text) };
        p.rect_filled(track, CornerRadius::same(10), fill);
        p.rect_stroke(track, CornerRadius::same(10), egui::Stroke::new(1.0_f32, c.text), egui::StrokeKind::Inside);
        p.circle_filled(egui::pos2(x, track.center().y), r - 1.0, knob);
        return;
    }
    p.rect_filled(track, CornerRadius::same(10), theme::mix(c.track, c.accent_fill, t));
    p.circle_filled(egui::pos2(x, track.center().y), r, theme::mix(c.muted, c.on_accent, t));
}

/// One line of text in a box, cut with an ellipsis when it does not fit.
pub fn draw_text(ui: &Ui, rect: Rect, text: &str, font: FontId, color: Color32, right: bool) {
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

pub fn select_range(ctx: &egui::Context, id: Id, from: usize, to: usize) {
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

pub fn select_all(ctx: &egui::Context, id: Id, len: usize) {
    select_range(ctx, id, 0, len);
}
