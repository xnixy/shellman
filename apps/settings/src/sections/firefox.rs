//! The Firefox group (on the Appearance page): whether Firefox is installed, which profile (the
//! installation's own preselected, a list for the others), the Islands look's state there, and
//! Install / Update / Remove with a "with scripts" switch (`shellman_core::firefox`).
//!
//! Reading the profiles and doing the work happen on threads of their own; the profiles are read
//! again every few seconds while the page shows, so closing Firefox clears the "Firefox is open"
//! line without a click.

use crate::sections::updates_rows::row_button;
use shellman_core::firefox::detect::{self, Install, Profiles};
use shellman_core::firefox::look::{self, Status};
use shellman_core::firefox::{actions, Source};
use shellman_core::registry::Real;
use shellman_ui::theme;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const RESCAN_SECS: f64 = 4.0;

type Slot<T> = Arc<Mutex<Option<T>>>;

/// One profile as last read.
#[derive(Clone)]
struct Row {
    name: String,
    path: PathBuf,
    status: Status,
    in_use: bool,
}

/// What a read found.
#[derive(Clone, Default)]
struct Scan {
    installs: Vec<Install>,
    profiles: Profiles,
    rows: Vec<Row>,
    /// The first installation's own profile.
    default: Option<PathBuf>,
    source: Option<Source>,
}

fn scan() -> Scan {
    let installs = detect::installs(&Real);
    if installs.is_empty() {
        return Scan::default();
    }
    let profiles = Profiles::read();
    let source = Source::find();
    let rows = profiles
        .list
        .iter()
        .filter(|p| p.path.is_dir())
        .map(|p| Row { name: p.name.clone(), path: p.path.clone(), status: look::status(&p.path, source.as_ref()), in_use: detect::in_use(&p.path) })
        .collect();
    let default = installs.iter().find_map(|i| profiles.default_for(i));
    Scan { installs, profiles, rows, default, source }
}

/// The group's live state.
#[derive(Default)]
pub struct FirefoxView {
    data: Option<Scan>,
    scan: Slot<Scan>,
    scanning: bool,
    last_scan: f64,
    /// The profile picked (None: the default, once known).
    chosen: Option<PathBuf>,
    /// The switch, and the profile it was last set from.
    scripts: bool,
    scripts_for: Option<PathBuf>,
    work: Option<Slot<Result<String, String>>>,
    note: Option<Result<String, String>>,
}

impl FirefoxView {
    fn start_scan(&mut self, ctx: &egui::Context, now: f64) {
        self.scanning = true;
        self.last_scan = now;
        let (slot, ctx) = (self.scan.clone(), ctx.clone());
        std::thread::spawn(move || {
            *slot.lock().unwrap() = Some(scan());
            ctx.request_repaint();
        });
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if let Some(s) = self.scan.lock().unwrap().take() {
            self.data = Some(s);
            self.scanning = false;
        }
        let finished = self.work.as_ref().and_then(|w| w.lock().unwrap().take());
        if let Some(r) = finished {
            self.work = None;
            self.note = Some(r);
            self.scripts_for = None; // set the switch again from what is there now
            self.start_scan(ctx, now);
        }
        if !self.scanning && self.work.is_none() && (self.data.is_none() || now - self.last_scan > RESCAN_SECS) {
            self.start_scan(ctx, now);
        }
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(RESCAN_SECS));
    }

    fn run(&mut self, ctx: &egui::Context, job: impl FnOnce() -> Result<String, String> + Send + 'static) {
        let slot: Slot<Result<String, String>> = Arc::default();
        self.work = Some(slot.clone());
        self.note = None;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = job();
            *slot.lock().unwrap() = Some(r);
            ctx.request_repaint();
        });
    }
}

/// The installation the scripts in `profile` are for: the one that opens it by default, else the
/// first.
fn install_for(s: &Scan, profile: &Path) -> Option<Install> {
    s.installs.iter().find(|i| s.profiles.default_for(i).is_some_and(|d| d == profile)).or(s.installs.first()).cloned()
}

fn muted(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(theme::muted(text).size(12.0)).wrap());
}

fn status_text(row: &Row) -> &'static str {
    match row.status {
        Status::NotInstalled => "The look is not installed in this profile.",
        Status::Installed { scripts: false } => "The look is installed.",
        Status::Installed { scripts: true } => "The look is installed, with scripts.",
        Status::Outdated { .. } => "The look is installed but out of date: Update brings it in line with this shellman.",
    }
}

pub fn group(ui: &mut egui::Ui, v: &mut FirefoxView) {
    let ctx = ui.ctx().clone();
    v.poll(&ctx);
    let Some(s) = v.data.clone() else {
        muted(ui, "Looking for Firefox…");
        return;
    };
    if s.installs.is_empty() {
        muted(ui, "Firefox is not installed. When it is, shellman can give it the Islands look here.");
        return;
    }
    let folders: Vec<String> = s.installs.iter().map(|i| i.dir.display().to_string()).collect();
    muted(ui, &format!("Firefox is installed ({}). The Islands look: floating glass pills and rounded islands, no window frame.", folders.join(", ")));
    if s.source.is_none() {
        muted(ui, "The look's files are missing from shellman's folder (firefox). Reinstall shellman to get them back.");
        return;
    }
    if s.rows.is_empty() {
        muted(ui, "Firefox has no profiles yet. Start Firefox once, then come back.");
        return;
    }
    ui.add_space(4.0);

    // The profile: the default until another is picked.
    let chosen = v.chosen.clone().or_else(|| s.default.clone());
    let row = chosen.as_ref().and_then(|c| s.rows.iter().find(|r| &r.path == c)).cloned();
    let label = |r: &Row| {
        let default = s.default.as_ref() == Some(&r.path);
        if default { format!("{} (default)", r.name) } else { r.name.clone() }
    };
    let shown = row.as_ref().map(label).unwrap_or_else(|| "Choose a profile".into());
    let resp = egui::ComboBox::from_id_salt("firefox-profile").width(300.0_f32.min(ui.available_width())).selected_text(&shown).show_ui(ui, |ui| {
        for r in &s.rows {
            let selected = row.as_ref().is_some_and(|c| c.path == r.path);
            if ui.selectable_label(selected, label(r)).on_hover_text(r.path.display().to_string()).clicked() {
                v.chosen = Some(r.path.clone());
                v.note = None;
            }
        }
    });
    resp.response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, format!("Firefox profile, {shown}")));
    let Some(row) = row else {
        muted(ui, "Firefox does not say which profile it opens by default; choose one.");
        return;
    };

    // The switch follows what is installed whenever the profile changes; on (the default) where
    // the look is not installed yet.
    if v.scripts_for.as_ref() != Some(&row.path) {
        v.scripts_for = Some(row.path.clone());
        v.scripts = match row.status {
            Status::NotInstalled => true,
            Status::Installed { scripts } | Status::Outdated { scripts } => scripts,
        };
    }
    ui.add_space(4.0);
    muted(ui, status_text(&row));
    if row.in_use {
        ui.add(egui::Label::new(theme::muted("Firefox is open with this profile. Close Firefox to change the look.").size(12.0).color(theme::colors().danger)).wrap());
    }
    ui.add_space(6.0);
    let busy = v.work.is_some();
    ui.add_enabled_ui(!busy, |ui| {
        theme::setting_row(
            ui,
            "With scripts",
            "Accent and glass follow shellman live, and more, smaller islands. Firefox then runs shellman's script files from the profile with full browser rights. Putting their loader in Firefox's folder asks for administrator rights once.",
            &mut v.scripts,
        );
    });
    ui.add_space(6.0);

    let installed_scripts = matches!(row.status, Status::Installed { scripts: true } | Status::Outdated { scripts: true });
    let can = !busy && !row.in_use;
    let mut press = None;
    ui.horizontal(|ui| {
        match row.status {
            Status::NotInstalled => {
                if row_button(ui, "Install", can).clicked() {
                    press = Some(true);
                }
            }
            _ => {
                let stale = matches!(row.status, Status::Outdated { .. }) || installed_scripts != v.scripts;
                if stale && row_button(ui, "Update", can).clicked() {
                    press = Some(true);
                }
                if row_button(ui, "Remove", can).clicked() {
                    press = Some(false);
                }
            }
        }
        if busy {
            ui.add(egui::Spinner::new().size(14.0));
            ui.label(theme::muted("Working…").size(12.0));
        }
    });
    match press {
        Some(true) => {
            let (src, profile, profiles, installs) = (s.source.clone().expect("checked"), row.path.clone(), s.profiles.clone(), s.installs.clone());
            let target = if v.scripts { install_for(&s, &row.path) } else { None };
            v.run(&ctx, move || actions::apply(&profile, &src, target.as_ref(), &profiles, &installs, None));
        }
        Some(false) => {
            let (profile, profiles, installs) = (row.path.clone(), s.profiles.clone(), s.installs.clone());
            v.run(&ctx, move || actions::remove(&profile, &profiles, &installs, None));
        }
        None => {}
    }
    match &v.note {
        Some(Ok(msg)) => muted(ui, msg),
        Some(Err(msg)) => {
            ui.add(egui::Label::new(egui::RichText::new(msg).size(12.0).color(theme::colors().danger)).wrap());
        }
        None => {}
    }
}
