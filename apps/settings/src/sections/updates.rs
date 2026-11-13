//! Windows Update: one line saying where things stand with its button (check, resume, or update
//! and restart after a confirmation), the updates Windows has waiting with live progress, the
//! optional ones to ask for, pausing, and the history. A worker owns Windows' update manager
//! while the page shows (`shellman_core::system::updates`) and sends what changes.
//!
//! Left out on purpose: active hours and "get the latest updates as soon as they're available".
//! Their values were written and read back on 26300, but nothing Windows reports changed, so a
//! switch here could do nothing. Pausing was seen to take hold (see `update_pause`).

use super::sound_rows::link_row;
use super::updates_rows::{bar, button, history_row, toggle_row, update_row};
use crate::page;
use crate::state::State;
use egui::{Align, Align2, Layout, RichText, Sense, Vec2};
use shellman_core::system::power;
use shellman_core::system::update_history::Entry;
use shellman_core::system::update_logic::{self as logic, checked_text, date_text, Group, Headline, Primary};
use shellman_core::system::updates::{self as wu, Msg, Snapshot, Watch};
use shellman_ui::theme::{self, colors, icon_font};
use std::collections::HashSet;
use std::sync::mpsc::{channel, Receiver};

const SETTINGS_URI: &str = "ms-settings:windowsupdate";

/// The page's live state; the worker runs only while the page shows.
pub struct UpdatesView {
    watch: Option<Watch>,
    rx: Option<Receiver<Msg>>,
    snap: Option<Snapshot>,
    history: Option<Result<Vec<Entry>, String>>,
    history_open: bool,
    /// "Update and restart" was pressed; the page asks before restarting.
    confirm_restart: bool,
    restart_refused: bool,
    weeks: u32,
    /// Optional updates already asked for, so their button does not take a second press.
    asked: HashSet<String>,
    /// Windows' own Settings can open (Explorer or its app host runs); asked once per showing.
    settings_opens: bool,
}

impl Default for UpdatesView {
    fn default() -> Self {
        Self { watch: None, rx: None, snap: None, history: None, history_open: false, confirm_restart: false, restart_refused: false, weeks: 1, asked: HashSet::new(), settings_opens: false }
    }
}

impl UpdatesView {
    pub fn stop(&mut self) {
        *self = Self::default();
    }

    fn start(&mut self, ctx: &egui::Context) {
        if self.watch.is_some() {
            return;
        }
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        self.watch = Some(Watch::start(move |m| {
            let _ = tx.send(m);
            ctx.request_repaint();
        }));
        self.rx = Some(rx);
        self.settings_opens = shellman_core::app_host::modern_apps_available();
    }

    fn poll(&mut self) {
        let Some(rx) = &self.rx else { return };
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Snap(s) => self.snap = Some(s),
                Msg::History(h) => self.history = Some(h),
            }
        }
    }

    fn watch(&self) -> Option<&Watch> {
        self.watch.as_ref()
    }
}

/// A UTC time as the page writes dates, in local time.
fn date(utc: i64) -> String {
    date_text(wu::local(utc))
}

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "Windows Update", "Windows downloads and installs updates by itself. Here you can see what it is doing, check now, and pause it for a while.");
    let v = &mut st.updates;
    v.start(ui.ctx());
    v.poll();
    let Some(snap) = v.snap.clone() else {
        page::group(ui, "", |ui| {
            ui.label(theme::muted("Asking Windows Update..."));
        });
        return;
    };
    let now = wu::now();
    let head = logic::headline(&snap.overview(now));
    status(ui, v, &snap, head, now);
    if snap.unavailable.is_none() {
        let (optional, pending): (Vec<_>, Vec<_>) = snap.updates.iter().partition(|u| u.facts.optional());
        if !pending.is_empty() {
            page::group(ui, "Updates", |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for u in &pending {
                    update_row(ui, u, None);
                }
            });
        }
        if !optional.is_empty() {
            page::group(ui, "Optional updates", |ui| {
                ui.label(theme::muted("Windows installs these only if you ask.").size(12.0));
                ui.add_space(8.0);
                ui.spacing_mut().item_spacing.y = 4.0;
                for u in &optional {
                    let asked = v.asked.contains(&u.id);
                    let label = if asked { "Asked" } else { "Download and install" };
                    if update_row(ui, u, Some((label, snap.v2 && !asked))) {
                        v.asked.insert(u.id.clone());
                        if let Some(w) = v.watch() {
                            w.approve(&u.id);
                        }
                    }
                }
                if !snap.v2 {
                    ui.add_space(6.0);
                    ui.label(theme::muted("This version of Windows installs optional updates only from its own Settings.").size(12.0));
                }
            });
        }
        pause_group(ui, v, &snap, now);
    }
    history_group(ui, v);
}

/// The headline, the last check, and the main button.
fn status(ui: &mut egui::Ui, v: &mut UpdatesView, snap: &Snapshot, head: Headline, now: i64) {
    let c = colors();
    page::group(ui, "", |ui| {
        ui.horizontal(|ui| {
            let (disc, _) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
            let (fill, ink) = match head {
                Headline::RestartNeeded => (c.accent_fill, c.on_accent),
                Headline::Unavailable => (c.control, c.muted),
                _ => (c.control, c.accent_text),
            };
            ui.painter().circle_filled(disc.center(), 22.0, fill);
            ui.painter().text(disc.center(), Align2::CENTER_CENTER, head_glyph(head), icon_font(20.0), ink);
            ui.add_space(10.0);
            ui.vertical(|ui| {
                ui.add_space(2.0);
                ui.label(RichText::new(head.text(date)).size(18.0).strong().color(c.text));
                let sub = match (&snap.unavailable, snap.last_check) {
                    (Some(why), _) => why.clone(),
                    (None, Some(t)) => checked_text(wu::local(t), wu::local(now)),
                    (None, None) => "Not checked yet".to_owned(),
                };
                ui.label(theme::muted(sub).size(12.5));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| primary(ui, v, head));
        });
        let active: Vec<f32> = snap.updates.iter().filter_map(|u| u.facts.progress).collect();
        if !active.is_empty() {
            ui.add_space(10.0);
            let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 4.0), Sense::hover());
            bar(ui, r, active.iter().sum::<f32>() / active.len() as f32);
        }
        if v.confirm_restart {
            confirm_restart(ui, v);
        }
        if let Some(n) = &snap.notice {
            ui.add_space(8.0);
            ui.label(theme::muted(n).size(12.0));
        }
        if snap.pause.until.is_some() && !snap.pause.windows_agrees {
            ui.add_space(8.0);
            ui.label(theme::muted("Windows takes in the pause at its next check, in a moment.").size(12.0));
        }
    });
}

fn head_glyph(head: Headline) -> char {
    match head {
        Headline::UpToDate => '\u{E930}',
        Headline::RestartNeeded => '\u{E777}',
        Headline::Paused(_) => '\u{E769}',
        Headline::Downloading => '\u{E896}',
        Headline::Unavailable => '\u{E7BA}',
        _ => '\u{E895}',
    }
}

fn primary(ui: &mut egui::Ui, v: &mut UpdatesView, head: Headline) {
    match head.primary() {
        Primary::None => {}
        Primary::Check => {
            if button(ui, "Check for updates", true, true).clicked() {
                if let Some(w) = v.watch() {
                    w.check();
                }
            }
        }
        Primary::Checking => {
            button(ui, "Check for updates", true, false);
        }
        Primary::Restart => {
            if button(ui, "Update and restart", true, !v.confirm_restart).clicked() {
                v.confirm_restart = true;
                v.restart_refused = false;
            }
        }
        Primary::Resume => {
            if button(ui, "Resume updates", true, true).clicked() {
                if let Some(w) = v.watch() {
                    w.resume();
                }
            }
        }
    }
}

/// Restarting ends every app, so the page asks first.
fn confirm_restart(ui: &mut egui::Ui, v: &mut UpdatesView) {
    ui.add_space(14.0);
    page::label(ui, "Restart now?");
    ui.label(theme::muted("Your PC restarts right away to finish installing. Save your work first; an app with unsaved work can still stop the restart and ask you.").size(12.5));
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if button(ui, "Restart now", true, true).clicked() {
            v.restart_refused = !power::update_and_restart();
        }
        if button(ui, "Cancel", false, true).clicked() {
            v.confirm_restart = false;
        }
    });
    if v.restart_refused {
        ui.add_space(6.0);
        ui.label(theme::muted("Windows refused to restart.").size(12.0));
    }
}

/// Pause for one to five weeks, or resume; and the way to Windows' own page when it can open.
fn pause_group(ui: &mut egui::Ui, v: &mut UpdatesView, snap: &Snapshot, now: i64) {
    page::group(ui, "Pause updates", |ui| {
        match snap.pause.until.filter(|t| *t > now) {
            Some(until) => {
                ui.label(format!("Paused until {}.", date(until)));
                ui.add_space(8.0);
                if button(ui, "Resume updates", false, true).clicked() {
                    if let Some(w) = v.watch() {
                        w.resume();
                    }
                }
            }
            None => {
                ui.label(theme::muted("While paused, Windows does not download or install updates.").size(12.5));
                ui.add_space(10.0);
                let choices: Vec<(u32, String)> = logic::pause_weeks(snap.pause.max_days).into_iter().map(|w| (w, if w == 1 { "1 week".to_owned() } else { format!("{w} weeks") })).collect();
                if !choices.iter().any(|(w, _)| *w == v.weeks) {
                    v.weeks = 1;
                }
                let opts: Vec<(u32, &str)> = choices.iter().map(|(w, t)| (*w, t.as_str())).collect();
                ui.horizontal(|ui| {
                    page::segmented(ui, &mut v.weeks, &opts);
                    ui.add_space(8.0);
                    if button(ui, "Pause", false, true).clicked() {
                        if let Some(w) = v.watch() {
                            w.pause(v.weeks);
                        }
                    }
                });
            }
        }
        if v.settings_opens {
            ui.add_space(14.0);
            if link_row(ui, "More options in Windows Settings").clicked() {
                open_windows_settings();
            }
        }
    });
}

/// Open Windows' own Windows Update page, from a thread of its own.
fn open_windows_settings() {
    use windows::core::HSTRING;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    std::thread::spawn(|| {
        // SAFETY: COM is started on this thread first; the strings outlive the call.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(SETTINGS_URI), None, None, SW_SHOWNORMAL);
        }
    });
}

/// What was installed, by group; read when first opened.
fn history_group(ui: &mut egui::Ui, v: &mut UpdatesView) {
    page::group(ui, "Update history", |ui| {
        let text = if v.history_open { "Hide update history" } else { "Show update history" };
        if toggle_row(ui, text, v.history_open).clicked() {
            v.history_open = !v.history_open;
            if v.history_open && v.history.is_none() {
                if let Some(w) = v.watch() {
                    w.history();
                }
            }
        }
        if !v.history_open {
            return;
        }
        ui.add_space(10.0);
        match &v.history {
            None => {
                ui.label(theme::muted("Reading the history...").size(12.5));
            }
            Some(Err(e)) => {
                ui.label(theme::muted(e).size(12.5));
            }
            Some(Ok(list)) if list.is_empty() => {
                ui.label(theme::muted("Nothing has been installed yet.").size(12.5));
            }
            Some(Ok(list)) => {
                for g in Group::ALL {
                    let rows: Vec<&Entry> = list.iter().filter(|e| e.group == g).collect();
                    if rows.is_empty() {
                        continue;
                    }
                    page::label(ui, &format!("{} ({})", g.label(), rows.len()));
                    ui.spacing_mut().item_spacing.y = 4.0;
                    for e in rows {
                        history_row(ui, e, &date(e.when));
                    }
                    ui.add_space(12.0);
                }
            }
        }
    });
}
