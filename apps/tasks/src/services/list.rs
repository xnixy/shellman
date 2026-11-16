//! The services list island (headers that sort, the rows in view) and the card for the selected
//! service under it.

use crate::app::State;
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke, UiBuilder};
use shellman_core::services::logic::Column;
use shellman_core::services::{Service, State as Run};
use shellman_ui::glass::islands::shows_as;
use shellman_ui::theme::{self, colors, glyph, icon_font, semibold_font};

const HEADER_H: f32 = 40.0;
const ROW_H: f32 = 30.0;
/// Room left and right of the rows inside the island.
const PAD: f32 = 12.0;

/// The columns after the display name, with their widths, laid out from the right.
const COLUMNS: [(Column, &str, f32); 4] = [
    (Column::Name, "Service name", 190.0),
    (Column::Status, "Status", 104.0),
    (Column::Startup, "Startup type", 150.0),
    (Column::Pid, "PID", 70.0),
];

/// Where `column`'s cell is in a row spanning `rect`'s width.
fn cell(rect: Rect, column: Column) -> Rect {
    let mut right = rect.right();
    for (c, _, width) in COLUMNS.iter().rev() {
        let left = right - width;
        if *c == column {
            return Rect::from_x_y_ranges(left..=right, rect.y_range());
        }
        right = left;
    }
    display_cell(rect)
}

/// The display name's cell: everything left of the other columns.
fn display_cell(rect: Rect) -> Rect {
    let others: f32 = COLUMNS.iter().map(|c| c.2).sum();
    Rect::from_x_y_ranges(rect.left()..=(rect.right() - others).max(rect.left() + 120.0), rect.y_range())
}

pub fn show(ui: &mut egui::Ui, rect: Rect, st: &mut State) {
    let header = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER_H));
    headers(ui, header.shrink2(vec2(PAD, 0.0)), st);
    let c = colors();
    ui.painter().line_segment(
        [pos2(rect.left() + 18.0, header.bottom()), pos2(rect.right() - 18.0, header.bottom())],
        Stroke::new(1.0_f32, c.hairline),
    );
    let body = Rect::from_min_max(pos2(rect.left() + PAD - 4.0, header.bottom() + 2.0), pos2(rect.right() - 4.0, rect.bottom() - 10.0));
    st.services.take_focus = std::mem::take(&mut st.services.row_focus);
    keys(ui, st);
    if st.services.rows.is_empty() {
        let text = if !st.services.loaded {
            "Reading services..."
        } else if st.services.all.is_empty() {
            "Windows did not list any services."
        } else {
            "No service matches the search."
        };
        ui.painter().text(body.center_top() + vec2(0.0, 40.0), Align2::CENTER_CENTER, text, FontId::proportional(13.0), c.muted);
        return;
    }
    let mut clicked = None;
    ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
        ui.spacing_mut().scroll = egui::style::ScrollStyle::floating();
        egui::ScrollArea::vertical().id_salt("service-list").auto_shrink([false, false]).drag_to_scroll(false).show_viewport(
            ui,
            |ui, view| {
                let rows = st.services.rows.len();
                let width = ui.available_width() - 8.0;
                let (all, area) = ui.allocate_exact_size(vec2(width, rows as f32 * ROW_H), Sense::hover());
                area.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("Services, {rows} rows")));
                if std::mem::take(&mut st.services.reveal) {
                    let at = st.services.selected.as_deref().and_then(|name| {
                        st.services.rows.iter().position(|&i| st.services.all[i].name == name)
                    });
                    if let Some(r) = at {
                        ui.scroll_to_rect(Rect::from_min_size(pos2(all.left(), all.top() + r as f32 * ROW_H), vec2(width, ROW_H)), None);
                    }
                }
                let first = (view.top() / ROW_H).floor().max(0.0) as usize;
                let last = ((view.bottom() / ROW_H).ceil() as usize).min(rows);
                for r in first..last {
                    let service = &st.services.all[st.services.rows[r]];
                    let line = Rect::from_min_size(pos2(all.left(), all.top() + r as f32 * ROW_H), vec2(width, ROW_H));
                    let selected = st.services.selected.as_deref() == Some(service.name.as_str());
                    let tab_stop = selected || (st.services.selected.is_none() && r == 0);
                    let take = st.services.take_focus;
                    let (click, focused) = row(ui, line, service, selected, tab_stop, take);
                    if click {
                        clicked = Some(service.name.clone());
                    }
                    st.services.row_focus |= focused;
                }
            },
        );
    });
    if clicked.is_some() {
        st.services.selected = clicked;
    }
}

/// One service's row: whether it was clicked (or tabbed into), and whether it has the keyboard.
/// Only `tab_stop` (the selected row, else the first) is a Tab stop; the arrows move it.
fn row(ui: &mut egui::Ui, rect: Rect, s: &Service, selected: bool, tab_stop: bool, take_focus: bool) -> (bool, bool) {
    let c = colors();
    let id = egui::Id::new(("service", &s.name));
    let resp = ui.interact(rect, id, if tab_stop { Sense::click() } else { Sense::CLICK });
    let name = format!("{}, {}, {}, {}{}", s.display, s.name, s.state.label(), s.startup_label(), if s.pid == 0 { String::new() } else { format!(", PID {}", s.pid) });
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &name));
    let focused = tab_stop && resp.has_focus();
    if focused {
        ui.memory_mut(|m| m.set_focus_lock_filter(id, egui::EventFilter { horizontal_arrows: true, vertical_arrows: true, ..Default::default() }));
    } else if tab_stop && selected && take_focus {
        resp.request_focus();
    }
    let row_rect = rect.shrink2(vec2(0.0, 1.0));
    if c.high_contrast {
        if selected {
            ui.painter().rect_stroke(row_rect, 10.0, Stroke::new(2.0_f32, c.accent_fill), egui::StrokeKind::Inside);
        }
    } else {
        let fill = if selected {
            shows_as(theme::with_alpha(c.accent_fill, if c.light { 40 } else { 52 }))
        } else if resp.hovered() {
            shows_as(c.row_hover)
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(row_rect, 10.0, fill);
    }
    theme::focus_ring(ui, &resp, row_rect, 10.0);
    let tabbed_in = resp.gained_focus() && !selected;

    let font = FontId::proportional(13.0);
    let small = FontId::proportional(12.5);
    let y = rect.center().y;
    let text_at = |cell: Rect, text: &str, font: &FontId, color: Color32| {
        let clip = Rect::from_x_y_ranges((cell.left() + 8.0)..=(cell.right() - 8.0), rect.y_range());
        ui.painter().with_clip_rect(clip).text(pos2(cell.left() + 8.0, y), Align2::LEFT_CENTER, text, font.clone(), color);
    };
    text_at(display_cell(rect), &s.display, &font, c.text);
    text_at(cell(rect, Column::Name), &s.name, &small, c.muted);

    let status = cell(rect, Column::Status);
    let (dot, tone) = match s.state {
        Run::Running => (c.accent_text, c.text),
        Run::Stopped => (theme::with_alpha(c.muted, 120), c.muted),
        Run::Starting | Run::Stopping | Run::Paused => (c.warn, c.text),
    };
    ui.painter().circle_filled(pos2(status.left() + 13.0, y), 3.0, dot);
    ui.painter().text(pos2(status.left() + 24.0, y), Align2::LEFT_CENTER, s.state.label(), small.clone(), tone);

    let startup = cell(rect, Column::Startup);
    let tone = if s.startup.is_some() { c.text } else { c.muted };
    text_at(startup, s.startup_label(), &small, tone);
    let pid = cell(rect, Column::Pid);
    ui.painter().text(pos2(pid.right() - 8.0, y), Align2::RIGHT_CENTER, s.pid_label(), small, theme::with_alpha(c.muted, 220));

    let tip = match (&s.description, s.startup) {
        (Some(d), _) => Some(d.clone()),
        (None, None) => Some("Windows does not let this window read this service's settings.".to_owned()),
        (None, Some(_)) => None,
    };
    let resp = match tip {
        Some(t) => resp.on_hover_text_at_pointer(t),
        None => resp,
    };
    (resp.clicked() || tabbed_in, focused)
}

/// The column titles; the one sorted on carries an arrow.
fn headers(ui: &mut egui::Ui, rect: Rect, st: &mut State) {
    let c = colors();
    let sort = st.services.sort;
    let mut cells = vec![(Column::Display, "Name", display_cell(rect))];
    cells.extend(COLUMNS.iter().map(|(col, title, _)| (*col, *title, cell(rect, *col))));
    for (column, title, cell) in cells {
        let resp = ui.interact(cell, egui::Id::new(("service-header", title)), Sense::click());
        let current = sort.column == column;
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, crate::processes::header_name(title, current, sort.descending)));
        theme::focus_ring(ui, &resp, cell.shrink(2.0), 8.0);
        let color = if current || resp.hovered() { c.text } else { c.muted };
        let right = column == Column::Pid;
        let (anchor, x) = if right { (Align2::RIGHT_CENTER, cell.right() - 8.0) } else { (Align2::LEFT_CENTER, cell.left() + 8.0) };
        let text = ui.painter().text(pos2(x, cell.center().y), anchor, title, FontId::proportional(12.5), color);
        if current {
            let arrow = if sort.descending { glyph::SORT_DOWN } else { glyph::SORT_UP };
            let ax = if right { text.left() - 9.0 } else { text.right() + 9.0 };
            ui.painter().text(pos2(ax, cell.center().y), Align2::CENTER_CENTER, arrow, icon_font(9.0), theme::with_alpha(c.text, 200));
        }
        if resp.clicked() {
            st.services.sort = sort.clicked(column);
        }
    }
}

/// Up and Down move the selection. Not while the search field has the keyboard.
fn keys(ui: &mut egui::Ui, st: &mut State) {
    let elsewhere = ui.memory(|m| m.focused().is_some()) && !st.services.take_focus;
    if ui.ctx().wants_keyboard_input() || elsewhere {
        return;
    }
    let (up, down) = ui.input(|i| (i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown)));
    if !(up || down) || st.services.rows.is_empty() {
        return;
    }
    let page = &mut st.services;
    let at = page.selected.as_deref().and_then(|name| page.rows.iter().position(|&i| page.all[i].name == name));
    let next = match (at, down) {
        (None, _) => 0,
        (Some(i), true) => (i + 1).min(page.rows.len() - 1),
        (Some(i), false) => i.saturating_sub(1),
    };
    page.selected = Some(page.all[page.rows[next]].name.clone());
    page.reveal = true;
}

/// The selected service: its names, how it starts, and what it says it is for.
pub fn card(ui: &mut egui::Ui, rect: Rect, st: &mut State) {
    let Some(s) = st.services.current() else {
        return;
    };
    let c = colors();
    let inner = rect.shrink2(vec2(22.0, 14.0));
    let clip = ui.painter().with_clip_rect(rect.shrink(2.0));
    let title = clip.text(inner.left_top(), Align2::LEFT_TOP, &s.display, semibold_font(14.0), c.text);
    let pid = if s.pid == 0 { String::new() } else { format!("  ·  PID {}", s.pid) };
    let meta = format!("{}  ·  {}  ·  {}{pid}", s.name, s.state.label(), s.startup_label());
    clip.text(pos2(inner.left(), title.bottom() + 3.0), Align2::LEFT_TOP, meta, FontId::proportional(12.0), c.muted);
    let top = title.bottom() + 26.0;
    let (text, color) = match &s.description {
        Some(d) => (d.as_str(), c.text),
        None => ("No description.", c.muted),
    };
    let galley = ui.painter().layout(text.to_owned(), FontId::proportional(12.5), color, inner.width());
    let body = Rect::from_min_max(pos2(inner.left(), top), pos2(inner.right(), rect.bottom() - 10.0));
    ui.painter().with_clip_rect(body).galley(pos2(inner.left(), top), galley, color);
}
