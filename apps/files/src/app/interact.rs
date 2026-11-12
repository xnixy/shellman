//! What the list and the grid share: the folder's empty area and its menu, the error and empty
//! states, clicks on items, the inline rename box, and scrolling to the cursor.

use super::actions::{Action, Rename};
use super::tab::Tab;
use egui::{
    Color32, Id, Key, Modifiers, PointerButton, Rect, RichText, ScrollArea, Sense, Ui, Vec2,
};
use shellman_ui::theme::{self, colors, glyph};
use shellman_ui::widgets::select_range;
use std::path::Path;

/// What happened to items this frame, applied after drawing so the tab is not borrowed twice.
#[derive(Default)]
pub(super) struct ItemEvents {
    clicked: Option<(usize, Modifiers)>,
    secondary: Option<usize>,
    pub(super) commit_rename: bool,
    pub(super) cancel_rename: bool,
}

impl ItemEvents {
    /// Note a click, double click, or right click on item `r`.
    pub(super) fn record(
        &mut self,
        ui: &Ui,
        resp: &egui::Response,
        r: usize,
        actions: &mut Vec<Action>,
    ) {
        if resp.double_clicked() {
            actions.push(Action::OpenRow(r));
        } else if resp.clicked() {
            self.clicked = Some((r, ui.input(|i| i.modifiers)));
        }
        if resp.secondary_clicked() {
            self.secondary = Some(r);
        }
    }

    /// Update the selection: Ctrl toggles, Shift extends, a plain click selects one. A right click
    /// on an unselected item selects it, so its menu acts on what it was opened on.
    pub(super) fn apply(&self, tab: &mut Tab) {
        if let Some((r, m)) = self.clicked {
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
        if let Some(r) = self.secondary {
            if !tab.selected.contains(&tab.entry(r).path) {
                tab.select_only(r);
            }
        }
    }
}

/// The right-click menu of item `r`. In the bin, its own short menu; elsewhere Files' menu with
/// the shell's entries, whose walk starts as soon as the button goes down.
pub(super) fn item_menu(
    ui: &Ui,
    resp: &egui::Response,
    actions: &mut Vec<Action>,
    r: usize,
    bin: bool,
) {
    if bin {
        resp.context_menu(|ui| super::bin::item_menu(ui, actions));
    } else {
        menu_clicks(ui, resp, actions, Some(r));
    }
}

/// Right button down on `resp`: read its menu ahead. Right click: open it.
fn menu_clicks(ui: &Ui, resp: &egui::Response, actions: &mut Vec<Action>, row: Option<usize>) {
    let pressed = ui.input(|i| i.pointer.button_pressed(PointerButton::Secondary));
    if pressed && resp.hovered() {
        actions.push(Action::MenuPress(row));
    }
    if resp.secondary_clicked() {
        let pos = resp
            .interact_pointer_pos()
            .or_else(|| ui.input(|i| i.pointer.latest_pos()))
            .unwrap_or(resp.rect.center());
        actions.push(Action::MenuOpen(row, pos));
    }
}

/// Shows why a folder can't be listed. True when it did, and there is nothing else to draw.
pub(super) fn error_state(ui: &mut Ui, tab: &Tab) -> bool {
    let Some(err) = &tab.error else { return false };
    let c = colors();
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new("Can't open this folder")
                .size(16.0)
                .color(c.text),
        );
        ui.label(theme::muted(err.clone()));
    });
    true
}

/// Shows that a folder, or a search in it, has nothing. True when it did.
pub(super) fn empty_state(ui: &mut Ui, tab: &Tab) -> bool {
    if !tab.view.is_empty() || tab.loading {
        return false;
    }
    let bin = tab.loc == super::tab::Loc::Bin;
    ui.add_space(if bin { 80.0 } else { 40.0 });
    ui.vertical_centered(|ui| {
        let msg = match (bin, tab.filter.is_empty()) {
            (true, true) => "Recycle Bin is empty",
            (_, true) => "This folder is empty",
            _ => "Nothing matches",
        };
        if bin && tab.filter.is_empty() {
            let c = colors();
            ui.label(theme::icon(glyph::DELETE, 34.0).color(theme::with_alpha(c.muted, 140)));
            ui.add_space(10.0);
            ui.label(RichText::new(msg).size(16.0).color(c.text));
            ui.label(theme::muted(
                "Deleted files show up here until you empty it.",
            ));
        } else {
            ui.label(theme::muted(msg));
        }
    });
    true
}

/// The space behind the items: a click clears the selection, a right click offers what can be
/// done to the folder itself. Call before drawing items, so items sit on top of it.
pub(super) fn folder_backdrop(ui: &mut Ui, tab: &mut Tab, actions: &mut Vec<Action>) {
    let bg = ui.interact(
        ui.available_rect_before_wrap(),
        Id::new("folder-bg"),
        // Drags too, so a drag here does not reach the gaps under it and move the window.
        Sense::click_and_drag(),
    );
    if bg.clicked() {
        tab.selected.clear();
        tab.cursor = None;
    }
    if tab.loc == super::tab::Loc::Bin {
        bg.context_menu(|ui| super::bin::backdrop_menu(ui, tab.view.is_empty(), actions));
        return;
    }
    menu_clicks(ui, &bg, actions, None);
}

/// The text box for renaming an item in place. The name is selected without its extension.
/// Enter or clicking away commits; Escape cancels.
pub(super) fn rename_box(
    ui: &mut Ui,
    rect: Rect,
    rn: &mut Rename,
    is_dir: bool,
    ev: &mut ItemEvents,
) {
    let id = Id::new("rename");
    let te = ui.put(
        rect,
        egui::TextEdit::singleline(&mut rn.text)
            .id(id)
            .margin(Vec2::new(6.0, 3.0)),
    );
    if std::mem::take(&mut rn.focus) {
        te.request_focus();
        let stem = if is_dir {
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
            ev.cancel_rename = true;
        } else {
            ev.commit_rename = true;
        }
    }
}

/// A vertical scroll area that, when asked, brings the cursor's line into view. `per_line` is
/// how many items share a line: 1 in the list, the column count in the grid.
pub(super) fn scroll_to_cursor(tab: &mut Tab, line_h: f32, per_line: usize) -> ScrollArea {
    let mut scroll = ScrollArea::vertical().auto_shrink([false, false]);
    if std::mem::take(&mut tab.scroll_to_cursor) {
        if let Some(cur) = tab.cursor {
            let y = (cur / per_line.max(1)) as f32 * line_h;
            let mut off = tab.list_offset;
            if y < off {
                off = y;
            } else if y + line_h > off + tab.list_height {
                off = y + line_h - tab.list_height;
            }
            scroll = scroll.vertical_scroll_offset(off.max(0.0));
        }
    }
    scroll
}

/// Hidden items, and items cut to the clipboard, are drawn faded.
pub(super) fn fade(hidden: bool, cut: bool) -> impl Fn(Color32) -> Color32 {
    move |col: Color32| {
        let col = if hidden {
            col.gamma_multiply(0.55)
        } else {
            col
        };
        if cut {
            col.gamma_multiply(0.5)
        } else {
            col
        }
    }
}

impl super::FileMan {
    /// Finish a rename the list or grid reported.
    pub(super) fn finish_rename(&mut self, ev: &ItemEvents) {
        if ev.cancel_rename {
            self.rename = None;
        } else if ev.commit_rename {
            self.commit_rename();
        }
    }
}
