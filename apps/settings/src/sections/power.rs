//! Power: the plan and power mode, the screen and sleep timeouts, what the lid and power button
//! do (each only where the machine has one), and the battery. Windows is read on a worker when
//! the page is shown, and each change is written by a worker; the page then shows what Windows
//! reports back. A write Windows refuses is said in one line under the group.

use shellman_ui::menu::dropdown::dropdown;
use crate::page;
use crate::state::State;
use shellman_core::system::power_logic::{self as logic, Key};
use shellman_core::system::power_settings::{self as pw, Refused, Snapshot, Source};
use shellman_ui::theme;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

enum Msg {
    Snap(Snapshot),
    Refused(Refused),
}

/// The page's live state; it reads again each time the page is shown.
#[derive(Default)]
pub struct PowerView {
    snap: Option<Snapshot>,
    chan: Option<(Sender<Msg>, Receiver<Msg>)>,
    notice: Option<String>,
}

impl PowerView {
    pub fn stop(&mut self) {
        *self = Self::default();
    }

    fn start(&mut self, ctx: &egui::Context) {
        if self.chan.is_none() {
            let (tx, rx) = channel();
            self.chan = Some((tx, rx));
            self.spawn(ctx, None::<Box<dyn FnOnce() -> Result<(), Refused> + Send>>);
        }
    }

    /// Run `write` (if any) and then read everything again, on a worker.
    fn spawn(&self, ctx: &egui::Context, write: Option<Box<dyn FnOnce() -> Result<(), Refused> + Send>>) {
        let Some((tx, _)) = &self.chan else { return };
        let (tx, ctx) = (tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            if let Some(Err(e)) = write.map(|w| w()) {
                let _ = tx.send(Msg::Refused(e));
            }
            let _ = tx.send(Msg::Snap(pw::read()));
            ctx.request_repaint();
        });
    }

    fn poll(&mut self) {
        let Some((_, rx)) = &self.chan else { return };
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Snap(s) => self.snap = Some(s),
                Msg::Refused(e) => self.notice = Some(e.line()),
            }
        }
    }
}

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "Power", "When the screen turns off, when the computer sleeps, and what the buttons do. Changes apply at once.");
    st.power.start(ui.ctx());
    st.power.poll();
    let Some(snap) = st.power.snap.clone() else {
        page::group(ui, "", |ui| {
            page::row_with(ui, |ui| {
                ui.label(theme::muted("Reading the power settings..."));
            });
        });
        return;
    };
    let v = &mut st.power;
    let ctx = ui.ctx().clone();
    let caps = snap.caps;
    // Only a machine with a battery has an "on battery" column and a battery to show.
    let status = pw::status().filter(|s| pw::has_battery(s, &caps));
    let battery = logic::has_battery_column(&caps) && status.is_some();

    if snap.mode.is_some() || snap.plans.len() > 1 {
        page::group(ui, "Power plan", |ui| {
            if snap.plans.len() > 1 {
                let mut pick = None;
                let rows: Vec<(String, bool)> = snap.plans.iter().map(|p| (p.name.clone(), Some(p.guid) == snap.active)).collect();
                let picked = page::row(ui, "Plan", "", |ui| dropdown(ui, "power-plan", 300.0_f32.min(ui.available_width() * 0.6), &snap.active_name(), &rows));
                if let Some(k) = picked {
                    let g = snap.plans[k].guid;
                    if Some(g) != snap.active {
                        pick = Some(g);
                    }
                }
                if let Some(g) = pick {
                    v.spawn(&ctx, Some(Box::new(move || pw::set_plan(&g))));
                }
            }
            if let Some(mut mode) = snap.mode {
                let opts: Vec<_> = pw::Mode::ALL.iter().map(|m| (*m, m.label())).collect();
                if page::row(ui, "Power mode", "", |ui| page::segmented(ui, &mut mode, &opts)) && Some(mode) != snap.mode {
                    v.spawn(&ctx, Some(Box::new(move || pw::set_mode(mode))));
                }
            }
        });
    }
    page::group(ui, "Screen and sleep", |ui| {
        for key in [Key::ScreenOff, Key::Sleep] {
            if logic::applies(key, &caps) {
                row(ui, v, &snap, key, battery);
            }
        }
        notice(ui, v);
    });
    let buttons: Vec<Key> = [Key::LidClose, Key::PowerButton].into_iter().filter(|k| logic::applies(*k, &caps)).collect();
    if !buttons.is_empty() {
        page::group(ui, "Buttons and lid", |ui| {
            for key in &buttons {
                row(ui, v, &snap, *key, battery);
            }
        });
    }
    if let Some(s) = status.filter(|_| battery) {
        page::group(ui, "Battery", |ui| {
            page::row_with(ui, |ui| {
                ui.label(logic::battery_text(&s));
            });
        });
        ui.ctx().request_repaint_after(Duration::from_secs(5));
    }
}

fn notice(ui: &mut egui::Ui, v: &PowerView) {
    if let Some(n) = &v.notice {
        page::row_with(ui, |ui| {
            ui.add(egui::Label::new(theme::muted(n).size(12.5)).wrap());
        });
    }
}

/// One setting: a row with a picker for plugged in, and (when there is a battery) a second row
/// for on battery.
fn row(ui: &mut egui::Ui, v: &mut PowerView, snap: &Snapshot, key: Key, battery: bool) {
    let pair = snap.pair(key);
    page::row(ui, key.label(), if battery { "Plugged in" } else { "" }, |ui| picker(ui, v, snap, key, Source::Ac, pair.ac));
    if battery {
        page::row(ui, key.label(), "On battery", |ui| picker(ui, v, snap, key, Source::Dc, pair.dc));
    }
}

fn picker(ui: &mut egui::Ui, v: &mut PowerView, snap: &Snapshot, key: Key, source: Source, value: Option<u32>) {
    let id = ("power", key, source);
    let Some(now) = value else {
        ui.add_enabled(false, egui::Button::new("Not available").min_size(egui::vec2(180.0, 0.0)))
            .on_disabled_hover_text("This plan has no such setting.");
        return;
    };
    let choices: Vec<(u32, String)> = if key.is_timeout() {
        logic::timeout_choices(Some(now)).into_iter().map(|s| (s, logic::timeout_label(s))).collect()
    } else {
        logic::action_choices(&snap.caps, Some(now)).into_iter().map(|(n, t)| (n, t.to_owned())).collect()
    };
    let shown = choices.iter().find(|(n, _)| *n == now).map(|(_, t)| t.clone()).unwrap_or_default();
    let mut pick = None;
    let rows: Vec<(String, bool)> = choices.iter().map(|(n, text)| (text.to_string(), *n == now)).collect();
    if let Some(k) = dropdown(ui, id, 180.0, &shown, &rows) {
        if choices[k].0 != now {
            pick = Some(choices[k].0);
        }
    }
    if let Some(n) = pick {
        v.notice = None;
        // Show the choice while the worker writes it; the answer it sends back replaces this.
        if let Some(s) = v.snap.as_mut() {
            let p = match key {
                Key::ScreenOff => &mut s.screen_off,
                Key::Sleep => &mut s.sleep,
                Key::LidClose => &mut s.lid,
                Key::PowerButton => &mut s.power_button,
            };
            match source {
                Source::Ac => p.ac = Some(n),
                Source::Dc => p.dc = Some(n),
            }
        }
        let ctx = ui.ctx().clone();
        v.spawn(&ctx, Some(Box::new(move || pw::write(key, source, n))));
    }
}
