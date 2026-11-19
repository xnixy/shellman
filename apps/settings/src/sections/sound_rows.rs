//! The pieces the Sound page is built from: a device row, a volume row, the input meter and a
//! link row. They draw and report; the page decides what a press means.

use crate::page;
use egui::{Align2, FontId, Id, Response, Sense, Ui, Vec2};
use shellman_core::system::sound_dev::{Device, Kind};
use shellman_ui::slider;
use shellman_ui::theme::{self, colors, glyph, icon_font};
use shellman_ui::widgets::{draw_text, round_button};

/// The height of a device or app item's content; the item adds its own padding around it.
pub const ROW_H: f32 = 26.0;
/// Segoe Fluent's headphones, which the shared glyph list has no name for.
const HEADPHONES: char = '\u{E7F6}';

/// The icon for a kind of device.
pub fn kind_glyph(kind: Kind) -> char {
    match kind {
        Kind::Speakers | Kind::Line | Kind::Other => glyph::VOLUME,
        Kind::Headphones | Kind::Headset => HEADPHONES,
        Kind::Microphone => glyph::MICROPHONE,
        Kind::Display => glyph::DESKTOP,
        Kind::Digital => glyph::AUDIO,
    }
}

/// One device: its icon, its name, and what it is the default for, as an item of the group's
/// list. The chosen one wears the selection wash (an outline in high contrast) under ordinary
/// text.
pub fn device_row(ui: &mut Ui, d: &Device, selected: bool) -> Response {
    let c = colors();
    page::item(ui, selected, |ui| {
        let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
        let text = c.text;
        let icon = egui::pos2(row.left() + 12.0, row.center().y);
        ui.painter().text(icon, Align2::CENTER_CENTER, kind_glyph(d.kind), icon_font(18.0), text);
        let tag = match (d.default, d.default_comm) {
            (true, true) => "Default",
            (true, false) => "Default, not for calls",
            (false, true) => "Default for calls",
            (false, false) => "",
        };
        let tag_w = if tag.is_empty() {
            0.0
        } else {
            ui.fonts(|f| f.layout_no_wrap(tag.to_owned(), FontId::proportional(12.5), c.text)).size().x + 12.0
        };
        let name = egui::Rect::from_min_max(egui::pos2(row.left() + 34.0, row.top()), egui::pos2(row.right() - tag_w - 4.0, row.bottom()));
        let name_font = FontId::proportional(13.5);
        let cut = ui.fonts(|f| f.layout_no_wrap(d.name.clone(), name_font.clone(), text)).size().x > name.width();
        draw_text(ui, name, &d.name, name_font, text, false);
        if !tag.is_empty() {
            let tag_rect = egui::Rect::from_min_max(egui::pos2(row.right() - tag_w - 4.0, row.top()), egui::pos2(row.right(), row.bottom()));
            draw_text(ui, tag_rect, tag, FontId::proportional(12.5), c.muted, true);
        }
        theme::focus_ring(ui, &resp, row.expand2(Vec2::new(6.0, 3.0)), theme::shape::TILE);
        if cut {
            return resp.on_hover_text(&d.name);
        }
        resp
    })
}

/// A device that is turned off: muted, with "Enable" at its end; pressing the row turns it on.
pub fn disabled_row(ui: &mut Ui, d: &Device) -> Response {
    let c = colors();
    page::item(ui, false, |ui| {
        let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
        let icon = egui::pos2(row.left() + 12.0, row.center().y);
        ui.painter().text(icon, Align2::CENTER_CENTER, kind_glyph(d.kind), icon_font(18.0), c.muted);
        let name = egui::Rect::from_min_max(egui::pos2(row.left() + 34.0, row.top()), egui::pos2(row.right() - 64.0, row.bottom()));
        draw_text(ui, name, &d.name, FontId::proportional(13.5), c.muted, false);
        let tag = egui::Rect::from_min_max(egui::pos2(row.right() - 60.0, row.top()), egui::pos2(row.right(), row.bottom()));
        draw_text(ui, tag, "Enable", FontId::proportional(12.5), c.text, true);
        theme::focus_ring(ui, &resp, row.expand2(Vec2::new(6.0, 3.0)), theme::shape::TILE);
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    })
}

/// What a volume row asked for.
#[derive(Default)]
pub struct Level {
    pub set: Option<f32>,
    pub toggle_mute: bool,
}

/// A 0..1 slider whose width is what is left less `tail`, with the percentage after it. `id`
/// keeps the value being dragged apart from what the audio system reports back meanwhile, so
/// the knob does not jump. Returns the new level in the frames it is moved.
pub fn level_slider(ui: &mut Ui, id: Id, name: &str, served: f32, muted: bool, tail: f32) -> Option<f32> {
    let held = ui.data(|d| d.get_temp::<f32>(id));
    let width = (ui.available_width() - 44.0 - tail).max(60.0);
    let out = slider::thin(ui, id.with("thin"), held.unwrap_or(served), Some(width), true).named(name);
    let v = out.value.or(held).unwrap_or(served);
    if out.dragging() {
        ui.data_mut(|d| d.insert_temp(id, v));
    } else {
        ui.data_mut(|d| d.remove::<f32>(id));
    }
    let shown = if muted { 0 } else { (v * 100.0).round() as u32 };
    ui.label(theme::muted(format!("{shown}%")).size(12.0));
    out.value
}

/// A mute button and a level slider.
pub fn volume_row(ui: &mut Ui, key: impl std::hash::Hash, served: f32, muted: bool) -> Level {
    let mut out = Level::default();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if mute_button(ui, muted).clicked() {
            out.toggle_mute = true;
        }
        out.set = level_slider(ui, Id::new(key), "Volume", served, muted, 0.0);
    });
    out
}

/// The speaker glyph that mutes, and says what it will do.
pub fn mute_button(ui: &mut Ui, muted: bool) -> Response {
    let (g, tip) = if muted { (glyph::MUTED, "Unmute") } else { (glyph::VOLUME, "Mute") };
    round_button(ui, g, true, tip)
}

/// The level of what a microphone hears, as a bar. Drawn on a square-root scale: speech sits low
/// on the meter's linear scale, and the bar is for seeing that the microphone works.
pub fn meter_bar(ui: &mut Ui, peak: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 8.0), Sense::hover());
    let fraction = peak.clamp(0.0, 1.0).sqrt();
    // Under a point of level is silence: no dot for it.
    let fraction = if rect.width() * fraction > 1.0 { fraction } else { 0.0 };
    page::thin_bar(ui, rect, fraction, colors().accent_fill);
}

/// A row that opens something outside the page: its text and the "open" glyph.
pub fn link_row(ui: &mut Ui, text: &str) -> Response {
    let c = colors();
    page::item(ui, false, |ui| {
        let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click());
        let name = egui::Rect::from_min_max(row.min, egui::pos2(row.right() - 30.0, row.bottom()));
        draw_text(ui, name, text, FontId::proportional(13.5), c.text, false);
        ui.painter().text(egui::pos2(row.right() - 10.0, row.center().y), Align2::CENTER_CENTER, glyph::OPEN_EXTERNAL, icon_font(14.0), c.muted);
        theme::focus_ring(ui, &resp, row.expand2(Vec2::new(6.0, 4.0)), theme::shape::TILE);
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    })
}
