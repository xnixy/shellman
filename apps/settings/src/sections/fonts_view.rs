//! The Fonts page's live state: the workers, what they last published, the choice being edited
//! and the faces loaded to draw it. The workers start when the page is first drawn and end when
//! another page is shown, so nothing runs for it otherwise.

use super::fonts_load::FontLoader;
use super::fonts_logic::{self, Tab};
use super::fonts_preview::Faces;
use super::fonts_worker::{Ask, Change, FontsWorker, Page};
use shellman_sysfont::fonts::catalog::{Family, Origin};
use shellman_sysfont::settings::{self, Settings};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long the preview waits after the last change before it is built, so a slider drag builds
/// once at the end.
const PREVIEW_AFTER: Duration = Duration::from_millis(350);

/// What is being confirmed in place (never in a window of its own).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confirm {
    Apply,
    Restore,
}

pub struct FontsView {
    pub(super) worker: Option<FontsWorker>,
    pub(super) page: Page,
    /// The choices, as Fontman saves them.
    pub(super) settings: Settings,
    pub(super) selected: Option<(String, Origin)>,
    pub(super) tab: Tab,
    pub(super) query: String,
    pub(super) fonts: FontLoader,
    pub(super) confirm: Option<Confirm>,
    /// Compare with Windows' own Segoe UI in the preview.
    pub(super) compare: bool,
    /// The preview: what is wanted and when, what is shown, and the build number.
    preview_due: Option<(String, Instant)>,
    preview_built: Option<String>,
    preview_running: bool,
    preview_n: u64,
    pub(super) preview_problems: Vec<String>,
    /// The faces the preview shows now, by role.
    preview_keys: Vec<(String, String)>,
    /// The file picker's answer, while it is open.
    picking: Option<Receiver<Result<Option<PathBuf>, String>>>,
    /// The message shown in the command pill: the last change's, or a problem of the page's own.
    pub(super) message: Option<Result<String, String>>,
    seen_outcome: u64,
    /// A slider moved and the choices are not saved yet (they are once it is let go).
    pub(super) edited: bool,
}

impl Default for FontsView {
    fn default() -> Self {
        let tab = std::env::var("SHELLMAN_SYSFONT_TAB").ok().and_then(|w| Tab::from_word(&w)).unwrap_or(Tab::Fonts);
        Self {
            worker: None,
            page: Page::default(),
            settings: Settings::default(),
            selected: None,
            tab,
            // SHELLMAN_SYSFONT_QUERY: open with the list searched, for pictures of it.
            query: std::env::var("SHELLMAN_SYSFONT_QUERY").unwrap_or_default(),
            fonts: FontLoader::default(),
            confirm: None,
            compare: false,
            preview_due: None,
            preview_built: None,
            preview_running: false,
            preview_n: 0,
            preview_problems: Vec::new(),
            preview_keys: Vec::new(),
            picking: None,
            message: None,
            seen_outcome: 0,
            edited: false,
        }
    }
}

impl FontsView {
    pub fn stop(&mut self) {
        if self.worker.take().is_some() {
            // The tab and the search are the user's, and stay for the next visit.
            *self = Self { tab: self.tab, query: std::mem::take(&mut self.query), ..Self::default() };
        }
    }

    pub(super) fn start(&mut self, ctx: &egui::Context) {
        if self.worker.is_some() {
            return;
        }
        let repaint_ctx = ctx.clone();
        self.worker = Some(FontsWorker::start(Arc::new(move || repaint_ctx.request_repaint())));
        self.settings = settings::load();
        if let Some(family) = std::env::var("SHELLMAN_SYSFONT_SELECT").ok().filter(|f| !f.is_empty()) {
            self.settings.family = Some(family);
        }
    }

    /// Take in what the workers published.
    pub(super) fn poll(&mut self, ctx: &egui::Context) {
        self.fonts.begin_frame(ctx);
        let Some(worker) = &self.worker else { return };
        let page = worker.read();
        for (key, bytes, index) in &page.tiles {
            self.fonts.insert(ctx, key.clone(), bytes, *index);
        }
        if let Some((key, faces, problems)) = &page.preview {
            for (k, bytes, index) in faces {
                self.fonts.insert(ctx, k.clone(), bytes, *index);
            }
            self.preview_keys = faces.iter().map(|(k, _, _)| (k.clone(), role_of(k).to_string())).collect();
            self.preview_problems = problems.clone();
            self.preview_built = Some(key.clone());
            self.preview_running = false;
        }
        if let Some(name) = &page.added {
            self.settings.family = Some(name.clone());
            self.selected = Some((name.clone(), Origin::Added));
        }
        if page.outcome_id != self.seen_outcome {
            self.seen_outcome = page.outcome_id;
            self.message = page.outcome.clone();
            // An apply saved the choices with what it applied: read them back.
            self.settings = settings::load();
        }
        self.page = page;
        self.pick_selection();
        self.auto_run();
        self.poll_picker();
        self.preview_tick(ctx);
    }

    /// Pick the family the saved choices name, once the list is read.
    fn pick_selection(&mut self) {
        if self.selected.is_some() {
            return;
        }
        let Some(catalog) = &self.page.catalog else { return };
        let wanted = self.settings.family.clone().or_else(|| self.page.status.current.as_ref().map(|c| c.family.clone()));
        if let Some(found) = wanted.and_then(|w| shellman_sysfont::actions::find_family(&catalog.families, &w)) {
            self.selected = Some((found.name.clone(), found.origin));
        }
    }

    /// Only in a dry run: `SHELLMAN_SYSFONT_AUTO=apply` or `restore` goes through that change once,
    /// as if confirmed, so a dry run can be tried and pictured without clicking.
    fn auto_run(&mut self) {
        static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !settings::dry_run() || DONE.load(std::sync::atomic::Ordering::Relaxed) || self.busy() || self.page.catalog.is_none() {
            return;
        }
        let asked = std::env::var("SHELLMAN_SYSFONT_AUTO").unwrap_or_default();
        let what = match asked.as_str() {
            "apply" if self.family().is_some() => Some(Confirm::Apply),
            "restore" => Some(Confirm::Restore),
            word if word.starts_with("reuse:") => None,
            _ => return,
        };
        DONE.store(true, std::sync::atomic::Ordering::Relaxed);
        match what {
            Some(what) => self.run(what),
            None => self.reuse(asked.trim_start_matches("reuse:")),
        }
    }

    pub(super) fn family(&self) -> Option<&Family> {
        let (name, origin) = self.selected.as_ref()?;
        self.page.catalog.as_ref()?.families.iter().find(|f| &f.name == name && f.origin == *origin)
    }

    /// The family Windows uses now, by name.
    pub(super) fn in_use(&self) -> Option<&str> {
        self.page.status.current.as_ref().map(|c| c.family.as_str())
    }

    pub(super) fn select(&mut self, family: &Family) {
        let key = (family.name.clone(), family.origin);
        if self.selected.as_ref() == Some(&key) {
            return;
        }
        // Faces picked by hand belong to the family they were picked from.
        if self.settings.family.as_deref() != Some(family.name.as_str()) {
            self.settings.overrides.clear();
        }
        self.selected = Some(key);
        self.settings.family = Some(family.name.clone());
        self.save();
    }

    /// Save the choices (on the change worker; never in the frame).
    pub(super) fn save(&mut self) {
        if let Some(w) = &self.worker {
            w.change(Change::Save(Box::new(self.settings.clone())));
        }
    }

    pub(super) fn busy(&self) -> bool {
        self.page.job.is_some()
    }

    /// Ask for the samples of the families in view that are not loaded yet.
    pub(super) fn want_tiles(&mut self, families: &[&Family]) {
        let Some(w) = &self.worker else { return };
        let wanted: Vec<(String, PathBuf, u32)> = families
            .iter()
            .filter_map(|f| {
                let key = fonts_logic::tile_key(f);
                if self.fonts.known(&key) {
                    return None;
                }
                let face = f.regular()?;
                Some((key, face.path.clone(), face.index))
            })
            .collect();
        if wanted.is_empty() {
            return;
        }
        // Asked for once; the answer (a face, or an empty one that will not load) settles it.
        for (key, _, _) in &wanted {
            self.fonts.mark_asked(key);
        }
        w.ask(Ask::Tiles(wanted));
    }

    /// The preview is out of date: build it again once things settle.
    pub(super) fn preview_tick(&mut self, ctx: &egui::Context) {
        if self.tab != Tab::Preview && self.tab != Tab::Weights && self.tab != Tab::Tune {
            return;
        }
        let Some(family) = self.family() else { return };
        let key = fonts_logic::preview_key(family, &self.settings.tune, &self.settings.overrides);
        let (name, origin) = (family.name.clone(), family.origin);
        if self.preview_built.as_deref() == Some(key.as_str()) {
            self.preview_due = None;
            return;
        }
        let due = match &self.preview_due {
            Some((k, at)) if *k == key => *at,
            _ => {
                // The first build is at once; a change waits for things to settle.
                let at = if self.preview_built.is_none() { Instant::now() } else { Instant::now() + PREVIEW_AFTER };
                self.preview_due = Some((key.clone(), at));
                at
            }
        };
        if self.preview_running {
            return;
        }
        let now = Instant::now();
        if now < due {
            ctx.request_repaint_after(due - now);
            return;
        }
        self.preview_due = None;
        self.preview_running = true;
        self.preview_n += 1;
        if let Some(w) = &self.worker {
            w.ask(Ask::Preview { key, family: name, origin, overrides: self.settings.overrides.clone(), tune: self.settings.tune, n: self.preview_n });
        }
    }

    pub(super) fn preview_ready(&self) -> bool {
        self.family().is_some_and(|f| {
            self.preview_built.as_deref() == Some(fonts_logic::preview_key(f, &self.settings.tune, &self.settings.overrides).as_str())
        })
    }

    pub(super) fn preview_busy(&self) -> bool {
        self.preview_running || self.preview_due.is_some()
    }

    /// The preview's faces: the new font's, or Windows' own with `segoe`.
    pub(super) fn faces(&self, segoe: bool) -> Faces {
        let get = |role: &str| {
            self.preview_keys
                .iter()
                .find(|(k, r)| r == role && (k.starts_with("seg:") == segoe))
                .and_then(|(k, _)| self.fonts.family(k))
        };
        Faces {
            light: get("light"),
            semilight: get("semilight"),
            regular: get("regular"),
            semibold: get("semibold"),
            bold: get("bold"),
            black: get("black"),
            italic: get("italic"),
            display: get("display"),
            text: get("text"),
            small: get("small"),
        }
    }

    /// Open shellman's own file picker for font files, on a thread of its own.
    pub(super) fn pick_font_file(&mut self, ctx: &egui::Context) {
        if self.picking.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let _ = std::thread::Builder::new().name("font picker".into()).spawn(move || {
            let _ = tx.send(pick_font());
            ctx.request_repaint();
        });
        self.picking = Some(rx);
    }

    pub(super) fn picking(&self) -> bool {
        self.picking.is_some()
    }

    fn poll_picker(&mut self) {
        let Some(rx) = &self.picking else { return };
        let Ok(answer) = rx.try_recv() else { return };
        self.picking = None;
        match answer {
            Ok(Some(path)) => {
                if let Some(w) = &self.worker {
                    w.change(Change::AddFile(path));
                }
            }
            Ok(None) => {}
            Err(err) => self.message = Some(Err(err)),
        }
    }

    /// Start the change the command pill confirmed.
    pub(super) fn run(&mut self, what: Confirm) {
        self.confirm = None;
        self.message = None;
        let Some(w) = &self.worker else { return };
        match what {
            Confirm::Restore => w.change(Change::Restore),
            Confirm::Apply => {
                let (Some(catalog), Some((family, origin))) = (self.page.catalog.clone(), self.selected.clone()) else { return };
                w.change(Change::Apply {
                    catalog,
                    family,
                    origin,
                    overrides: self.settings.overrides.clone(),
                    tune: self.settings.tune,
                    layers: self.settings.layers,
                    text_sizes: self.settings.text_sizes,
                });
            }
        }
    }

    /// "Apply again" after Windows put Segoe UI back: the saved copies when they are there.
    pub(super) fn apply_again(&mut self) {
        self.message = None;
        let generation = self.page.status.current.as_ref().map(|c| c.generation.clone());
        match (generation, &self.worker) {
            (Some(g), Some(w)) if shellman_sysfont::machine::history_plan(&g).is_some() => w.change(Change::Reuse(g)),
            _ => self.run(Confirm::Apply),
        }
    }

    pub(super) fn reuse(&mut self, generation: &str) {
        self.message = None;
        if let Some(w) = &self.worker {
            w.change(Change::Reuse(generation.to_string()));
        }
    }

    pub(super) fn set_check(&mut self, on: bool) {
        if let Some(w) = &self.worker {
            w.change(Change::SetCheck(on));
        }
    }
}

/// The role in a preview face's key: `new:<n>:<role>` or `seg:<role>`.
fn role_of(key: &str) -> &str {
    key.rsplit(':').next().unwrap_or(key)
}

/// Ask shellman's file picker (`shellman-files --pick`) for one font file, and wait for it.
fn pick_font() -> Result<Option<PathBuf>, String> {
    use shellman_pick::{FileType, Mode, Request};
    let exe = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("shellman-files.exe")))
        .filter(|p| p.is_file())
        .ok_or("The file picker (shellman-files.exe) was not found beside Settings.")?;
    let req = Request {
        mode: Mode::Open,
        title: "Add a font file".into(),
        ok_label: "Add".into(),
        types: vec![FileType::parse("Font files (*.ttf; *.otf; *.ttc; *.otc)", "*.ttf;*.otf;*.ttc;*.otc")],
        must_exist: true,
        client: "shellman-settings.exe".into(),
        host_pid: std::process::id(),
        ..Default::default()
    };
    let path = shellman_pick::new_request_path(&std::env::temp_dir(), std::process::id(), 1);
    shellman_pick::write_json(&path, &req).map_err(|e| format!("The picker could not be asked: {e}"))?;
    let response = shellman_pick::response_path(&path);
    let _ = std::fs::remove_file(&response);
    let status = std::process::Command::new(exe).arg("--pick").arg(&path).status();
    let answer = shellman_pick::read_response(&response);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&response);
    match status {
        Err(e) => Err(format!("The file picker did not start: {e}")),
        Ok(_) => Ok(answer.filter(|r| r.ok).and_then(|r| r.paths.into_iter().next())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_keys_name_their_role() {
        assert_eq!(role_of("new:3:semibold"), "semibold");
        assert_eq!(role_of("seg:display"), "display");
    }
}
