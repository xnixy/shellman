//! Picker mode's side of the window: what it remembers while the user picks, the name field
//! following the selection, the main button's work (see `pick::decide`), and the ending, which
//! writes the response for the asking app and closes the window.
//!
//! Browsing works as ever (the rail, the path, search, the views, new folder); tabs are hidden
//! and the bottom pill holds the name, the file type and the buttons (see `pick_band`).

use super::actions::Action;
use super::tab::{Loc, Only};
use crate::pick::decide::{self, Input, Kind, Outcome};
use crate::pick::filter::{type_extension, Patterns};
use crate::pick::{host, name, start, Session};
use shellman_pick::{exit, Mode, Response, Verdict};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

/// How often the picker looks for the asking app's verdict, and checks the app is still there.
const POLL_WAITING: f64 = 0.05;
const POLL_IDLE: f64 = 1.0;

pub(crate) struct Picker {
    pub(super) session: Session,
    /// The name field.
    pub(super) name: String,
    /// Whether the field was typed in since the selection last changed.
    pub(super) typed_since: bool,
    /// The chosen file type, from 0.
    pub(super) type_index: usize,
    /// A filter typed into the name field (`*.log`), shown instead of the type's until the type
    /// is chosen again.
    pub(super) typed_filter: Option<String>,
    /// An existing file the user chose to save over: the pill asks first.
    pub(super) replace: Option<PathBuf>,
    /// Since when the picker has been waiting for the app to accept a choice.
    pub(super) waiting: Option<f64>,
    /// The picker has ended (chosen or cancelled); only the closing is left.
    pub(super) done: bool,
    /// Put the keyboard in the name field on the next frame.
    pub(super) focus_name: bool,
    /// The selection's fingerprint when the field last followed it.
    sel_sig: Option<(u64, u64, usize, Option<usize>)>,
    last_poll: f64,
    /// Whether the details island shows; hidden in a picker until asked for.
    pub(super) details: bool,
}

impl Picker {
    /// The picker for `session`, and the folder it opens on.
    pub(crate) fn new(session: Session) -> (Self, Loc) {
        let req = &session.req;
        let remembered = start::Remembered::load();
        let (folder, name) = start::start(
            req,
            remembered.get(&req.client),
            dirs::document_dir().or_else(dirs::home_dir),
            &|p: &Path| p.is_dir(),
        );
        let type_index = req.type_index.min(req.types.len().saturating_sub(1));
        let picker = Self {
            name,
            typed_since: false,
            type_index,
            typed_filter: None,
            replace: None,
            waiting: None,
            done: false,
            focus_name: req.mode == Mode::Save,
            sel_sig: None,
            last_poll: 0.0,
            details: false,
            session,
        };
        (picker, folder.map_or(Loc::Pc, Loc::Dir))
    }

    /// What the folder shows, by the mode and the chosen (or typed) file type.
    pub(super) fn only(&self) -> Only {
        let req = &self.session.req;
        let files = match &self.typed_filter {
            Some(t) => Patterns::typed(t),
            None => req
                .types
                .get(self.type_index)
                .map_or_else(Patterns::all, |t| Patterns::new(&t.patterns)),
        };
        Only {
            folders: req.mode == Mode::Folder,
            files,
        }
    }

    /// The chosen type's own extension, for a saved name without one.
    fn type_ext(&self) -> Option<String> {
        self.session
            .req
            .types
            .get(self.type_index)
            .and_then(|t| type_extension(&t.patterns))
    }
}

impl super::FileMan {
    pub(super) fn picker(&self) -> Option<&Picker> {
        self.pick.as_ref()
    }

    /// Tell every tab (there is one) what the picker shows of a folder.
    pub(super) fn pick_apply_filter(&mut self) {
        let Some(only) = self.pick.as_ref().map(Picker::only) else { return };
        for t in &mut self.tabs {
            t.only = Some(only.clone());
            t.dirty = true;
        }
    }

    pub(super) fn pick_set_type(&mut self, i: usize) {
        if let Some(p) = &mut self.pick {
            p.type_index = i;
            p.typed_filter = None;
        }
        self.pick_apply_filter();
    }

    /// The selection, as `decide` takes it: each path and whether it is a folder.
    pub(super) fn pick_selection(&self) -> Vec<(PathBuf, bool)> {
        self.tabs[self.active]
            .selection()
            .into_iter()
            .map(|e| (e.path, e.is_dir))
            .collect()
    }

    /// Each frame: when the selection changed, the name field follows it. Files' names go in it
    /// (open and save); in folder mode, the selected folder's.
    pub(super) fn pick_follow_selection(&mut self) {
        let tab = &self.tabs[self.active];
        let sig = (tab.id, tab.gen, tab.selected.len(), tab.cursor);
        let Some(p) = &self.pick else { return };
        if p.sel_sig == Some(sig) {
            return;
        }
        let first_frame = p.sel_sig.is_none();
        let selection = tab.selection();
        let p = self.pick.as_mut().unwrap();
        p.sel_sig = Some(sig);
        if first_frame {
            return;
        }
        p.typed_since = false;
        p.replace = None;
        let mut names: Vec<String> = Vec::new();
        match p.session.req.mode {
            Mode::Folder => {
                let dirs: Vec<&str> = selection.iter().filter(|e| e.is_dir).map(|e| e.name.as_str()).collect();
                p.name = if dirs.len() == 1 { dirs[0].to_owned() } else { String::new() };
                return;
            }
            mode => {
                names.extend(selection.iter().filter(|e| !e.is_dir).map(|e| e.name.clone()));
                if mode == Mode::Save || !p.session.req.multiple() {
                    names.truncate(1);
                }
            }
        }
        if !names.is_empty() {
            p.name = name::join(&names);
        }
    }

    /// The main button, Enter, or a double-clicked file.
    pub(super) fn pick_submit(&mut self) {
        let Some(p) = &self.pick else { return };
        if p.done || p.waiting.is_some() || p.replace.is_some() {
            return;
        }
        let selected = self.pick_selection();
        let here = self.tabs[self.active].loc.dir().map(Path::to_path_buf);
        let typed = super::toolbar::expand_env(&p.name);
        let type_ext = p.type_ext();
        let req = &p.session.req;
        let input = Input {
            mode: req.mode,
            multiple: req.multiple(),
            must_exist: req.must_exist,
            overwrite_prompt: req.overwrite_prompt,
            here: here.as_deref(),
            typed: &typed,
            typed_since_selection: p.typed_since,
            selected: &selected,
            default_ext: &req.default_ext,
            type_ext: type_ext.as_deref(),
        };
        let outcome = decide::decide(&input, &kind);
        let typed_since = p.typed_since;
        match outcome {
            Outcome::Choose(paths) => self.pick_finish(paths),
            Outcome::Enter(dir) => {
                // A typed folder is used up by going there.
                if typed_since {
                    if let Some(p) = &mut self.pick {
                        p.name.clear();
                    }
                }
                self.apply(Action::Navigate(Loc::Dir(dir)));
            }
            Outcome::Filter(f) => {
                if let Some(p) = &mut self.pick {
                    p.typed_filter = Some(f);
                    p.name.clear();
                }
                self.pick_apply_filter();
            }
            Outcome::Replace(path) => {
                if let Some(p) = &mut self.pick {
                    p.replace = Some(path);
                }
            }
            Outcome::Problem(why) => self.toast(why),
            Outcome::Nothing => {}
        }
    }

    /// A double click on item `r`: into a folder, or choose a file.
    pub(super) fn pick_open_row(&mut self, r: usize) {
        let e = self.tabs[self.active].entry(r).clone();
        if e.is_dir {
            self.apply(Action::Navigate(Loc::Dir(e.path)));
            return;
        }
        if let Some(p) = &mut self.pick {
            p.name = e.name.clone();
            p.typed_since = true;
        }
        self.pick_submit();
    }

    /// A file path typed in the path bar: as if typed in the name field.
    pub(super) fn pick_typed_path(&mut self, path: &Path) {
        if let Some(p) = &mut self.pick {
            p.name = path.display().to_string();
            p.typed_since = true;
        }
        self.pick_submit();
    }

    /// Hand `paths` to the app. When it wants to check them first, wait for its verdict.
    pub(super) fn pick_finish(&mut self, paths: Vec<PathBuf>) {
        let folder = self.tabs[self.active].loc.dir().map(Path::to_path_buf);
        let now = self.ctx.input(|i| i.time);
        let Some(p) = &mut self.pick else { return };
        p.replace = None;
        let response = Response {
            ok: true,
            paths,
            type_index: p.type_index,
            folder,
        };
        let path = shellman_pick::response_path(&p.session.path);
        if let Err(e) = shellman_pick::write_json(&path, &response) {
            self.toast(format!("Couldn't answer the app: {e}"));
            return;
        }
        if p.session.req.confirm {
            let _ = std::fs::remove_file(shellman_pick::verdict_path(&p.session.path));
            p.waiting = Some(now);
            self.ctx.request_repaint();
        } else {
            self.pick_end(exit::OK);
        }
    }

    /// Cancel: tell the app, and close.
    pub(super) fn pick_cancel(&mut self) {
        let Some(p) = &self.pick else { return };
        if p.done {
            return;
        }
        let response = Response {
            ok: false,
            type_index: p.type_index,
            ..Default::default()
        };
        let _ = shellman_pick::write_json(&shellman_pick::response_path(&p.session.path), &response);
        self.pick_end(exit::CANCELLED);
    }

    /// Esc: away with the replace question if it is up, else cancel.
    pub(super) fn pick_escape(&mut self) {
        if let Some(p) = &mut self.pick {
            if p.replace.take().is_some() {
                return;
            }
        }
        self.pick_cancel();
    }

    /// Close with `code`, remembering the folder for the app when something was chosen.
    fn pick_end(&mut self, code: i32) {
        let here = self.tabs[self.active].loc.dir().map(Path::to_path_buf);
        let Some(p) = &mut self.pick else { return };
        p.done = true;
        p.waiting = None;
        p.session.exit.store(code, Ordering::SeqCst);
        if code == exit::OK {
            if let Some(dir) = here {
                let mut r = start::Remembered::load();
                r.set(&p.session.req.client, &dir);
                r.save();
            }
        }
        host::let_host_take_focus(p.session.req.host_pid);
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// Each frame: look for the app's verdict while waiting for one, and close if the app has
    /// gone away.
    pub(super) fn pick_poll(&mut self) {
        let now = self.ctx.input(|i| i.time);
        let Some(p) = &mut self.pick else { return };
        if p.done {
            return;
        }
        let every = if p.waiting.is_some() { POLL_WAITING } else { POLL_IDLE };
        self.ctx
            .request_repaint_after(std::time::Duration::from_secs_f64(every));
        if now - p.last_poll < every {
            return;
        }
        p.last_poll = now;
        if !host::alive(p.session.req.host_pid) {
            self.pick_end(exit::CANCELLED);
            return;
        }
        if p.waiting.is_none() {
            return;
        }
        let verdict_path = shellman_pick::verdict_path(&p.session.path);
        match shellman_pick::read_verdict(&verdict_path) {
            Some(Verdict::Ok) => {
                let _ = std::fs::remove_file(&verdict_path);
                self.pick_end(exit::OK);
            }
            Some(Verdict::Veto) => {
                // The app said why itself (a message box, usually); keep picking.
                let _ = std::fs::remove_file(&verdict_path);
                p.waiting = None;
            }
            None => {}
        }
    }

    /// The window is closing (its close button, Alt+F4): unless the picker ended, that is a
    /// cancel.
    pub(super) fn pick_closing(&mut self) {
        if self.pick.as_ref().is_some_and(|p| !p.done) {
            self.pick_cancel();
        }
    }
}

fn kind(p: &Path) -> Kind {
    match std::fs::metadata(p) {
        Ok(m) if m.is_dir() => Kind::Dir,
        Ok(_) => Kind::File,
        Err(_) => Kind::Missing,
    }
}
