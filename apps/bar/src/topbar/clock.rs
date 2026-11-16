//! The top bar's centre island: the date, then the time, muted beside it (with the weekday and
//! seconds as Settings › Top bar says). A click opens the calendar under it; the hover tip says
//! the full date in the region's long format. Segoe UI's digits are all one width, so the island
//! does not change size as the clock ticks. (Notifications have their own bell in the right
//! island.)

use super::{highlight, layout};
use shellman_core::settings::bar::BarSettings;
use shellman_core::system::{clock, region};

/// Space inside the island's ends, and between the date and the time.
const PAD: f32 = 18.0;
const GAP: f32 = 10.0;
/// The hover highlight's inset from the island's edge.
const INSET: f32 = 5.0;

/// Draw the clock in `row`, centred on `bar` but clear of the right island (which begins at
/// `right_starts`); `open` marks it while the calendar shows. Returns its island, and whether it
/// was clicked.
pub fn draw(ui: &mut egui::Ui, bar: egui::Rect, row: egui::Rect, right_starts: f32, settings: &BarSettings, open: bool) -> (egui::Rect, bool) {
    let c = shellman_ui::theme::colors();
    let (date, time) = clock::parts(&settings.clock);
    let lay = |text: &str, font: egui::FontId, color| ui.painter().layout_no_wrap(text.to_owned(), font, color);
    // With the date, it leads and the time is muted beside it; alone, the time is the main text.
    let parts = if settings.clock.show_date {
        vec![lay(&date, shellman_ui::theme::semibold_font(14.0), c.text), lay(&time, egui::FontId::proportional(14.0), c.muted)]
    } else {
        vec![lay(&time, shellman_ui::theme::semibold_font(14.0), c.text)]
    };
    let content = parts.iter().map(|g| g.size().x).sum::<f32>() + GAP * (parts.len() - 1) as f32;
    let width = content + 2.0 * PAD;
    let left = layout::centre_left(bar, width, right_starts);
    let island = egui::Rect::from_x_y_ranges(left..=left + width, row.y_range());

    let resp = ui.interact(island, egui::Id::new("clock calendar"), egui::Sense::click());
    // A button that opens the calendar, read with the full date and the time; "selected" while
    // the calendar is open.
    shellman_ui::theme::speak_selected(&resp, egui::WidgetType::Button, open, || format!("Calendar, {}, {time}", region::long_date_now()));
    let radius = ((layout::ISLAND_H - 2.0 * INSET) / 2.0) as u8;
    if open && !resp.hovered() {
        // Marked faintly, so the muted time stays legible on it.
        ui.painter().rect_filled(island.shrink(INSET), radius, shellman_ui::theme::with_alpha(c.hover_soft, c.hover_soft.a() / 2));
    }
    highlight(ui, island.shrink(INSET), &resp, radius);
    let mut x = island.left() + PAD;
    for galley in parts {
        let w = galley.size().x;
        let at = egui::pos2(x, row.center().y - galley.size().y / 2.0);
        ui.painter().galley(at, galley, c.text);
        x += w + GAP;
    }
    // The full date, read only while hovered.
    let clicked = resp
        .on_hover_ui(|ui| {
            ui.label(region::long_date_now());
        })
        .clicked();
    (island, clicked)
}
