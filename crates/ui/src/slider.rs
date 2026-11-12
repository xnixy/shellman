//! The Islands slider: a thin track (6 pt) filled with the accent up to a white 18 pt knob.
//! Every app's sliders use this one, so a volume bar looks the same in Settings and in Control
//! Center. The value is always 0 to 1; a caller with another range maps it.

use crate::glass::islands::shows_as;
use crate::theme::{colors, with_alpha};
use egui::{pos2, vec2, Color32, Id, Rect, Response, Sense, Stroke, Ui};

/// Thickness of the track, in points.
pub const TRACK_H: f32 = 6.0;
/// Diameter of the knob.
pub const KNOB: f32 = 18.0;
/// The widget is as tall as this, so the knob has room to be grabbed.
pub const HEIGHT: f32 = 26.0;

/// What a slider did this frame.
pub struct Out {
    /// A new value, while the pointer presses or drags on it.
    pub value: Option<f32>,
    pub response: Response,
}

impl Out {
    pub fn dragging(&self) -> bool {
        self.response.is_pointer_button_down_on()
    }
}

/// Where the knob's centre is for `value`: it travels between half a knob in from each end, so
/// the knob never hangs over the track's ends.
pub fn knob_x(value: f32, left: f32, width: f32) -> f32 {
    let span = (width - KNOB).max(0.0);
    left + KNOB / 2.0 + value.clamp(0.0, 1.0) * span
}

/// The value under a pointer at `x`: the inverse of [`knob_x`].
pub fn value_at(x: f32, left: f32, width: f32) -> f32 {
    let span = (width - KNOB).max(1.0);
    ((x - left - KNOB / 2.0) / span).clamp(0.0, 1.0)
}

/// The empty part of the track, as it should show on glass.
fn track_color(light: bool) -> Color32 {
    if light {
        Color32::from_black_alpha(36)
    } else {
        shows_as(Color32::from_rgba_unmultiplied(255, 255, 255, 30))
    }
}

/// A slider filling the width of `rect`, which should be about [`HEIGHT`] tall. `value` is the
/// level to show; while the pointer is on it the pointer's level is shown and returned.
pub fn thin_in(ui: &mut Ui, rect: Rect, id: Id, value: f32, enabled: bool) -> Out {
    let c = colors();
    let sense = if enabled { Sense::click_and_drag() } else { Sense::hover() };
    let response = ui.interact(rect, id, sense);
    let mut picked = None;
    if enabled && response.is_pointer_button_down_on() {
        if let Some(p) = response.interact_pointer_pos() {
            picked = Some(value_at(p.x, rect.left(), rect.width()));
        }
    }
    let shown = picked.unwrap_or(value);
    let y = rect.center().y;
    let x = knob_x(shown, rect.left(), rect.width());
    let track = Rect::from_min_max(pos2(rect.left(), y - TRACK_H / 2.0), pos2(rect.right(), y + TRACK_H / 2.0));
    let fill = Rect::from_min_max(track.min, pos2(x, track.bottom()));
    let round = TRACK_H / 2.0;
    let p = ui.painter();
    let (fill_color, knob) = if enabled { (c.accent_fill, Color32::WHITE) } else { (with_alpha(c.muted, 120), Color32::from_gray(200)) };
    p.rect_filled(track, round, track_color(c.light));
    p.rect_filled(fill, round, fill_color);
    // A soft shadow under the knob, and a hairline so a white knob still shows on a light page.
    p.circle_filled(pos2(x, y + 1.0), KNOB / 2.0 + 1.0, Color32::from_black_alpha(if c.light { 30 } else { 70 }));
    let grow = if response.hovered() || response.dragged() { 0.5 } else { 0.0 };
    p.circle(pos2(x, y), KNOB / 2.0 + grow, knob, Stroke::new(1.0_f32, Color32::from_black_alpha(if c.light { 50 } else { 0 })));
    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    Out { value: picked, response }
}

/// A slider across `width` points (or what is left of the row when `None`), on a line of its
/// own height, in the current layout.
pub fn thin(ui: &mut Ui, id: Id, value: f32, width: Option<f32>, enabled: bool) -> Out {
    let w = width.unwrap_or_else(|| ui.available_width()).max(KNOB * 2.0);
    let (rect, _) = ui.allocate_exact_size(vec2(w, HEIGHT), Sense::hover());
    thin_in(ui, rect, id, value, enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_knob_stays_inside_the_track() {
        assert_eq!(knob_x(0.0, 10.0, 200.0), 10.0 + KNOB / 2.0);
        assert_eq!(knob_x(1.0, 10.0, 200.0), 210.0 - KNOB / 2.0);
        assert_eq!(knob_x(7.0, 10.0, 200.0), knob_x(1.0, 10.0, 200.0));
    }

    #[test]
    fn the_pointer_reads_back_the_knob() {
        for v in [0.0, 0.3, 0.62, 1.0] {
            let x = knob_x(v, 20.0, 300.0);
            assert!((value_at(x, 20.0, 300.0) - v).abs() < 1e-4);
        }
        assert_eq!(value_at(-50.0, 20.0, 300.0), 0.0);
        assert_eq!(value_at(900.0, 20.0, 300.0), 1.0);
    }
}
