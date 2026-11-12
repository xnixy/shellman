//! The control centre's building blocks: tiles, round glyph chips, thin sliders and round
//! buttons. Borderless, in the text colour at low strength, with the accent for things that
//! are on.

use egui::{Align2, Color32, FontId, Pos2, Rect, Response, Sense, Ui, Vec2};
use shellman_ui::theme::{self, colors, icon_font, with_alpha, Colors};

/// Spacing, in points.
pub const GAP: f32 = 10.0;
pub const TILE_RADIUS: u8 = 18;
pub const TILE_INSET: f32 = 12.0;
pub const CHIP: f32 = 30.0;
pub const GLYPH: f32 = 16.0;

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

/// A full-width block holding `add`. It has no fill or padding of its own: it sits in an island
/// (see `island`), which is the surface.
pub fn tile<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope(|ui| {
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

/// Width of the glyph that sits before a slider's track.
const SLIDER_GLYPH_W: f32 = 26.0;

/// A glyph (it can be clicked) and the shared thin slider after it. `value` is 0..=1.
pub fn slider(ui: &mut Ui, id: egui::Id, value: f32, glyph: char) -> SliderOut {
    let c = colors();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), shellman_ui::slider::HEIGHT), Sense::hover());
    let icon = Rect::from_min_size(rect.min, Vec2::new(SLIDER_GLYPH_W, rect.height()));
    let glyph_resp = ui.interact(icon, id.with("glyph"), Sense::click());
    let fg = if glyph_resp.hovered() { c.text } else { c.muted };
    ui.painter().text(icon.center(), Align2::CENTER_CENTER, glyph, icon_font(15.0), fg);
    if glyph_resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let track = Rect::from_min_max(Pos2::new(icon.right() + 6.0, rect.top()), rect.max);
    let out = shellman_ui::slider::thin_in(ui, track, id, value, true);
    SliderOut { value: out.value, glyph_clicked: glyph_resp.clicked(), dragging: out.dragging() }
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
