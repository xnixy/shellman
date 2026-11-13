//! Power: the plan and power mode, the screen and sleep timeouts, what the lid and power button
//! do (each only where the machine has one), and the battery. Windows is read on a worker when
//! the page is shown, and each change is written by a worker; the page then shows what Windows
//! reports back. A write Windows refuses is said in one line under the group.

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
            ui.label(theme::muted("Reading the power settings..."));
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
                page::label(ui, "Plan");
                let mut pick = None;
                egui::ComboBox::from_id_salt("power-plan").width(300.0).selected_text(snap.active_name()).show_ui(ui, |ui| {
                    for p in &snap.plans {
                        if ui.selectable_label(Some(p.guid) == snap.active, &p.name).clicked() && Some(p.guid) != snap.active {
                            pick = Some(p.guid);
                        }
                    }
                });
                if let Some(g) = pick {
                    v.spawn(&ctx, Some(Box::new(move || pw::set_plan(&g))));
                }
            }
            if let Some(mut mode) = snap.mode {
                if snap.plans.len() > 1 {
                    ui.add_space(14.0);
                }
                page::label(ui, "Power mode");
                let opts: Vec<_> = pw::Mode::ALL.iter().map(|m| (*m, m.label())).collect();
                if page::segmented(ui, &mut mode, &opts) && Some(mode) != snap.mode {
                    v.spawn(&ctx, Some(Box::new(move || pw::set_mode(mode))));
                }
            }
        });
    }
    page::group(ui, "Screen and sleep", |ui| {
        let mut first = true;
        for key in [Key::ScreenOff, Key::Sleep] {
            if !logic::applies(key, &caps) {
                continue;
            }
            if !first {
                ui.add_space(12.0);
            }
            first = false;
            row(ui, v, &snap, key, battery);
        }
        notice(ui, v);
    });
    let buttons: Vec<Key> = [Key::LidClose, Key::PowerButton].into_iter().filter(|k| logic::applies(*k, &caps)).collect();
    if !buttons.is_empty() {
        page::group(ui, "Buttons and lid", |ui| {
            for (i, key) in buttons.iter().enumerate() {
                if i > 0 {
                    ui.add_space(12.0);
                }
                row(ui, v, &snap, *key, battery);
            }
        });
    }
    if let Some(s) = status.filter(|_| battery) {
        page::group(ui, "Battery", |ui| {
            ui.label(logic::battery_text(&s));
        });
        ui.ctx().request_repaint_after(Duration::from_secs(5));
    }
}

fn notice(ui: &mut egui::Ui, v: &PowerView) {
    if let Some(n) = &v.notice {
        ui.add_space(8.0);
        ui.label(theme::muted(n).size(12.0));
    }
}

/// One setting: its label, and a picker for plugged in (and on battery when there is one).
fn row(ui: &mut egui::Ui, v: &mut PowerView, snap: &Snapshot, key: Key, battery: bool) {
    page::label(ui, key.label());
    let pair = snap.pair(key);
    ui.horizontal_wrapped(|ui| {
        if battery {
            ui.label(theme::muted("Plugged in").size(12.0));
        }
        picker(ui, v, snap, key, Source::Ac, pair.ac);
        if battery {
            ui.add_space(16.0);
            ui.label(theme::muted("On battery").size(12.0));
            picker(ui, v, snap, key, Source::Dc, pair.dc);
        }
    });
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
    egui::ComboBox::from_id_salt(id).width(180.0).selected_text(shown).show_ui(ui, |ui| {
        for (n, text) in &choices {
            if ui.selectable_label(*n == now, text).clicked() && *n != now {
                pick = Some(*n);
            }
        }
    });
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
