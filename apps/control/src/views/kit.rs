//! The control centre's building blocks: tiles, round glyph chips, thick sliders and round
//! buttons. Borderless, in the text colour at low strength, with the accent for things that
//! are on.

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Ui, Vec2};
use shellman_ui::theme::{self, colors, icon_font, with_alpha, Colors};

/// Spacing, in points.
pub const PAD: f32 = 14.0;
pub const GAP: f32 = 10.0;
pub const TILE_RADIUS: u8 = 16;
pub const TILE_INSET: f32 = 12.0;
pub const CHIP: f32 = 30.0;
pub const GLYPH: f32 = 16.0;
pub const SLIDER: f32 = 22.0;

pub fn label_font() -> FontId {
    FontId::proportional(13.0)
}

pub fn small_font() -> FontId {
    FontId::proportional(11.5)
}

/// The panel's translucent fills. The window is composited so that a translucent colour lifts
/// the blurred backdrop much more than its alpha suggests (measured on the dark panel: white at
/// 7% opacity lifted it by a quarter of the way to white), so the alphas here are small and
/// were picked by eye on screen captures. A light tile's white is at 1% opacity; black, which
/// only dims, behaves as its alpha says. Dark panels get a mid grey, not white, which keeps the
/// steps between alphas fine; light panels get white for raised tiles and black for what sinks.
fn veil(dark_alpha: u8, light: (u8, bool)) -> Color32 {
    if colors().light {
        let base = if light.1 { Color32::WHITE } else { Color32::BLACK };
        with_alpha(base, light.0)
    } else {
        with_alpha(Color32::from_gray(120), dark_alpha)
    }
}

/// A tile's fill: faintly lighter than the glass, a little more under the pointer.
pub fn tile_fill(hovered: bool) -> Color32 {
    if hovered {
        veil(15, (4, true))
    } else {
        veil(10, (2, true))
    }
}

/// Something set into a tile or the panel: a chip, a slider's track, a round button at rest;
/// `hovered` is the stronger step.
pub fn well(hovered: bool) -> Color32 {
    if hovered {
        veil(22, (34, false))
    } else {
        veil(13, (20, false))
    }
}

/// A full-width tile holding `add`, padded on every side.
pub fn tile<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(tile_fill(false))
        .corner_radius(CornerRadius::same(TILE_RADIUS))
        .inner_margin(egui::Margin::same(TILE_INSET as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// The fill and glyph colour of something that is on. The accent, unless it would hardly stand
/// out from the panel (an accent of pure black on dark, or white on light); then the text colour.
pub fn on_colors(c: &Colors) -> (Color32, Color32) {
    if contrast(c.accent, c.bg) >= 1.6 {
        (c.accent, c.on_accent)
    } else {
        (c.text, c.bg)
    }
}

/// A round chip with a glyph: filled with the "on" colour when on, faint text when off.
pub fn chip(ui: &Ui, center: Pos2, glyph: char, on: bool, hovered: bool) {
    let c = colors();
    let (on_fill, on_fg) = on_colors(&c);
    let (mut fill, fg) = if on { (on_fill, on_fg) } else { (well(false), c.text) };
    if hovered {
        fill = if on { theme::mix(fill, c.text, 0.12) } else { well(true) };
    }
    ui.painter().circle_filled(center, CHIP / 2.0, fill);
    ui.painter().text(center, Align2::CENTER_CENTER, glyph, icon_font(GLYPH), fg);
}

/// What a slider did this frame.
#[derive(Default)]
pub struct SliderOut {
    /// A new value, while the pointer drags or presses on the track.
    pub value: Option<f32>,
    /// The glyph at the left end was clicked (mute, for sound).
    pub glyph_clicked: bool,
    pub dragging: bool,
}

/// A thick rounded track with `glyph` in its left end and the fill in the text colour; no
/// knob. `value` is 0..=1.
pub fn slider(ui: &mut Ui, id: egui::Id, value: f32, glyph: char) -> SliderOut {
    let c = colors();
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, SLIDER), Sense::hover());
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    let origin = ui.input(|i| i.pointer.press_origin());
    let from_glyph = origin.is_some_and(|p| p.x < rect.left() + SLIDER && rect.contains(p));
    let mut out = SliderOut::default();
    if resp.clicked() && from_glyph {
        out.glyph_clicked = true;
    } else if resp.is_pointer_button_down_on() && !from_glyph {
        if let Some(p) = resp.interact_pointer_pos() {
            out.value = Some(value_at(p.x, rect.left(), rect.width(), SLIDER));
        }
        out.dragging = true;
    }
    let shown = out.value.unwrap_or(value);
    let radius = CornerRadius::same((SLIDER / 2.0) as u8);
    ui.painter().rect_filled(rect, radius, well(false));
    let fill = Rect::from_min_size(rect.min, Vec2::new(fill_width(shown, rect.width(), SLIDER), SLIDER));
    ui.painter().rect_filled(fill, radius, with_alpha(c.text, 217));
    let at = Pos2::new(rect.left() + SLIDER / 2.0, rect.center().y);
    let fg = if from_glyph && resp.hovered() { with_alpha(c.bg, 200) } else { c.bg };
    ui.painter().text(at, Align2::CENTER_CENTER, glyph, icon_font(12.0), fg);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    out
}

/// How wide the fill is for `value`: never narrower than the round end that holds the glyph.
pub fn fill_width(value: f32, width: f32, height: f32) -> f32 {
    height + value.clamp(0.0, 1.0) * (width - height).max(0.0)
}

/// The value under a pointer at `x`, the inverse of `fill_width` at the fill's right edge less
/// half the round end, so the pointer sits in the middle of the fill's end.
pub fn value_at(x: f32, left: f32, width: f32, height: f32) -> f32 {
    let span = (width - height).max(1.0);
    ((x - left - height / 2.0) / span).clamp(0.0, 1.0)
}

/// A round, borderless button with a glyph, `size` points across.
pub fn round_button(ui: &mut Ui, glyph: char, size: f32, active: bool, tip: &str) -> Response {
    let c = colors();
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let (on_fill, on_fg) = on_colors(&c);
    let (fill, fg) = match (active, resp.hovered()) {
        (true, _) => (on_fill, on_fg),
        (false, true) => (well(true), c.text),
        (false, false) => (tile_fill(false), c.text),
    };
    ui.painter().circle_filled(rect.center(), size / 2.0, fill);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, glyph, icon_font(GLYPH), fg);
    resp.on_hover_text(tip)
}

/// Text cut to `width` with an ellipsis, in one line.
pub fn clipped_text(ui: &Ui, pos: Pos2, anchor: Align2, text: &str, font: FontId, color: Color32, width: f32) {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(1.0));
    let galley = ui.fonts(|f| f.layout_job(job));
    let rect = anchor.anchor_size(pos, galley.size());
    ui.painter().galley(rect.min, galley, color);
}

/// WCAG contrast ratio between two colours (1 = none, 21 = black on white).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn luminance(c: Color32) -> f32 {
    let ch = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_fill_and_pointer_agree() {
        let (left, w, h) = (10.0, 300.0, 22.0);
        assert_eq!(fill_width(0.0, w, h), h);
        assert_eq!(fill_width(1.0, w, h), w);
        assert_eq!(fill_width(2.0, w, h), w);
        for v in [0.0, 0.25, 0.5, 1.0] {
            // The pointer at the middle of the fill's round end reads back the same value.
            let x = left + fill_width(v, w, h) - h / 2.0;
            assert!((value_at(x, left, w, h) - v).abs() < 1e-4);
        }
        assert_eq!(value_at(-100.0, left, w, h), 0.0);
        assert_eq!(value_at(1000.0, left, w, h), 1.0);
    }

    #[test]
    fn black_and_white_accents_fall_back_to_text() {
        for light in [false, true] {
            for accent in [Color32::BLACK, Color32::WHITE, Color32::from_rgb(0, 103, 192)] {
                let c = Colors::new(light, accent);
                let (fill, fg) = on_colors(&c);
                assert!(contrast(fill, c.bg) >= 1.6, "on fill must stand out ({light}, {accent:?})");
                assert!(contrast(fill, fg) >= 3.0, "glyph must read on the fill ({light}, {accent:?})");
            }
        }
    }
}
