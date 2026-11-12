//! The Recycle Bin as a place: loading it, restoring and deleting its items, emptying it, and the
//! buttons and menus for that.
//!
//! The bin rides on the ordinary tab. Each item becomes an `Entry` keyed by the path the bin
//! stores it at, so selection, sorting, filtering and thumbnails need nothing new; the original
//! location and the handle that restore and delete need sit beside it in `Tab::bin`. Opening an
//! item does nothing, so a file is never launched from the bin.

use super::actions::{Action, Dialog};
use super::tab::{Loc, SortKey};
use super::{FileMan, Msg};
use egui::{Align2, CornerRadius, Id, Rect, Sense, Ui};
use shellman_core::recycle_bin::{self, BinItem, BinState, Handle};
use shellman_ui::theme::{self, colors, glyph, icon_font};
use shellman_ui::widgets::menu_item;

/// Width of the toolbar slot the two buttons take, in place of the view toggle.
pub(super) const BUTTONS_W: f32 = 268.0;
const RESTORE_W: f32 = 96.0;

/// Which bin actions make sense now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Enabled {
    pub restore: bool,
    pub delete: bool,
    pub empty: bool,
}

/// `selected` items are chosen out of `total` in the bin (before any search filter).
pub(super) fn enabled(selected: usize, total: usize) -> Enabled {
    Enabled { restore: selected > 0, delete: selected > 0, empty: total > 0 }
}

/// Actions that mean nothing to the bin: there is nothing to open, rename, paste into or make a
/// folder in, and the Windows menu and Properties would act on the bin's stored copy.
pub(super) fn blocked(action: &Action) -> bool {
    matches!(
        action,
        Action::OpenRow(_)
            | Action::OpenSelected
            | Action::Rename
            | Action::Recycle
            | Action::Clip(_)
            | Action::Paste
            | Action::NewFolder
            | Action::MenuPress(_)
            | Action::MenuOpen(..)
            | Action::MenuKey
            | Action::Properties
    )
}

/// The question asked before items are deleted for good.
pub(super) fn delete_title(names: &[String]) -> String {
    match names {
        [one] => format!("Permanently delete \"{one}\"?"),
        many => format!("Permanently delete {} items?", many.len()),
    }
}

/// "Restored 3 items", "Deleted 1 item".
pub(super) fn done_text(verb: &str, count: usize) -> String {
    format!("{verb} {count} item{}", if count == 1 { "" } else { "s" })
}

impl FileMan {
    /// List the bin on a worker thread, and tell the folder watcher which folders to follow.
    pub(super) fn load_bin(&mut self, i: usize) {
        let tab = &mut self.tabs[i];
        let (tx, ctx, id, gen) = (self.tx.clone(), self.ctx.clone(), tab.id, tab.gen);
        std::thread::spawn(move || {
            let result = recycle_bin::list();
            let dirs = recycle_bin::watch_dirs();
            let _ = tx.send(Msg::Bin { tab: id, gen, dirs, result });
            ctx.request_repaint();
        });
    }

    /// A listing arrived.
    pub(super) fn bin_loaded(&mut self, tab: u64, gen: u64, result: Result<Vec<BinItem>, String>) {
        let Some(t) = self.tabs.iter_mut().find(|t| t.id == tab && t.gen == gen) else {
            return;
        };
        t.loading = false;
        match result {
            Ok(items) => {
                t.set_entries(items.iter().map(BinItem::to_entry).collect());
                t.bin = items.into_iter().map(|i| (i.path.clone(), i)).collect();
                t.rebuild_view(false);
            }
            Err(e) => {
                t.error = Some(e);
                t.entries.clear();
                t.view.clear();
                t.bin.clear();
            }
        }
    }

    /// Re-list every tab that shows the bin.
    pub(super) fn reload_bins(&mut self) {
        for i in 0..self.tabs.len() {
            if self.tabs[i].loc == Loc::Bin {
                self.load(i);
            }
        }
    }

    /// Opening the bin lists newest first, which is what you look for there; leaving it puts the
    /// tab's own sort back.
    pub(super) fn bin_sort(&mut self, i: usize) {
        let tab = &mut self.tabs[i];
        if tab.loc == Loc::Bin {
            if tab.sort_before_bin.is_none() {
                tab.sort_before_bin = Some((tab.sort, tab.ascending));
            }
            (tab.sort, tab.ascending) = (SortKey::Modified, false);
            tab.dirty = true;
        } else if let Some((sort, ascending)) = tab.sort_before_bin.take() {
            (tab.sort, tab.ascending) = (sort, ascending);
            tab.dirty = true;
        }
    }

    /// The handles of the selected items.
    fn bin_selection(&self) -> Vec<(String, Handle)> {
        let tab = &self.tabs[self.active];
        tab.selection()
            .iter()
            .filter_map(|e| tab.bin.get(&e.path))
            .map(|i| (i.name.clone(), i.handle.clone()))
            .collect()
    }

    pub(super) fn bin_restore(&mut self) {
        let items = self.bin_selection();
        if items.is_empty() {
            return;
        }
        let (count, hwnd) = (items.len(), self.hwnd);
        let handles: Vec<Handle> = items.into_iter().map(|(_, h)| h).collect();
        self.run_bin(done_text("Restored", count), move || recycle_bin::restore(hwnd, &handles));
    }

    /// Ask before deleting the selection for good.
    pub(super) fn bin_ask_delete(&mut self) {
        let items = self.bin_selection();
        if !items.is_empty() {
            let (names, handles) = items.into_iter().unzip();
            self.dialog = Some(Dialog::BinDelete(handles, names));
        }
    }

    pub(super) fn bin_delete(&mut self, handles: Vec<Handle>) {
        let (count, hwnd) = (handles.len(), self.hwnd);
        self.run_bin(done_text("Deleted", count) + " for good", move || recycle_bin::delete(hwnd, &handles));
    }

    /// Ask how much is in the bin, then confirm. The count comes back as `Msg::BinState`.
    pub(super) fn bin_ask_empty(&self) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::BinState(recycle_bin::state()));
            ctx.request_repaint();
        });
    }

    pub(super) fn bin_empty(&mut self) {
        let hwnd = self.hwnd;
        self.run_bin("Recycle Bin emptied".into(), move || recycle_bin::empty(hwnd));
    }

    /// Run a bin operation on a worker thread; its result arrives as `Msg::BinDone`.
    fn run_bin(&self, done: String, work: impl FnOnce() -> Result<(), String> + Send + 'static) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let result = work().map(|()| done);
            let _ = tx.send(Msg::BinDone(result));
            ctx.request_repaint();
        });
    }

    /// What the bin says about itself, for the confirmation of an empty.
    pub(super) fn bin_state_arrived(&mut self, state: Option<BinState>) {
        match state {
            Some(s) if !s.is_empty() => self.dialog = Some(Dialog::BinEmpty(s)),
            Some(_) => self.toast("Recycle Bin is already empty"),
            None => self.toast("Couldn't read the Recycle Bin"),
        }
    }

    /// Go to the folder the first selected item was deleted from and select the item's name there.
    /// The item itself is in the bin, so that folder may not have it; the folder must exist.
    pub(super) fn bin_show_original(&mut self) {
        let tab = &self.tabs[self.active];
        let original = tab
            .selection()
            .first()
            .and_then(|e| tab.bin.get(&e.path))
            .map(|i| i.original.clone());
        let Some(original) = original else { return };
        match original.parent() {
            Some(parent) if parent.is_dir() => {
                self.apply(Action::Navigate(Loc::Dir(parent.to_path_buf())));
                self.tabs[self.active].select_after_load = Some((original, false));
            }
            _ => self.toast("That folder no longer exists"),
        }
    }

    /// The original locations of the selection, one per line, for the clipboard.
    pub(super) fn bin_copy_paths(&mut self) {
        let tab = &self.tabs[self.active];
        let text: Vec<String> = tab
            .selection()
            .iter()
            .filter_map(|e| tab.bin.get(&e.path))
            .map(|i| format!("\"{}\"", i.original.display()))
            .collect();
        if !text.is_empty() {
            self.ctx.copy_text(text.join(" "));
            self.toast("Original path copied");
        }
    }

    /// Restore and Empty, as two pills in the toolbar where the view toggle sits elsewhere.
    pub(super) fn bin_buttons(&self, ui: &mut Ui, rect: Rect, actions: &mut Vec<Action>) {
        let tab = &self.tabs[self.active];
        let on = enabled(tab.selected.len(), tab.entries.len());
        let (restore, empty) = rect.split_left_right_at_x(rect.left() + RESTORE_W);
        let empty = Rect::from_min_max(egui::pos2(empty.left() + 8.0, empty.top()), empty.max);
        if pill(ui, restore, glyph::UNDO, "Restore", on.restore, "Restore the selected items") {
            actions.push(Action::BinRestore);
        }
        if pill(ui, empty, glyph::DELETE, "Empty Recycle Bin", on.empty, "Delete everything in the bin for good") {
            actions.push(Action::BinEmpty);
        }
    }
}

/// A rounded button with a glyph and a label. True when clicked.
fn pill(ui: &mut Ui, rect: Rect, g: char, label: &str, enabled: bool, tip: &str) -> bool {
    let c = colors();
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let resp = ui.interact(rect, Id::new(("bin-pill", label)), sense).on_hover_text(tip);
    let fill = if enabled && resp.hovered() { c.control_hover } else { c.control };
    ui.painter().rect_filled(rect, CornerRadius::same(16), fill);
    let color = if enabled { c.text } else { theme::with_alpha(c.muted, 90) };
    ui.painter().text(
        egui::pos2(rect.left() + 14.0, rect.center().y),
        Align2::LEFT_CENTER,
        g,
        icon_font(13.0),
        if enabled { c.accent_text } else { color },
    );
    ui.painter().text(
        egui::pos2(rect.left() + 36.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        color,
    );
    enabled && resp.clicked()
}

/// The right-click menu of an item in the bin.
pub(super) fn item_menu(ui: &mut Ui, actions: &mut Vec<Action>) {
    if menu_item(ui, glyph::UNDO, "Restore", "") {
        actions.push(Action::BinRestore);
        ui.close_menu();
    }
    ui.separator();
    if menu_item(ui, glyph::DELETE, "Delete permanently", "Del") {
        actions.push(Action::BinDelete);
        ui.close_menu();
    }
    if menu_item(ui, glyph::FOLDER, "Show in original folder", "") {
        actions.push(Action::BinShowOriginal);
        ui.close_menu();
    }
    if menu_item(ui, glyph::COPY, "Copy original path", "Ctrl+Shift+C") {
        actions.push(Action::CopyPath);
        ui.close_menu();
    }
}

/// The right-click menu of the empty space in the bin.
pub(super) fn backdrop_menu(ui: &mut Ui, no_items: bool, actions: &mut Vec<Action>) {
    if !no_items && menu_item(ui, glyph::DELETE, "Empty Recycle Bin", "") {
        actions.push(Action::BinEmpty);
        ui.close_menu();
    }
    if menu_item(ui, glyph::REFRESH, "Refresh", "F5") {
        actions.push(Action::Refresh);
        ui.close_menu();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_follow_selection_and_contents() {
        assert_eq!(enabled(0, 0), Enabled { restore: false, delete: false, empty: false });
        assert_eq!(enabled(0, 5), Enabled { restore: false, delete: false, empty: true });
        assert_eq!(enabled(2, 5), Enabled { restore: true, delete: true, empty: true });
    }

    #[test]
    fn nothing_in_the_bin_opens_or_changes() {
        for a in [Action::OpenSelected, Action::OpenRow(0), Action::Rename, Action::Paste, Action::NewFolder] {
            assert!(blocked(&a), "{a:?}");
        }
        assert!(blocked(&Action::MenuKey) && blocked(&Action::MenuPress(None)) && blocked(&Action::Properties));
        for a in [Action::Refresh, Action::SelectAll, Action::BinDelete, Action::BinRestore, Action::BinEmpty, Action::CopyPath] {
            assert!(!blocked(&a), "{a:?}");
        }
    }

    #[test]
    fn confirmation_and_result_text() {
        assert_eq!(delete_title(&["a.txt".into()]), "Permanently delete \"a.txt\"?");
        assert_eq!(delete_title(&["a".into(), "b".into()]), "Permanently delete 2 items?");
        assert_eq!(done_text("Restored", 1), "Restored 1 item");
        assert_eq!(done_text("Restored", 3), "Restored 3 items");
    }
}
