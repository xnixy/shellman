//! The control centre's building blocks: tiles, round glyph chips, thin sliders and round
//! buttons. Glasshouse's quiet look: borderless, in the field fill (the text colour, faint), with
//! the accent only for things that are on.

use egui::{Align2, Color32, FontId, Pos2, Rect, Response, Sense, Ui, Vec2};
use shellman_ui::chips::soft_colors;
use shellman_ui::theme::{self, colors, icon_font, Colors};

/// Spacing, in points.
pub const GAP: f32 = 10.0;
/// A quick-setting tile is a card set in an island: a little tighter than the island's own
/// corner (`shape::ISLAND`), so the two read as nested.
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

/// The fill of something quiet set on an island (a chip, a round button, a pill, the art's
/// plate): Glasshouse's field, the text colour faint, a step deeper under the pointer. Taken from
/// the shared chips, so it shows over the glass as faint as drawn (see
/// `glass::islands::shows_as`) and is the window colour in high contrast, where things are
/// outlined instead.
pub fn field(hovered: bool) -> Color32 {
    let (rest, hover, _) = soft_colors(colors().light);
    if hovered {
        hover
    } else {
        rest
    }
}

/// Behind a hovered row in a list: Glasshouse's row hover, as it shows over the glass. Nothing in
/// high contrast, where the row is outlined instead (`theme::contrast_outline`).
pub fn row_hover() -> Color32 {
    colors().row_hover
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

/// The fill and glyph colour of something that is on: the accent fill with its ink. The theme
/// already falls back to the text colour where the accent would hardly show (black on dark, white
/// on light), and in high contrast it is highlight under highlight text.
pub fn on_colors(c: &Colors) -> (Color32, Color32) {
    (c.accent_fill, c.on_accent)
}

/// A round chip with a glyph: filled with the "on" colour when on, faint text when off.
pub fn chip(ui: &Ui, center: Pos2, glyph: char, on: bool, hovered: bool) {
    let c = colors();
    let (on_fill, on_fg) = on_colors(&c);
    let (mut fill, fg) = if on { (on_fill, on_fg) } else { (field(false), c.text) };
    if hovered && !on {
        fill = field(true);
    } else if hovered && !c.high_contrast {
        // On, the accent leans toward the text colour, which keeps its ink readable; high
        // contrast keeps Windows' highlight pair and shows hover by the outline.
        fill = theme::mix(fill, c.text, 0.12);
    }
    ui.painter().circle_filled(center, CHIP / 2.0, fill);
    outline_circle(ui, center, CHIP / 2.0, hovered);
    ui.painter().text(center, Align2::CENTER_CENTER, glyph, icon_font(GLYPH), fg);
}

/// High contrast: a round thing outlined in the text colour, 2 px while hovered.
pub fn outline_circle(ui: &Ui, center: Pos2, radius: f32, hovered: bool) {
    let c = colors();
    if c.high_contrast {
        let w = if hovered { 2.0 } else { 1.0 };
        ui.painter().circle_stroke(center, radius - w / 2.0, egui::Stroke::new(w, c.text));
    }
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

/// A glyph and the shared thin slider after it. `value` is 0..=1. `name` is what a screen
/// reader calls the slider; `glyph_name` names the glyph as a button (mute, for sound), or None
/// when it is only a picture.
pub fn slider(ui: &mut Ui, id: egui::Id, value: f32, glyph: char, name: &str, glyph_name: Option<&str>) -> SliderOut {
    let c = colors();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), shellman_ui::slider::HEIGHT), Sense::hover());
    let icon = Rect::from_min_size(rect.min, Vec2::new(SLIDER_GLYPH_W, rect.height()));
    let mut glyph_clicked = false;
    let mut fg = c.muted;
    if let Some(glyph_name) = glyph_name {
        let glyph_resp = ui.interact(icon, id.with("glyph"), Sense::click());
        theme::speak(&glyph_resp, egui::WidgetType::Button, || glyph_name.to_owned());
        theme::focus_ring(ui, &glyph_resp, icon.shrink(1.0), 8.0);
        if glyph_resp.hovered() {
            fg = c.text;
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        glyph_clicked = glyph_resp.clicked();
    }
    ui.painter().text(icon.center(), Align2::CENTER_CENTER, glyph, icon_font(15.0), fg);
    let track = Rect::from_min_max(Pos2::new(icon.right() + 6.0, rect.top()), rect.max);
    let out = shellman_ui::slider::thin_in(ui, track, id, value, true).named(name);
    SliderOut { value: out.value, glyph_clicked, dragging: out.dragging() }
}

/// A round, borderless button with a glyph, `size` points across.
pub fn round_button(ui: &mut Ui, glyph: char, size: f32, active: bool, tip: &str) -> Response {
    let c = colors();
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    // A plain button (Settings, Lock, Sleep invoke); the power one says when its choices show.
    theme::speak(&resp, egui::WidgetType::Button, || if active { format!("{tip}, choices shown") } else { tip.to_owned() });
    theme::focus_ring(ui, &resp, rect, size / 2.0);
    let (on_fill, on_fg) = on_colors(&c);
    let (fill, fg) = match (active, resp.hovered()) {
        (true, _) => (on_fill, on_fg),
        (false, true) => (field(true), c.text),
        (false, false) => (field(false), c.text),
    };
    ui.painter().circle_filled(rect.center(), size / 2.0, fill);
    if !active {
        outline_circle(ui, rect.center(), size / 2.0, resp.hovered());
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use shellman_ui::theme::contrast;

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
