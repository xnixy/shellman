//! Files' menus, all of them shellman's glass popup menu (`shellman_ui::menu::popup`), the one
//! the desktop and the dock use.
//!
//! The right-click menu of files, folders and a folder's background: Files' own entries come
//! first and show at once; under them, the shell's (what Windows and installed apps add) appear
//! as soon as the worker has read them, often before the click, since a walk starts when the
//! right button goes down and when the selection settles. The open menu is updated in place as
//! they arrive.
//!
//! Files' other menus (the bin's, a tab's, the sidebar's, the Sort, Move to and file type
//! dropdowns) are plain lists of [`Action`]s: the code that draws their button or row asks for
//! one with [`request`], and the same popup shows it at the end of the frame.

use super::actions::Action;
use super::tab::Loc;
use egui::{ColorImage, Context, Pos2, Rect, Response, TextureHandle, TextureOptions, Vec2};
use shellman_core::fsops::Op;
use shellman_core::shell_menu::{Cascade, Item, Loader, Palette, Route, Target};
use shellman_ui::menu::popup::{Anchor, Entry, PopupMenu};
use shellman_ui::theme::{colors, glyph};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;


/// Icon textures kept; past this they are dropped and made again as needed.
const MAX_TEXTURES: usize = 512;
/// A dropdown's button clicked this soon after its menu closed by that same click closes it,
/// rather than opening it again.
const TOGGLE: f64 = 0.4;

/// What a menu row does.
#[derive(Clone, Debug)]
pub(super) enum Pick {
    Native(Action),
    /// The shell's entry at this path in the loader's view.
    Shell(Vec<usize>),
    /// A shell submenu at this path, left unfilled because it was slow, is opening.
    Expand(Vec<usize>),
    /// "More options", the shell's entries, is opening.
    More,
}

/// What the open right-click menu is for.
struct Open {
    target: Target,
    /// The single folder the item menu is for, if it is one.
    is_dir: bool,
    background: bool,
    /// What the clipboard holds: anything to paste, and files (for shortcuts to them).
    has_clip: (bool, bool),
}

/// A plain menu asked for this frame: where, its entries, and the button it drops from.
struct Request {
    at: Anchor,
    entries: Vec<Entry<Action>>,
    key: &'static str,
}

thread_local! {
    static REQUEST: RefCell<Option<Request>> = const { RefCell::new(None) };
}

/// Ask for a plain menu of Files' own actions at `at`. `key` names the button or row kind it
/// belongs to: clicking a dropdown's button again closes its menu.
pub(super) fn request(at: Anchor, key: &'static str, entries: Vec<Entry<Action>>) {
    REQUEST.with(|r| *r.borrow_mut() = Some(Request { at, entries, key }));
}

/// On a right click on `resp`, ask for the menu `entries` builds, at the pointer.
pub(super) fn on_right_click(resp: &Response, key: &'static str, entries: impl FnOnce() -> Vec<Entry<Action>>) {
    if resp.secondary_clicked() {
        let ctx = &resp.ctx;
        let pos = resp
            .interact_pointer_pos()
            .or_else(|| ctx.input(|i| i.pointer.latest_pos()))
            .unwrap_or(resp.rect.center());
        request(Anchor::in_window(ctx, pos), key, entries());
    }
}

/// An item with one of Files' glyphs.
pub(super) fn item(a: Action, g: char, label: impl Into<String>) -> Entry<Action> {
    Entry::item(a, label).icon(g)
}

pub(super) struct ContextMenu {
    menu: PopupMenu<Pick>,
    loader: Loader,
    open: Option<Open>,
    textures: HashMap<u64, TextureHandle>,

    /// The plain menu open now, by its key; and the last one that closed, and when.
    plain: Option<&'static str>,
    closed: Option<(&'static str, f64)>,
    /// A New entry was picked in this folder: the tab looks for the item it makes when its
    /// listing next comes back (see `take_made`).
    made: Option<PathBuf>,
}

impl ContextMenu {
    pub(super) fn new(ctx: &Context, hwnd: isize) -> Self {
        let ctx2 = ctx.clone();
        Self {
            menu: PopupMenu::new(),
            loader: Loader::new(hwnd, Arc::new(move || ctx2.request_repaint())),
            open: None,
            textures: HashMap::new(),

            plain: None,
            closed: None,
            made: None,
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.menu.is_open()
    }

    /// Start reading the menu for `target` now: the right button just went down.
    pub(super) fn warm(&mut self, target: &Target) {
        self.loader.want(target);
    }

    /// Open the right-click menu at `pos` (a point of the window) for `target`. `is_dir`: the one
    /// item is a folder, as the listing says (the disk is not asked on the window's thread).
    pub(super) fn open(&mut self, ctx: &Context, pos: Pos2, target: Target, is_dir: bool, background: bool, has_clip: (bool, bool)) {
        let is_dir = target.items.len() == 1 && is_dir;
        let open = Open { target, is_dir, background, has_clip };
        let entries = self.entries(ctx, &open);
        self.menu.open(ctx, Anchor::in_window(ctx, pos), entries);
        self.open = Some(open);
        self.plain = None;
    }

    /// The right-click menu's entries now: Files' own, then "More options", the shell's entries
    /// as far as they are read (the walk starts when the right button goes down, or when the
    /// submenu opens).
    fn entries(&mut self, ctx: &Context, open: &Open) -> Vec<Entry<Pick>> {
        let mut entries = native_entries(ctx, open);
        let mut more = Vec::new();
        match self.loader.view(&open.target) {
            Some(view) => {
                let items: Vec<(usize, &Item)> = view.items.iter().enumerate().filter(|(_, i)| !duplicate(i)).collect();
                more = shell_entries(ctx, &mut self.textures, &items, &[]);
                if more.is_empty() && view.loading {
                    more.push(Entry::note("Loading…"));
                }
                if let Some(note) = view.note {
                    if !more.is_empty() {
                        more.push(Entry::separator());
                    }
                    more.push(Entry::note(note));
                }
            }
            None => more.push(Entry::note("Loading…")),
        }
        if more.is_empty() {
            more.push(Entry::note("Nothing more"));
        }
        entries.push(Entry::separator());
        entries.push(Entry::submenu("More options", more).icon(glyph::MORE).on_open(Pick::More));
        entries
    }

    /// Each frame: take in what the worker sent, keep the open menu up to date, open a plain
    /// menu asked for this frame, and turn a pick into actions.
    pub(super) fn show(&mut self, ctx: &Context, actions: &mut Vec<Action>) -> Option<String> {
        let c = colors();
        let rgb = |c: egui::Color32| u32::from(c.r()) | u32::from(c.g()) << 8 | u32::from(c.b()) << 16;
        self.loader.set_palette(Palette { text: rgb(c.text), background: rgb(c.popup) });
        self.loader.poll();
        let notice = self.loader.take_notice();
        let now = ctx.input(|i| i.time);
        // What was picked, before the entries are updated (a pick is looked up among them).
        match self.menu.take_pick() {
            Some(Pick::Native(a)) => actions.push(a),
            Some(Pick::Shell(path)) => self.pick_shell(&path, actions),
            _ => {}
        }
        match (self.menu.take_opened(), &self.open) {
            // Read now if the right button's press did not already (the Menu key).
            (Some(Pick::More), Some(open)) => self.loader.want(&open.target),
            (Some(Pick::Expand(path)), Some(open)) => {
                let item = self.loader.view(&open.target).and_then(|v| chain(v.items, &path).last().cloned().cloned());
                if let Some(item) = item {
                    self.loader.expand(&open.target, &item);
                }
            }
            _ => {}
        }
        if !self.menu.is_open() {
            self.open = None;
            if let Some(key) = self.plain.take() {
                self.closed = Some((key, now));
            }
        }
        if let Some(open) = self.open.take() {
            let entries = self.entries(ctx, &open);
            self.menu.update(entries);
            self.open = Some(open);
        }
        if let Some(req) = REQUEST.with(|r| r.borrow_mut().take()) {
            let toggled_off = self.closed.is_some_and(|(key, at)| key == req.key && now - at < TOGGLE);
            if toggled_off {
                self.closed = None;
            } else {
                self.open = None;
                self.menu.open(ctx, req.at, req.entries.into_iter().map(lift).collect());
                self.plain = Some(req.key);
            }
        }
        notice
    }

    /// The folder a New entry was just picked in, once. The shell makes the file a moment
    /// later; the folder's watcher re-lists it on a worker, and the tab then selects and renames
    /// the item that was not there before (see `Tab::expect_new`), as Explorer does.
    pub(super) fn take_made(&mut self) -> Option<PathBuf> {
        self.made.take()
    }

    fn pick_shell(&mut self, path: &[usize], actions: &mut Vec<Action>) {
        let Some(open) = self.open.take() else { return };
        let Some(view) = self.loader.view(&open.target) else { return };
        let fresh = view.fresh;
        let owned: Vec<Item> = chain(view.items, path).into_iter().cloned().collect();
        // Entries of the New submenu carry the file type as their verb (".txt"), or "newlink".
        let makes_file = open.background
            && owned.len() > 1
            && owned.last().and_then(|i| i.verb.as_deref()).is_some_and(|v| v.starts_with('.') || v == "newlink");
        if makes_file {
            self.made = Some(open.target.folder.clone());
        }
        let refs: Vec<&Item> = owned.iter().collect();
        let point = cursor();
        match self.loader.pick(&open.target, &refs, fresh, point) {
            Route::Invoke => {}
            Route::Rename => actions.push(Action::Rename),
            Route::OpenFolders if open.background => {}
            Route::OpenFolders => actions.push(Action::OpenSelected),
            Route::OpenFoldersInTabs => {
                let dirs = if open.background { vec![open.target.folder.clone()] } else { open.target.items.clone() };
                actions.extend(dirs.into_iter().map(|d| Action::NewTab(Loc::Dir(d))));
            }
            Route::OpenTargetFolder => {
                for item in &open.target.items {
                    let target = shellman_core::fsops::link_target(item).unwrap_or_else(|| item.clone());
                    actions.push(Action::Reveal(target));
                }
            }
        }
    }
}

/// A plain menu's entry as the right-click menu's handle takes it.
fn lift(e: Entry<Action>) -> Entry<Pick> {
    match e {
        Entry::Item(i) => {
            let mut out = Entry::item(Pick::Native(i.id), i.label).hint(i.hint).enabled(i.enabled).checked(i.checked);
            if let Some(g) = i.icon {
                out = out.icon(g);
            }
            if let Some(t) = i.image {
                out = out.image(t);
            }
            if i.danger {
                out = out.danger();
            }
            out
        }
        Entry::Submenu { label, icon, image, enabled, entries, on_open } => Entry::Submenu {
            label,
            icon,
            image,
            enabled,
            entries: entries.into_iter().map(lift).collect(),
            on_open: on_open.map(Pick::Native),
        },
        Entry::Separator => Entry::Separator,
        Entry::Heading(t) => Entry::Heading(t),
        Entry::Note(t) => Entry::Note(t),
        Entry::Picture { id, texture, size, enabled } => Entry::Picture { id: Pick::Native(id), texture, size, enabled },
    }
}

/// The entries at each step of `path`, from the top.
fn chain<'a>(items: &'a [Item], path: &[usize]) -> Vec<&'a Item> {
    let mut out = Vec::new();
    let mut level = items;
    for &i in path {
        let Some(item) = level.get(i) else { break };
        out.push(item);
        level = &item.submenu;
    }
    out
}

/// What decides Files' own entries, apart from the target's paths.
#[derive(Clone, Debug, Default)]
pub(super) struct Facts {
    pub background: bool,
    /// The one item is a folder.
    pub is_dir: bool,
    pub can_paste: bool,
    pub can_paste_link: bool,
    /// The folder the background menu is for, or the one folder the item menu is for.
    pub dir: PathBuf,
    /// "Pin to sidebar" or "Unpin from sidebar", for a folder.
    pub pin_label: &'static str,
    /// The six tags' names and whether the whole selection has each.
    pub tags: Vec<(String, bool)>,
    /// The one item, when it is something that can run as administrator (a program, a script,
    /// a shortcut: told by its name alone).
    pub admin: Option<PathBuf>,
}

/// Files' own entries.
fn native_entries(ctx: &Context, open: &Open) -> Vec<Entry<Pick>> {
    let dir = if open.background { open.target.folder.clone() } else { open.target.items.first().cloned().unwrap_or_default() };
    let admin = match open.target.items.as_slice() {
        [one] if !open.background && !open.is_dir => Some(one.clone())
            .filter(|p| shellman_core::apps_index::admin::by_name(&p.to_string_lossy()) == Some(true)),
        _ => None,
    };
    let facts = Facts {
        admin,
        background: open.background,
        is_dir: open.is_dir,
        can_paste: open.has_clip.0,
        can_paste_link: open.has_clip.1,
        pin_label: if open.is_dir { super::pins::menu_label(ctx, &dir) } else { "" },
        dir,
        tags: ctx.data(|d| d.get_temp(egui::Id::new("tag_menu_state"))).unwrap_or_default(),
    };
    own_entries(&facts).into_iter().map(lift).collect()
}

/// Files' own entries for these facts, as plain actions.
pub(super) fn own_entries(f: &Facts) -> Vec<Entry<Action>> {
    let mut e = Vec::new();
    if f.background {
        e.push(item(Action::NewFolder, glyph::NEW_FOLDER, "New folder").hint("Ctrl+Shift+N"));
        // Always there, so the entry does not come and go; only usable with something to paste.
        e.push(item(Action::Paste, glyph::PASTE, "Paste").hint("Ctrl+V").enabled(f.can_paste));
        e.push(item(Action::PasteShortcut, glyph::COPY, "Paste shortcut").enabled(f.can_paste_link));
        e.push(Entry::separator());
        e.push(item(Action::Refresh, glyph::REFRESH, "Refresh").hint("F5"));
        e.push(item(Action::CopyPath, glyph::COPY, "Copy folder path"));
        e.push(item(Action::NewTab(Loc::Dir(f.dir.clone())), glyph::ADD, "Open in new tab"));
        e.push(item(Action::OpenTerminal(f.dir.clone()), glyph::APP, "Open Terminal here"));
    } else {
        e.push(item(Action::OpenSelected, glyph::OPEN_EXTERNAL, "Open").hint("Enter"));
        if let Some(program) = &f.admin {
            e.push(item(Action::RunAsAdmin(program.clone()), glyph::ADMIN, "Run as administrator"));
        }
        if f.is_dir {
            e.push(item(Action::NewTab(Loc::Dir(f.dir.clone())), glyph::ADD, "Open in new tab"));
            e.push(item(Action::OpenTerminal(f.dir.clone()), glyph::APP, "Open Terminal here"));
            e.push(item(Action::TogglePin(f.dir.clone()), glyph::PIN, f.pin_label));
        }
        e.push(Entry::separator());
        e.push(item(Action::Clip(Op::Move), glyph::CUT, "Cut").hint("Ctrl+X"));
        e.push(item(Action::Clip(Op::Copy), glyph::COPY, "Copy").hint("Ctrl+C"));
        e.push(item(Action::CopyPath, glyph::COPY, "Copy as path").hint("Ctrl+Shift+C"));
        e.push(Entry::separator());
        e.push(item(Action::Rename, glyph::RENAME, "Rename").hint("F2"));
        e.push(tags_entry(&f.tags));
        e.push(item(Action::Recycle, glyph::DELETE, "Move to Recycle Bin").hint("Del").danger());
    }
    e.push(Entry::separator());
    e.push(item(Action::Properties, glyph::INFO, "Properties").hint("Alt+Enter"));
    e
}

/// "Tags", a submenu of the six tags, each ticked when the whole selection has it, and a way to
/// take them all off. The names and ticks are left in the context's memory each frame (see
/// `FileMan::context_menu`), since the menu is built far from the tags.
fn tags_entry(state: &[(String, bool)]) -> Entry<Action> {
    let mut sub: Vec<Entry<Action>> =
        state.iter().enumerate().map(|(i, (name, on))| Entry::item(Action::ToggleTag(i), name.clone()).checked(*on)).collect();
    sub.push(Entry::separator());
    sub.push(item(Action::ClearTags, glyph::CLOSE, "Remove all tags"));
    Entry::submenu("Tags", sub).icon(glyph::PIN)
}

/// A label with its keyboard letter marked with `&` again (the shell's menus give the letter on
/// its own): before the first `letter` in it, else none. Any `&` in the text stays one.
pub(super) fn with_mnemonic(label: &str, letter: Option<char>) -> String {
    let escaped = label.replace('&', "&&");
    let Some(m) = letter else { return escaped };
    let mut out = String::with_capacity(escaped.len() + 1);
    let mut done = false;
    let mut chars = label.chars();
    while let Some(c) = chars.next() {
        if c == '&' {
            out.push_str("&&");
            continue;
        }
        if !done && c.to_lowercase().eq(m.to_lowercase()) {
            out.push('&');
            done = true;
        }
        out.push(c);
    }
    out
}

/// Verbs and labels of the shell's entries that Files' own entries already offer.
const DUPLICATES: [&str; 17] = [
    "open",
    "cut",
    "copy",
    "paste",
    "delete",
    "rename",
    "properties",
    "copyaspath",
    "pastelink",
    "refresh",
    "open in terminal",
    "open in new tab",
    "copy as path",
    "paste shortcut",
    "new folder",
    "openinterminal",
    "opennewtab",
];

/// Whether a top-level shell entry repeats one of Files' own (by its verb or its label).
fn duplicate(item: &Item) -> bool {
    if item.separator || item.is_submenu() {
        return false;
    }
    let verb = item.verb.as_deref().map(str::to_lowercase);
    let label = item.title().to_lowercase();
    DUPLICATES.iter().any(|d| verb.as_deref() == Some(*d) || label == *d)
}

/// Separators only between entries: none first, last or twice in a row.
fn tidy<T>(entries: Vec<Entry<T>>) -> Vec<Entry<T>> {
    let mut out: Vec<Entry<T>> = Vec::with_capacity(entries.len());
    for e in entries {
        let sep = matches!(e, Entry::Separator);
        if sep && out.last().is_none_or(|l| matches!(l, Entry::Separator)) {
            continue;
        }
        out.push(e);
    }
    if out.last().is_some_and(|l| matches!(l, Entry::Separator)) {
        out.pop();
    }
    out
}

/// The shell's entries (each with its index in the shell's list), `prefix` being the path of
/// the submenu they are in.
fn shell_entries(ctx: &Context, textures: &mut HashMap<u64, TextureHandle>, items: &[(usize, &Item)], prefix: &[usize]) -> Vec<Entry<Pick>> {
    let mut out = Vec::with_capacity(items.len());
    let ppp = ctx.pixels_per_point();
    for &(i, item) in items {
        if item.separator {
            out.push(Entry::separator());
            continue;
        }
        let mut path = prefix.to_vec();
        path.push(i);
        if let Some(pic) = &item.drawn {
            let size = Vec2::new(pic.size[0] as f32, pic.size[1] as f32) / ppp;
            out.push(Entry::picture(Pick::Shell(path), texture(ctx, textures, pic).id(), size).enabled(item.enabled));
            continue;
        }
        let label = with_mnemonic(&item.title(), item.mnemonic);
        let mut entry = if item.is_submenu() {
            let children: Vec<(usize, &Item)> = item.submenu.iter().enumerate().collect();
            let mut sub = shell_entries(ctx, textures, &children, &path);
            let mut entry = None;
            match item.cascade {
                Cascade::Pending if sub.is_empty() => sub.push(Entry::note("Loading…")),
                Cascade::Deferred => {
                    sub.push(Entry::note("Loading…"));
                    entry = Some(Entry::submenu(label.clone(), std::mem::take(&mut sub)).on_open(Pick::Expand(path.clone())));
                }
                _ => {}
            }
            entry.unwrap_or_else(|| Entry::submenu(label, sub))
        } else {
            Entry::item(Pick::Shell(path), label).hint(item.shortcut.clone()).checked(item.checked)
        };
        entry = entry.enabled(item.enabled);
        if let Some(pic) = &item.icon {
            entry = entry.image(texture(ctx, textures, pic).id());
        }
        out.push(entry);
    }
    tidy(out)
}

/// One texture per distinct picture.
fn texture<'a>(ctx: &Context, textures: &'a mut HashMap<u64, TextureHandle>, pic: &shellman_core::shell_menu::Icon) -> &'a TextureHandle {
    if textures.len() >= MAX_TEXTURES && !textures.contains_key(&pic.hash) {
        textures.clear();
    }
    textures.entry(pic.hash).or_insert_with(|| {
        let img = ColorImage::from_rgba_premultiplied(pic.size, &pic.rgba);
        ctx.load_texture(format!("menu-icon-{:x}", pic.hash), img, TextureOptions::LINEAR)
    })
}

/// The pointer in screen pixels, where a command is said to be invoked.
fn cursor() -> (i32, i32) {
    let mut pt = windows::Win32::Foundation::POINT::default();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut pt);
    }
    (pt.x, pt.y)
}

/// Where the Menu key and Shift+F10 open the menu: beside the row of the selection the cursor
/// is on, when it was drawn last frame, else the pointer.
pub(super) fn key_point(row_rect: Option<Rect>, pointer: Option<Pos2>, screen: Rect) -> Pos2 {
    match row_rect {
        Some(r) => egui::pos2(r.left() + 24.0_f32.min(r.width() / 2.0), r.center().y),
        None => pointer.filter(|p| screen.contains(*p)).unwrap_or_else(|| screen.center()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(e: &[Entry<Action>]) -> Vec<String> {
        e.iter()
            .map(|e| match e {
                Entry::Item(i) => i.label.clone(),
                Entry::Submenu { label, .. } => format!("{label} >"),
                Entry::Separator => "-".into(),
                Entry::Heading(t) | Entry::Note(t) => format!("({t})"),
                Entry::Picture { .. } => "[picture]".into(),
            })
            .collect()
    }

    fn item_of<'a>(e: &'a [Entry<Action>], label: &str) -> &'a shellman_ui::menu::popup::Item<Action> {
        e.iter()
            .find_map(|e| match e {
                Entry::Item(i) if i.label == label => Some(i),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no {label}"))
    }

    fn tags() -> Vec<(String, bool)> {
        ["Red", "Orange", "Yellow", "Green", "Blue", "Purple"].iter().enumerate().map(|(i, n)| (n.to_string(), i == 1)).collect()
    }

    #[test]
    fn a_single_file() {
        let e = own_entries(&Facts { tags: tags(), ..Default::default() });
        assert_eq!(
            labels(&e),
            ["Open", "-", "Cut", "Copy", "Copy as path", "-", "Rename", "Tags >", "Move to Recycle Bin", "-", "Properties"]
        );
        assert_eq!(item_of(&e, "Open").hint, "Enter");
        assert_eq!(item_of(&e, "Copy as path").hint, "Ctrl+Shift+C");
        assert_eq!(item_of(&e, "Properties").hint, "Alt+Enter");
        let bin = item_of(&e, "Move to Recycle Bin");
        assert!(bin.danger && bin.hint == "Del");
        let Some(Entry::Submenu { entries: sub, .. }) = e.iter().find(|e| matches!(e, Entry::Submenu { .. })) else { panic!() };
        assert_eq!(labels(sub), ["Red", "Orange", "Yellow", "Green", "Blue", "Purple", "-", "Remove all tags"]);
        assert!(item_of(sub, "Orange").checked && !item_of(sub, "Red").checked);
    }

    #[test]
    fn a_program_can_run_as_administrator() {
        let exe = PathBuf::from(r"C:\x\app.exe");
        let e = own_entries(&Facts { admin: Some(exe.clone()), ..Default::default() });
        assert_eq!(labels(&e)[..3], ["Open".to_string(), "Run as administrator".into(), "-".into()]);
        assert!(matches!(&item_of(&e, "Run as administrator").id, Action::RunAsAdmin(p) if *p == exe));
        // A document, a folder, several items: not offered.
        assert!(!labels(&own_entries(&Facts::default())).iter().any(|l| l == "Run as administrator"));
    }

    #[test]
    fn a_folder_adds_tab_terminal_and_pin() {
        let dir = PathBuf::from(r"C:\x\folder");
        let e = own_entries(&Facts { is_dir: true, dir: dir.clone(), pin_label: "Pin to sidebar", ..Default::default() });
        assert_eq!(
            labels(&e)[..4],
            ["Open".to_string(), "Open in new tab".into(), "Open Terminal here".into(), "Pin to sidebar".into()]
        );
        assert!(matches!(&item_of(&e, "Open Terminal here").id, Action::OpenTerminal(d) if *d == dir));
        assert!(matches!(&item_of(&e, "Pin to sidebar").id, Action::TogglePin(d) if *d == dir));
    }

    #[test]
    fn many_items_get_the_file_menu() {
        // Several items (or one file): no folder-only entries; the actions act on the selection.
        let e = own_entries(&Facts { is_dir: false, ..Default::default() });
        assert!(!labels(&e).iter().any(|l| l == "Open in new tab" || l == "Open Terminal here"));
        assert!(matches!(item_of(&e, "Cut").id, Action::Clip(Op::Move)));
    }

    #[test]
    fn the_background_menu_and_paste() {
        let dir = PathBuf::from(r"D:\work");
        let f = Facts { background: true, dir: dir.clone(), ..Default::default() };
        let e = own_entries(&f);
        assert_eq!(
            labels(&e),
            [
                "New folder",
                "Paste",
                "Paste shortcut",
                "-",
                "Refresh",
                "Copy folder path",
                "Open in new tab",
                "Open Terminal here",
                "-",
                "Properties"
            ]
        );
        assert!(!item_of(&e, "Paste").enabled && !item_of(&e, "Paste shortcut").enabled);
        assert_eq!(item_of(&e, "New folder").hint, "Ctrl+Shift+N");
        assert!(matches!(&item_of(&e, "Open Terminal here").id, Action::OpenTerminal(d) if *d == dir));
        let e = own_entries(&Facts { can_paste: true, can_paste_link: true, ..f });
        assert!(item_of(&e, "Paste").enabled && item_of(&e, "Paste shortcut").enabled);
    }

    #[test]
    fn plain_entries_keep_everything_when_lifted() {
        let e = lift(item(Action::Refresh, glyph::REFRESH, "Refresh").hint("F5").enabled(false).checked(true).danger());
        let Entry::Item(i) = e else { panic!() };
        assert!(matches!(i.id, Pick::Native(Action::Refresh)));
        assert_eq!((i.icon, i.hint.as_str(), i.enabled, i.checked, i.danger), (Some(glyph::REFRESH), "F5", false, true, true));
    }

    #[test]
    fn shell_entries_that_repeat_files_own_are_left_out() {
        let shell = |label: &str, verb: Option<&str>| Item { label: label.into(), verb: verb.map(Into::into), ..Default::default() };
        assert!(duplicate(&shell("Open in Terminal", None)));
        assert!(duplicate(&shell("Cut", Some("cut"))));
        assert!(duplicate(&shell("Ausschneiden", Some("cut"))));
        assert!(duplicate(&shell("Properties", Some("properties"))));
        assert!(!duplicate(&shell("Pin to Quick access", Some("pintohome"))));
        assert!(!duplicate(&shell("Open with Zed", None)));
        // A submenu is never dropped for its name.
        let send = Item { label: "Open".into(), cascade: Cascade::Pending, ..Default::default() };
        assert!(!duplicate(&send));
        assert!(!duplicate(&Item::separator()));
    }

    #[test]
    fn separators_are_only_between_entries() {
        let e: Vec<Entry<u8>> = vec![
            Entry::separator(),
            Entry::item(1, "a"),
            Entry::separator(),
            Entry::separator(),
            Entry::item(2, "b"),
            Entry::separator(),
        ];
        let kinds: Vec<bool> = tidy(e).iter().map(|e| matches!(e, Entry::Separator)).collect();
        assert_eq!(kinds, [false, true, false]);
        assert!(tidy::<u8>(vec![Entry::separator()]).is_empty());
    }

    #[test]
    fn shell_letters_are_marked_again() {
        assert_eq!(with_mnemonic("Open with", Some('h')), "Open wit&h");
        assert_eq!(with_mnemonic("Send to", Some('n')), "Se&nd to");
        assert_eq!(with_mnemonic("Tom & Jerry", Some('j')), "Tom && &Jerry");
        assert_eq!(with_mnemonic("Plain", None), "Plain");
        assert_eq!(with_mnemonic("Plain", Some('z')), "Plain");
    }

    #[test]
    fn the_menu_key_opens_by_the_cursor_row() {
        let screen = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let row = Rect::from_min_size(egui::pos2(100.0, 200.0), egui::vec2(400.0, 30.0));
        assert_eq!(key_point(Some(row), None, screen), egui::pos2(124.0, 215.0));
        assert_eq!(key_point(None, Some(egui::pos2(5.0, 5.0)), screen), egui::pos2(5.0, 5.0));
        assert_eq!(key_point(None, Some(egui::pos2(-5.0, 5.0)), screen), screen.center());
    }
}
