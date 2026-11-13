//! Date, time & region: the time zone and its daylight saving switch, the 12 or 24 hour clock,
//! the short and long date formats and the first day of the week. Windows is read on a worker
//! when the page is shown and every change is written by a worker; the page then shows what
//! Windows reports back. A refused write is said in one line. Setting the clock itself is not
//! offered.

use crate::page;
use crate::state::State;
use shellman_core::system::region::{self, Current, Intl, Refused};
use shellman_core::system::region_logic::{self as logic, Zone};
use shellman_ui::theme;
use shellman_ui::widgets::switch;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

/// All the page shows, read together.
struct Data {
    zones: Vec<Zone>,
    current: Option<Current>,
    intl: Intl,
    short_dates: Vec<String>,
    long_dates: Vec<String>,
}

fn read() -> Data {
    Data {
        zones: region::zones(),
        current: region::current(),
        intl: region::read_intl(),
        short_dates: region::date_formats(true),
        long_dates: region::date_formats(false),
    }
}

enum Msg {
    Data(Box<Data>),
    Refused(Refused),
}

type Write = Box<dyn FnOnce() -> Result<(), Refused> + Send>;

/// The page's live state; it reads again each time the page is shown.
#[derive(Default)]
pub struct DateTimeView {
    data: Option<Data>,
    chan: Option<(Sender<Msg>, Receiver<Msg>)>,
    notice: Option<String>,
    /// The zone list's search box.
    filter: String,
}

impl DateTimeView {
    pub fn stop(&mut self) {
        *self = Self::default();
    }

    fn start(&mut self, ctx: &egui::Context) {
        if self.chan.is_none() {
            self.chan = Some(channel());
            self.spawn(ctx, None);
        }
    }

    /// Run `write` (if any) and then read everything again, on a worker.
    fn spawn(&self, ctx: &egui::Context, write: Option<Write>) {
        let Some((tx, _)) = &self.chan else { return };
        let (tx, ctx) = (tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            if let Some(Err(e)) = write.map(|w| w()) {
                let _ = tx.send(Msg::Refused(e));
            }
            let _ = tx.send(Msg::Data(Box::new(read())));
            ctx.request_repaint();
        });
    }

    fn poll(&mut self) {
        let Some((_, rx)) = &self.chan else { return };
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Data(d) => self.data = Some(*d),
                Msg::Refused(e) => self.notice = Some(e.line()),
            }
        }
    }

    fn write(&mut self, ctx: &egui::Context, w: Write) {
        self.notice = None;
        self.spawn(ctx, Some(w));
    }
}

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "Date, time & region", "Your time zone, and how the time and date are written. Changes apply at once.");
    let v = &mut st.datetime;
    v.start(ui.ctx());
    v.poll();
    let Some(data) = v.data.take() else {
        page::group(ui, "", |ui| {
            ui.label(theme::muted("Reading the time settings..."));
        });
        return;
    };
    let ctx = ui.ctx().clone();
    page::group(ui, "Time zone", |ui| time_zone(ui, v, &ctx, &data));
    page::group(ui, "Format", |ui| formats(ui, v, &ctx, &data));
    v.data = Some(data);
    ui.ctx().request_repaint_after(Duration::from_secs(10));
}

fn note(ui: &mut egui::Ui, v: &DateTimeView) {
    if let Some(n) = &v.notice {
        ui.add_space(8.0);
        ui.label(theme::muted(n).size(12.0));
    }
}

fn time_zone(ui: &mut egui::Ui, v: &mut DateTimeView, ctx: &egui::Context, data: &Data) {
    let now = &data.intl;
    ui.label(format!("{}, {}", region::sample_time(&now.short_time), region::sample_date(&now.long_date)));
    ui.add_space(12.0);
    let current = data.current.as_ref();
    let shown = current
        .and_then(|c| data.zones.iter().find(|z| z.key == c.key))
        .map(|z| z.display.clone())
        .unwrap_or_else(|| "Unknown time zone".to_owned());
    let mut pick = None;
    egui::ComboBox::from_id_salt("tz").width(460.0).selected_text(shown).height(360.0).show_ui(ui, |ui| {
        ui.add(egui::TextEdit::singleline(&mut v.filter).hint_text("Filter").desired_width(f32::INFINITY));
        let q = v.filter.trim().to_lowercase();
        for z in data.zones.iter().filter(|z| q.is_empty() || z.display.to_lowercase().contains(&q)) {
            let on = current.is_some_and(|c| c.key == z.key);
            if ui.selectable_label(on, &z.display).clicked() && !on {
                pick = Some(z.key.clone());
            }
        }
    });
    if let Some(key) = pick {
        v.filter.clear();
        v.write(ctx, Box::new(move || region::set_zone(&key)));
    }
    if let Some(c) = current.filter(|c| c.observes_dst) {
        ui.add_space(12.0);
        let mut on = !c.dst_disabled;
        if switch(ui, &mut on, "Adjust for daylight saving time automatically").changed() {
            v.write(ctx, Box::new(move || region::set_dst_adjust(on)));
        }
    }
    note(ui, v);
}

fn formats(ui: &mut egui::Ui, v: &mut DateTimeView, ctx: &egui::Context, data: &Data) {
    let intl = &data.intl;
    let mut h24 = intl.is_24h();
    if switch(ui, &mut h24, "Use a 24-hour clock").changed() {
        v.write(ctx, Box::new(move || region::set_24h(h24)));
    }
    ui.add_space(4.0);
    ui.label(theme::muted(format!("Shown as {}", region::sample_time(&intl.long_time))).size(12.0));
    ui.add_space(14.0);
    if let Some(p) = pattern_picker(ui, "Short date", "short-date", &data.short_dates, &intl.short_date) {
        v.write(ctx, Box::new(move || region::set_short_date(&p)));
    }
    ui.add_space(12.0);
    if let Some(p) = pattern_picker(ui, "Long date", "long-date", &data.long_dates, &intl.long_date) {
        v.write(ctx, Box::new(move || region::set_long_date(&p)));
    }
    ui.add_space(12.0);
    page::label(ui, "First day of the week");
    let mut pick = None;
    egui::ComboBox::from_id_salt("first-day").width(220.0).selected_text(logic::day_name(intl.first_day)).show_ui(ui, |ui| {
        for (n, name) in logic::DAYS.iter().enumerate() {
            if ui.selectable_label(n as u32 == intl.first_day, *name).clicked() && n as u32 != intl.first_day {
                pick = Some(n as u32);
            }
        }
    });
    if let Some(day) = pick {
        v.write(ctx, Box::new(move || region::set_first_day(day)));
    }
    note(ui, v);
}

/// A date pattern picker; each entry is shown as today's date in that pattern. The current
/// pattern is listed even when the locale does not offer it. Returns the pattern picked.
fn pattern_picker(ui: &mut egui::Ui, title: &str, id: &str, offered: &[String], current: &str) -> Option<String> {
    page::label(ui, title);
    let mut list: Vec<&str> = offered.iter().map(String::as_str).collect();
    if !current.is_empty() && !list.contains(&current) {
        list.insert(0, current);
    }
    let text = |p: &str| format!("{}    ({p})", region::sample_date(p));
    let mut pick = None;
    egui::ComboBox::from_id_salt(id).width(460.0).selected_text(text(current)).show_ui(ui, |ui| {
        for p in list {
            if ui.selectable_label(p == current, text(p)).clicked() && p != current {
                pick = Some(p.to_owned());
            }
        }
    });
    pick
}
