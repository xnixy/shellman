//! Small pieces every page is built from: the heading, field labels, and a segmented choice.

use egui::{Align2, Color32, CornerRadius, FontId, RichText, Sense, Ui, Vec2};
use shellman_ui::theme::{self, colors};

/// The page's title and a line under it saying what it is for.
pub fn heading(ui: &mut Ui, title: &str, about: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(title).size(20.0).strong().color(colors().text));
    ui.label(theme::muted(about).size(12.5));
    ui.add_space(16.0);
}

/// The small label over a control.
pub fn label(ui: &mut Ui, text: &str) {
    ui.label(theme::muted(text).size(12.0));
    ui.add_space(2.0);
}

/// Space between one setting and the next.
pub fn gap(ui: &mut Ui) {
    ui.add_space(20.0);
}

/// A row of options of which one is chosen, drawn as a track with the choice lifted out in the
/// accent. True when the choice changed this frame.
pub fn segmented<T: Copy + PartialEq>(ui: &mut Ui, current: &mut T, options: &[(T, &str)]) -> bool {
    let c = colors();
    let font = FontId::proportional(13.0);
    let widths: Vec<f32> = options
        .iter()
        .map(|(_, text)| {
            ui.fonts(|f| f.layout_no_wrap((*text).to_owned(), font.clone(), c.text))
                .size()
                .x
                + 28.0
        })
        .collect();
    let height = 32.0;
    let (track, _) = ui.allocate_exact_size(
        Vec2::new(widths.iter().sum::<f32>() + 6.0, height),
        Sense::hover(),
    );
    ui.painter().rect_filled(track, CornerRadius::same(10), c.control);
    let mut changed = false;
    let mut x = track.left() + 3.0;
    for ((value, text), width) in options.iter().zip(&widths) {
        let seg = egui::Rect::from_min_size(
            egui::pos2(x, track.top() + 3.0),
            Vec2::new(*width, height - 6.0),
        );
        x += width;
        let resp = ui.interact(seg, ui.id().with(("segment", *text)), Sense::click());
        let selected = *value == *current;
        if selected {
            ui.painter().rect_filled(seg, CornerRadius::same(8), c.accent);
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(seg, CornerRadius::same(8), theme::with_alpha(c.text, 14));
        }
        let color: Color32 = if selected { c.on_accent } else { c.text };
        ui.painter()
            .text(seg.center(), Align2::CENTER_CENTER, text, font.clone(), color);
        if resp.clicked() && !selected {
            *current = *value;
            changed = true;
        }
    }
    changed
}
