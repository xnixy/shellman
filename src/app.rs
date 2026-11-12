//! The window: tabs and a toolbar across the top, places on the left, the folder in the middle.

use crate::fsops::{self, Drive, Entry, Op};
use crate::theme::{self, colors, glyph, icon, icon_font, AccentChoice};
use crate::Settings;
use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat, TextWrapping};
use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Id, Key, Layout, Modifiers, PointerButton, Rect,
    RichText, ScrollArea, Sense, Stroke, Ui, Vec2, WidgetText,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

const ROW_H: f32 = 30.0;
const HEADER_H: f32 = 28.0;
const SIDEBAR_W: f32 = 210.0;
const TOAST_SECS: f64 = 3.0;

#[derive(Clone, PartialEq, Debug)]
pub enum Loc {
    Pc,
    Dir(PathBuf),
}

impl Loc {
    fn title(&self) -> String {
        match self {
            Loc::Pc => "This PC".into(),
            Loc::Dir(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.display().to_string()),
        }
    }

    fn dir(&self) -> Option<&Path> {
        match self {
            Loc::Pc => None,
            Loc::Dir(p) => Some(p),
        }
    }

    fn parent(&self) -> Option<Loc> {
        match self {
            Loc::Pc => None,
            Loc::Dir(p) => Some(p.parent().map_or(Loc::Pc, |q| Loc::Dir(q.to_path_buf()))),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum SortKey {
    Name,
    Modified,
    Kind,
    Size,
}

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

struct Tab {
    id: u64,
    loc: Loc,
    back: Vec<Loc>,
    fwd: Vec<Loc>,
    entries: Vec<Entry>,
    /// Indexes into `entries`, filtered and sorted.
    view: Vec<usize>,
    dirty: bool,
    loading: bool,
    error: Option<String>,
    gen: u64,
    selected: HashSet<PathBuf>,
    anchor: Option<usize>,
    cursor: Option<usize>,
    filter: String,
    sort: SortKey,
    ascending: bool,
    scroll_to_cursor: bool,
    list_offset: f32,
    list_height: f32,
    /// After the next load, select this item, and maybe start renaming it.
    select_after_load: Option<(PathBuf, bool)>,
}

impl Tab {
    fn new(id: u64, loc: Loc) -> Self {
        Self {
            id,
            loc,
            back: Vec::new(),
            fwd: Vec::new(),
            entries: Vec::new(),
            view: Vec::new(),
            dirty: false,
            loading: false,
            error: None,
            gen: 0,
            selected: HashSet::new(),
            anchor: None,
            cursor: None,
            filter: String::new(),
            sort: SortKey::Name,
            ascending: true,
            scroll_to_cursor: false,
            list_offset: 0.0,
            list_height: 0.0,
            select_after_load: None,
        }
    }

    fn rebuild_view(&mut self, show_hidden: bool) {
        let needle = self.filter.trim().to_lowercase();
        let entries = &self.entries;
        let mut view: Vec<usize> = (0..entries.len())
            .filter(|&i| {
                let e = &entries[i];
                (show_hidden || !e.hidden) && (needle.is_empty() || e.key.contains(&needle))
            })
            .collect();
        let (key, asc) = (self.sort, self.ascending);
        view.sort_by(|&a, &b| {
            let (a, b) = (&entries[a], &entries[b]);
            // Folders always come first, whichever way the list is sorted.
            b.is_dir.cmp(&a.is_dir).then_with(|| {
                let ord = match key {
                    SortKey::Name => std::cmp::Ordering::Equal,
                    SortKey::Modified => a.modified.cmp(&b.modified),
                    SortKey::Kind => a.kind.cmp(&b.kind),
                    SortKey::Size => a.size.cmp(&b.size),
                }
                .then_with(|| fsops::natural_cmp(&a.key, &b.key));
                if asc {
                    ord
                } else {
                    ord.reverse()
                }
            })
        });
        self.view = view;
        self.dirty = false;
        self.selected
            .retain(|p| self.view.iter().any(|&i| &entries[i].path == p));
        self.cursor = self.cursor.filter(|&c| c < self.view.len());
        self.anchor = self.anchor.filter(|&c| c < self.view.len());
    }

    fn entry(&self, row: usize) -> &Entry {
        &self.entries[self.view[row]]
    }

    fn row_of(&self, path: &Path) -> Option<usize> {
        (0..self.view.len()).find(|&r| self.entry(r).path == path)
    }

    /// Selected items in list order.
    fn selection(&self) -> Vec<Entry> {
        (0..self.view.len())
            .map(|r| self.entry(r))
            .filter(|e| self.selected.contains(&e.path))
            .cloned()
            .collect()
    }

    fn select_only(&mut self, row: usize) {
        self.selected.clear();
        self.selected.insert(self.entry(row).path.clone());
        self.cursor = Some(row);
        self.anchor = Some(row);
    }

    fn select_range(&mut self, to: usize) {
        let from = self.anchor.unwrap_or(to);
        self.selected.clear();
        for r in from.min(to)..=from.max(to) {
            self.selected.insert(self.entry(r).path.clone());
        }
        self.cursor = Some(to);
    }
}

struct Rename {
    path: PathBuf,
    text: String,
    focus: bool,
}

enum Dialog {
    Settings,
    ConfirmRecycle(Vec<PathBuf>),
}

#[derive(Debug)]
enum Action {
    Navigate(Loc),
    NewTab(Loc),
    CloseTab(usize),
    SwitchTab(usize),
    Back,
    Forward,
    Up,
    Refresh,
    OpenRow(usize),
    OpenSelected,
    Rename,
    Recycle,
    Clip(Op),
    Paste,
    NewFolder,
    CopyPath,
    Reveal,
    ExplorerHere,
    SelectAll,
    Settings,
}

pub struct FileMan {
    ctx: egui::Context,
    settings: Settings,
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
    last_accent_check: f64,
    toast: Option<(String, f64)>,
    save_pending: bool,
    title: String,
}

/// The places shown in the sidebar and on This PC.
fn quick_places() -> Vec<(char, &'static str, PathBuf)> {
    [
        (glyph::HOME, "Home", dirs::home_dir()),
        (glyph::DESKTOP, "Desktop", dirs::desktop_dir()),
        (glyph::DOWNLOADS, "Downloads", dirs::download_dir()),
        (glyph::DOCUMENTS, "Documents", dirs::document_dir()),
        (glyph::PICTURES, "Pictures", dirs::picture_dir()),
        (glyph::MUSIC, "Music", dirs::audio_dir()),
        (glyph::VIDEOS, "Videos", dirs::video_dir()),
    ]
    .into_iter()
    .filter_map(|(g, n, p)| p.map(|p| (g, n, p)))
    .collect()
}

impl FileMan {
    pub fn new(
        ctx: egui::Context,
        settings: Settings,
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
        let mut app = Self {
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
            last_accent_check: 0.0,
            toast: None,
            save_pending: false,
            title: String::new(),
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
            let _ = tx.send(Msg::Loaded { tab: id, gen, result });
            ctx.request_repaint();
        });
    }

    fn navigate(&mut self, loc: Loc) {
        let tab = &mut self.tabs[self.active];
        if tab.loc == loc {
            self.load(self.active);
            return;
        }
        let old = std::mem::replace(&mut tab.loc, loc);
        tab.back.push(old);
        tab.fwd.clear();
        self.arrive();
    }

    /// Reset the active tab for the folder it now shows, and list it.
    fn arrive(&mut self) {
        let tab = &mut self.tabs[self.active];
        tab.entries.clear();
        tab.view.clear();
        tab.selected.clear();
        tab.cursor = None;
        tab.anchor = None;
        tab.filter.clear();
        tab.list_offset = 0.0;
        self.rename = None;
        self.editing_path = None;
        self.save_pending = true;
        self.load(self.active);
    }

    fn toast(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), self.ctx.input(|i| i.time)));
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

    fn drain(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Loaded { tab, gen, result } => {
                    let show_hidden = self.settings.show_hidden;
                    let Some(t) = self.tabs.iter_mut().find(|t| t.id == tab && t.gen == gen)
                    else {
                        continue;
                    };
                    t.loading = false;
                    match result {
                        Ok(entries) => {
                            t.entries = entries;
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
                                self.rename = Some(Rename { path, text, focus: true });
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

    /// Follow the Windows app mode, and the Windows accent while that is the choice.
    fn track_system(&mut self, ctx: &egui::Context) {
        let mut repaint_frame = theme::set_light(ctx.theme() == egui::Theme::Light);
        let now = ctx.input(|i| i.time);
        if self.settings.accent == AccentChoice::System && now - self.last_accent_check > 2.0 {
            self.last_accent_check = now;
            repaint_frame |= theme::set_accent(ctx, AccentChoice::System.resolve());
        }
        if repaint_frame {
            crate::paint_frame(self.hwnd);
        }
        if self.settings.accent == AccentChoice::System {
            ctx.request_repaint_after(std::time::Duration::from_secs(2));
        }
    }

    fn set_accent(&mut self, choice: AccentChoice) {
        self.settings.accent = choice;
        theme::set_accent(&self.ctx, choice.resolve());
        crate::paint_frame(self.hwnd);
        self.save_pending = true;
    }

    fn keyboard(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        if self.dialog.is_some() {
            return;
        }
        let typing = ctx.wants_keyboard_input();
        let (mouse_back, mouse_fwd) = ctx.input(|i| {
            (
                i.pointer.button_pressed(PointerButton::Extra1),
                i.pointer.button_pressed(PointerButton::Extra2),
            )
        });
        ctx.input_mut(|i| {
            let mut key = |m: Modifiers, k: Key| i.consume_key(m, k);
            if key(Modifiers::CTRL, Key::L) {
                self.focus_path = true;
            }
            if key(Modifiers::CTRL, Key::F) {
                self.focus_search = true;
            }
            if key(Modifiers::CTRL, Key::T) {
                actions.push(Action::NewTab(self.tabs[self.active].loc.clone()));
            }
            if key(Modifiers::CTRL, Key::W) {
                actions.push(Action::CloseTab(self.active));
            }
            if key(Modifiers::CTRL, Key::Tab) {
                actions.push(Action::SwitchTab((self.active + 1) % self.tabs.len()));
            }
            if key(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab) {
                let n = self.tabs.len();
                actions.push(Action::SwitchTab((self.active + n - 1) % n));
            }
            if key(Modifiers::ALT, Key::ArrowLeft) || mouse_back
            {
                actions.push(Action::Back);
            }
            if key(Modifiers::ALT, Key::ArrowRight) || mouse_fwd
            {
                actions.push(Action::Forward);
            }
            if key(Modifiers::ALT, Key::ArrowUp) {
                actions.push(Action::Up);
            }
            if key(Modifiers::NONE, Key::F5) {
                actions.push(Action::Refresh);
            }
            if key(Modifiers::CTRL | Modifiers::SHIFT, Key::N) {
                actions.push(Action::NewFolder);
            }
            if typing {
                return;
            }
            if key(Modifiers::NONE, Key::Backspace) {
                actions.push(Action::Back);
            }
            if key(Modifiers::NONE, Key::Enter) {
                actions.push(Action::OpenSelected);
            }
            if key(Modifiers::NONE, Key::F2) {
                actions.push(Action::Rename);
            }
            if key(Modifiers::NONE, Key::Delete) {
                actions.push(Action::Recycle);
            }
            if key(Modifiers::CTRL, Key::A) {
                actions.push(Action::SelectAll);
            }
            if key(Modifiers::CTRL, Key::C) {
                actions.push(Action::Clip(Op::Copy));
            }
            if key(Modifiers::CTRL, Key::X) {
                actions.push(Action::Clip(Op::Move));
            }
            if key(Modifiers::CTRL, Key::V) {
                actions.push(Action::Paste);
            }
            if key(Modifiers::CTRL | Modifiers::SHIFT, Key::C) {
                actions.push(Action::CopyPath);
            }
            // Arrow keys, Home and End move the cursor; Shift extends the selection.
            let tab = &mut self.tabs[self.active];
            let n = tab.view.len();
            if n == 0 {
                return;
            }
            let page = ((tab.list_height / ROW_H) as usize).max(1);
            let cur = tab.cursor;
            let mut target = None;
            for shift in [false, true] {
                let m = if shift { Modifiers::SHIFT } else { Modifiers::NONE };
                let t = if i.consume_key(m, Key::ArrowDown) {
                    Some(cur.map_or(0, |c| (c + 1).min(n - 1)))
                } else if i.consume_key(m, Key::ArrowUp) {
                    Some(cur.map_or(0, |c| c.saturating_sub(1)))
                } else if i.consume_key(m, Key::PageDown) {
                    Some(cur.map_or(0, |c| (c + page).min(n - 1)))
                } else if i.consume_key(m, Key::PageUp) {
                    Some(cur.map_or(0, |c| c.saturating_sub(page)))
                } else if i.consume_key(m, Key::Home) {
                    Some(0)
                } else if i.consume_key(m, Key::End) {
                    Some(n - 1)
                } else {
                    None
                };
                if let Some(t) = t {
                    target = Some((t, shift));
                }
            }
            if let Some((t, shift)) = target {
                if shift {
                    tab.select_range(t);
                } else {
                    tab.select_only(t);
                }
                tab.scroll_to_cursor = true;
            }
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                tab.selected.clear();
            }
        });
    }

    fn apply(&mut self, action: Action) {
        let show_hidden = self.settings.show_hidden;
        match action {
            Action::Navigate(loc) => self.navigate(loc),
            Action::NewTab(loc) => {
                self.active = self.push_tab(loc);
                self.save_pending = true;
            }
            Action::CloseTab(i) => {
                if self.tabs.len() > 1 && i < self.tabs.len() {
                    self.tabs.remove(i);
                    if self.active > i || self.active >= self.tabs.len() {
                        self.active = self.active.saturating_sub(1);
                    }
                    self.rename = None;
                    self.save_pending = true;
                }
            }
            Action::SwitchTab(i) => {
                if i < self.tabs.len() {
                    self.active = i;
                    self.rename = None;
                    self.editing_path = None;
                }
            }
            Action::Back => {
                let tab = &mut self.tabs[self.active];
                if let Some(prev) = tab.back.pop() {
                    let cur = std::mem::replace(&mut tab.loc, prev);
                    tab.fwd.push(cur.clone());
                    self.arrive();
                    // Land on the folder we came out of.
                    if let Loc::Dir(p) = cur {
                        self.tabs[self.active].select_after_load = Some((p, false));
                    }
                }
            }
            Action::Forward => {
                let tab = &mut self.tabs[self.active];
                if let Some(next) = tab.fwd.pop() {
                    let cur = std::mem::replace(&mut tab.loc, next);
                    tab.back.push(cur);
                    self.arrive();
                }
            }
            Action::Up => {
                let cur = self.tabs[self.active].loc.clone();
                if let Some(parent) = cur.parent() {
                    self.navigate(parent);
                    if let Loc::Dir(p) = cur {
                        self.tabs[self.active].select_after_load = Some((p, false));
                    }
                }
            }
            Action::Refresh => self.load(self.active),
            Action::OpenRow(r) => {
                let e = self.tabs[self.active].entry(r).clone();
                self.open_entry(&e, false);
            }
            Action::OpenSelected => {
                let sel = self.tabs[self.active].selection();
                let many = sel.len() > 1;
                for e in &sel {
                    self.open_entry(e, many);
                }
            }
            Action::Rename => {
                let tab = &self.tabs[self.active];
                let row = tab.cursor.filter(|&c| tab.selected.contains(&tab.entry(c).path));
                let entry = row.map(|r| tab.entry(r)).or_else(|| {
                    tab.selection().first().and_then(|e| tab.row_of(&e.path).map(|r| tab.entry(r)))
                });
                if let Some(e) = entry {
                    self.rename = Some(Rename {
                        path: e.path.clone(),
                        text: e.name.clone(),
                        focus: true,
                    });
                }
            }
            Action::Recycle => {
                let paths: Vec<PathBuf> =
                    self.tabs[self.active].selection().into_iter().map(|e| e.path).collect();
                if !paths.is_empty() {
                    self.dialog = Some(Dialog::ConfirmRecycle(paths));
                }
            }
            Action::Clip(op) => {
                let paths: Vec<PathBuf> =
                    self.tabs[self.active].selection().into_iter().map(|e| e.path).collect();
                if !paths.is_empty() {
                    let n = paths.len();
                    self.clipboard = Some((op, paths));
                    let verb = if op == Op::Move { "Cut" } else { "Copied" };
                    self.toast(format!("{verb} {n} item{}", if n == 1 { "" } else { "s" }));
                }
            }
            Action::Paste => {
                let Some(dir) = self.tabs[self.active].loc.dir().map(Path::to_path_buf) else {
                    return;
                };
                if let Some((op, paths)) = self.clipboard.clone() {
                    let same_dir = paths.iter().all(|p| p.parent() == Some(dir.as_path()));
                    let (op, what) = match op {
                        Op::Move if same_dir => return,
                        Op::Move => (Op::Move, "Move"),
                        _ if same_dir => (Op::Duplicate, "Copy"),
                        _ => (Op::Copy, "Copy"),
                    };
                    if op == Op::Move {
                        self.clipboard = None;
                    }
                    self.run_op(op, what, paths, Some(dir));
                }
            }
            Action::NewFolder => {
                let Some(dir) = self.tabs[self.active].loc.dir().map(Path::to_path_buf) else {
                    return;
                };
                let path = fsops::unique_child(&dir, "New folder");
                match std::fs::create_dir(&path) {
                    Ok(()) => {
                        self.tabs[self.active].select_after_load = Some((path, true));
                        self.load(self.active);
                    }
                    Err(e) => self.toast(format!("Couldn't make a folder: {e}")),
                }
            }
            Action::CopyPath => {
                let tab = &self.tabs[self.active];
                let sel = tab.selection();
                let text = if sel.is_empty() {
                    tab.loc.dir().map(|d| d.display().to_string()).unwrap_or_default()
                } else {
                    sel.iter()
                        .map(|e| format!("\"{}\"", e.path.display()))
                        .collect::<Vec<_>>()
                        .join(" ")
                };
                if !text.is_empty() {
                    self.ctx.copy_text(text);
                    self.toast("Path copied");
                }
            }
            Action::Reveal => {
                if let Some(e) = self.tabs[self.active].selection().first() {
                    fsops::reveal_in_explorer(&e.path);
                }
            }
            Action::ExplorerHere => {
                if let Some(d) = self.tabs[self.active].loc.dir() {
                    fsops::open_in_explorer(d);
                }
            }
            Action::SelectAll => {
                let tab = &mut self.tabs[self.active];
                tab.selected = tab.view.iter().map(|&i| tab.entries[i].path.clone()).collect();
            }
            Action::Settings => self.dialog = Some(Dialog::Settings),
        }
        let _ = show_hidden;
    }

    fn open_entry(&mut self, e: &Entry, many: bool) {
        if e.is_dir {
            if many {
                self.push_tab(Loc::Dir(e.path.clone()));
            } else {
                self.navigate(Loc::Dir(e.path.clone()));
            }
        } else {
            fsops::open(&e.path);
        }
    }

    fn commit_rename(&mut self) {
        let Some(r) = self.rename.take() else { return };
        let new_name = r.text.trim();
        let old_name = r.path.file_name().map(|n| n.to_string_lossy().into_owned());
        if Some(new_name) == old_name.as_deref() {
            return;
        }
        if let Err(why) = fsops::check_name(new_name) {
            self.toast(why);
            return;
        }
        let Some(parent) = r.path.parent() else { return };
        let target = parent.join(new_name);
        // A change of case only is allowed even though the target "exists".
        let case_only = old_name.is_some_and(|o| o.eq_ignore_ascii_case(new_name));
        if target.exists() && !case_only {
            self.toast(format!("There is already an item named {new_name}"));
            return;
        }
        match std::fs::rename(&r.path, &target) {
            Ok(()) => {
                self.tabs[self.active].select_after_load = Some((target, false));
                self.load(self.active);
            }
            Err(e) => self.toast(format!("Couldn't rename: {e}")),
        }
    }

    // ---------------------------------------------------------------- drawing

    fn tab_strip(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let c = colors();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (i, tab) in self.tabs.iter().enumerate() {
                let active = i == self.active;
                let mut title = tab.loc.title();
                if title.chars().count() > 24 {
                    title = title.chars().take(22).collect::<String>() + "…";
                }
                let g = if tab.loc == Loc::Pc { glyph::PC } else { glyph::FOLDER };
                let text = icon_text(g, &title, if active { c.text } else { c.muted }, 13.0);
                let fill = if active { c.card } else { Color32::TRANSPARENT };
                let resp = ui.add(
                    egui::Button::new(text)
                        .fill(fill)
                        .stroke(if active { Stroke::new(1.0_f32, c.hairline) } else { Stroke::NONE })
                        .corner_radius(CornerRadius::same(8))
                        .min_size(Vec2::new(120.0, 30.0)),
                );
                if active {
                    let r = resp.rect;
                    ui.painter().rect_filled(
                        Rect::from_min_max(
                            egui::pos2(r.left() + 12.0, r.bottom() - 3.0),
                            egui::pos2(r.right() - 12.0, r.bottom() - 1.0),
                        ),
                        CornerRadius::same(1),
                        c.accent,
                    );
                }
                if resp.clicked() {
                    actions.push(Action::SwitchTab(i));
                }
                if resp.middle_clicked() {
                    actions.push(Action::CloseTab(i));
                }
                resp.context_menu(|ui| {
                    if menu_item(ui, glyph::ADD, "Duplicate tab", "") {
                        actions.push(Action::NewTab(tab.loc.clone()));
                        ui.close_menu();
                    }
                    if self.tabs.len() > 1 && menu_item(ui, glyph::CLOSE, "Close tab", "Ctrl+W") {
                        actions.push(Action::CloseTab(i));
                        ui.close_menu();
                    }
                });
                if self.tabs.len() > 1 && active {
                    if theme::icon_button(ui, glyph::CLOSE, true, "Close tab (Ctrl+W)").clicked() {
                        actions.push(Action::CloseTab(i));
                    }
                }
            }
            if theme::icon_button(ui, glyph::ADD, true, "New tab (Ctrl+T)").clicked() {
                actions.push(Action::NewTab(self.tabs[self.active].loc.clone()));
            }
        });
    }

    fn toolbar(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let c = colors();
        let tab = &self.tabs[self.active];
        let (can_back, can_fwd, can_up) =
            (!tab.back.is_empty(), !tab.fwd.is_empty(), tab.loc != Loc::Pc);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if theme::icon_button(ui, glyph::BACK, can_back, "Back (Alt+Left)").clicked() {
                actions.push(Action::Back);
            }
            if theme::icon_button(ui, glyph::FORWARD, can_fwd, "Forward (Alt+Right)").clicked() {
                actions.push(Action::Forward);
            }
            if theme::icon_button(ui, glyph::UP, can_up, "Up (Alt+Up)").clicked() {
                actions.push(Action::Up);
            }
            if theme::icon_button(ui, glyph::REFRESH, true, "Refresh (F5)").clicked() {
                actions.push(Action::Refresh);
            }
            ui.add_space(6.0);
            let search_w = 230.0;
            let path_w = (ui.available_width() - search_w - 12.0).max(120.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::new(path_w, 32.0), Sense::hover());
            self.path_bar(ui, rect, actions);
            ui.add_space(8.0);
            // Search box.
            let tab = &mut self.tabs[self.active];
            let hint = format!("Search {}", tab.loc.title());
            let resp = ui.add(
                egui::TextEdit::singleline(&mut tab.filter)
                    .hint_text(RichText::new(hint).color(c.muted))
                    .desired_width(search_w - 16.0)
                    .margin(Vec2::new(10.0, 7.0)),
            );
            if std::mem::take(&mut self.focus_search) {
                resp.request_focus();
            }
            if resp.changed() {
                tab.dirty = true;
            }
            if resp.has_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
                tab.filter.clear();
                tab.dirty = true;
            }
        });
    }

    /// Breadcrumbs, or a text box for typing a path after Ctrl+L or a click on empty space.
    fn path_bar(&mut self, ui: &mut Ui, rect: Rect, actions: &mut Vec<Action>) {
        let c = colors();
        ui.painter()
            .rect(rect, CornerRadius::same(8), c.well, Stroke::new(1.0_f32, c.hairline), egui::StrokeKind::Inside);
        let loc = self.tabs[self.active].loc.clone();
        if std::mem::take(&mut self.focus_path) && self.editing_path.is_none() {
            self.editing_path = Some(loc.dir().map(|d| d.display().to_string()).unwrap_or_default());
        }
        if let Some(text) = &mut self.editing_path {
            let resp = ui.put(
                rect.shrink2(Vec2::new(4.0, 2.0)),
                egui::TextEdit::singleline(text)
                    .id(Id::new("path-edit"))
                    .frame(false)
                    .margin(Vec2::new(8.0, 6.0))
                    .desired_width(rect.width()),
            );
            if !resp.has_focus() && !resp.lost_focus() {
                resp.request_focus();
                select_all(ui.ctx(), resp.id, text.chars().count());
            }
            if resp.lost_focus() {
                let entered = ui.input(|i| i.key_pressed(Key::Enter));
                let typed = std::mem::take(text);
                self.editing_path = None;
                if entered {
                    let typed = typed.trim().trim_matches('"');
                    let p = PathBuf::from(expand_env(typed));
                    if typed.eq_ignore_ascii_case("this pc") || typed.is_empty() {
                        actions.push(Action::Navigate(Loc::Pc));
                    } else if p.is_dir() {
                        actions.push(Action::Navigate(Loc::Dir(p)));
                    } else if p.is_file() {
                        fsops::open(&p);
                    } else {
                        self.toast(format!("Can't find {typed}"));
                    }
                }
            }
            return;
        }
        // Clicking empty space in the bar switches to typing.
        let bg = ui.interact(rect, Id::new("path-bg"), Sense::click());
        if bg.clicked() {
            self.focus_path = true;
        }
        let mut crumbs: Vec<(String, Loc)> = vec![("This PC".into(), Loc::Pc)];
        if let Some(dir) = loc.dir() {
            let mut parts: Vec<&Path> = dir.ancestors().collect();
            parts.reverse();
            for p in parts {
                let name = match p.file_name() {
                    Some(n) => n.to_string_lossy().into_owned(),
                    None => self
                        .drives
                        .iter()
                        .find(|d| d.root.as_path() == p)
                        .map_or_else(|| p.display().to_string(), Drive::title),
                };
                crumbs.push((name, Loc::Dir(p.to_path_buf())));
            }
        }
        // Keep the deepest crumbs that fit, roughly.
        let budget = rect.width() - 40.0;
        let mut used = 0.0;
        let mut first = crumbs.len();
        while first > 0 {
            let w = crumbs[first - 1].0.chars().count() as f32 * 7.5 + 30.0;
            if used + w > budget && first < crumbs.len() {
                break;
            }
            used += w;
            first -= 1;
        }
        let inner = rect.shrink2(Vec2::new(6.0, 1.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::left_to_right(Align::Center)));
        child.spacing_mut().item_spacing.x = 0.0;
        if first > 0 {
            child.label(RichText::new("…").color(c.muted));
        }
        let last = crumbs.len() - 1;
        for (i, (name, target)) in crumbs.into_iter().enumerate().skip(first) {
            if i > first || first > 0 {
                child.label(icon(glyph::CHEVRON, 9.0).color(c.muted));
            }
            let text = RichText::new(name).color(if i == last { c.text } else { c.muted });
            let resp = child.add(egui::Button::new(text).frame(false).min_size(Vec2::new(0.0, 26.0)));
            if resp.clicked() {
                actions.push(Action::Navigate(target.clone()));
            }
            if resp.middle_clicked() {
                actions.push(Action::NewTab(target));
            }
        }
    }

    fn sidebar(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let c = colors();
        let current = self.tabs[self.active].loc.clone();
        ui.add_space(2.0);
        ui.label(theme::muted("Places").size(12.0));
        for (g, name, path) in quick_places() {
            let loc = Loc::Dir(path);
            let resp = nav_row(ui, g, name, current == loc);
            if resp.clicked() {
                actions.push(Action::Navigate(loc.clone()));
            }
            if resp.middle_clicked() {
                actions.push(Action::NewTab(loc));
            }
        }
        ui.add_space(10.0);
        ui.label(theme::muted("This PC").size(12.0));
        if nav_row(ui, glyph::PC, "This PC", current == Loc::Pc).clicked() {
            actions.push(Action::Navigate(Loc::Pc));
        }
        for d in &self.drives {
            let loc = Loc::Dir(d.root.clone());
            let resp = nav_row(ui, glyph::DRIVE, &d.title(), current == loc);
            if resp.clicked() {
                actions.push(Action::Navigate(loc.clone()));
            }
            if resp.middle_clicked() {
                actions.push(Action::NewTab(loc));
            }
        }
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.add_space(4.0);
            if nav_row(ui, glyph::SETTINGS, "Appearance", false).clicked() {
                actions.push(Action::Settings);
            }
            let _ = c;
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let c = colors();
        let tab = &self.tabs[self.active];
        ui.horizontal(|ui| {
            let mut text = match &tab.loc {
                Loc::Pc => format!("{} drives", self.drives.len()),
                Loc::Dir(_) => {
                    let n = tab.view.len();
                    format!("{n} item{}", if n == 1 { "" } else { "s" })
                }
            };
            if !tab.selected.is_empty() {
                let sel = tab.selection();
                let bytes: u64 = sel.iter().filter(|e| !e.is_dir).map(|e| e.size).sum();
                text += &format!("   ·   {} selected", sel.len());
                if bytes > 0 {
                    text += &format!("  ({})", fsops::format_size(bytes));
                }
            }
            if tab.loading {
                text += "   ·   Loading…";
            }
            ui.label(RichText::new(text).size(12.5).color(c.muted));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let now = ui.input(|i| i.time);
                if let Some((msg, at)) = &self.toast {
                    if now - at < TOAST_SECS {
                        ui.label(RichText::new(msg).size(12.5).color(c.accent_text));
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                    }
                }
                if let Some((op, paths)) = &self.clipboard {
                    let verb = if *op == Op::Move { "cut" } else { "copied" };
                    ui.label(
                        RichText::new(format!("{} {verb}  ·  ", paths.len())).size(12.5).color(c.muted),
                    );
                }
            });
        });
    }

    fn pc_page(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let c = colors();
        ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(8.0);
            ui.label(RichText::new("Places").size(18.0).strong().color(c.text));
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(12.0, 12.0);
                for (g, name, path) in quick_places() {
                    let resp = theme::card(ui, |ui| {
                        ui.set_width(150.0);
                        ui.horizontal(|ui| {
                            ui.label(icon(g, 20.0).color(c.accent_text));
                            ui.label(RichText::new(name).size(14.5).color(c.text));
                        });
                    })
                    .response
                    .interact(Sense::click());
                    hover_outline(ui, &resp);
                    if resp.clicked() {
                        actions.push(Action::Navigate(Loc::Dir(path.clone())));
                    }
                    if resp.middle_clicked() {
                        actions.push(Action::NewTab(Loc::Dir(path)));
                    }
                }
            });
            ui.add_space(18.0);
            ui.label(RichText::new("Drives").size(18.0).strong().color(c.text));
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(12.0, 12.0);
                for d in &self.drives {
                    let resp = theme::card(ui, |ui| {
                        ui.set_width(250.0);
                        ui.horizontal(|ui| {
                            ui.label(icon(glyph::DRIVE, 26.0).color(c.accent_text));
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 4.0;
                                ui.label(RichText::new(d.title()).size(14.5).color(c.text));
                                if d.total > 0 {
                                    let used = 1.0 - d.free as f32 / d.total as f32;
                                    let (bar, _) =
                                        ui.allocate_exact_size(Vec2::new(190.0, 6.0), Sense::hover());
                                    let p = ui.painter();
                                    p.rect_filled(bar, CornerRadius::same(3), c.well);
                                    let mut fill = bar;
                                    fill.set_width(bar.width() * used.clamp(0.0, 1.0));
                                    let col = if used > 0.9 { c.danger } else { c.accent };
                                    p.rect_filled(fill, CornerRadius::same(3), col);
                                    ui.label(theme::muted(format!(
                                        "{} free of {}",
                                        fsops::format_size(d.free),
                                        fsops::format_size(d.total)
                                    )).size(12.0));
                                }
                            });
                        });
                    })
                    .response
                    .interact(Sense::click());
                    hover_outline(ui, &resp);
                    if resp.clicked() {
                        actions.push(Action::Navigate(Loc::Dir(d.root.clone())));
                    }
                    if resp.middle_clicked() {
                        actions.push(Action::NewTab(Loc::Dir(d.root.clone())));
                    }
                }
            });
        });
    }

    fn file_list(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let c = colors();
        let has_clip = self.clipboard.is_some();
        let tab = &mut self.tabs[self.active];
        let rename = &mut self.rename;
        let mut commit = false;
        let mut cancel = false;

        if let Some(err) = &tab.error {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("Can't open this folder").size(16.0).color(c.text));
                ui.label(theme::muted(err.clone()));
            });
            return;
        }

        let width = ui.available_width();
        // Columns: name takes what the others leave. Type goes first when space is short.
        let size_w = 90.0;
        let mod_w = 140.0;
        let kind_w = if width > 620.0 { 130.0 } else { 0.0 };
        let name_w = (width - size_w - mod_w - kind_w).max(160.0);
        let cols = [
            (SortKey::Name, "Name", 0.0, name_w),
            (SortKey::Modified, "Date modified", name_w, mod_w),
            (SortKey::Kind, "Type", name_w + mod_w, kind_w),
            (SortKey::Size, "Size", name_w + mod_w + kind_w, size_w),
        ];

        // Header.
        let (head, _) = ui.allocate_exact_size(Vec2::new(width, HEADER_H), Sense::hover());
        for (key, label, x, w) in cols {
            if w <= 0.0 {
                continue;
            }
            let r = Rect::from_min_size(head.min + Vec2::new(x, 0.0), Vec2::new(w, HEADER_H));
            let resp = ui.interact(r, Id::new(("col", label)), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r.shrink(2.0), CornerRadius::same(6), c.row_hover);
            }
            let right = key == SortKey::Size;
            let text_rect = r.shrink2(Vec2::new(10.0, 0.0));
            let sorted = tab.sort == key;
            draw_text(
                ui,
                text_rect,
                label,
                FontId::proportional(12.5),
                if sorted { c.text } else { c.muted },
                right,
            );
            if sorted {
                let g = if tab.ascending { glyph::SORT_UP } else { glyph::SORT_DOWN };
                let pos = if right {
                    egui::pos2(r.left() + 8.0, r.center().y)
                } else {
                    egui::pos2(r.right() - 10.0, r.center().y)
                };
                let align = if right { Align2::LEFT_CENTER } else { Align2::RIGHT_CENTER };
                ui.painter().text(pos, align, g, icon_font(9.0), c.accent_text);
            }
            if resp.clicked() {
                if tab.sort == key {
                    tab.ascending = !tab.ascending;
                } else {
                    tab.sort = key;
                    tab.ascending = key == SortKey::Name || key == SortKey::Kind;
                }
                tab.dirty = true;
            }
        }
        ui.painter().hline(
            head.x_range(),
            head.bottom(),
            Stroke::new(1.0_f32, c.hairline),
        );

        // Empty space behind the rows: clicks clear the selection, right-click acts on the folder.
        let bg_rect = ui.available_rect_before_wrap();
        let bg = ui.interact(bg_rect, Id::new("list-bg"), Sense::click());
        if bg.clicked() {
            tab.selected.clear();
            tab.cursor = None;
        }
        bg.context_menu(|ui| {
            if menu_item(ui, glyph::NEW_FOLDER, "New folder", "Ctrl+Shift+N") {
                actions.push(Action::NewFolder);
                ui.close_menu();
            }
            if has_clip && menu_item(ui, glyph::PASTE, "Paste", "Ctrl+V") {
                actions.push(Action::Paste);
                ui.close_menu();
            }
            ui.separator();
            if menu_item(ui, glyph::REFRESH, "Refresh", "F5") {
                actions.push(Action::Refresh);
                ui.close_menu();
            }
            if menu_item(ui, glyph::COPY, "Copy folder path", "") {
                tab.selected.clear();
                actions.push(Action::CopyPath);
                ui.close_menu();
            }
            if menu_item(ui, glyph::OPEN_EXTERNAL, "Open in Explorer", "") {
                actions.push(Action::ExplorerHere);
                ui.close_menu();
            }
        });

        if tab.view.is_empty() && !tab.loading {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                let msg = if tab.filter.is_empty() { "This folder is empty" } else { "Nothing matches" };
                ui.label(theme::muted(msg));
            });
            return;
        }

        let mut scroll = ScrollArea::vertical().auto_shrink([false, false]);
        if std::mem::take(&mut tab.scroll_to_cursor) {
            if let Some(cur) = tab.cursor {
                let y = cur as f32 * ROW_H;
                let mut off = tab.list_offset;
                if y < off {
                    off = y;
                } else if y + ROW_H > off + tab.list_height {
                    off = y + ROW_H - tab.list_height;
                }
                scroll = scroll.vertical_scroll_offset(off.max(0.0));
            }
        }
        let n = tab.view.len();
        let mut clicked: Option<(usize, Modifiers)> = None;
        let mut secondary: Option<usize> = None;
        let out = scroll.show_rows(ui, ROW_H, n, |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for r in range {
                let e = &tab.entries[tab.view[r]];
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::click());
                let selected = tab.selected.contains(&e.path);
                let row_rect = rect.shrink2(Vec2::new(2.0, 1.0));
                let p = ui.painter();
                if selected {
                    p.rect_filled(row_rect, CornerRadius::same(6), c.row_selected);
                    if tab.cursor == Some(r) {
                        p.rect_filled(
                            Rect::from_min_size(row_rect.min + Vec2::new(0.0, 8.0), Vec2::new(3.0, ROW_H - 18.0)),
                            CornerRadius::same(2),
                            c.accent,
                        );
                    }
                } else if resp.hovered() {
                    p.rect_filled(row_rect, CornerRadius::same(6), c.row_hover);
                }
                let dim = |col: Color32| if e.hidden { col.gamma_multiply(0.55) } else { col };
                let cut = matches!(&self.clipboard, Some((Op::Move, ps)) if ps.contains(&e.path));
                let dim = |col: Color32| if cut { dim(col).gamma_multiply(0.5) } else { dim(col) };
                let (g, gcol) = if e.is_dir {
                    (glyph::FOLDER, c.accent_text)
                } else {
                    (glyph::FILE, c.muted)
                };
                p.text(
                    egui::pos2(rect.left() + 14.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    g,
                    icon_font(16.0),
                    dim(gcol),
                );
                let name_rect = Rect::from_min_max(
                    egui::pos2(rect.left() + 40.0, rect.top()),
                    egui::pos2(rect.left() + name_w - 8.0, rect.bottom()),
                );
                let renaming = rename.as_ref().is_some_and(|rn| rn.path == e.path);
                if renaming {
                    let rn = rename.as_mut().unwrap();
                    let id = Id::new("rename");
                    let te = ui.put(
                        name_rect.shrink2(Vec2::new(0.0, 2.0)),
                        egui::TextEdit::singleline(&mut rn.text)
                            .id(id)
                            .margin(Vec2::new(6.0, 3.0)),
                    );
                    if std::mem::take(&mut rn.focus) {
                        te.request_focus();
                        let stem = if e.is_dir {
                            rn.text.chars().count()
                        } else {
                            Path::new(&rn.text)
                                .file_stem()
                                .map_or(0, |s| s.to_string_lossy().chars().count())
                        };
                        select_range(ui.ctx(), id, 0, stem);
                    }
                    if te.lost_focus() {
                        if ui.input(|i| i.key_pressed(Key::Escape)) {
                            cancel = true;
                        } else {
                            commit = true;
                        }
                    }
                } else {
                    draw_text(ui, name_rect, &e.name, FontId::proportional(14.0), dim(c.text), false);
                }
                let col = |x: f32, w: f32| {
                    Rect::from_min_size(egui::pos2(rect.left() + x + 10.0, rect.top()), Vec2::new(w - 20.0, ROW_H))
                };
                let small = FontId::proportional(12.5);
                draw_text(ui, col(name_w, mod_w), &e.modified_text, small.clone(), dim(c.muted), false);
                if kind_w > 0.0 {
                    draw_text(ui, col(name_w + mod_w, kind_w), &e.kind, small.clone(), dim(c.muted), false);
                }
                if !e.is_dir {
                    draw_text(ui, col(name_w + mod_w + kind_w, size_w), &fsops::format_size(e.size), small, dim(c.muted), true);
                }

                if resp.double_clicked() {
                    actions.push(Action::OpenRow(r));
                } else if resp.clicked() {
                    clicked = Some((r, ui.input(|i| i.modifiers)));
                }
                if resp.secondary_clicked() {
                    secondary = Some(r);
                }
                let is_dir = e.is_dir;
                let loc = Loc::Dir(e.path.clone());
                resp.context_menu(|ui| {
                    if menu_item(ui, glyph::OPEN_EXTERNAL, "Open", "Enter") {
                        actions.push(Action::OpenSelected);
                        ui.close_menu();
                    }
                    if is_dir && menu_item(ui, glyph::ADD, "Open in new tab", "") {
                        actions.push(Action::NewTab(loc.clone()));
                        ui.close_menu();
                    }
                    if menu_item(ui, glyph::FOLDER, "Show in Explorer", "") {
                        actions.push(Action::Reveal);
                        ui.close_menu();
                    }
                    ui.separator();
                    if menu_item(ui, glyph::CUT, "Cut", "Ctrl+X") {
                        actions.push(Action::Clip(Op::Move));
                        ui.close_menu();
                    }
                    if menu_item(ui, glyph::COPY, "Copy", "Ctrl+C") {
                        actions.push(Action::Clip(Op::Copy));
                        ui.close_menu();
                    }
                    if menu_item(ui, glyph::COPY, "Copy path", "Ctrl+Shift+C") {
                        actions.push(Action::CopyPath);
                        ui.close_menu();
                    }
                    ui.separator();
                    if menu_item(ui, glyph::RENAME, "Rename", "F2") {
                        actions.push(Action::Rename);
                        ui.close_menu();
                    }
                    if menu_item(ui, glyph::DELETE, "Move to Recycle Bin", "Del") {
                        actions.push(Action::Recycle);
                        ui.close_menu();
                    }
                });
            }
        });
        tab.list_offset = out.state.offset.y;
        tab.list_height = out.inner_rect.height();

        if let Some((r, m)) = clicked {
            if m.command {
                let p = tab.entry(r).path.clone();
                if !tab.selected.remove(&p) {
                    tab.selected.insert(p);
                }
                tab.cursor = Some(r);
                tab.anchor = Some(r);
            } else if m.shift {
                tab.select_range(r);
            } else {
                tab.select_only(r);
            }
        }
        if let Some(r) = secondary {
            if !tab.selected.contains(&tab.entry(r).path) {
                tab.select_only(r);
            }
        }
        if cancel {
            self.rename = None;
        } else if commit {
            self.commit_rename();
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.take() else { return };
        let c = colors();
        let mut keep = true;
        match &dialog {
            Dialog::ConfirmRecycle(paths) => {
                let paths = paths.clone();
                let modal = egui::Modal::new(Id::new("recycle")).frame(theme::dialog()).show(ctx, |ui| {
                    ui.set_width(380.0);
                    let title = if paths.len() == 1 {
                        format!(
                            "Move \"{}\" to the Recycle Bin?",
                            paths[0].file_name().unwrap_or_default().to_string_lossy()
                        )
                    } else {
                        format!("Move {} items to the Recycle Bin?", paths.len())
                    };
                    ui.label(RichText::new(title).size(16.0).strong().color(c.text));
                    ui.label(theme::muted("You can restore them from the Recycle Bin."));
                    ui.add_space(12.0);
                    let mut result = None;
                    ui.horizontal(|ui| {
                        if ui.add(theme::danger_button("Move to Recycle Bin")).clicked()
                            || ui.input(|i| i.key_pressed(Key::Enter))
                        {
                            result = Some(true);
                        }
                        if ui.button("Cancel").clicked() {
                            result = Some(false);
                        }
                    });
                    result
                });
                let answer = modal.inner.or(modal.should_close().then_some(false));
                if let Some(yes) = answer {
                    keep = false;
                    if yes {
                        self.run_op(Op::Recycle, "Delete", paths, None);
                    }
                }
            }
            Dialog::Settings => {
                let modal = egui::Modal::new(Id::new("settings")).frame(theme::dialog()).show(ctx, |ui| {
                    ui.set_width(420.0);
                    ui.label(RichText::new("Appearance").size(18.0).strong().color(c.text));
                    ui.add_space(8.0);
                    ui.label(RichText::new("Accent color").color(c.text));
                    ui.add_space(2.0);
                    let current = self.settings.accent;
                    let mut pick = None;
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                        let sys = theme::system_accent().unwrap_or(c.accent);
                        if swatch(ui, sys, current == AccentChoice::System, "Windows accent").clicked() {
                            pick = Some(AccentChoice::System);
                        }
                        ui.add_space(6.0);
                        for (i, (name, rgb)) in theme::PRESETS.iter().enumerate() {
                            let col = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                            if swatch(ui, col, current == AccentChoice::Preset(i), name).clicked() {
                                pick = Some(AccentChoice::Preset(i));
                            }
                        }
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let mut rgb = match current {
                            AccentChoice::Custom(v) => v,
                            _ => [c.accent.r(), c.accent.g(), c.accent.b()],
                        };
                        if ui.color_edit_button_srgb(&mut rgb).changed() {
                            pick = Some(AccentChoice::Custom(rgb));
                        }
                        let label = match current {
                            AccentChoice::System => "Following Windows",
                            AccentChoice::Preset(i) => theme::PRESETS[i.min(theme::PRESETS.len() - 1)].0,
                            AccentChoice::Custom(_) => "Custom",
                        };
                        ui.label(theme::muted(format!("{label}  ·  click the box for any color")));
                    });
                    if let Some(p) = pick {
                        self.set_accent(p);
                    }
                    ui.add_space(14.0);
                    if ui.checkbox(&mut self.settings.show_hidden, "Show hidden files").changed() {
                        self.save_pending = true;
                        for t in &mut self.tabs {
                            t.dirty = true;
                        }
                    }
                    ui.add_space(14.0);
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        ui.add(theme::primary_button("Done")).clicked()
                    })
                    .inner
                });
                if modal.inner || modal.should_close() {
                    keep = false;
                }
            }
        }
        if keep {
            self.dialog = Some(dialog);
        }
    }

    fn take_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
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
        self.track_system(ctx);
        let show_hidden = self.settings.show_hidden;
        for t in &mut self.tabs {
            if t.dirty {
                t.rebuild_view(show_hidden);
            }
        }
        let mut actions = Vec::new();
        self.keyboard(ctx, &mut actions);
        self.take_dropped_files(ctx);

        let glass = self.glass;
        egui::TopBottomPanel::top("tabs")
            .frame(theme::chrome(glass).inner_margin(egui::Margin { left: 10, right: 10, top: 6, bottom: 0 }))
            .show_separator_line(false)
            .show(ctx, |ui| self.tab_strip(ui, &mut actions));
        egui::TopBottomPanel::top("toolbar")
            .frame(theme::chrome(glass))
            .show_separator_line(false)
            .show(ctx, |ui| self.toolbar(ui, &mut actions));
        egui::TopBottomPanel::bottom("status")
            .frame(theme::chrome(glass).inner_margin(egui::Margin::symmetric(14, 4)))
            .show_separator_line(false)
            .show(ctx, |ui| self.status_bar(ui));
        egui::SidePanel::left("sidebar")
            .frame(theme::sidebar(glass))
            .exact_width(SIDEBAR_W)
            .resizable(false)
            .show_separator_line(false)
            .show(ctx, |ui| self.sidebar(ui, &mut actions));
        egui::CentralPanel::default()
            .frame(theme::page(glass))
            .show(ctx, |ui| {
                if self.tabs[self.active].loc == Loc::Pc {
                    self.pc_page(ui, &mut actions);
                } else {
                    self.file_list(ui, &mut actions);
                }
            });
        self.dialogs(ctx);

        for a in actions {
            self.apply(a);
        }

        let title = format!("{} - Fileman", self.tabs[self.active].loc.title());
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
            crate::save_settings(&self.settings);
        }
    }
}

// ---------------------------------------------------------------- small helpers

/// A glyph and a label in one piece of text.
fn icon_text(g: char, text: &str, color: Color32, size: f32) -> WidgetText {
    let mut job = LayoutJob::default();
    job.append(
        &g.to_string(),
        0.0,
        TextFormat {
            font_id: icon_font(size),
            color,
            valign: Align::Center,
            ..Default::default()
        },
    );
    job.append(
        text,
        8.0,
        TextFormat {
            font_id: FontId::proportional(size + 1.0),
            color,
            valign: Align::Center,
            ..Default::default()
        },
    );
    WidgetText::LayoutJob(job)
}

fn menu_item(ui: &mut Ui, g: char, label: &str, shortcut: &str) -> bool {
    let c = colors();
    let mut b = egui::Button::new(icon_text(g, label, c.text, 13.0)).min_size(Vec2::new(220.0, 0.0));
    if !shortcut.is_empty() {
        b = b.shortcut_text(RichText::new(shortcut).color(c.muted).size(12.0));
    }
    ui.add(b).clicked()
}

/// A place in the sidebar, marked with the accent while it is open.
fn nav_row(ui: &mut Ui, g: char, label: &str, active: bool) -> egui::Response {
    let c = colors();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::click());
    let p = ui.painter();
    if active {
        p.rect_filled(rect, CornerRadius::same(8), c.row_selected);
        p.rect_filled(
            Rect::from_min_size(rect.min + Vec2::new(0.0, 9.0), Vec2::new(3.0, 14.0)),
            CornerRadius::same(2),
            c.accent,
        );
    } else if resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(8), c.row_hover);
    }
    p.text(
        egui::pos2(rect.left() + 14.0, rect.center().y),
        Align2::LEFT_CENTER,
        g,
        icon_font(15.0),
        if active { c.accent_text } else { c.muted },
    );
    let text_rect = Rect::from_min_max(egui::pos2(rect.left() + 40.0, rect.top()), rect.max);
    draw_text(ui, text_rect, label, FontId::proportional(14.0), c.text, false);
    resp
}

fn swatch(ui: &mut Ui, col: Color32, selected: bool, tip: &str) -> egui::Response {
    let c = colors();
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::click());
    let center = rect.center();
    let p = ui.painter();
    p.circle_filled(center, if resp.hovered() { 13.5 } else { 12.5 }, col);
    if selected {
        p.circle_stroke(center, 15.0, Stroke::new(2.0_f32, c.text));
        let on = theme::Colors::new(c.light, col).on_accent;
        p.text(center, Align2::CENTER_CENTER, glyph::CHECK, icon_font(11.0), on);
    }
    resp.on_hover_text(tip)
}

fn hover_outline(ui: &Ui, resp: &egui::Response) {
    if resp.hovered() {
        ui.painter().rect_stroke(
            resp.rect,
            CornerRadius::same(16),
            Stroke::new(1.5_f32, colors().accent),
            egui::StrokeKind::Inside,
        );
    }
}

/// One line of text in a box, cut with an ellipsis when it does not fit.
fn draw_text(ui: &Ui, rect: Rect, text: &str, font: FontId, color: Color32, right: bool) {
    if rect.width() <= 4.0 {
        return;
    }
    let mut job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat {
            font_id: font,
            color,
            ..Default::default()
        },
    );
    job.wrap = TextWrapping::truncate_at_width(rect.width());
    let galley = ui.fonts(|f| f.layout_job(job));
    let x = if right { rect.right() - galley.size().x } else { rect.left() };
    let pos = egui::pos2(x, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(pos, galley, color);
}

fn select_range(ctx: &egui::Context, id: Id, from: usize, to: usize) {
    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
        state
            .cursor
            .set_char_range(Some(CCursorRange::two(CCursor::new(from), CCursor::new(to))));
        state.store(ctx, id);
    } else {
        let mut state = egui::text_edit::TextEditState::default();
        state
            .cursor
            .set_char_range(Some(CCursorRange::two(CCursor::new(from), CCursor::new(to))));
        state.store(ctx, id);
    }
}

fn select_all(ctx: &egui::Context, id: Id, len: usize) {
    select_range(ctx, id, 0, len);
}

/// Expands %VARS% in a typed path, like Explorer's address bar.
fn expand_env(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}
