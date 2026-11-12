//! Everything the UI can ask for: actions, dialogs, renames, and what each action does.

use super::tab::Loc;
use shellman_core::fsops::{self, Entry, Op};
use shellman_core::recycle_bin::{BinState, Handle};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(super) enum Action {
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
    /// In the Recycle Bin: go to the folder the selected item came from and select it there.
    BinShowOriginal,
    SelectAll,
    /// Ctrl+H, as in Explorer: show or hide hidden files.
    ToggleHidden,
    SetView(crate::settings::ViewMode),
    /// The right button went down on a row (or the background, None): start reading its menu.
    MenuPress(Option<usize>),
    /// A right click on a row (or the background, None) at this point: open its menu. With
    /// Shift held, the extra entries Explorer shows then are asked for too.
    MenuOpen(Option<usize>, egui::Pos2),
    /// Shift+F10: the menu for the selection, or the folder.
    MenuKey,
    /// Go to the folder `path` is in and select it there ("Open file location").
    Reveal(PathBuf),
    /// Explorer's Properties sheet for the selected item, or the folder.
    Properties,
    /// In the Recycle Bin: put the selection back.
    BinRestore,
    /// In the Recycle Bin: delete the selection for good, after asking.
    BinDelete,
    /// In the Recycle Bin: delete everything for good, after asking.
    BinEmpty,
    /// Pin a folder to the sidebar, or unpin it when it is pinned.
    TogglePin(PathBuf),
    /// Hide, or bring back, an app's entry in the sidebar, by its id.
    HidePlace(String),
    ShowPlace(String),
}

pub(super) struct Rename {
    pub(super) path: PathBuf,
    pub(super) text: String,
    pub(super) focus: bool,
}

pub(super) enum Dialog {
    Recycle(Vec<PathBuf>),
    /// Items to delete for good, and their names for the question.
    BinDelete(Vec<Handle>, Vec<String>),
    /// Emptying the whole bin, with what is in it.
    BinEmpty(BinState),
}

impl super::FileMan {
    pub(super) fn apply(&mut self, action: Action) {
        let show_hidden = self.settings.show_hidden;
        if self.tabs[self.active].loc == Loc::Bin && super::bin::blocked(&action) {
            return;
        }
        match action {
            Action::TogglePin(p) => self.toggle_pin(p),
            Action::HidePlace(id) => self.set_place_hidden(&id, true),
            Action::ShowPlace(id) => self.set_place_hidden(&id, false),
            Action::BinRestore => self.bin_restore(),
            Action::BinShowOriginal => self.bin_show_original(),
            Action::BinDelete => self.bin_ask_delete(),
            Action::BinEmpty => self.bin_ask_empty(),
            Action::CopyPath if self.tabs[self.active].loc == Loc::Bin => self.bin_copy_paths(),
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
                let row = tab
                    .cursor
                    .filter(|&c| tab.selected.contains(&tab.entry(c).path));
                let entry = row.map(|r| tab.entry(r)).or_else(|| {
                    tab.selection()
                        .first()
                        .and_then(|e| tab.row_of(&e.path).map(|r| tab.entry(r)))
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
                let paths: Vec<PathBuf> = self.tabs[self.active]
                    .selection()
                    .into_iter()
                    .map(|e| e.path)
                    .collect();
                if !paths.is_empty() {
                    self.dialog = Some(Dialog::Recycle(paths));
                }
            }
            Action::Clip(op) => {
                let paths: Vec<PathBuf> = self.tabs[self.active]
                    .selection()
                    .into_iter()
                    .map(|e| e.path)
                    .collect();
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
                    tab.loc
                        .dir()
                        .map(|d| d.display().to_string())
                        .unwrap_or_default()
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
            Action::ToggleHidden => {
                self.settings.show_hidden = !self.settings.show_hidden;
                for t in &mut self.tabs {
                    t.dirty = true;
                }
                self.save_pending = true;
            }
            Action::SelectAll => {
                let tab = &mut self.tabs[self.active];
                tab.selected = tab
                    .view
                    .iter()
                    .map(|&i| tab.entries[i].path.clone())
                    .collect();
            }
            Action::MenuPress(row) => {
                if let Some(t) = self.menu_target(row) {
                    self.menu.warm(&t);
                }
            }
            Action::MenuOpen(row, pos) => self.open_menu(row, pos),
            Action::MenuKey => {
                let tab = &self.tabs[self.active];
                let row = tab.cursor.filter(|&c| tab.selected.contains(&tab.entry(c).path));
                let pos = self.ctx.input(|i| i.pointer.latest_pos()).unwrap_or_else(|| self.ctx.screen_rect().center());
                self.open_menu(row, pos);
            }
            Action::Reveal(path) => {
                if let Some(parent) = path.parent().filter(|p| p.is_dir()) {
                    self.navigate(Loc::Dir(parent.to_path_buf()));
                    self.tabs[self.active].select_after_load = Some((path, false));
                }
            }
            Action::Properties => {
                let tab = &self.tabs[self.active];
                let target = match tab.selection().first() {
                    Some(e) => Some(e.path.clone()),
                    None => tab.loc.dir().map(Path::to_path_buf),
                };
                if let Some(p) = target {
                    shellman_core::shell_menu::properties(self.hwnd, &p);
                }
            }
            Action::SetView(view) => {
                self.settings.view = view;
                self.tabs[self.active].scroll_to_cursor = true;
                self.save_pending = true;
            }
        }
        let _ = show_hidden;
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
    pub(super) fn arrive(&mut self) {
        let tab = &mut self.tabs[self.active];
        tab.entries.clear();
        tab.bin.clear();
        tab.view.clear();
        tab.selected.clear();
        tab.cursor = None;
        tab.anchor = None;
        tab.filter.clear();
        tab.list_offset = 0.0;
        self.thumbs.cancel_pending();
        self.bin_sort(self.active);
        self.rename = None;
        self.editing_path = None;
        self.save_pending = true;
        self.load(self.active);
    }

    /// What the menu for `row` (or the background) acts on: the selection when the row is in it,
    /// else just the row. Shift held asks for the extra entries.
    pub(super) fn menu_target(&self, row: Option<usize>) -> Option<shellman_core::shell_menu::Target> {
        let tab = &self.tabs[self.active];
        let folder = tab.loc.dir()?.to_path_buf();
        let items: Vec<PathBuf> = match row {
            None => Vec::new(),
            Some(r) if r >= tab.view.len() => return None,
            Some(r) if tab.selected.contains(&tab.entry(r).path) => {
                tab.selection().into_iter().map(|e| e.path).collect()
            }
            Some(r) => vec![tab.entry(r).path.clone()],
        };
        let extended = self.ctx.input(|i| i.modifiers.shift);
        Some(shellman_core::shell_menu::Target { folder, items, extended })
    }

    /// Open the menu for `row` (or the background) at `pos`. Files' own entries show this frame;
    /// the shell's join them as they arrive.
    fn open_menu(&mut self, row: Option<usize>, pos: egui::Pos2) {
        let Some(target) = self.menu_target(row) else { return };
        let background = row.is_none();
        if background {
            self.tabs[self.active].selected.clear();
        }
        let has_clip = self.clipboard.is_some();
        self.menu.open(&self.ctx, pos, target, background, has_clip);
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

    pub(super) fn commit_rename(&mut self) {
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
        let Some(parent) = r.path.parent() else {
            return;
        };
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
}
