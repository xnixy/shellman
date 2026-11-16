//! The calendar panel's island: the month (its first day of the
//! week, week numbers and names from the Windows region), today on the accent, a dot under days
//! with events, and below it the chosen day's events (time, calendar colour, title, place).
//! Previous / next month and "Today" in its header. Its height never changes (six weeks, three
//! event rows and a "more" line), so its glass never resizes while the user steps through months.
//!
//! Events come from the bar's calendar worker (`live`): the regular reading plus months read on
//! demand (`core::calendar::span`). With no calendar connected the grid still works and a quiet
//! line offers to connect one (Settings › Desktop, where the account lives).

use crate::notify::layout::{CARD_W, PAD_X};
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Ui};
use shellman_core::calendar::month::{self, Grid, WeekRule, WEEKS};
use shellman_core::calendar::time::{civil_from_days, days_from_civil};
use shellman_core::calendar::{color_of, select::clock, CalendarRef, Instance};
use shellman_core::system::region;
use shellman_ui::chips::{chip, chip_width, round_icon, soft_colors, Chip};
use shellman_ui::theme::{colors, glyph, mix, semibold_font, with_alpha};
use shellman_ui::widgets::draw_text;
use std::collections::BTreeSet;

const PAD_Y: f32 = 14.0;
const HEADER_H: f32 = 32.0;
const DOW_GAP: f32 = 6.0;
const DOW_H: f32 = 22.0;
const CELL_H: f32 = 34.0;
const SEP: f32 = 12.0;
const DAY_HEAD: f32 = 24.0;
const EVENT_H: f32 = 40.0;
pub const EVENT_ROWS: usize = 3;
const MORE_H: f32 = 18.0;
const WEEK_COL: f32 = 28.0;
const TIME_COL: f32 = 66.0;

/// The island's height: always the same.
pub const HEIGHT: f32 =
    PAD_Y + HEADER_H + DOW_GAP + DOW_H + WEEKS as f32 * CELL_H + SEP + DAY_HEAD + EVENT_ROWS as f32 * EVENT_H + MORE_H + PAD_Y;

/// Where the calendar stands, as a line under the grid when there are no events to list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    NotConnected,
    Connecting,
    SignInFailed,
    /// Can't reach it and nothing cached.
    Unreachable,
    Ready,
}

/// What the island shows, gathered by the bar from `live`.
pub struct Data<'a> {
    pub status: Status,
    /// The events touching the shown grid.
    pub instances: Vec<&'a Instance>,
    pub calendars: &'a [CalendarRef],
    pub offset: &'a dyn Fn(i64) -> i64,
    pub h24: bool,
    /// 0 Monday to 6 Sunday.
    pub first_day: u32,
    pub rule: WeekRule,
    pub weeks: bool,
    /// The viewer's today.
    pub today: i64,
}

/// The month shown and the day chosen; kept while the list is closed, reset to today on open.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub year: i64,
    pub month: u32,
    pub selected: i64,
    /// Names in the region's language, made once per month or first day.
    labels: Option<Labels>,
}

#[derive(Clone, Debug, PartialEq)]
struct Labels {
    key: (i64, u32, u32, i64),
    month: String,
    weekdays: Vec<String>,
    day: String,
}

impl View {
    pub fn on(day: i64) -> Self {
        let (year, month, _) = civil_from_days(day);
        Self { year, month, selected: day, labels: None }
    }

    pub fn grid(&self, first_day: u32, rule: WeekRule) -> Grid {
        Grid::new(self.year, self.month, first_day, rule)
    }

    fn labels(&mut self, first_day: u32) -> &Labels {
        let key = (self.year, self.month, first_day, self.selected);
        if self.labels.as_ref().is_none_or(|l| l.key != key) {
            let first = days_from_civil(self.year, self.month, 1);
            let week = month::week_start(first, first_day);
            // Two letters a day ("Mo"), as Windows' own calendar shows them.
            let weekdays = (0..7).map(|i| region::format_day(week + i, "ddd").chars().take(2).collect()).collect();
            self.labels = Some(Labels {
                key,
                month: region::format_day(first, "MMMM yyyy"),
                weekdays,
                day: region::format_day(self.selected, "dddd d MMMM"),
            });
        }
        self.labels.as_ref().unwrap()
    }
}

/// What a click on the island asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    Step(i32),
    Today,
    Select(i64),
    /// Open Settings › Desktop to connect a calendar (or fix its password).
    Connect,
}

/// Painted text a screen reader should read: a label node over `rect`.
fn label_for(ui: &Ui, rect: Rect, id: &str, text: &str) {
    let resp = ui.interact(rect, egui::Id::new(id), Sense::hover());
    shellman_ui::theme::speak(&resp, egui::WidgetType::Label, || text.to_owned());
}

/// What a screen reader reads for a day of the grid: its full date (in the region's words),
/// then "today", "has events" and "next or last month" where they apply.
pub fn spoken_day(date: &str, today: bool, has_events: bool, in_month: bool) -> String {
    let mut s = date.to_owned();
    if today {
        s.push_str(", today");
    }
    if has_events {
        s.push_str(", has events");
    }
    if !in_month {
        s.push_str(", another month");
    }
    s
}

fn rgb(c: Option<[u8; 3]>, fallback: Color32) -> Color32 {
    c.map_or(fallback, |[r, g, b]| Color32::from_rgb(r, g, b))
}

/// Draw the island in `rect` (its height is `HEIGHT`). Returns what was asked.
pub fn draw(ui: &mut Ui, rect: Rect, view: &mut View, data: &Data) -> Option<Asked> {
    let c = colors();
    let mut asked = None;
    let inner = rect.shrink2(vec2(PAD_X, PAD_Y));
    let grid = view.grid(data.first_day, data.rule);
    let labels = view.labels(data.first_day).clone();

    // Header: the month, then Today, previous and next.
    let head = Rect::from_min_size(inner.min, vec2(inner.width(), HEADER_H));
    let next = Rect::from_center_size(pos2(head.right() - 14.0, head.center().y), vec2(28.0, 28.0));
    let prev = next.translate(vec2(-32.0, 0.0));
    if round_icon(ui, next, egui::Id::new("cal next"), glyph::FORWARD, true, false, "Next month").on_hover_text("Next month").clicked() {
        asked = Some(Asked::Step(1));
    }
    if round_icon(ui, prev, egui::Id::new("cal prev"), glyph::BACK, true, false, "Previous month").on_hover_text("Previous month").clicked() {
        asked = Some(Asked::Step(-1));
    }
    let mut title_end = prev.left() - 8.0;
    let (ty, tm, _) = civil_from_days(data.today);
    if (view.year, view.month) != (ty, tm) || view.selected != data.today {
        let w = chip_width(ui, "Today", false, Chip::Soft);
        let r = Rect::from_center_size(pos2(prev.left() - 8.0 - w / 2.0, head.center().y), vec2(w, 26.0));
        if chip(ui, r, egui::Id::new("cal today"), "Today", None, Chip::Soft).clicked() {
            asked = Some(Asked::Today);
        }
        title_end = r.left() - 8.0;
    }
    let title = Rect::from_min_max(pos2(head.left() + 4.0, head.top()), pos2(title_end, head.bottom()));
    draw_text(ui, title, &labels.month, semibold_font(15.0), c.text, false);
    label_for(ui, title, "cal month", &labels.month);

    // Weekday names, and the week-number column's heading.
    let week_col = if data.weeks { WEEK_COL } else { 0.0 };
    let cell_w = (inner.width() - week_col) / 7.0;
    let dow_top = head.bottom() + DOW_GAP;
    let small = FontId::proportional(12.0);
    // Grey text in high contrast: a mix is not one of Windows' colours.
    let dim = if c.high_contrast { c.muted } else { mix(c.muted, c.bg, 0.35) };
    for (i, name) in labels.weekdays.iter().enumerate() {
        let x = inner.left() + week_col + cell_w * (i as f32 + 0.5);
        ui.painter().text(pos2(x, dow_top + DOW_H / 2.0), Align2::CENTER_CENTER, name, small.clone(), c.muted);
    }
    // The days.
    let busy: BTreeSet<i64> = month::busy_days(&data.instances, grid.first_cell, grid.end(), data.offset);
    let cells_top = dow_top + DOW_H;
    for w in 0..WEEKS {
        let y = cells_top + w as f32 * CELL_H;
        if data.weeks {
            ui.painter().text(pos2(inner.left() + WEEK_COL / 2.0 - 2.0, y + CELL_H / 2.0), Align2::CENTER_CENTER, grid.weeks[w].to_string(), FontId::proportional(11.0), dim);
        }
        for col in 0..7 {
            let day = grid.day(w, col);
            let cell = Rect::from_min_size(pos2(inner.left() + week_col + col as f32 * cell_w, y), vec2(cell_w, CELL_H));
            let round = Rect::from_center_size(cell.center() - vec2(0.0, 2.0), vec2(30.0, 30.0));
            let resp = ui.interact(cell, egui::Id::new(("cal day", day)), Sense::click());
            let today = day == data.today;
            let chosen = day == view.selected;
            let has_events = busy.contains(&day);
            let in_month = grid.in_month(day);
            shellman_ui::theme::speak_selected(&resp, egui::WidgetType::SelectableLabel, chosen, || {
                spoken_day(&region::format_day(day, "dddd d MMMM yyyy"), today, has_events, in_month)
            });
            shellman_ui::theme::focus_ring(ui, &resp, round, 15.0);
            let (soft, soft_hover, line) = soft_colors(c.light);
            let hc = c.high_contrast;
            if today {
                let fill = if resp.hovered() && !hc { mix(c.accent_fill, Color32::WHITE, 0.12) } else { c.accent_fill };
                ui.painter().circle_filled(round.center(), 15.0, fill);
                if hc && chosen {
                    // Highlight under highlight text already; the chosen ring sits outside it.
                    ui.painter().circle_stroke(round.center(), 16.5, egui::Stroke::new(2.0_f32, c.text));
                }
            } else if chosen && hc {
                ui.painter().circle_stroke(round.center(), 14.0, egui::Stroke::new(2.0_f32, c.row_selected));
            } else if chosen {
                ui.painter().circle(round.center(), 15.0, soft_hover, line);
            } else if resp.hovered() && hc {
                ui.painter().circle_stroke(round.center(), 14.5, egui::Stroke::new(1.0_f32, c.text));
            } else if resp.hovered() {
                ui.painter().circle_filled(round.center(), 15.0, soft);
            }
            let (_, _, d) = civil_from_days(day);
            let color = if today {
                c.on_accent
            } else if grid.in_month(day) {
                c.text
            } else {
                dim
            };
            let font = if today || chosen { semibold_font(13.5) } else { FontId::proportional(13.5) };
            ui.painter().text(round.center(), Align2::CENTER_CENTER, d.to_string(), font, color);
            if has_events {
                let dot = if today { with_alpha(c.on_accent, 230) } else { with_alpha(c.accent_text, if grid.in_month(day) { 255 } else { 120 }) };
                ui.painter().circle_filled(pos2(round.center().x, round.bottom() - 4.5), 2.0, dot);
            }
            if resp.clicked() {
                asked = Some(Asked::Select(day));
            }
        }
    }

    // The chosen day's events.
    let mut y = cells_top + WEEKS as f32 * CELL_H + SEP;
    ui.painter().hline(inner.x_range(), y - SEP / 2.0, egui::Stroke::new(1.0_f32, c.separator));
    let heading = Rect::from_min_size(pos2(inner.left() + 4.0, y), vec2(inner.width() - 4.0, DAY_HEAD));
    draw_text(ui, heading, &labels.day, semibold_font(13.0), c.text, false);
    label_for(ui, heading, "cal day heading", &labels.day);
    y += DAY_HEAD;
    let list_area = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), EVENT_ROWS as f32 * EVENT_H + MORE_H));
    let quiet = |ui: &mut Ui, text: &str, link: bool| {
        let r = Rect::from_min_size(pos2(list_area.left() + 4.0, list_area.top() + 6.0), vec2(list_area.width() - 4.0, 22.0));
        if link {
            let w = chip_width(ui, text, false, Chip::Soft);
            let r = Rect::from_min_size(r.min, vec2(w, 28.0));
            chip(ui, r, egui::Id::new("cal connect"), text, None, Chip::Soft).clicked()
        } else {
            draw_text(ui, r, text, FontId::proportional(13.0), c.muted, false);
            label_for(ui, r, "cal quiet", text);
            false
        }
    };
    match data.status {
        Status::NotConnected => {
            if quiet(ui, "Connect a calendar…", true) {
                asked = Some(Asked::Connect);
            }
        }
        Status::SignInFailed => {
            if quiet(ui, "Calendar sign-in failed · fix it in Settings", true) {
                asked = Some(Asked::Connect);
            }
        }
        Status::Connecting => {
            quiet(ui, "Connecting to your calendar…", false);
        }
        Status::Unreachable => {
            quiet(ui, "Can't reach the calendar", false);
        }
        Status::Ready => {
            let events = month::on_day(&data.instances, view.selected, data.offset);
            if events.is_empty() {
                quiet(ui, "No events", false);
            }
            for (n, e) in events.iter().take(EVENT_ROWS).enumerate() {
                let row = Rect::from_min_size(pos2(list_area.left(), y + n as f32 * EVENT_H), vec2(list_area.width(), EVENT_H));
                event_row(ui, row, n, e, data, view.selected);
            }
            let more = events.len().saturating_sub(EVENT_ROWS);
            if more > 0 {
                let r = Rect::from_min_size(pos2(list_area.left() + TIME_COL, y + EVENT_ROWS as f32 * EVENT_H), vec2(list_area.width() - TIME_COL, MORE_H));
                draw_text(ui, r, &format!("{more} more"), FontId::proportional(12.0), dim, false);
                label_for(ui, r, "cal more", &format!("{more} more"));
            }
        }
    }
    asked
}

fn event_row(ui: &mut Ui, row: Rect, n: usize, e: &Instance, data: &Data, day: i64) {
    let c = colors();
    let (first_day, _) = month::days_of(e, data.offset);
    let time = match e.days {
        Some(_) => "All day".to_owned(),
        // An event that began on an earlier day shows as going on.
        None if first_day < day => "…".to_owned(),
        None => clock(e.start + (data.offset)(e.start), data.h24),
    };
    let mid = row.center().y;
    draw_text(ui, Rect::from_min_max(pos2(row.left() + 4.0, row.top()), pos2(row.left() + TIME_COL - 8.0, row.bottom())), &time, FontId::proportional(12.5), c.muted, false);
    let color = rgb(color_of(data.calendars, &e.calendar), c.accent);
    let bar = Rect::from_center_size(pos2(row.left() + TIME_COL - 2.0, mid), vec2(3.0, EVENT_H - 14.0));
    ui.painter().rect_filled(bar, 2, color);
    let text_left = row.left() + TIME_COL + 8.0;
    let detail = if e.location.is_empty() {
        data.calendars.iter().find(|cal| cal.href == e.calendar).map(|cal| cal.name.clone()).unwrap_or_default()
    } else {
        e.location.clone()
    };
    // Painted text is not seen by a screen reader: the row says itself as a label.
    let resp = ui.interact(row, egui::Id::new(("cal event", n)), Sense::hover());
    shellman_ui::theme::speak(&resp, egui::WidgetType::Label, || {
        [time.as_str(), e.title.as_str(), detail.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(", ")
    });
    let title_rect = Rect::from_min_max(pos2(text_left, row.top() + 3.0), pos2(row.right(), mid + 1.0));
    draw_text(ui, title_rect, &e.title, semibold_font(13.5), c.text, false);
    if !detail.is_empty() {
        let r = Rect::from_min_max(pos2(text_left, mid + 1.0), pos2(row.right(), row.bottom() - 3.0));
        draw_text(ui, r, &detail, FontId::proportional(12.0), c.muted, false);
    }
}

/// The island's rectangle at the top of a column `width` wide, from `top`.
pub fn rect(top: f32, width: f32) -> Rect {
    let left = ((width - CARD_W) / 2.0).round();
    Rect::from_min_size(pos2(left, top), vec2(CARD_W, HEIGHT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_island_keeps_one_height_and_fits_a_small_screen() {
        // Under 480 points: it leaves room for notifications on a 1080p screen at 100%.
        assert!(HEIGHT < 480.0, "{HEIGHT}");
        let r = rect(10.0, crate::notify::layout::COLUMN_W);
        assert_eq!(r.left(), crate::notify::layout::EDGE);
        assert_eq!(r.height(), HEIGHT);
    }

    #[test]
    fn a_day_reads_its_date_and_marks() {
        assert_eq!(spoken_day("Sunday 15 November 2026", true, true, true), "Sunday 15 November 2026, today, has events");
        assert_eq!(spoken_day("Monday 30 November 2026", false, false, false), "Monday 30 November 2026, another month");
    }

    #[test]
    fn a_view_opens_on_its_days_month() {
        let v = View::on(days_from_civil(2026, 11, 12));
        assert_eq!((v.year, v.month), (2026, 11));
        let g = v.grid(0, WeekRule::FirstFourDays);
        assert!(g.in_month(v.selected));
    }
}
