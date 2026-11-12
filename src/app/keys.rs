//! Keyboard shortcuts.

use super::actions::Action;
use super::grid::GRID_LINE_H;
use super::ROW_H;
use crate::settings::ViewMode;
use crate::fsops::Op;
use egui::{Key, Modifiers, PointerButton};

impl super::FileMan {
    pub(super) fn keyboard(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
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
            if key(Modifiers::ALT, Key::ArrowLeft) || mouse_back {
                actions.push(Action::Back);
            }
            if key(Modifiers::ALT, Key::ArrowRight) || mouse_fwd {
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
            if key(Modifiers::CTRL, Key::Num1) {
                actions.push(Action::SetView(ViewMode::List));
            }
            if key(Modifiers::CTRL, Key::Num2) {
                actions.push(Action::SetView(ViewMode::Grid));
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
            // Arrow keys, Home and End move the cursor; Shift extends the selection. In the grid,
            // Up and Down move a whole line and Left and Right one tile.
            let grid = self.settings.view == ViewMode::Grid;
            let (line, line_h) = if grid {
                (self.grid_cols.max(1), GRID_LINE_H)
            } else {
                (1, ROW_H)
            };
            let tab = &mut self.tabs[self.active];
            let n = tab.view.len();
            if n == 0 {
                return;
            }
            let page = ((tab.list_height / line_h) as usize).max(1) * line;
            let cur = tab.cursor;
            let mut target = None;
            for shift in [false, true] {
                let m = if shift {
                    Modifiers::SHIFT
                } else {
                    Modifiers::NONE
                };
                let t = if i.consume_key(m, Key::ArrowDown) {
                    Some(cur.map_or(0, |c| (c + line).min(n - 1)))
                } else if i.consume_key(m, Key::ArrowUp) {
                    Some(cur.map_or(0, |c| c.saturating_sub(line)))
                } else if grid && i.consume_key(m, Key::ArrowRight) {
                    Some(cur.map_or(0, |c| (c + 1).min(n - 1)))
                } else if grid && i.consume_key(m, Key::ArrowLeft) {
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
}
