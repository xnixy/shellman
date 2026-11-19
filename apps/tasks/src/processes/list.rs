//! The list box: the column headers (a click sorts), and the rows below, only those in view
//! drawn. While the pointer is over it the order is held, so rows never move from under it.

use super::row::{self, Press, COLUMNS, ROW_H};
use super::details::DETAILS_H;
use crate::actions::Act;
use crate::app::State;
use crate::model::{self, Line};
use egui::{pos2, vec2, Align2, FontId, Rect, Sense, Stroke, UiBuilder};
use shellman_core::procs::sort::Column;
use shellman_ui::theme::{self, colors, glyph, icon_font};

const HEADER_H: f32 = 40.0;
/// Room left and right of the rows inside the box.
pub const PAD: f32 = 12.0;

pub fn show(ui: &mut egui::Ui, rect: Rect, st: &mut State) {
    let header = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER_H));
    headers(ui, header.shrink2(vec2(PAD, 0.0)), st);
    let c = colors();
    ui.painter().line_segment(
        [pos2(rect.left(), header.bottom()), pos2(rect.right(), header.bottom())],
        Stroke::new(1.0_f32, if c.high_contrast { c.text } else { c.hairline }),
    );
    let body = Rect::from_min_max(pos2(rect.left() + PAD - 4.0, header.bottom() + 2.0), pos2(rect.right() - 4.0, rect.bottom() - 10.0));
    let over = ui.rect_contains_pointer(rect);
    st.list.hold(over);
    st.list.take_focus = std::mem::take(&mut st.list.row_focus);
    st.list.tab_stop = st.list.lines.iter().find_map(|l| match *l {
        Line::Group { group, .. } => Some(st.list.procs[st.list.groups[group].root].key),
        _ => None,
    });
    keys(ui, st);
    let mut press = None;
    ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
        ui.spacing_mut().scroll = egui::style::ScrollStyle::floating();
        let out = egui::ScrollArea::vertical()
            .id_salt("process-list")
            .auto_shrink([false, false])
            .drag_to_scroll(false)
            .show_viewport(ui, |ui, view| {
                let (tops, height) = model::tops(&st.list.lines, ROW_H, DETAILS_H);
                let width = ui.available_width() - 8.0;
                let (all, area) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
                let rows = st.list.lines.iter().filter(|l| matches!(l, Line::Group { .. } | Line::Member { .. })).count();
                area.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("Processes, {rows} rows")));
                if std::mem::take(&mut st.list.reveal) {
                    // The selected row, and its details under it when they are open.
                    let at = |i: usize| Rect::from_min_size(pos2(all.left(), all.top() + tops[i]), vec2(width, ROW_H));
                    let details = st.list.lines.iter().position(|l| matches!(l, Line::Details { .. }));
                    let row = details.map(|d| d.saturating_sub(1)).or_else(|| selected_line(st));
                    if let Some(r) = row {
                        let mut target = at(r);
                        if let Some(d) = details {
                            target.max.y = all.top() + tops[d] + DETAILS_H;
                        }
                        ui.scroll_to_rect(target, None);
                    }
                }
                for i in model::visible(&tops, height, view.top(), view.bottom()) {
                    let line = st.list.lines[i];
                    let h = if matches!(line, Line::Details { .. }) { DETAILS_H } else { ROW_H };
                    let r = Rect::from_min_size(pos2(all.left(), all.top() + tops[i]), vec2(width, h));
                    if let Some(p) = row::show(ui, r, line, st) {
                        press = Some(p);
                    }
                }
            });
        shellman_ui::widgets::name_scroll_bar(ui.ctx(), &out, "processes");
    });
    match press {
        Some(Press::Select(k, member)) => st.list.select_row(k, member),
        Some(Press::Toggle(k)) => st.list.toggle(k),
        Some(Press::Details(k, member)) => {
            st.list.select_row(k, member);
            st.list.toggle_details();
        }
        None => {}
    }
}

/// The index of the selected row among the lines.
fn selected_line(st: &State) -> Option<usize> {
    let sel = st.list.selected?;
    let list = &st.list;
    list.lines.iter().position(|l| match *l {
        Line::Group { group, .. } => list.procs[list.groups[group].root].key == sel && !list.as_member,
        Line::Member { proc, .. } => list.procs[proc].key == sel,
        _ => false,
    })
}

/// The column titles; the one sorted on carries an arrow.
fn headers(ui: &mut egui::Ui, rect: Rect, st: &mut State) {
    let c = colors();
    let sort = st.list.sort;
    let mut cells = vec![(Column::Name, "Name", row::name_cell(rect))];
    cells.extend(COLUMNS.iter().map(|col| (col.column, col.title, row::cell(rect, col.column))));
    for (column, title, cell) in cells {
        let resp = ui.interact(cell, egui::Id::new(("header", title)), Sense::click());
        let current = sort.column == column;
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, header_name(title, current, sort.descending)));
        theme::focus_ring(ui, &resp, cell.shrink(2.0), theme::shape::MENU_ROW);
        let color = if current || resp.hovered() { c.text } else { c.muted };
        let right = column != Column::Name && column != Column::Status;
        let font = FontId::proportional(12.5);
        let (anchor, x) = if right { (Align2::RIGHT_CENTER, cell.right() - 8.0) } else { (Align2::LEFT_CENTER, cell.left() + 8.0) };
        let text = ui.painter().text(pos2(x, cell.center().y), anchor, title, font, color);
        if current {
            let arrow = if sort.descending { glyph::SORT_DOWN } else { glyph::SORT_UP };
            let ax = if right { text.left() - 9.0 } else { text.right() + 9.0 };
            ui.painter().text(pos2(ax, cell.center().y), Align2::CENTER_CENTER, arrow, icon_font(9.0), theme::with_alpha(c.text, 200));
        }
        if resp.clicked() {
            st.list.set_sort(sort.clicked(column));
        }
    }
}

/// A column header as a screen reader says it: "CPU, sorted descending".
pub fn header_name(title: &str, current: bool, descending: bool) -> String {
    match (current, descending) {
        (false, _) => format!("{title}, sort by"),
        (true, true) => format!("{title}, sorted descending"),
        (true, false) => format!("{title}, sorted ascending"),
    }
}

/// Up and Down move the selection, Right and Left open and close a group, Delete ends the
/// selection, Enter opens its details. Not while the search field has the keyboard.
fn keys(ui: &mut egui::Ui, st: &mut State) {
    // Not while a button or the search has the keyboard: there the keys are theirs.
    let elsewhere = ui.memory(|m| m.focused().is_some()) && !st.list.take_focus && !st.list.focus_pending;
    if ui.ctx().wants_keyboard_input() || elsewhere {
        return;
    }
    let (up, down, left, right, delete, enter) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::ArrowLeft),
            i.key_pressed(egui::Key::ArrowRight),
            i.key_pressed(egui::Key::Delete),
            i.key_pressed(egui::Key::Enter),
        )
    });
    let list = &st.list;
    let keys: Vec<_> = list
        .lines
        .iter()
        .filter_map(|l| match *l {
            Line::Group { group, .. } => Some(list.procs[list.groups[group].root].key),
            Line::Member { proc, .. } => Some(list.procs[proc].key),
            _ => None,
        })
        .collect();
    if up || down {
        let at = list.selected.and_then(|s| keys.iter().position(|k| *k == s));
        let next = match (at, down) {
            (None, _) => keys.first(),
            (Some(i), true) => keys.get(i + 1).or(keys.get(i)),
            (Some(i), false) => keys.get(i.saturating_sub(1)),
        };
        st.list.select(next.copied());
        st.list.reveal = true;
        st.list.focus_pending |= st.list.take_focus;
    }
    let Some(sel) = st.list.selected else {
        return;
    };
    let is_closed_group = |open: bool| st.list.groups.iter().any(|g| st.list.procs[g.root].key == sel && !g.members.is_empty()) && st.list.expanded.contains(&sel) == open;
    if (right && is_closed_group(false)) || (left && is_closed_group(true)) {
        st.list.toggle(sel);
    }
    if enter {
        st.list.toggle_details();
    }
    if delete {
        st.actions.start(&st.list, Act::End, &st.control, st.own_pid);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn headers_say_their_sort() {
        assert_eq!(super::header_name("CPU", true, true), "CPU, sorted descending");
        assert_eq!(super::header_name("Name", true, false), "Name, sorted ascending");
        assert_eq!(super::header_name("PID", false, true), "PID, sort by");
    }
}
