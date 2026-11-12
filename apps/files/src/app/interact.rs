//! What the list and the grid share: the folder's empty area and its menu, the error and empty
//! states, clicks on items, the inline rename box, and scrolling to the cursor.

use super::actions::{Action, Rename};
use super::select;
use super::tab::{Band, Tab};
use egui::{
    Color32, Id, Key, Modifiers, PointerButton, Rect, ScrollArea, Sense, Ui, Vec2,
};
use shellman_ui::theme::{self, colors, glyph};
use shellman_ui::widgets::select_range;
use std::path::Path;

/// The keys held for a click this frame. egui's `modifiers` is the state at the end of the
/// frame's input, which misses a Ctrl let go in the same frame as the click, and a Ctrl pressed
/// while another window had the keyboard (the click that activates the window): so the state
/// carried by the button's own release event is taken, and what Windows says is held now is
/// added to it.
pub(super) fn click_mods(ui: &Ui) -> select::Mods {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_SHIFT};
    let at_release = ui.input(|i| {
        i.events
            .iter()
            .rev()
            .find_map(|e| match e {
                egui::Event::PointerButton {
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers,
                    ..
                } => Some(*modifiers),
                _ => None,
            })
            .unwrap_or(i.modifiers)
    });
    let down = |vk: u16| unsafe { GetAsyncKeyState(i32::from(vk)) } as u16 & 0x8000 != 0;
    mods_of(at_release, down(VK_CONTROL.0), down(VK_SHIFT.0))
}

/// egui's modifiers and the keys Windows says are down, as one.
fn mods_of(m: Modifiers, ctrl_down: bool, shift_down: bool) -> select::Mods {
    select::Mods {
        ctrl: m.command || m.ctrl || ctrl_down,
        shift: m.shift || shift_down,
    }
}

/// What happened to items this frame, applied after drawing so the tab is not borrowed twice.
#[derive(Default)]
pub(super) struct ItemEvents {
    clicked: Option<(usize, select::Mods)>,
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
            self.clicked = Some((r, click_mods(ui)));
        }
        if resp.secondary_clicked() {
            self.secondary = Some(r);
        }
    }

    /// Update the selection (see `select::click`). A right click on an unselected item selects
    /// it, so its menu acts on what it was opened on.
    pub(super) fn apply(&self, tab: &mut Tab) {
        if let Some((r, m)) = self.clicked {
            if r < tab.view.len() {
                let sel = select::click(&tab.sel(), r, m);
                tab.set_sel(sel);
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

/// A state in place of the items, in the middle of what is left of `ui`: a large faint glyph, a
/// line saying what it is, and a muted note under it.
pub(super) fn state_message(ui: &mut Ui, g: char, title: &str, note: &str) {
    let c = colors();
    let area = ui.available_rect_before_wrap();
    let centre = egui::pos2(area.center().x, area.top() + (area.height() * 0.42).max(70.0));
    let p = ui.painter();
    p.text(
        centre - egui::vec2(0.0, 34.0),
        egui::Align2::CENTER_CENTER,
        g,
        theme::icon_font(34.0),
        theme::with_alpha(c.muted, 150),
    );
    p.text(
        centre + egui::vec2(0.0, 8.0),
        egui::Align2::CENTER_CENTER,
        title,
        theme::semibold_font(15.0),
        c.text,
    );
    if !note.is_empty() {
        let room = Rect::from_center_size(
            centre + egui::vec2(0.0, 32.0),
            egui::vec2(area.width().min(520.0) - 24.0, 20.0),
        );
        let font = egui::FontId::proportional(12.5);
        let w = ui
            .fonts(|f| f.layout_no_wrap(note.to_owned(), font.clone(), c.muted))
            .size()
            .x
            .min(room.width());
        let line = Rect::from_center_size(room.center(), egui::vec2(w + 1.0, room.height()));
        shellman_ui::widgets::draw_text(ui, line, note, font, c.muted, false);
    }
}

/// Shows why a folder can't be listed. True when it did, and there is nothing else to draw.
pub(super) fn error_state(ui: &mut Ui, tab: &Tab) -> bool {
    let Some(err) = &tab.error else { return false };
    let denied = err.contains("os error 5") || err.to_lowercase().contains("access is denied");
    let (title, note) = if denied {
        ("You don't have access to this folder", "Windows refused to list it (access is denied).".to_owned())
    } else {
        ("Can't open this folder", err.clone())
    };
    state_message(ui, if denied { glyph::LOCK } else { glyph::INFO }, title, &note);
    true
}

/// While a folder is first being listed: a quiet note. True when it showed.
pub(super) fn loading_state(ui: &mut Ui, tab: &Tab) -> bool {
    if !(tab.loading && tab.view.is_empty() && tab.entries.is_empty()) {
        return false;
    }
    state_message(ui, glyph::REFRESH, "Loading…", "");
    true
}

/// Shows that a folder, or a search in it, has nothing. True when it did.
pub(super) fn empty_state(ui: &mut Ui, tab: &Tab) -> bool {
    if !tab.view.is_empty() || tab.loading {
        return false;
    }
    let bin = tab.loc == super::tab::Loc::Bin;
    match (bin, tab.filter.is_empty()) {
        (true, true) => state_message(
            ui,
            glyph::DELETE,
            "Recycle Bin is empty",
            "Deleted files show up here until you empty it.",
        ),
        (_, true) => state_message(ui, glyph::FOLDER, "This folder is empty", "Paste or drop files here."),
        _ => state_message(
            ui,
            glyph::SEARCH,
            "Nothing matches",
            &format!("Nothing here has \"{}\" in its name.", tab.filter.trim()),
        ),
    }
    true
}

/// The id of the space behind the items, which takes the rubber band's drags.
fn backdrop_id() -> Id {
    Id::new("folder-bg")
}

/// The space behind the items, all of the content island: a click clears the selection, a drag
/// draws a rectangle that selects (see `rubber_band`), and a right click offers what can be done
/// to the folder itself. Call before drawing items, so items sit on top of it. Its drags never
/// reach the gaps under it, so the island never moves the window.
pub(super) fn folder_backdrop(ui: &mut Ui, island: Rect, tab: &mut Tab, actions: &mut Vec<Action>) {
    let bg = ui.interact(island, backdrop_id(), Sense::click_and_drag());
    if bg.clicked() && !click_mods(ui).ctrl {
        tab.selected.clear();
        tab.cursor = None;
    }
    if bg.drag_started_by(PointerButton::Primary) {
        if let Some(at) = ui.input(|i| i.pointer.press_origin()).or(bg.interact_pointer_pos()) {
            let add = click_mods(ui).ctrl;
            let before = if add { tab.sel().rows } else { Default::default() };
            if !add {
                tab.selected.clear();
            }
            tab.band = Some(Band {
                from: to_content(tab, at),
                before,
                add,
            });
        }
    }
    if tab.loc == super::tab::Loc::Bin {
        bg.context_menu(|ui| super::bin::backdrop_menu(ui, tab.view.is_empty(), actions));
        return;
    }
    menu_clicks(ui, &bg, actions, None);
}

/// A point on screen in the scrolled content's coordinates (see `select::Layout`), by where the
/// view was last drawn.
fn to_content(tab: &Tab, p: egui::Pos2) -> egui::Pos2 {
    egui::pos2(p.x - tab.view_rect.left(), p.y - tab.view_rect.top() + tab.list_offset)
}

/// The rubber band, after the items are drawn: select what it touches, draw it, and scroll when
/// it reaches the top or bottom of the view. Ends when the button is let go.
pub(super) fn rubber_band(ui: &Ui, tab: &mut Tab, layout: select::Layout) {
    let Some(band) = tab.band.clone() else { return };
    let dragging = ui.ctx().dragged_id() == Some(backdrop_id());
    let pointer = ui.input(|i| i.pointer.latest_pos());
    let (true, Some(at)) = (dragging, pointer) else {
        tab.band = None;
        return;
    };
    let now = to_content(tab, at);
    let rect = select::band(band.from, now);
    let hits = layout.hits(rect);
    let rows = select::with_band(&band.before, &hits, band.add);
    let cursor = hits.last().copied().or(tab.cursor);
    tab.set_sel(select::Sel {
        rows,
        anchor: hits.first().copied().or(tab.anchor),
        cursor,
    });
    // On screen, inside the view.
    let view = tab.view_rect;
    let screen = Rect::from_two_pos(
        egui::pos2(band.from.x + view.left(), band.from.y - tab.list_offset + view.top()),
        at,
    )
    .intersect(view);
    let c = colors();
    let fill = shellman_ui::glass::islands::shows_as(theme::with_alpha(c.accent_fill, 40));
    ui.painter().with_clip_rect(view).rect(
        screen,
        egui::CornerRadius::same(4),
        fill,
        egui::Stroke::new(1.0_f32, theme::with_alpha(c.accent_fill, 220)),
        egui::StrokeKind::Inside,
    );
    let dt = ui.input(|i| i.stable_dt);
    let step = select::edge_scroll(at.y, view.top(), view.bottom(), dt);
    if step != 0.0 {
        let lines = layout.count.div_ceil(layout.cols.max(1)) as f32;
        let max = (lines * layout.step.y - view.height()).max(0.0);
        let to = (tab.list_offset + step).clamp(0.0, max);
        if to != tab.list_offset {
            tab.scroll_to = Some(to);
            ui.ctx().request_repaint();
        }
    }
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
    // Dragging the empty space draws a rectangle; it does not scroll.
    let mut scroll = ScrollArea::vertical()
        .auto_shrink([false, false])
        .drag_to_scroll(false);
    if let Some(off) = tab.scroll_to.take() {
        return scroll.vertical_scroll_offset(off);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_held_counts_from_either_source() {
        let none = Modifiers::NONE;
        assert_eq!(mods_of(none, false, false), select::Mods::default());
        // Let go within the frame: egui's event still says Ctrl.
        assert!(mods_of(Modifiers::CTRL, false, false).ctrl);
        // Pressed while another window had the keyboard: Windows says it is down.
        let m = mods_of(none, true, true);
        assert!(m.ctrl && m.shift);
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
