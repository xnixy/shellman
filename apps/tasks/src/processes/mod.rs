//! The Processes page: the machine's summary on top, the list, and the command bar at the
//! bottom for the selection, each a box on the sheet.

mod details;
mod list;
mod pill;
mod row;
mod summary;

pub use list::header_name;

use crate::app::{card_box, State, MARGIN};
use egui::{pos2, Rect};

const SUMMARY_H: f32 = 100.0;
const PILL_H: f32 = 52.0;

pub fn show(ui: &mut egui::Ui, area: Rect, st: &mut State) {
    let summary = Rect::from_min_max(area.min, pos2(area.right(), area.top() + SUMMARY_H));
    let pill = Rect::from_min_max(pos2(area.left(), area.bottom() - PILL_H), area.max);
    let list = Rect::from_min_max(pos2(area.left(), summary.bottom() + MARGIN), pos2(area.right(), pill.top() - MARGIN));
    card_box(ui, summary);
    summary::show(ui, summary, st);
    if list.height() > 60.0 {
        card_box(ui, list);
        list::show(ui, list, st);
    }
    card_box(ui, pill);
    pill::show(ui, pill, st);
}
