//! The top bar's centre island: the date, then the time, muted beside it. Segoe UI's digits are
//! all one width, so the island does not change size as the clock ticks.

use super::layout;
use shellman_core::settings::bar::BarSettings;
use shellman_core::system::clock;
use shellman_ui::theme::{colors, semibold_font};

/// Space inside the island's ends, and between the date and the time.
const PAD: f32 = 18.0;
const GAP: f32 = 10.0;

/// Draw the clock in `row`, centred on `bar` but clear of the right island (which begins at
/// `right_starts`). Returns its island.
pub fn draw(ui: &egui::Ui, bar: egui::Rect, row: egui::Rect, right_starts: f32, settings: &BarSettings) -> egui::Rect {
    let c = colors();
    let (date, time) = clock::now_text(settings.clock.hour_cycle.pattern());
    let lay = |text: &str, font: egui::FontId, color| ui.painter().layout_no_wrap(text.to_owned(), font, color);
    // With the date, it leads and the time is muted beside it; alone, the time is the main text.
    let parts = if settings.clock.show_date {
        vec![lay(&date, semibold_font(14.0), c.text), lay(&time, egui::FontId::proportional(14.0), c.muted)]
    } else {
        vec![lay(&time, semibold_font(14.0), c.text)]
    };
    let content = parts.iter().map(|g| g.size().x).sum::<f32>() + GAP * (parts.len() - 1) as f32;
    let width = content + 2.0 * PAD;
    let left = layout::centre_left(bar, width, right_starts);
    let island = egui::Rect::from_x_y_ranges(left..=left + width, row.y_range());
    let mut x = island.left() + PAD;
    for galley in parts {
        let w = galley.size().x;
        let at = egui::pos2(x, row.center().y - galley.size().y / 2.0);
        ui.painter().galley(at, galley, c.text);
        x += w + GAP;
    }
    island
}
