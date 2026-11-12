//! The small labels above the grid's sections ("MOST USED", "ALL APPS"): muted, small,
//! upper-case and letter-spaced. The font has no semibold, so spacing carries the emphasis.

use egui::text::{LayoutJob, TextFormat};
use egui::{Painter, Rect};
use shellman_ui::theme::colors;

/// Draw `title` at the left of `rect`, a little below its middle (so it sits near what it labels).
pub fn label(p: &Painter, rect: Rect, title: &str) {
    let job = LayoutJob::single_section(
        title.to_uppercase(),
        TextFormat {
            font_id: egui::FontId::proportional(11.0),
            color: colors().muted,
            extra_letter_spacing: 0.9,
            ..Default::default()
        },
    );
    let galley = p.ctx().fonts(|f| f.layout_job(job));
    let pos = egui::pos2(rect.left(), rect.center().y + 3.0 - galley.size().y / 2.0);
    p.galley(pos, galley, colors().muted);
}
