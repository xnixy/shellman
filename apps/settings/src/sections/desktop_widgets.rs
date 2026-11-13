//! Desktop › the widgets: which are shown and on which displays, the calendar account the Today
//! widget reads (a CalDAV server, Fastmail's unless changed; the app password goes straight
//! into Windows Credential Manager and is never shown or written to a file), and the place and
//! units of its weather. Everything is saved to `desktop.json`, which the shelf follows live.
//! "Test connection" and "Find" run on worker threads.

use super::desktop::row;
use crate::page;
use crate::state::State;
use egui::{pos2, vec2, Align, CornerRadius, FontId, Rect, Sense, Ui};
use shellman_core::calendar::{self, CalError, CalendarRef};
use shellman_core::credentials;
use shellman_core::settings::desktop::ShelfOn;
use shellman_core::weather::{self, Place, Units};
use shellman_ui::theme::{self, colors};
use shellman_ui::widgets::switch;
use std::sync::mpsc::{channel, Receiver};

/// The page's own state: what is being typed, and the jobs running.
#[derive(Default)]
pub struct WidgetsView {
    started: bool,
    server: String,
    username: String,
    /// Typed, not yet saved; cleared once saved. Never filled from the saved one.
    password: String,
    /// A password is saved in Credential Manager.
    has_password: bool,
    testing: Option<Receiver<Result<Vec<CalendarRef>, CalError>>>,
    /// The last test's outcome: true for success, and the words.
    test_line: Option<(bool, String)>,
    city: String,
    finding: Option<Receiver<Result<Vec<Place>, String>>>,
    found: Vec<Place>,
    find_line: Option<String>,
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

impl WidgetsView {
    fn start(&mut self, st: &State) {
        if self.started {
            return;
        }
        self.started = true;
        let cal = &st.desktop.widgets.calendar;
        self.server = cal.server.clone();
        self.username = cal.username.clone();
        self.has_password = credentials::read(calendar::CREDENTIAL).is_some_and(|s| !s.secret.is_empty());
        self.city = st.desktop.widgets.weather.place.clone();
    }
}

/// A text box filling the rest of a row. `secret` hides what is typed.
fn field(ui: &mut Ui, id: &str, text: &mut String, hint: &str, secret: bool) -> egui::Response {
    let c = colors();
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, 32.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(10), c.control);
    let inner = Rect::from_min_max(pos2(rect.left() + 12.0, rect.top()), pos2(rect.right() - 10.0, rect.bottom()));
    let edit = egui::TextEdit::singleline(text)
        .id(egui::Id::new(("desktop-widgets", id)))
        .font(FontId::proportional(13.0))
        .frame(false)
        .password(secret)
        .margin(egui::Margin::ZERO)
        .vertical_align(Align::Center)
        .hint_text(egui::RichText::new(hint).color(c.muted))
        .desired_width(inner.width());
    ui.put(inner, edit)
}

/// A pill button in the control colour.
fn button(ui: &mut Ui, label: &str, enabled: bool) -> bool {
    let c = colors();
    let font = FontId::proportional(13.0);
    let w = ui.fonts(|f| f.layout_no_wrap(label.to_owned(), font.clone(), c.text).size().x) + 28.0;
    let (rect, _) = ui.allocate_exact_size(vec2(w, 32.0), Sense::hover());
    let resp = ui.interact(rect, ui.id().with(("btn", label)), if enabled { Sense::click() } else { Sense::hover() });
    let fill = if enabled && resp.hovered() { c.control_hover } else { c.control };
    ui.painter().rect_filled(rect, CornerRadius::same(10), fill);
    let text = if enabled { c.text } else { theme::with_alpha(c.muted, 150) };
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, font, text);
    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    enabled && resp.clicked()
}

/// A muted line under a group's rows.
fn note(ui: &mut Ui, text: &str, good: Option<bool>) {
    let c = colors();
    let color = match good {
        Some(true) => c.text,
        Some(false) => c.danger_text,
        None => c.muted,
    };
    ui.add_space(4.0);
    ui.label(egui::RichText::new(text).size(12.0).color(color));
}

/// A row with a name, a line under it (or a colour dot before the name), and a switch.
fn switch_row(ui: &mut Ui, name: &str, about: &str, on: &mut bool) -> bool {
    switch_row_dot(ui, name, about, None, on)
}

fn switch_row_dot(ui: &mut Ui, name: &str, about: &str, dot: Option<egui::Color32>, on: &mut bool) -> bool {
    let c = colors();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.top(), egui::Stroke::new(1.0_f32, c.hairline));
    let text = if ui.is_enabled() { c.text } else { theme::with_alpha(c.muted, 150) };
    let (mut x, name_y) = (rect.left(), if about.is_empty() { rect.center().y } else { rect.center().y - 9.0 });
    if let Some(d) = dot {
        ui.painter().circle_filled(pos2(x + 5.0, name_y), 5.0, d);
        x += 18.0;
    }
    ui.painter().text(pos2(x, name_y), egui::Align2::LEFT_CENTER, name, FontId::proportional(14.0), text);
    if !about.is_empty() {
        ui.painter().text(pos2(rect.left(), rect.center().y + 10.0), egui::Align2::LEFT_CENTER, about, FontId::proportional(12.0), c.muted);
    }
    let right = Rect::from_min_max(pos2(rect.right() - 60.0, rect.top()), rect.max);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(right).layout(egui::Layout::right_to_left(Align::Center)));
    switch(&mut child, on, "").changed()
}

pub fn show(ui: &mut Ui, st: &mut State, clean: bool) {
    let mut view = std::mem::take(&mut st.widgets_view);
    view.start(st);
    let mut changed = false;
    page::group(ui, "Widgets", |ui| {
        if clean {
            ui.label(theme::muted("Clean shows no widgets. Pick Shelf + widgets to use them.").size(12.0));
            ui.add_space(6.0);
        }
        ui.add_enabled_ui(!clean, |ui| {
            let w = &mut st.desktop.widgets;
            changed |= row(ui, "Show widgets", true, |ui| switch(ui, &mut w.show, "").changed());
            changed |= row(ui, "Show widgets on", false, |ui| {
                let options = ShelfOn::ALL.map(|o| (o, o.label()));
                // Its own id: the shelf's row above has the same choices.
                ui.push_id("widgets-on", |ui| page::segmented(ui, &mut w.on, &options)).inner
            });
            ui.add_enabled_ui(w.show, |ui| {
                changed |= switch_row(ui, "Today", "Date, weather and next events", &mut w.today);
                changed |= switch_row(ui, "Now playing", "Media from any app", &mut w.now_playing);
            });
        });
    });
    let off = clean || !st.desktop.widgets.show || !st.desktop.widgets.today;
    page::group(ui, "Calendar", |ui| {
        ui.add_enabled_ui(!off, |ui| changed |= calendar_rows(ui, st, &mut view));
    });
    page::group(ui, "Weather", |ui| {
        ui.add_enabled_ui(!off, |ui| changed |= weather_rows(ui, st, &mut view));
    });
    if changed {
        st.save_desktop();
    }
    if view.testing.is_some() || view.finding.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
    }
    st.widgets_view = view;
}

fn calendar_rows(ui: &mut Ui, st: &mut State, v: &mut WidgetsView) -> bool {
    let mut changed = false;
    let cal = &mut st.desktop.widgets.calendar;
    changed |= row(ui, "Server", true, |ui| {
        let r = field(ui, "server", &mut v.server, calendar::client::FASTMAIL, false);
        if r.lost_focus() && v.server.trim() != cal.server {
            cal.server = if v.server.trim().is_empty() { calendar::client::FASTMAIL.to_owned() } else { v.server.trim().to_owned() };
            v.server = cal.server.clone();
            return true;
        }
        false
    });
    changed |= row(ui, "User name", false, |ui| {
        let r = field(ui, "user", &mut v.username, "you@fastmail.com", false);
        if r.lost_focus() && v.username.trim() != cal.username {
            cal.username = v.username.trim().to_owned();
            return true;
        }
        false
    });
    let hint = if v.has_password { "Saved (type a new one to replace it)" } else { "App password" };
    row(ui, "App password", false, |ui| {
        field(ui, "password", &mut v.password, hint, true);
        false
    });
    ui.add_space(8.0);
    let mut test = false;
    let mut forget = false;
    ui.horizontal(|ui| {
        let ready = v.testing.is_none() && !v.username.trim().is_empty() && (v.has_password || !v.password.is_empty());
        test = button(ui, if v.testing.is_some() { "Testing…" } else { "Test connection" }, ready);
        if v.has_password && button(ui, "Forget password", v.testing.is_none()) {
            forget = true;
        }
    });
    if forget {
        credentials::delete(calendar::CREDENTIAL);
        calendar::clear_cache();
        v.has_password = false;
        v.test_line = Some((false, "Password removed from Credential Manager".into()));
        cal.saved = now();
        changed = true;
    }
    if test {
        // The fields as typed count, saved first; the password goes to Credential Manager only.
        cal.server = if v.server.trim().is_empty() { calendar::client::FASTMAIL.to_owned() } else { v.server.trim().to_owned() };
        cal.username = v.username.trim().to_owned();
        changed = true;
        let mut ok = true;
        if !v.password.is_empty() {
            match credentials::write(calendar::CREDENTIAL, &cal.username, &v.password) {
                Ok(()) => {
                    v.has_password = true;
                    cal.saved = now();
                }
                Err(e) => {
                    v.test_line = Some((false, e));
                    ok = false;
                }
            }
            v.password.clear();
        }
        if ok {
            if let Some(account) = calendar::account(&cal.server, &cal.username) {
                let (tx, rx) = channel();
                std::thread::spawn(move || {
                    let _ = tx.send(account.discover());
                });
                v.testing = Some(rx);
                v.test_line = None;
            }
        }
    }
    if let Some(rx) = &v.testing {
        if let Ok(result) = rx.try_recv() {
            v.testing = None;
            match result {
                Ok(list) => {
                    v.test_line = Some((true, format!("Connected · {} calendar{}", list.len(), if list.len() == 1 { "" } else { "s" })));
                    cal.calendars = list;
                    changed = true;
                }
                Err(e) => v.test_line = Some((false, e.line())),
            }
        }
    }
    match &v.test_line {
        Some((good, line)) => note(ui, line, Some(*good)),
        None if v.testing.is_none() && cal.calendars.is_empty() => note(
            ui,
            "For Fastmail, make an app password in Fastmail › Settings › Privacy & Security › App passwords (access: Calendars, CalDAV), and use your Fastmail address as the user name.",
            None,
        ),
        None => {}
    }
    if !cal.calendars.is_empty() {
        ui.add_space(8.0);
        page::label(ui, "Calendars to show");
        let list = cal.calendars.clone();
        for c in &list {
            let mut on = cal.shows(&c.href);
            if switch_row_dot(ui, &c.name, "", c.color.as_deref().and_then(hex), &mut on) {
                cal.hidden.retain(|h| h != &c.href);
                if !on {
                    cal.hidden.push(c.href.clone());
                }
                changed = true;
            }
        }
    }
    changed
}

/// `#3A87AD` as a colour.
fn hex(s: &str) -> Option<egui::Color32> {
    let n = u32::from_str_radix(s.trim().strip_prefix('#')?.get(..6)?, 16).ok()?;
    Some(egui::Color32::from_rgb((n >> 16) as u8, (n >> 8) as u8, n as u8))
}

fn weather_rows(ui: &mut Ui, st: &mut State, v: &mut WidgetsView) -> bool {
    let mut changed = false;
    let w = &mut st.desktop.widgets.weather;
    let mut find = false;
    row(ui, "City", true, |ui| {
        let ready = v.finding.is_none() && !v.city.trim().is_empty();
        find |= button(ui, if v.finding.is_some() { "Finding…" } else { "Find" }, ready);
        ui.add_space(8.0);
        let r = field(ui, "city", &mut v.city, "London", false);
        find |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && ready;
        false
    });
    if find {
        let name = v.city.trim().to_owned();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(weather::geocode(&name));
        });
        v.finding = Some(rx);
        v.find_line = None;
        v.found.clear();
    }
    if let Some(rx) = &v.finding {
        if let Ok(result) = rx.try_recv() {
            v.finding = None;
            match result {
                Ok(list) if list.is_empty() => v.find_line = Some("No place of that name found".into()),
                Ok(list) => v.found = list,
                Err(e) => v.find_line = Some(format!("Can't reach the weather service: {e}")),
            }
        }
    }
    let mut picked = None;
    for (i, p) in v.found.iter().enumerate() {
        let label = if p.area.is_empty() { p.name.clone() } else { format!("{} · {}", p.name, p.area) };
        if ui.selectable_label(false, label).clicked() {
            picked = Some(i);
        }
    }
    if let Some(i) = picked {
        let p = v.found[i].clone();
        w.place = p.name.clone();
        w.lat = p.lat;
        w.lon = p.lon;
        v.city = p.name;
        v.found.clear();
        changed = true;
    }
    changed |= row(ui, "Units", false, |ui| {
        let options = Units::ALL.map(|u| (u, u.label()));
        ui.push_id("weather-units", |ui| page::segmented(ui, &mut w.units, &options)).inner
    });
    match (&v.find_line, w.is_set()) {
        (Some(line), _) => note(ui, line, Some(false)),
        (None, true) => note(ui, &format!("Showing the weather for {} ({:.2}, {:.2}), from Open-Meteo.", w.place, w.lat, w.lon), None),
        (None, false) => note(ui, "Type a city and press Find. Only the place's coordinates are sent to Open-Meteo.", None),
    }
    changed
}

