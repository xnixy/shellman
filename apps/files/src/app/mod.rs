//! The window, as islands of glass with the desktop between them: tabs and the window buttons
//! across the top, the bar with the path under them, the places as a rail on the left, the
//! folder in the middle, and the selection in a pill along the bottom (see `plan`).

mod actions;
mod band;
mod bin;
mod controls;
mod dialogs;
mod grid;
mod interact;
mod keys;
mod layout;
mod list;
mod menu;
mod pc;
mod pins;
mod places;
mod plan;
mod rail;
mod rows;
mod tab;
mod tab_bar;
mod toolbar;

use actions::{Dialog, Rename};
use shellman_core::fsops::{self, Drive, Entry, Op};
use shellman_core::recycle_bin::{BinItem, BinState};
use shellman_core::thumbs::Thumbs;
use shellman_ui::theme::{self, AccentChoice};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use tab::{Loc, Tab};

const ROW_H: f32 = 32.0;
/// The corner radius of the islands that are not pills (the list's), and of the item cards.
pub(crate) const RADIUS: f32 = 22.0;

enum Msg {
    Loaded {
        tab: u64,
        gen: u64,
        result: Result<Vec<Entry>, String>,
    },
    /// The Recycle Bin's items, and the folders that change when it does.
    Bin {
        tab: u64,
        gen: u64,
        dirs: Vec<PathBuf>,
        result: Result<Vec<BinItem>, String>,
    },
    /// What `SHQueryRecycleBin` said, before asking to empty it.
    BinState(Option<BinState>),
    /// A restore, delete or empty finished: the message to show, or why it failed.
    BinDone(Result<String, String>),
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
    /// The glass under the islands; None when the window could not be found.
    panel: Option<shellman_ui::glass::GlassPanel>,
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
    /// Re-lists tabs when their folder changes on disk.
    watcher: shellman_core::watch::Watcher,
    /// The folders last handed to the watcher, so it is told only when they change.
    watched: Vec<PathBuf>,
    thumbs: Thumbs,
    /// Columns in the grid as last drawn, and the height of one line of cards, for moving the
    /// cursor up and down a line and a page.
    grid_cols: usize,
    grid_line_h: f32,
    /// The Recycle Bin's folders, followed while a tab shows the bin.
    bin_dirs: Vec<PathBuf>,
    /// Whether the window had focus last frame, to re-list the bin when it comes back.
    had_focus: bool,
    /// The right-click menu, with the shell's entries it reads ahead.
    menu: menu::ContextMenu,
    /// The selection's fingerprint when the menu target to read ahead was last worked out.
    menu_sig: Option<(u64, u64, usize, Option<usize>)>,
    menu_settle_target: Option<shellman_core::shell_menu::Target>,
    /// The places other apps registered for the navigation pane, followed live.
    apps: places::Registered,
    /// Which pinned folders are still there.
    pin_exists: pins::Exists,
}

impl FileMan {
    pub(crate) fn new(
        ctx: egui::Context,
        settings: crate::settings::Settings,
        look: shellman_ui::appearance::Appearance,
        start: crate::args::Start,
        hwnd: isize,
        panel: Option<shellman_ui::glass::GlassPanel>,
    ) -> Self {
        let (tx, rx) = channel();
        let mut locs: Vec<Loc> = settings
            .tabs
            .iter()
            .filter(|p| p.is_dir())
            .map(|p| Loc::Dir(p.clone()))
            .collect();
        match start {
            crate::args::Start::Saved => {}
            crate::args::Start::Folder(dir) => locs = vec![Loc::Dir(dir)],
            crate::args::Start::RecycleBin => locs = vec![Loc::Bin],
        }
        if locs.is_empty() {
            locs.push(dirs::home_dir().map_or(Loc::Pc, Loc::Dir));
        }
        let thumbs = Thumbs::new(ctx.clone());
        // Follow the look live: the settings app saves it when you change the accent or glass.
        let look_watch = shellman_core::watch::Watcher::quick(ctx.clone());
        if let Some(dir) =
            shellman_ui::appearance::file().and_then(|f| f.parent().map(Path::to_path_buf))
        {
            look_watch.set_dirs(vec![dir]);
        }
        let menu = menu::ContextMenu::new(&ctx, hwnd);
        let ctx_for_apps = ctx.clone();
        let mut app = Self {
            watcher: shellman_core::watch::Watcher::new(ctx.clone()),
            watched: Vec::new(),
            ctx,
            settings,
            hwnd,
            panel,
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
            thumbs,
            grid_cols: 1,
            grid_line_h: 200.0,
            bin_dirs: Vec::new(),
            had_focus: true,
            menu,
            menu_sig: None,
            menu_settle_target: None,
            apps: places::Registered::new(&ctx_for_apps),
            pin_exists: pins::Exists::default(),
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
        self.bin_sort(i);
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
        if tab.loc == Loc::Bin {
            return self.load_bin(i);
        }
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
                Msg::Bin {
                    tab,
                    gen,
                    dirs,
                    result,
                } => {
                    self.bin_dirs = dirs;
                    self.bin_loaded(tab, gen, result);
                }
                Msg::BinState(state) => self.bin_state_arrived(state),
                Msg::BinDone(result) => {
                    match result {
                        Ok(done) => self.toast(done),
                        Err(e) => self.toast(format!("Recycle Bin: {e}")),
                    }
                    self.reload_bins();
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
        // The bin's own folders, so a delete from anywhere shows up in an open bin tab.
        if self.tabs.iter().any(|t| t.loc == Loc::Bin) {
            let extra: Vec<PathBuf> = self
                .bin_dirs
                .iter()
                .filter(|d| !dirs.contains(d))
                .cloned()
                .collect();
            dirs.extend(extra);
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
        if changed.iter().any(|d| self.bin_dirs.contains(d)) {
            self.reload_bins();
        }
    }

    /// Follow the shared look when its file changes, the Windows app mode, and the Windows
    /// accent while that is the choice.
    fn track_system(&mut self, ctx: &egui::Context) {
        // The glass reads its look from the theme every frame (see `layout`), so nothing here
        // needs to tell it.
        if !self.look_watch.poll().is_empty() {
            let look = shellman_ui::appearance::Appearance::load_or(self.look);
            if look != self.look {
                self.look = look;
                look.apply(ctx);
            }
        }
        theme::set_light(
            std::env::var_os("SHELLMAN_FORCE_LIGHT").is_some() || ctx.theme() == egui::Theme::Light,
        );
        let now = ctx.input(|i| i.time);
        if self.look.accent == AccentChoice::System && now - self.last_accent_check > 2.0 {
            self.last_accent_check = now;
            theme::set_accent(ctx, AccentChoice::System.resolve());
        }
        if self.look.accent == AccentChoice::System {
            ctx.request_repaint_after(std::time::Duration::from_secs(2));
        }
    }

    /// Draw the right-click menu and run what was picked; read ahead the menu for a selection
    /// that has settled.
    fn context_menu(&mut self, ctx: &egui::Context) {
        let mut picked = Vec::new();
        if let Some(notice) = self.menu.show(ctx, &mut picked) {
            self.toast(notice);
        }
        for a in picked {
            self.apply(a);
        }
        let tab = &self.tabs[self.active];
        // A cheap fingerprint of the selection, so the target is only built when it changes.
        let sig = (tab.id, tab.gen, tab.selected.len(), tab.cursor);
        if self.menu_sig != Some(sig) {
            self.menu_sig = Some(sig);
            self.menu_settle_target = if tab.loc == Loc::Bin || tab.selected.len() > 64 {
                None
            } else {
                let row = tab
                    .cursor
                    .filter(|&c| tab.selected.contains(&tab.entry(c).path));
                let row = row.or_else(|| tab.selection().first().and_then(|e| tab.row_of(&e.path)));
                self.menu_target(row)
            };
        }
        let now = ctx.input(|i| i.time);
        if let Some(wait) = self.menu.settle(self.menu_settle_target.clone(), now) {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
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
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Remember where the window is; a maximized one keeps the earlier spot.
        if let Some(rect) = crate::placement::window_rect(self.hwnd) {
            self.settings.window = Some(rect);
            crate::settings::save_settings(&self.settings);
        }
    }

    /// Clear: the desktop shows between the islands.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain();
        self.watch_tabs();
        // The bin is also re-listed when the window is brought back, in case a change slipped by.
        let focused = ctx.input(|i| i.focused);
        if focused && !self.had_focus {
            self.reload_bins();
        }
        self.had_focus = focused;
        self.track_system(ctx);
        shellman_ui::app_icon::sync_window_icon(ctx, shellman_ui::app_icon::Kind::Files);
        let show_hidden = self.settings.show_hidden;
        for t in &mut self.tabs {
            if t.dirty {
                t.rebuild_view(show_hidden);
            }
        }
        self.thumbs.poll(ctx);
        self.apps.poll(ctx);
        let mut actions = Vec::new();
        self.keyboard(ctx, &mut actions);
        self.take_dropped_files(ctx);

        self.layout(ctx, &mut actions);
        self.dialogs(ctx);

        for a in actions {
            self.apply(a);
        }
        // After the actions, so a menu opened by this frame's right click is drawn in this frame.
        self.context_menu(ctx);

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
