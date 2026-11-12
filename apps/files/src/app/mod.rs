//! The window: tabs and a toolbar across the top, places on the left, the folder in the middle.

mod actions;
mod dialogs;
mod footer;
mod grid;
mod interact;
mod keys;
mod layout;
mod list;
mod pc;
mod rows;
mod sidebar;
mod tab;
mod tab_bar;
mod toolbar;

use actions::{Dialog, Rename};
use egui::Vec2;
use shellman_core::fsops::{self, Drive, Entry, Op};
use shellman_core::thumbs::Thumbs;
use shellman_ui::theme::{self, colors, AccentChoice};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use tab::{Loc, Tab};

const ROW_H: f32 = 32.0;
const FOOTER_H: f32 = 26.0;
const SIDEBAR_W: f32 = 200.0;
/// Height of the tab strip, which is also the title bar.
const CAPTION_H: f32 = 40.0;

enum Msg {
    Loaded {
        tab: u64,
        gen: u64,
        result: Result<Vec<Entry>, String>,
    },
    Drives(Vec<Drive>),
    OpDone {
        what: &'static str,
        result: Result<bool, String>,
    },
}

pub(crate) struct FileMan {
    ctx: egui::Context,
    settings: crate::settings::Settings,
    hwnd: isize,
    glass: bool,
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    drives: Vec<Drive>,
    clipboard: Option<(Op, Vec<PathBuf>)>,
    dialog: Option<Dialog>,
    rename: Option<Rename>,
    editing_path: Option<String>,
    focus_path: bool,
    focus_search: bool,
    /// The shared look, which the settings app edits; Files only follows it.
    look: shellman_ui::appearance::Appearance,
    look_watch: shellman_core::watch::Watcher,
    last_accent_check: f64,
    toast: Option<(String, f64)>,
    save_pending: bool,
    title: String,
    /// Size of the Windows caption buttons in points, when Windows draws them over the glass.
    caption: Option<Vec2>,
    /// Re-lists tabs when their folder changes on disk.
    watcher: shellman_core::watch::Watcher,
    /// The folders last handed to the watcher, so it is told only when they change.
    watched: Vec<PathBuf>,
    thumbs: Thumbs,
    /// Columns in the grid as last drawn, for moving the cursor up and down a line.
    grid_cols: usize,
}

impl FileMan {
    pub(crate) fn new(
        ctx: egui::Context,
        settings: crate::settings::Settings,
        look: shellman_ui::appearance::Appearance,
        arg: Option<PathBuf>,
        hwnd: isize,
        glass: bool,
    ) -> Self {
        let (tx, rx) = channel();
        let mut locs: Vec<Loc> = settings
            .tabs
            .iter()
            .filter(|p| p.is_dir())
            .map(|p| Loc::Dir(p.clone()))
            .collect();
        if let Some(a) = arg {
            locs = vec![Loc::Dir(a)];
        }
        if locs.is_empty() {
            locs.push(dirs::home_dir().map_or(Loc::Pc, Loc::Dir));
        }
        let thumbs = Thumbs::new(ctx.clone());
        // Follow the look live: the settings app saves it when you change the accent or glass.
        let look_watch = shellman_core::watch::Watcher::new(ctx.clone());
        if let Some(dir) = shellman_ui::appearance::file().and_then(|f| f.parent().map(Path::to_path_buf)) {
            look_watch.set_dirs(vec![dir]);
        }
        let mut app = Self {
            watcher: shellman_core::watch::Watcher::new(ctx.clone()),
            watched: Vec::new(),
            ctx,
            settings,
            hwnd,
            glass,
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
            tx,
            rx,
            drives: Vec::new(),
            clipboard: None,
            dialog: None,
            rename: None,
            editing_path: None,
            focus_path: false,
            focus_search: false,
            look,
            look_watch,
            last_accent_check: 0.0,
            toast: None,
            save_pending: false,
            title: String::new(),
            caption: None,
            thumbs,
            grid_cols: 1,
        };
        for loc in locs {
            app.push_tab(loc);
        }
        app.active = 0;
        app.load_drives();
        app
    }

    fn push_tab(&mut self, loc: Loc) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(Tab::new(id, loc));
        let i = self.tabs.len() - 1;
        self.load(i);
        i
    }

    fn load_drives(&self) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Drives(fsops::drives()));
            ctx.request_repaint();
        });
    }

    fn load(&mut self, i: usize) {
        let tab = &mut self.tabs[i];
        tab.gen += 1;
        tab.loading = true;
        tab.error = None;
        let Some(dir) = tab.loc.dir().map(Path::to_path_buf) else {
            tab.loading = false;
            tab.entries.clear();
            tab.view.clear();
            self.load_drives();
            return;
        };
        let (tx, ctx, id, gen) = (self.tx.clone(), self.ctx.clone(), tab.id, tab.gen);
        std::thread::spawn(move || {
            let result = fsops::list_dir(&dir).map_err(|e| e.to_string());
            let _ = tx.send(Msg::Loaded {
                tab: id,
                gen,
                result,
            });
            ctx.request_repaint();
        });
    }

    fn refresh_all(&mut self) {
        for i in 0..self.tabs.len() {
            if self.tabs[i].loc != Loc::Pc {
                self.load(i);
            }
        }
    }

    fn run_op(&self, op: Op, what: &'static str, from: Vec<PathBuf>, to: Option<PathBuf>) {
        let (tx, ctx, hwnd) = (self.tx.clone(), self.ctx.clone(), self.hwnd);
        std::thread::spawn(move || {
            let result = fsops::shell_op(hwnd, op, &from, to.as_deref());
            let _ = tx.send(Msg::OpDone { what, result });
            ctx.request_repaint();
        });
    }

    fn toast(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), self.ctx.input(|i| i.time)));
    }

    fn drain(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Loaded { tab, gen, result } => {
                    let show_hidden = self.settings.show_hidden;
                    let Some(t) = self.tabs.iter_mut().find(|t| t.id == tab && t.gen == gen) else {
                        continue;
                    };
                    t.loading = false;
                    match result {
                        Ok(entries) => {
                            t.set_entries(entries);
                            t.rebuild_view(show_hidden);
                        }
                        Err(e) => {
                            t.error = Some(e);
                            t.entries.clear();
                            t.view.clear();
                        }
                    }
                    if let Some((path, rename)) = t.select_after_load.take() {
                        if let Some(r) = t.row_of(&path) {
                            t.select_only(r);
                            t.scroll_to_cursor = true;
                            if rename {
                                let text = t.entry(r).name.clone();
                                self.rename = Some(Rename {
                                    path,
                                    text,
                                    focus: true,
                                });
                            }
                        }
                    }
                }
                Msg::Drives(d) => self.drives = d,
                Msg::OpDone { what, result } => {
                    match result {
                        Ok(true) => self.toast(format!("{what} done")),
                        Ok(false) => self.toast(format!("{what} stopped partway")),
                        Err(e) => self.toast(format!("{what} failed: {e}")),
                    }
                    self.refresh_all();
                }
            }
        }
    }

    /// Keep the watcher on the folders the tabs show, and re-list tabs whose folder changed. This
    /// is a plain `load`, not `arrive`, so selection and scroll position survive.
    fn watch_tabs(&mut self) {
        let mut dirs: Vec<PathBuf> = Vec::new();
        for t in &self.tabs {
            if let Loc::Dir(p) = &t.loc {
                if !dirs.contains(p) {
                    dirs.push(p.clone());
                }
            }
        }
        if dirs != self.watched {
            self.watcher.set_dirs(dirs.clone());
            self.watched = dirs;
        }
        let changed = self.watcher.poll();
        if changed.is_empty() {
            return;
        }
        for dir in &changed {
            self.thumbs.invalidate_dir(dir);
        }
        for i in 0..self.tabs.len() {
            if matches!(&self.tabs[i].loc, Loc::Dir(p) if changed.contains(p)) {
                self.load(i);
            }
        }
    }

    /// Follow the shared look when its file changes, the Windows app mode, and the Windows
    /// accent while that is the choice.
    fn track_system(&mut self, ctx: &egui::Context) {
        let mut repaint_frame = false;
        if !self.look_watch.poll().is_empty() {
            let look = shellman_ui::appearance::Appearance::load_or(self.look);
            if look != self.look {
                self.look = look;
                look.apply(ctx);
                repaint_frame = true;
            }
        }
        repaint_frame |= theme::set_light(ctx.theme() == egui::Theme::Light);
        let now = ctx.input(|i| i.time);
        if self.look.accent == AccentChoice::System && now - self.last_accent_check > 2.0 {
            self.last_accent_check = now;
            repaint_frame |= theme::set_accent(ctx, AccentChoice::System.resolve());
        }
        if repaint_frame {
            shellman_ui::window::paint_frame(self.hwnd);
        }
        if self.look.accent == AccentChoice::System {
            ctx.request_repaint_after(std::time::Duration::from_secs(2));
        }
    }

    fn take_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if dropped.is_empty() {
            return;
        }
        if let Some(dir) = self.tabs[self.active].loc.dir().map(Path::to_path_buf) {
            self.run_op(Op::Copy, "Copy", dropped, Some(dir));
        }
    }
}

impl eframe::App for FileMan {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if self.glass {
            [0.0, 0.0, 0.0, 0.0]
        } else {
            colors().bg.to_normalized_gamma_f32()
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain();
        self.watch_tabs();
        self.track_system(ctx);
        let show_hidden = self.settings.show_hidden;
        for t in &mut self.tabs {
            if t.dirty {
                t.rebuild_view(show_hidden);
            }
        }
        self.thumbs.poll(ctx);
        let mut actions = Vec::new();
        self.keyboard(ctx, &mut actions);
        self.take_dropped_files(ctx);

        self.layout(ctx, &mut actions);
        self.dialogs(ctx);

        for a in actions {
            self.apply(a);
        }

        let title = format!("{} - Files", self.tabs[self.active].loc.title());
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        if std::mem::take(&mut self.save_pending) {
            self.settings.tabs = self
                .tabs
                .iter()
                .filter_map(|t| t.loc.dir().map(Path::to_path_buf))
                .collect();
            crate::settings::save_settings(&self.settings);
        }
    }
}
