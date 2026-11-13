//! The pieces the Windows Update page is built from: an update row (title, a line of detail, its
//! kind as a chip, live progress, and a button for an optional one), a history row, buttons and
//! a progress bar. They draw and report; the page decides what a press means.

use egui::{Align2, Color32, CornerRadius, FontId, Rect, Response, Sense, Ui, Vec2};
use shellman_core::fsops::format_size;
use shellman_core::system::update_history::Entry;
use shellman_core::system::update_logic::Outcome;
use shellman_core::system::updates::Update;
use shellman_ui::chips;
use shellman_ui::theme::{self, colors, glyph, icon_font};
use shellman_ui::widgets::draw_text;

const ROW_H: f32 = 58.0;
const CHIP_H: f32 = 22.0;

/// A button on the page: in the accent for the main action, a soft one otherwise.
pub fn button(ui: &mut Ui, text: &str, accent: bool, enabled: bool) -> Response {
    let c = colors();
    let (fill, ink) = if accent { (c.accent_fill, c.on_accent) } else { (c.control_hover, c.text) };
    let b = egui::Button::new(egui::RichText::new(text).size(13.0).color(ink))
        .fill(fill)
        .stroke(egui::Stroke::NONE)
        .corner_radius(CornerRadius::same(8))
        .min_size(Vec2::new(0.0, 32.0));
    ui.add_enabled(enabled, b)
}

/// A thin bar, `fraction` (0 to 1) of it in the accent.
pub fn bar(ui: &Ui, rect: Rect, fraction: f32) {
    let c = colors();
    ui.painter().rect_filled(rect, CornerRadius::same(2), theme::with_alpha(c.text, 28));
    let w = rect.width() * fraction.clamp(0.0, 1.0);
    if w > 0.5 {
        ui.painter().rect_filled(Rect::from_min_size(rect.min, Vec2::new(w.max(4.0), rect.height())), CornerRadius::same(2), c.accent_fill);
    }
}

/// A small label in a soft pill, not clickable.
fn tag(ui: &Ui, right: f32, y: f32, text: &str) -> f32 {
    let c = colors();
    let font = FontId::proportional(12.0);
    let w = ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font.clone(), c.text)).size().x + 18.0;
    let rect = Rect::from_center_size(egui::pos2(right - w / 2.0, y), Vec2::new(w, CHIP_H));
    // The row's colour a little toward the text's, so it shows on a row in both themes (a soft
    // chip's translucent white disappears on a light row).
    let (_, _, line) = chips::soft_colors(c.light);
    ui.painter().rect(rect, CornerRadius::same((CHIP_H / 2.0) as u8), theme::mix(c.control, c.text, if c.light { 0.12 } else { 0.10 }), line, egui::StrokeKind::Inside);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, font, c.text);
    rect.left()
}

/// The detail line under an update's title: KB, size, and what is happening.
fn detail(u: &Update) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(kb) = &u.kb {
        parts.push(kb.clone());
    }
    if let Some(s) = u.size {
        parts.push(format_size(s));
    }
    parts.push(u.facts.status());
    parts.join("  \u{b7}  ")
}

/// One pending update. `action` is the label of its button, if it has one; true when pressed.
pub fn update_row(ui: &mut Ui, u: &Update, action: Option<(&str, bool)>) -> bool {
    let c = colors();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(10), c.control);
    let mut right = rect.right() - 14.0;
    let mut pressed = false;
    if let Some((label, enabled)) = action {
        let font = FontId::proportional(13.0);
        let w = ui.fonts(|f| f.layout_no_wrap(label.to_owned(), font, c.text)).size().x + 24.0;
        let b = Rect::from_center_size(egui::pos2(right - w / 2.0, rect.center().y), Vec2::new(w, 30.0));
        // A child that takes no room in the list, so the rows keep their spacing.
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(b).layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight)));
        pressed = button(&mut child, label, false, enabled).clicked();
        right = b.left() - 10.0;
    }
    right = tag(ui, right, rect.center().y, u.kind.label()) - 12.0;
    let left = rect.left() + 14.0;
    let line = |top: f32, bottom: f32| Rect::from_min_max(egui::pos2(left, top), egui::pos2(right, bottom));
    draw_text(ui, line(rect.top() + 8.0, rect.center().y), &u.title, FontId::proportional(14.0), c.text, false);
    let failed = u.facts.failed.is_some() && u.facts.progress.is_none() && !u.facts.restart_needed();
    let ink = if failed { c.danger } else { c.muted };
    draw_text(ui, line(rect.center().y, rect.bottom() - 10.0), &detail(u), FontId::proportional(12.0), ink, false);
    if let Some(p) = u.facts.progress {
        bar(ui, Rect::from_min_max(egui::pos2(left, rect.bottom() - 6.0), egui::pos2(right, rect.bottom() - 3.0)), p);
    }
    // A title cut short is shown whole on hover.
    let text_area = Rect::from_min_max(rect.min, egui::pos2(right, rect.bottom()));
    if ui.fonts(|f| f.layout_no_wrap(u.title.clone(), FontId::proportional(14.0), c.text)).size().x > right - left {
        ui.interact(text_area, ui.id().with(("update-tip", &u.id)), Sense::hover()).on_hover_text(&u.title);
    }
    pressed
}

/// One past update: title and date, and how it ended.
pub fn history_row(ui: &mut Ui, e: &Entry, date: &str) {
    let c = colors();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 50.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(10), c.control);
    let date_w = 96.0;
    let left = rect.left() + 14.0;
    let right = rect.right() - 14.0;
    let top = Rect::from_min_max(egui::pos2(left, rect.top() + 6.0), egui::pos2(right - date_w, rect.center().y + 2.0));
    draw_text(ui, top, &e.title, FontId::proportional(13.5), c.text, false);
    let when = Rect::from_min_max(egui::pos2(right - date_w, top.top()), egui::pos2(right, top.bottom()));
    draw_text(ui, when, date, FontId::proportional(12.0), c.muted, true);
    let ink: Color32 = if matches!(e.outcome, Outcome::Failed(_)) { c.danger } else { c.muted };
    let bottom = Rect::from_min_max(egui::pos2(left, rect.center().y + 2.0), egui::pos2(right, rect.bottom() - 6.0));
    draw_text(ui, bottom, &e.outcome.text(), FontId::proportional(12.0), ink, false);
}

/// A row that opens or closes a part of the page, with a chevron saying which.
pub fn toggle_row(ui: &mut Ui, text: &str, open: bool) -> Response {
    let c = colors();
    let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());
    let fill = if resp.hovered() { c.control_hover } else { c.control };
    ui.painter().rect_filled(row, CornerRadius::same(10), fill);
    let name = Rect::from_min_max(egui::pos2(row.left() + 14.0, row.top()), egui::pos2(row.right() - 44.0, row.bottom()));
    draw_text(ui, name, text, FontId::proportional(13.5), c.text, false);
    // The chevron points right when closed and down when open.
    let g = if open { '\u{E70D}' } else { glyph::CHEVRON };
    ui.painter().text(egui::pos2(row.right() - 22.0, row.center().y), Align2::CENTER_CENTER, g, icon_font(12.0), c.muted);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}
