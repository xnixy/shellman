//! The Fonts page's workers. Nothing slow runs on the UI thread: one thread reads (the font
//! families, what Windows uses now, the sample faces for rows in view, the preview's faces), the
//! other changes things (apply, restore, use again, add a font file, save the choices). Each
//! publishes into one shared page the window draws from, and both end when the page is closed.

use shellman_sysfont::actions::{self, Catalog, Choice, Progress};
use shellman_sysfont::elevated::{self, Step};
use shellman_sysfont::fonts::catalog::Origin;
use shellman_sysfont::fonts::rewrite::{self, Position, Tweaks};
use shellman_sysfont::fonts::segoe::Slot;
use shellman_sysfont::fonts::{resolve_font_path, FontFile};
use shellman_sysfont::machine::{self, Status};
use shellman_sysfont::metrics::{self, TextSizes, WindowText};
use shellman_sysfont::plan::{self, FaceChoice, Layers, Request, Tune};
use shellman_sysfont::{autostart, settings};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

/// Files larger than this are not loaded just to draw a row's sample.
const TILE_MAX_BYTES: u64 = 3 << 20;

/// The preview's faces, and the Segoe UI slot each is built for.
pub const ROLES: &[(&str, &str)] = &[
    ("light", "light"),
    ("semilight", "semilight"),
    ("regular", "regular"),
    ("semibold", "semibold"),
    ("bold", "bold"),
    ("black", "black"),
    ("italic", "italic"),
    ("display", "var-display-semibold"),
    ("text", "var-text-regular"),
    ("small", "var-small-regular"),
];

/// A face for egui to draw with: its key, the bytes, the face index.
pub type FaceBytes = (String, Arc<Vec<u8>>, u32);

/// A change under way.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub what: &'static str,
    pub stage: String,
    /// Faces built and checked so far, of how many.
    pub progress: Option<(usize, usize)>,
}

/// What the page draws from.
#[derive(Clone, Default)]
pub struct Page {
    pub catalog: Option<Arc<Catalog>>,
    pub catalog_error: Option<String>,
    pub status: Status,
    /// Generations in the history whose files are still saved, ready to use again.
    pub reusable: Vec<String>,
    pub window_text: Option<WindowText>,
    /// Window text was set by Fontman or here, and can be put back.
    pub window_text_ours: bool,
    pub check_on: bool,
    pub fontman_check_on: bool,
    pub job: Option<Job>,
    /// The last change's message, and a number that grows with each one.
    pub outcome: Option<Result<String, String>>,
    pub outcome_id: u64,
    /// The newest preview: its key, faces (`new:<n>:<role>`, `seg:<role>`) and problems.
    pub preview: Option<(String, Vec<FaceBytes>, Vec<String>)>,
    /// Samples read since the page last took them.
    pub tiles: Vec<FaceBytes>,
    /// A file just added: its family, to select it.
    pub added: Option<String>,
}

/// What the page asks for.
pub enum Ask {
    Catalog,
    Status,
    Tiles(Vec<(String, PathBuf, u32)>),
    Preview { key: String, family: String, origin: Origin, overrides: BTreeMap<String, FaceChoice>, tune: Tune, n: u64 },
}

/// A change.
pub enum Change {
    Apply { catalog: Arc<Catalog>, family: String, origin: Origin, overrides: BTreeMap<String, FaceChoice>, tune: Tune, layers: Layers, text_sizes: TextSizes },
    Restore,
    Reuse(String),
    AddFile(PathBuf),
    Save(Box<settings::Settings>),
    SetCheck(bool),
}

pub struct FontsWorker {
    shared: Arc<Mutex<Page>>,
    ask: Sender<Ask>,
    change: Sender<Change>,
}

impl FontsWorker {
    pub fn start(repaint: Arc<dyn Fn() + Send + Sync>) -> Self {
        let shared = Arc::new(Mutex::new(Page::default()));
        let (ask, asks) = channel();
        let (change, changes) = channel();
        let (s, r) = (shared.clone(), repaint.clone());
        let _ = std::thread::Builder::new().name("fonts reader".into()).spawn(move || reader(asks, s, r));
        let (s, r) = (shared.clone(), repaint);
        let _ = std::thread::Builder::new().name("fonts changes".into()).spawn(move || changer(changes, s, r));
        let worker = Self { shared, ask, change };
        worker.ask(Ask::Status);
        worker.ask(Ask::Catalog);
        worker
    }

    pub fn ask(&self, ask: Ask) {
        let _ = self.ask.send(ask);
    }

    pub fn change(&self, change: Change) {
        let _ = self.change.send(change);
    }

    /// The page as published, with the samples and the preview taken out of it.
    pub fn read(&self) -> Page {
        let mut shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        let page = shared.clone();
        shared.tiles.clear();
        shared.preview = None;
        shared.added = None;
        page
    }
}

fn publish(shared: &Mutex<Page>, repaint: &(dyn Fn() + Send + Sync), edit: impl FnOnce(&mut Page)) {
    edit(&mut shared.lock().unwrap_or_else(|e| e.into_inner()));
    repaint();
}

fn read_status(shared: &Mutex<Page>, repaint: &(dyn Fn() + Send + Sync)) {
    let status = machine::status();
    let reusable: Vec<String> = status.history.iter().filter(|a| machine::history_plan(&a.generation).is_some()).map(|a| a.generation.clone()).collect();
    let window_text = metrics::read().ok();
    let ours = metrics::has_backup();
    let check_on = autostart::is_enabled();
    let fontman_check_on = autostart::fontman_check_on();
    publish(shared, repaint, |p| {
        p.status = status;
        p.reusable = reusable;
        p.window_text = window_text;
        p.window_text_ours = ours;
        p.check_on = check_on;
        p.fontman_check_on = fontman_check_on;
    });
}

fn reader(asks: Receiver<Ask>, shared: Arc<Mutex<Page>>, repaint: Arc<dyn Fn() + Send + Sync>) {
    while let Ok(ask) = asks.recv() {
        match ask {
            Ask::Status => read_status(&shared, repaint.as_ref()),
            Ask::Catalog => {
                let result = actions::load_catalog();
                publish(&shared, repaint.as_ref(), |p| match result {
                    Ok(catalog) => {
                        p.catalog = Some(Arc::new(catalog));
                        p.catalog_error = None;
                    }
                    Err(err) => p.catalog_error = Some(err),
                });
            }
            Ask::Tiles(wanted) => {
                let mut read = Vec::new();
                for (key, path, index) in wanted {
                    let small = std::fs::metadata(&path).is_ok_and(|m| m.len() <= TILE_MAX_BYTES);
                    let bytes = small.then(|| std::fs::read(&path).ok()).flatten();
                    match bytes.filter(|b| shellman_sysfont::fonts::usable(b, index)) {
                        Some(bytes) => read.push((key, Arc::new(bytes), index)),
                        // An empty sample: the row draws in the window's own font.
                        None => read.push((key, Arc::new(Vec::new()), index)),
                    }
                }
                publish(&shared, repaint.as_ref(), |p| p.tiles.extend(read));
            }
            Ask::Preview { key, family, origin, overrides, tune, n } => {
                let catalog = shared.lock().unwrap_or_else(|e| e.into_inner()).catalog.clone();
                let Some(catalog) = catalog else { continue };
                let Some(found) = catalog.families.iter().find(|f| f.name == family && f.origin == origin) else { continue };
                let (faces, problems) = build_preview(&catalog, found, &overrides, tune, n);
                publish(&shared, repaint.as_ref(), |p| p.preview = Some((key, faces, problems)));
            }
        }
    }
}

/// The preview's faces as Windows would get them, built the way an apply builds them (without
/// the checks, which only an apply needs), and Windows' own faces to compare with.
fn build_preview(catalog: &Catalog, family: &shellman_sysfont::fonts::catalog::Family, overrides: &BTreeMap<String, FaceChoice>, tune: Tune, n: u64) -> (Vec<FaceBytes>, Vec<String>) {
    let request = Request { family, slots: &catalog.slots, overrides, tune, layers: Layers::default() };
    let mut faces = Vec::new();
    let mut problems = Vec::new();
    for (role, slot_key) in ROLES {
        let Some(slot) = catalog.slots.iter().find(|s| s.key == *slot_key) else { continue };
        match plan::build_face(&request, slot) {
            Ok((_, bytes)) if shellman_sysfont::fonts::usable(&bytes, 0) => faces.push((format!("new:{n}:{role}"), Arc::new(bytes), 0)),
            Ok(_) => problems.push(format!("{} could not be shown.", slot.label)),
            Err(err) => problems.push(err),
        }
        if let Some(bytes) = original_bytes(slot).filter(|b| shellman_sysfont::fonts::usable(b, 0)) {
            faces.push((format!("seg:{role}"), Arc::new(bytes), 0));
        }
    }
    (faces, problems)
}

/// Windows' own face for a slot: the file itself, or a Segoe UI Variable instance drawn out.
fn original_bytes(slot: &Slot) -> Option<Vec<u8>> {
    let path = resolve_font_path(&slot.original_file);
    if slot.size_group.is_none() {
        return std::fs::read(path).ok();
    }
    let file = FontFile::open(&path).ok()?;
    let font = file.face(0).ok()?;
    let mut axes = vec![("wght".to_string(), slot.target.weight as f32)];
    if let Some(opsz) = slot.target.opsz {
        axes.push(("opsz".to_string(), opsz));
    }
    rewrite::build(&font, &Position { axes, slant: false }, &slot.target, Tweaks { match_metrics: false, scale: 1.0 }, "Preview").ok()
}

fn changer(changes: Receiver<Change>, shared: Arc<Mutex<Page>>, repaint: Arc<dyn Fn() + Send + Sync>) {
    let repaint = repaint.as_ref();
    while let Ok(change) = changes.recv() {
        let result = match change {
            Change::Save(saved) => {
                if let Err(err) = settings::save(&saved) {
                    finish(&shared, repaint, Err(format!("The choices could not be saved: {err}")));
                }
                continue;
            }
            Change::SetCheck(on) => {
                let result = autostart::set_enabled(on).map(|_| String::new());
                read_status(&shared, repaint);
                if result.is_err() {
                    finish(&shared, repaint, result);
                }
                continue;
            }
            Change::AddFile(path) => match settings::add_font_file(&path).and_then(|copy| shellman_sysfont::fonts::read_faces(&copy)) {
                Ok(faces) => {
                    let name = faces[0].family.clone();
                    let catalog = actions::load_catalog();
                    publish(&shared, repaint, |p| {
                        if let Ok(catalog) = catalog {
                            p.catalog = Some(Arc::new(catalog));
                        }
                        p.added = Some(name.clone());
                    });
                    Ok(format!("Added {name}. It is in the list, ready to try."))
                }
                Err(err) => Err(err),
            },
            Change::Apply { catalog, family, origin, overrides, tune, layers, text_sizes } => {
                begin(&shared, repaint, "Applying", "Building the replacement faces...");
                let progress = |step: Progress| {
                    publish(&shared, repaint, |p| {
                        if let Some(job) = &mut p.job {
                            match step {
                                Progress::Building(done, total) => {
                                    job.stage = "Building and checking each face with GDI and DirectWrite...".into();
                                    job.progress = Some((done, total));
                                }
                                Progress::Permission => {
                                    job.stage = if settings::dry_run() { "Putting it in place (dry run)...".into() } else { "Waiting for administrator rights...".into() };
                                    job.progress = None;
                                }
                                Progress::WindowText => job.stage = "Setting window text...".into(),
                            }
                        }
                    })
                };
                match catalog.families.iter().find(|f| f.name == family && f.origin == origin) {
                    None => Err(format!("{family} is no longer installed.")),
                    Some(found) => {
                        let choice = Choice { family: found, slots: &catalog.slots, overrides: &overrides, tune, layers, text_sizes };
                        let result = actions::apply(&choice, &progress, &elevate);
                        if result.is_ok() {
                            let mut saved = settings::load();
                            saved.family = Some(family.clone());
                            saved.overrides = overrides.clone();
                            saved.tune = tune;
                            saved.layers = layers;
                            saved.text_sizes = text_sizes;
                            saved.applied = Some(saved.snapshot(&family));
                            let _ = settings::save(&saved);
                        }
                        result.map(|applied| applied.message)
                    }
                }
            }
            Change::Restore => {
                begin(&shared, repaint, "Restoring", if settings::dry_run() { "Putting Segoe UI back (dry run)..." } else { "Waiting for administrator rights..." });
                let result = actions::restore_all(true, true, &elevate);
                if result.is_ok() {
                    let mut saved = settings::load();
                    saved.applied = None;
                    let _ = settings::save(&saved);
                }
                result.map(|applied| applied.message)
            }
            Change::Reuse(generation) => {
                begin(&shared, repaint, "Applying again", if settings::dry_run() { "Putting it back (dry run)..." } else { "Waiting for administrator rights..." });
                let result = actions::reuse(&generation, &elevate);
                // The list then shows the family that is back in place as the choice.
                if let (Ok(_), Some(current)) = (&result, machine::status().current) {
                    let mut saved = settings::load();
                    if saved.family.as_deref() != Some(current.family.as_str()) {
                        saved.overrides.clear();
                    }
                    saved.family = Some(current.family);
                    let _ = settings::save(&saved);
                }
                result.map(|applied| applied.message)
            }
        };
        read_status(&shared, repaint);
        finish(&shared, repaint, result);
    }
}

fn begin(shared: &Mutex<Page>, repaint: &(dyn Fn() + Send + Sync), what: &'static str, stage: &str) {
    publish(shared, repaint, |p| p.job = Some(Job { what, stage: stage.into(), progress: None }));
}

fn finish(shared: &Mutex<Page>, repaint: &(dyn Fn() + Send + Sync), result: Result<String, String>) {
    publish(shared, repaint, |p| {
        p.job = None;
        p.outcome = Some(result);
        p.outcome_id += 1;
    });
}

/// The machine-wide step through shellman's administrator helper (the UAC prompt). Never called
/// in a dry run: `actions::machine_step` runs the step in this process then.
fn elevate(step: &Step) -> Result<String, String> {
    use shellman_core::elevate::{ops::Op, run, Outcome};
    debug_assert!(!settings::dry_run(), "a dry run never starts the administrator helper");
    if settings::dry_run() {
        return Err("A dry run never asks for administrator rights.".into());
    }
    let op = match step {
        Step::Apply { generation } => Op::sysfont_apply(generation),
        Step::Restore => Some(Op::SysfontRestore),
        Step::Reuse { generation } => Op::sysfont_reuse(generation),
    }
    .ok_or("That change cannot be asked for.")?;
    // A message left from an earlier run is not this run's.
    let _ = elevated::take_outcome();
    let outcome = run(&op);
    let message = elevated::take_outcome();
    match outcome {
        Outcome::Done => Ok(message.map(|m| m.message).unwrap_or_else(|| "Done. Restart Windows to see it everywhere.".into())),
        Outcome::Cancelled => Err("Windows did not get permission, so nothing was changed.".into()),
        other => Err(message
            .filter(|m| !m.ok)
            .map(|m| m.message)
            .or_else(|| other.message())
            .unwrap_or_else(|| "The change did not go through.".into())),
    }
}
