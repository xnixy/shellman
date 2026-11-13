//! The window, as islands of glass with the desktop between them: tabs and the window buttons
//! across the top, the bar with the path under them, the places as a rail on the left, the
//! folder in the middle, and the selection in a pill along the bottom (see `plan`).

mod actions;
mod band;
mod bin;
mod clip;
mod controls;
mod counts;
mod dest;
mod details;
mod details_ui;
mod dialogs;
mod dnd;
mod drag_image;
mod grid;
mod interact;
mod keys;
mod layout;
mod list;
mod menu;
mod pc;
mod pick_band;
mod picker;
mod pins;
mod places;
mod plan;
mod rail;
mod rows;
mod select;
mod tab;
mod tab_bar;
pub(crate) mod tag_ui;
mod text;
mod toolbar;

use actions::{Dialog, Rename};
use shellman_core::fsops::{self, Drive, Entry, Op};
use shellman_core::recycle_bin::{BinItem, BinState};
use shellman_core::thumbs::Thumbs;
use shellman_ui::theme::{self, AccentChoice};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use tab::{Loc, Tab};

/// One row of the list (the design's 8 px above and below a line of text).
const ROW_H: f32 = 34.0;
/// The corner radius of the islands that are not pills (the list's), and of the item cards.
pub(crate) const RADIUS: f32 = 22.0;

/// A cheap fingerprint of a tab's selection: tab, listing, count, cursor and anchor.
type SelSig = (u64, u64, usize, Option<usize>, Option<usize>);

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
    /// A tag place's items, and what checking the tagged items found.
    Tagged {
        tab: u64,
        gen: u64,
        found: Vec<Entry>,
        results: Vec<TagResult>,
    },
    /// What a check of the tagged items found (see `tags::check`).
    TagCheck(Vec<TagResult>),
    OpDone {
        plan: OpPlan,
        result: Result<bool, String>,
        /// Where each item arrived (from, to), for tags to follow a move or a copy.
        arrived: Vec<(PathBuf, PathBuf)>,
    },
    /// A drag out of the window ended.
    DragEnded(shellman_core::transfer::drag::Ended),
    /// Files read from the clipboard for a paste into `dir`.
    PasteFiles {
        paths: Vec<PathBuf>,
        effect: u32,
        seq: u32,
        dir: PathBuf,
    },
    /// A file made from a pasted or dropped picture or text.
    Created(Result<PathBuf, String>),
    Toast(String),
    /// The folder picker for Move to… or Copy to… answered.
    Chosen {
        op: Op,
        items: Vec<PathBuf>,
        dest: PathBuf,
    },
    /// The details island's reading of the selection.
    Details(details::Reading),
}

/// Where each of a plan's items arrived, as the disk shows it afterwards: (from, to) for those
/// that did. A moved item is gone from where it was and at its destination; a copy is at its
/// destination. `renamed` are the items the shell put under another name.
fn arrivals(plan: &OpPlan, renamed: &[(PathBuf, PathBuf)]) -> Vec<(PathBuf, PathBuf)> {
    use shellman_core::transfer::path_key as key;
    let Some(dir) = &plan.to else { return Vec::new() };
    plan.from
        .iter()
        .filter_map(|src| {
            let plain = dir.join(src.file_name()?);
            // The shell names either the item or the place it would have had.
            let to = renamed
                .iter()
                .find(|(a, _)| key(a) == key(src) || key(a) == key(&plain))
                .map_or(plain, |(_, b)| b.clone());
            let arrived = match plan.op {
                Op::Move => !src.exists() && to.exists(),
                Op::Copy | Op::Duplicate => key(&to) != key(src) && to.exists(),
                Op::Recycle => false,
            };
            arrived.then(|| (src.clone(), to))
        })
        .collect()
}

/// One item's check: its key in the tag store, its path when the check began, what was found.
type TagResult = (String, PathBuf, crate::tags::check::Verdict);

/// A shell operation to run, and what to do after it.
pub(super) struct OpPlan {
    op: Op,
    what: &'static str,
    from: Vec<PathBuf>,
    to: Option<PathBuf>,
    /// After a cut's files were moved: the clipboard's sequence number, to clear it if it still
    /// holds them.
    clear_clip: Option<u32>,
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
    /// The Windows clipboard, as far as files go.
    clip: clip::Clip,
    /// Dragging out, and the window as a drop target.
    dnd: dnd::Dnd,
    /// The background check of tagged items.
    tag_check: tag_ui::Checking,
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
    /// Picker mode: the window is another app's file dialog (see `picker`). None when browsing.
    pick: Option<picker::Picker>,
    /// The content island as last laid out: the rubber band's ground.
    content_island: egui::Rect,
    /// Folder cards' item counts.
    counts: counts::Counts,
    tags: tag_ui::Tags,
    /// How far the tabs are scrolled sideways, and the (active tab, tab count) last shown, to
    /// bring the active tab into view when either changes.
    tab_scroll: f32,
    tab_seen: (usize, usize),
    /// The open Move to… / Copy to… list.
    dest: Option<dest::DestMenu>,
    /// The details island's state and its worker.
    details: details::Details,
    /// The tag list shows its names as fields, to rename them.
    tag_edit: bool,
    /// The selection as the details island last worked it out, and its fingerprint.
    details_sel: (Option<SelSig>, Vec<Entry>),
}

impl FileMan {
    pub(crate) fn new(
        ctx: egui::Context,
        settings: crate::settings::Settings,
        look: shellman_ui::appearance::Appearance,
        start: crate::args::Start,
        pick: Option<crate::pick::Session>,
        hwnd: isize,
        panel: Option<shellman_ui::glass::GlassPanel>,
    ) -> Self {
        let (tx, rx) = channel();
        let picking = pick.is_some();
        let mut settings = settings;
        let pick = pick.map(|session| {
            if session.req.show_hidden {
                settings.show_hidden = true;
            }
            picker::Picker::new(session)
        });
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
            // Only the one place the picker opens on (below).
            crate::args::Start::Pick(_) => {}
        }
        let pick = pick.map(|(p, loc)| {
            locs = vec![loc];
            p
        });
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
            clip: clip::Clip::new(&ctx_for_apps),
            dnd: dnd::Dnd::new(&ctx_for_apps, hwnd, picking),
            tag_check: tag_ui::Checking::default(),
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
            pick,
            content_island: egui::Rect::NOTHING,
            counts: counts::Counts::new(&ctx_for_apps),
            tags: tag_ui::Tags::load(),
            tab_scroll: 0.0,
            tab_seen: (usize::MAX, 0),
            dest: None,
            details: details::Details::default(),
            tag_edit: false,
            details_sel: (None, Vec::new()),
        };
        for loc in locs {
            app.push_tab(loc);
        }
        app.active = 0;
        app.pick_apply_filter();
        app.load_drives();
        // Learn the tagged items' identities, and find any renamed or moved while Files was closed.
        app.want_tag_check();
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
        if let Loc::Tag(t) = tab.loc {
            // Every item with the tag, where it is now: they are checked first (renamed or moved
            // by anyone, saved anew, gone), on the worker, and listed at their current paths.
            let bit = 1u8 << t;
            let items: Vec<(String, crate::tags::Item)> = self
                .tags
                .store
                .items
                .iter()
                .filter(|(_, it)| it.tags & bit != 0)
                .map(|(k, it)| (k.clone(), it.clone()))
                .collect();
            let (tx, ctx, id, gen) = (self.tx.clone(), self.ctx.clone(), tab.id, tab.gen);
            std::thread::spawn(move || {
                use crate::tags::check;
                let results = check::run(items, &check::RealDisk);
                let found = results
                    .iter()
                    .filter_map(|(_, path, v)| check::now_at(path, v))
                    .filter_map(|p| fsops::entry_for(&p))
                    .collect();
                let _ = tx.send(Msg::Tagged { tab: id, gen, found, results });
                ctx.request_repaint();
            });
            return;
        }
        if let Some(dir) = tab.loc.dir() {
            self.counts.forget_in(dir);
        }
        let tab = &mut self.tabs[i];
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
        self.run_plan(OpPlan {
            op,
            what,
            from,
            to,
            clear_clip: None,
        });
    }

    /// Run a shell operation on a worker. Afterwards the worker works out where each item
    /// arrived (under its own name, or the one a conflict gave it), for the tags to follow.
    fn run_plan(&self, plan: OpPlan) {
        let (tx, ctx, hwnd) = (self.tx.clone(), self.ctx.clone(), self.hwnd);
        std::thread::spawn(move || {
            let result = fsops::shell_op_mapped(hwnd, plan.op, &plan.from, plan.to.as_deref());
            let renamed = result.as_ref().map(|(_, r)| r.clone()).unwrap_or_default();
            let arrived = arrivals(&plan, &renamed);
            if let (Ok((true, _)), Some(seq)) = (&result, plan.clear_clip) {
                shellman_core::transfer::clip::clear_if(seq);
            }
            let result = result.map(|(done, _)| done);
            let _ = tx.send(Msg::OpDone { plan, result, arrived });
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
                    // A folder changed: a tagged item in it may have been renamed by another app.
                    self.want_tag_check();
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
                Msg::Tagged {
                    tab,
                    gen,
                    found,
                    results,
                } => {
                    let show_hidden = self.settings.show_hidden;
                    if let Some(t) = self.tabs.iter_mut().find(|t| t.id == tab && t.gen == gen) {
                        t.loading = false;
                        t.set_entries(found);
                        t.rebuild_view(show_hidden);
                    }
                    // The listing came from this check: no need to list again.
                    self.tag_results(&results, false);
                }
                Msg::TagCheck(results) => {
                    self.tag_check.running = false;
                    self.tag_results(&results, true);
                }
                Msg::Chosen { op, items, dest } => self.send_to(op, items, dest),
                Msg::Details(reading) => self.details.arrived(reading),
                Msg::DragEnded(ended) => {
                    self.dnd.dragging_out = false;
                    // A target that moved the items (or will): the folders change.
                    if !ended.cancelled && (ended.effect | ended.performed) & shellman_core::transfer::MOVE != 0 {
                        self.refresh_all();
                        self.want_tag_check();
                    }
                }
                Msg::PasteFiles { paths, effect, seq, dir } => self.paste_files(paths, effect, seq, dir),
                Msg::Created(Ok(path)) => self.select_new(path, true),
                Msg::Created(Err(e)) => self.toast(format!("Couldn't make the file: {e}")),
                Msg::Toast(t) => self.toast(t),
                Msg::OpDone { plan, result, arrived } => {
                    // Tags follow what a move took and go with what a copy made, where each
                    // arrived (an operation stopped partway did only some).
                    let mut retagged = false;
                    for (from, to) in &arrived {
                        retagged |= match plan.op {
                            Op::Move => self.tags.store.rekey(from, to),
                            Op::Copy | Op::Duplicate => self.tags.store.copy_tree(from, to),
                            Op::Recycle => false,
                        };
                    }
                    if retagged {
                        self.tags_changed();
                    }
                    let what = plan.what;
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
        let forced = std::env::var_os("SHELLMAN_FORCE_LIGHT").is_some();
        // Forced light: egui's own popups and fields take the light look too.
        if forced && ctx.theme() != egui::Theme::Light {
            ctx.set_theme(egui::ThemePreference::Light);
        }
        theme::set_light(forced || ctx.theme() == egui::Theme::Light);
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
        if self.menu.is_open() {
            self.tag_menu_state();
        }
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
}

impl eframe::App for FileMan {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // A picker leaves Files' own settings alone; closing it unanswered is a cancel.
        if self.pick.is_some() {
            self.pick_closing();
            return;
        }
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
            // Tagged items may have been renamed or moved meanwhile.
            self.want_tag_check();
        }
        self.had_focus = focused;
        self.clip.poll();
        self.run_tag_check(ctx);
        self.track_system(ctx);
        shellman_ui::app_icon::sync_window_icon(ctx, shellman_ui::app_icon::Kind::Files);
        let show_hidden = self.settings.show_hidden;
        for t in &mut self.tabs {
            if t.dirty {
                t.rebuild_view(show_hidden);
            }
        }
        self.thumbs.poll(ctx);
        self.counts.poll();
        self.apps.poll(ctx);
        if self.pick.is_some() {
            self.pick_follow_selection();
            self.pick_poll();
        }
        let mut actions = Vec::new();
        self.pick_keys(ctx, &mut actions);
        self.keyboard(ctx, &mut actions);
        self.dnd_begin(ctx, &mut actions);

        self.layout(ctx, &mut actions);
        self.dialogs(ctx);

        for a in actions {
            self.apply(a);
        }
        // After the actions, so a menu opened by this frame's right click is drawn in this frame.
        self.context_menu(ctx);
        self.dnd_end(ctx);

        let title = match self.picker() {
            Some(p) => crate::pick::button::title(&p.session.req),
            None => format!("{} - Files", self.tabs[self.active].loc.title()),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        // A picker's changes (the view, hidden items) last only while it is open.
        if std::mem::take(&mut self.save_pending) && self.pick.is_none() {
            self.settings.tabs = self
                .tabs
                .iter()
                .filter_map(|t| t.loc.dir().map(Path::to_path_buf))
                .collect();
            crate::settings::save_settings(&self.settings);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(op: Op, from: Vec<PathBuf>, to: &Path) -> OpPlan {
        OpPlan {
            op,
            what: "",
            from,
            to: Some(to.to_path_buf()),
            clear_clip: None,
        }
    }

    #[test]
    fn arrivals_are_read_off_the_disk() {
        let root = std::env::temp_dir().join(format!("files-arrivals-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (src, dst) = (root.join("src"), root.join("dst"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        let (a, b) = (src.join("a.txt"), src.join("b.txt"));
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        // A copy: a arrived under its own name, b under the name a conflict gave it.
        std::fs::write(dst.join("a.txt"), "a").unwrap();
        std::fs::write(dst.join("b (2).txt"), "b").unwrap();
        let renamed = vec![(dst.join("b.txt"), dst.join("b (2).txt"))];
        let both = vec![a.clone(), b.clone()];
        assert_eq!(
            arrivals(&plan(Op::Copy, both.clone(), &dst), &renamed),
            vec![(a.clone(), dst.join("a.txt")), (b.clone(), dst.join("b (2).txt"))]
        );
        // A move counts only what left its folder.
        std::fs::remove_file(&a).unwrap();
        assert_eq!(
            arrivals(&plan(Op::Move, both, &dst), &renamed),
            vec![(a.clone(), dst.join("a.txt"))]
        );
        // A copy beside the original, named by the shell.
        std::fs::write(src.join("b - Copy.txt"), "b").unwrap();
        let dup = plan(Op::Duplicate, vec![b.clone()], &src);
        assert_eq!(
            arrivals(&dup, &[(b.clone(), src.join("b - Copy.txt"))]),
            vec![(b.clone(), src.join("b - Copy.txt"))]
        );
        assert!(arrivals(&dup, &[]).is_empty(), "no name from the shell: nothing to tag");
        let _ = std::fs::remove_dir_all(&root);
    }
}
