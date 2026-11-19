//! Storage: each fixed drive with its space as a bar, and the system drive's biggest user
//! folders, counted by a worker with progress and a Cancel. Read-only: a row's only action is to
//! open its place in Files. The page counts when it is shown and stops when another page is.

use super::storage_rows::{row, Usage};
use crate::page;
use crate::state::State;
use shellman_core::fsops::format_size;
use shellman_core::system::storage::{self as sto, Scan, State as Count, Target, Volume};
use shellman_ui::theme;
use std::sync::{Arc, Mutex};

type Shared = Arc<Mutex<Option<Vec<Volume>>>>;

/// The page's live state. Dropping the scan cancels it, so leaving the page stops the work.
#[derive(Default)]
pub struct StorageView {
    volumes: Shared,
    started: bool,
    scan: Option<Scan>,
}

impl StorageView {
    pub fn stop(&mut self) {
        *self = Self::default();
    }

    fn start(&mut self, ctx: &egui::Context) {
        if self.started {
            return;
        }
        self.started = true;
        let (shared, c) = (self.volumes.clone(), ctx.clone());
        std::thread::spawn(move || {
            let v = sto::fixed_volumes();
            if let Ok(mut s) = shared.lock() {
                *s = Some(v);
            }
            c.request_repaint();
        });
        self.rescan(ctx);
    }

    fn rescan(&mut self, ctx: &egui::Context) {
        let c = ctx.clone();
        // A scan's updates come often; the repaint is cheap and the window needs them to move.
        self.scan = Some(Scan::start(sto::user_targets(), Arc::new(move || c.request_repaint())));
    }
}

pub fn show(ui: &mut egui::Ui, st: &mut State) {
    page::heading(ui, "Storage", "How full your drives are and what takes the room on the system drive. Nothing is deleted from here.");
    let v = &mut st.storage;
    v.start(ui.ctx());
    let volumes = v.volumes.lock().ok().and_then(|g| g.clone());
    let Some(volumes) = volumes else {
        page::group(ui, "", |ui| {
            page::row_with(ui, |ui| {
                ui.label(theme::muted("Reading the drives..."));
            });
        });
        return;
    };
    page::group(ui, "Drives", |ui| {
        if volumes.is_empty() {
            page::row_with(ui, |ui| {
                ui.label(theme::muted("Windows reports no fixed drive."));
            });
        }
        for d in &volumes {
            let name = format!("{} ({})", d.label, d.root.to_string_lossy().trim_end_matches('\\'));
            let detail = format!("{} free of {}", format_size(d.free), format_size(d.total));
            let usage = Usage { fraction: sto::used_fraction(d.total, d.free), warn: sto::nearly_full(d.total, d.free) };
            if row(ui, &name, &detail, Some(usage), true) {
                shellman_core::sibling::start_with("files", &[&d.root.to_string_lossy()]);
            }
        }
    });
    let system = volumes.iter().find(|d| d.system).cloned();
    let ctx = ui.ctx().clone();
    page::group(ui, "On the system drive", |ui| folders(ui, v, &ctx, system.as_ref()));
}

fn folders(ui: &mut egui::Ui, v: &mut StorageView, ctx: &egui::Context, system: Option<&Volume>) {
    let Some(scan) = v.scan.as_ref() else { return };
    let states = scan.states();
    let running = scan.running();
    let total = states.len();
    let done = states.iter().filter(|s| !s.pending()).count();
    let targets: Vec<Target> = scan.targets.clone();
    // Bars are drawn against the largest folder, so the rows can be compared with each other.
    let biggest = states.iter().filter_map(Count::bytes).max().unwrap_or(0);
    let mut again = false;
    if running {
        let detail = format!("{done} of {total} done");
        if page::row(ui, "Counting", &detail, |ui| ui.button("Cancel").clicked()) {
            scan.cancel();
        }
    } else {
        let line = match system {
            Some(d) => format!("{} used on {}", format_size(d.used()), d.root.to_string_lossy().trim_end_matches('\\')),
            None => "Counted.".to_owned(),
        };
        again = page::row(ui, &line, "", |ui| ui.button("Count again").clicked());
    }
    if again {
        v.rescan(ctx);
        return;
    }
    for (t, s) in targets.iter().zip(&states) {
        let (detail, bytes) = match s {
            Count::Waiting => ("Waiting".to_owned(), None),
            Count::Counting { bytes, files } => (format!("{} so far, {files} files", format_size(*bytes)), Some(*bytes)),
            Count::Done(b) => (format_size(*b), Some(*b)),
            Count::Cancelled(b) => (format!("{} (stopped)", format_size(*b)), Some(*b)),
            Count::Missing => ("Not found".to_owned(), None),
        };
        let usage = bytes.map(|b| Usage { fraction: sto::share(b, biggest), warn: false });
        let can_open = *s != Count::Missing;
        if row(ui, t.name, &detail, usage, can_open) {
            shellman_core::sibling::start_with("files", &[&sto::open_arg(t)]);
        }
    }
    page::row_with(ui, |ui| {
        ui.add(egui::Label::new(theme::muted("Sizes are the files' own length. Links and cloud placeholders are not counted.").size(12.5)).wrap());
    });
}
