//! A labelled slider over a whole-number range, shared by the pages that edit one (Mouse &
//! keyboard). It reports the value as it moves and whether the pointer is still holding the knob,
//! so the page applies the change once the knob is let go.

use crate::page;
use shellman_ui::slider;
use shellman_ui::theme;

/// What a slider row did this frame.
pub struct Moved {
    /// A value was picked.
    pub value: Option<f32>,
    /// The knob is held down.
    pub held: bool,
}

/// The small label, then a slider with `shown` (the value in words) after it. `at` is the
/// knob's place, 0 to 1.
pub fn show(ui: &mut egui::Ui, id: egui::Id, title: &str, at: f32, shown: &str) -> Moved {
    page::label(ui, title);
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 130.0).clamp(120.0, 360.0);
        let out = slider::thin(ui, id, at, Some(width), true).named(title);
        ui.label(theme::muted(shown).size(12.0));
        Moved { value: out.value, held: out.dragging() }
    })
    .inner
}
