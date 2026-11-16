//! The Processes page: the machine's summary on top, the list, and the command pill at the
//! bottom for the selection.

mod details;
mod list;
mod pill;
mod row;
mod summary;

pub use list::header_name;

use crate::app::{State, MARGIN, RADIUS};
use egui::{pos2, Rect};
use shellman_ui::glass::islands::Island;

const SUMMARY_H: f32 = 100.0;
const PILL_H: f32 = 52.0;

pub fn show(ui: &mut egui::Ui, area: Rect, st: &mut State, islands: &mut Vec<Island>) {
    let summary = Rect::from_min_max(area.min, pos2(area.right(), area.top() + SUMMARY_H));
    let pill = Rect::from_min_max(pos2(area.left(), area.bottom() - PILL_H), area.max);
    let list = Rect::from_min_max(pos2(area.left(), summary.bottom() + MARGIN), pos2(area.right(), pill.top() - MARGIN));
    islands.push(Island::new(summary, RADIUS));
    summary::show(ui, summary, st);
    if list.height() > 60.0 {
        islands.push(Island::new(list, RADIUS));
        list::show(ui, list, st);
    }
    islands.push(Island::pill(pill));
    pill::show(ui, pill, st);
}
