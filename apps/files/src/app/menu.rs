//! The right-click menu of files, folders and a folder's background. Files' own entries come
//! first and show at once; under them, the shell's (what Windows and installed apps add) appear
//! as soon as the worker has read them, often before the click, since a walk starts when the
//! right button goes down and when the selection settles.

use super::actions::Action;
use super::tab::Loc;
use egui::{ColorImage, Context, Pos2, TextureHandle, TextureOptions, Vec2};
use shellman_core::fsops::Op;
use shellman_core::shell_menu::{Cascade, Item, Loader, Palette, Route, Target};
use shellman_ui::menu::{Event, Icon, Kind, Menu, Row};
use shellman_ui::theme::{colors, glyph};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// The selection has to stay this long before its menu is read ahead of a right click.
const SETTLE: f64 = 0.3;
/// Icon textures kept; past this they are dropped and made again as needed.
const MAX_TEXTURES: usize = 512;

/// What a menu row does.
#[derive(Clone)]
pub(super) enum Pick {
    Native(Action),
    /// The shell's entry at this path in the loader's view.
    Shell(Vec<usize>),
    /// A shell submenu at this path, left unfilled because it was slow, is opening.
    Expand(Vec<usize>),
}

/// What the open menu is for.
struct Open {
    target: Target,
    /// The single folder the item menu is for, if it is one.
    is_dir: bool,
    background: bool,
    has_clip: bool,
}

pub(super) struct ContextMenu {
    menu: Menu,
    loader: Loader,
    open: Option<Open>,
    textures: HashMap<u64, TextureHandle>,
    /// The target last seen, since when, and whether its walk was asked for yet.
    settling: Option<(Target, f64, bool)>,
}

impl ContextMenu {
    pub(super) fn new(ctx: &Context, hwnd: isize) -> Self {
        let ctx2 = ctx.clone();
        Self {
            menu: Menu::default(),
            loader: Loader::new(hwnd, Arc::new(move || ctx2.request_repaint())),
            open: None,
            textures: HashMap::new(),
            settling: None,
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.menu.is_open()
    }

    /// Start reading the menu for `target` now: the right button just went down.
    pub(super) fn warm(&mut self, target: &Target) {
        self.loader.want(target);
    }

    /// Read the menu for the current selection once it has settled, so it is usually ready
    /// before the right click. Returns how long until it settles, while it has not.
    pub(super) fn settle(&mut self, target: Option<Target>, now: f64) -> Option<f64> {
        let Some(target) = target else {
            self.settling = None;
            return None;
        };
        match &self.settling {
            Some((t, _, _)) if *t == target => {}
            _ => self.settling = Some((target, now, false)),
        }
        let (t, since, asked) = self.settling.as_mut()?;
        if *asked || self.menu.is_open() {
            return None;
        }
        let left = SETTLE - (now - *since);
        if left > 0.0 {
            return Some(left);
        }
        *asked = true;
        let t = t.clone();
        self.loader.want(&t);
        None
    }

    /// Open the menu at `pos` for `target`.
    pub(super) fn open(&mut self, ctx: &Context, pos: Pos2, target: Target, background: bool, has_clip: bool) {
        let is_dir = target.items.len() == 1 && target.items[0].is_dir();
        self.loader.refresh(&target);
        self.menu.open_at(ctx, pos);
        self.open = Some(Open { target, is_dir, background, has_clip });
    }

    /// Each frame: take in what the worker sent, draw the menu if open, and turn a pick into
    /// actions.
    pub(super) fn show(&mut self, ctx: &Context, actions: &mut Vec<Action>) -> Option<String> {
        let c = colors();
        let rgb = |c: egui::Color32| u32::from(c.r()) | u32::from(c.g()) << 8 | u32::from(c.b()) << 16;
        self.loader.set_palette(Palette { text: rgb(c.text), background: rgb(c.popup) });
        self.loader.poll();
        let notice = self.loader.take_notice();
        let Some(open) = &self.open else { return notice };
        if !self.menu.is_open() {
            self.open = None;
            return notice;
        }
        let mut rows = native_rows(ctx, open);
        if let Some(view) = self.loader.view(&open.target) {
            let shell = shell_rows(ctx, &mut self.textures, view.items, &[]);
            if !shell.is_empty() {
                rows.push(Row::separator());
                rows.extend(shell);
            } else if view.loading {
                rows.push(Row::separator());
                rows.push(Row::note("Loading more options…"));
            }
            if let Some(note) = view.note {
                rows.push(Row::separator());
                rows.push(Row::note(note));
            }
        }
        let event = self.menu.show(ctx, egui::Id::new("files-context-menu"), &rows);
        match event {
            Some(Event::Picked(Pick::Native(a))) => actions.push(a),
            Some(Event::Picked(Pick::Shell(path))) => self.pick_shell(&path, actions),
            Some(Event::Opened(Pick::Expand(path))) => {
                let open = self.open.as_ref().unwrap();
                let item = self.loader.view(&open.target).and_then(|v| chain(v.items, &path).last().cloned().cloned());
                if let Some(item) = item {
                    self.loader.expand(&open.target, &item);
                }
            }
            _ => {}
        }
        if !self.menu.is_open() {
            self.open = None;
        }
        notice
    }

    fn pick_shell(&mut self, path: &[usize], actions: &mut Vec<Action>) {
        let Some(open) = self.open.take() else { return };
        let Some(view) = self.loader.view(&open.target) else { return };
        let fresh = view.fresh;
        let owned: Vec<Item> = chain(view.items, path).into_iter().cloned().collect();
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

/// Files' own entries.
fn native_rows(ctx: &Context, open: &Open) -> Vec<Row<Pick>> {
    let act = |g, label: &str, key: &str, a: Action| Row::action(Icon::Glyph(g), label, key, Pick::Native(a));
    let mut rows = Vec::new();
    if open.background {
        rows.push(act(glyph::NEW_FOLDER, "New folder", "Ctrl+Shift+N", Action::NewFolder));
        if open.has_clip {
            rows.push(act(glyph::PASTE, "Paste", "Ctrl+V", Action::Paste));
        }
        rows.push(Row::separator());
        rows.push(act(glyph::REFRESH, "Refresh", "F5", Action::Refresh));
        rows.push(act(glyph::COPY, "Copy folder path", "", Action::CopyPath));
        let dir = open.target.folder.clone();
        rows.push(act(glyph::ADD, "Open in new tab", "", Action::NewTab(Loc::Dir(dir))));
    } else {
        rows.push(act(glyph::OPEN_EXTERNAL, "Open", "Enter", Action::OpenSelected));
        if open.is_dir {
            let dir: PathBuf = open.target.items[0].clone();
            rows.push(act(glyph::ADD, "Open in new tab", "", Action::NewTab(Loc::Dir(dir.clone()))));
            rows.push(act(glyph::PIN, super::pins::menu_label(ctx, &dir), "", Action::TogglePin(dir)));
        }
        rows.push(Row::separator());
        rows.push(act(glyph::CUT, "Cut", "Ctrl+X", Action::Clip(Op::Move)));
        rows.push(act(glyph::COPY, "Copy", "Ctrl+C", Action::Clip(Op::Copy)));
        rows.push(act(glyph::COPY, "Copy path", "Ctrl+Shift+C", Action::CopyPath));
        rows.push(Row::separator());
        rows.push(act(glyph::RENAME, "Rename", "F2", Action::Rename));
        rows.push(act(glyph::DELETE, "Move to Recycle Bin", "Del", Action::Recycle));
    }
    rows.push(Row::separator());
    rows.push(act(glyph::INFO, "Properties", "Alt+Enter", Action::Properties));
    rows
}

/// The shell's entries as rows, `prefix` being the path of the submenu they are in.
fn shell_rows(ctx: &Context, textures: &mut HashMap<u64, TextureHandle>, items: &[Item], prefix: &[usize]) -> Vec<Row<Pick>> {
    let mut rows = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        if item.separator {
            rows.push(Row::separator());
            continue;
        }
        let mut path = prefix.to_vec();
        path.push(i);
        let ppp = ctx.pixels_per_point();
        let kind = if let Some(pic) = &item.drawn {
            let size = Vec2::new(pic.size[0] as f32, pic.size[1] as f32) / ppp;
            Kind::Picture(texture(ctx, textures, pic).id(), size, Pick::Shell(path.clone()))
        } else if item.is_submenu() {
            let mut sub = shell_rows(ctx, textures, &item.submenu, &path);
            let mut on_open = None;
            match item.cascade {
                Cascade::Pending if sub.is_empty() => sub.push(Row::note("Loading…")),
                Cascade::Deferred => {
                    sub.push(Row::note("Loading…"));
                    on_open = Some(Pick::Expand(path.clone()));
                }
                _ => {}
            }
            Kind::Submenu { rows: sub, on_open }
        } else {
            Kind::Action(Pick::Shell(path.clone()))
        };
        let icon = match &item.icon {
            Some(pic) => Icon::Image(texture(ctx, textures, pic).id(), Vec2::splat(16.0)),
            None => Icon::None,
        };
        rows.push(Row {
            icon,
            label: item.title(),
            shortcut: item.shortcut.clone(),
            enabled: item.enabled,
            checked: item.checked,
            mnemonic: item.mnemonic,
            kind,
        });
    }
    rows
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
