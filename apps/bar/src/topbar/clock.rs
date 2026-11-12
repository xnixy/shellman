//! The top bar's centre island: the date, then the time, muted beside it, then the count of
//! unseen notifications on an accent chip (a moon instead while do-not-disturb is on and nothing
//! is new). The island is where notifications live: they hang under it, and a click on it opens
//! or closes their list. Segoe UI's digits are all one width, so the island does not change size
//! as the clock ticks.

use super::{highlight, layout};
use crate::notify::Badge;
use shellman_core::settings::bar::BarSettings;
use shellman_core::system::clock;
use shellman_ui::theme::{colors, icon_font, mix, semibold_font};

/// Space inside the island's ends, and between the date and the time.
const PAD: f32 = 18.0;
const GAP: f32 = 10.0;
/// The unseen-count chip (the mockup's accent chip: 3 px over 12 px text) and the moon.
const CHIP_H: f32 = 22.0;
const MOON: char = '\u{E708}';
/// The hover highlight's inset from the island's edge.
const INSET: f32 = 5.0;

/// What sits after the time: the count chip's text, or the moon.
enum Mark {
    Count(String),
    Moon,
}

/// Draw the clock in `row`, centred on `bar` but clear of the right island (which begins at
/// `right_starts`), with the notifications `badge`. Returns its island, and whether it was
/// clicked.
pub fn draw(
    ui: &mut egui::Ui,
    bar: egui::Rect,
    row: egui::Rect,
    right_starts: f32,
    settings: &BarSettings,
    badge: Badge,
) -> (egui::Rect, bool) {
    let c = colors();
    let (date, time) = clock::now_text(settings.clock.hour_cycle.pattern());
    let lay = |text: &str, font: egui::FontId, color| ui.painter().layout_no_wrap(text.to_owned(), font, color);
    // With the date, it leads and the time is muted beside it; alone, the time is the main text.
    let parts = if settings.clock.show_date {
        vec![lay(&date, semibold_font(14.0), c.text), lay(&time, egui::FontId::proportional(14.0), c.muted)]
    } else {
        vec![lay(&time, semibold_font(14.0), c.text)]
    };
    let mark = match (badge.unseen, badge.dnd) {
        (0, true) => Some(Mark::Moon),
        (0, false) => None,
        (n, _) => Some(Mark::Count(if n > 99 { "99+".into() } else { n.to_string() })),
    };
    let chip = match &mark {
        Some(Mark::Count(text)) => Some(lay(text, semibold_font(12.0), c.on_accent)),
        _ => None,
    };
    let mark_w = match (&mark, &chip) {
        (Some(Mark::Count(_)), Some(g)) => (g.size().x + 14.0).max(CHIP_H),
        (Some(Mark::Moon), _) => 14.0,
        _ => 0.0,
    };
    let content = parts.iter().map(|g| g.size().x).sum::<f32>()
        + GAP * (parts.len() - 1) as f32
        + if mark.is_some() { GAP + mark_w } else { 0.0 };
    // A chip at the end sits closer to the island's edge, as the mockup's does.
    let end_pad = if chip.is_some() { (layout::ISLAND_H - CHIP_H) / 2.0 } else { PAD };
    let width = content + PAD + end_pad;
    let left = layout::centre_left(bar, width, right_starts);
    let island = egui::Rect::from_x_y_ranges(left..=left + width, row.y_range());

    let resp = ui.interact(island, egui::Id::new("clock notifications"), egui::Sense::click());
    highlight(ui, island.shrink(INSET), &resp, ((layout::ISLAND_H - 2.0 * INSET) / 2.0) as u8);
    let mut x = island.left() + PAD;
    for galley in parts {
        let w = galley.size().x;
        let at = egui::pos2(x, row.center().y - galley.size().y / 2.0);
        ui.painter().galley(at, galley, c.text);
        x += w + GAP;
    }
    let mid = row.center().y;
    match (mark, chip) {
        (Some(Mark::Count(_)), Some(galley)) => {
            let rect = egui::Rect::from_min_max(egui::pos2(x, mid - CHIP_H / 2.0), egui::pos2(x + mark_w, mid + CHIP_H / 2.0));
            let fill = if resp.hovered() { mix(c.accent_fill, egui::Color32::WHITE, 0.12) } else { c.accent_fill };
            ui.painter().rect_filled(rect, (CHIP_H / 2.0) as u8, fill);
            let at = rect.center() - galley.size() / 2.0;
            ui.painter().galley(at, galley, c.on_accent);
        }
        (Some(Mark::Moon), _) => {
            ui.painter().text(egui::pos2(x + mark_w / 2.0, mid), egui::Align2::CENTER_CENTER, MOON, icon_font(13.0), c.muted);
        }
        _ => {}
    }
    let tip = match (badge.unseen, badge.dnd) {
        (0, true) => "Notifications (do not disturb)".to_string(),
        (0, false) => "Notifications".to_string(),
        (1, _) => "1 new notification".to_string(),
        (n, _) => format!("{n} new notifications"),
    };
    let clicked = resp.on_hover_text(tip).clicked();
    (island, clicked)
}
