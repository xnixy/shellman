//! Keyboard shortcuts.

use super::actions::Action;
use super::tab::Loc;
use super::ROW_H;
use crate::settings::ViewMode;
use egui::{Key, Modifiers, PointerButton};
use shellman_core::fsops::Op;

impl super::FileMan {
    pub(super) fn keyboard(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        // The Menu key has no egui key, so it is read from the keyboard's state.
        let menu_key = menu_key_pressed(ctx);
        // An open menu has the keyboard: its arrows, Enter and Escape are its own.
        if self.dialog.is_some() || self.menu.is_open() {
            return;
        }
        if menu_key {
            actions.push(Action::MenuKey);
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
            if key(Modifiers::CTRL, Key::H) {
                actions.push(Action::ToggleHidden);
            }
            if key(Modifiers::CTRL, Key::T) {
                actions.push(Action::NewTab(self.tabs[self.active].loc.clone()));
            }
            if key(Modifiers::CTRL, Key::W) {
                actions.push(Action::CloseTab(self.active));
            }
            // With Shift first: egui lets Ctrl+Tab's pattern match Ctrl+Shift+Tab too.
            if key(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab) {
                let n = self.tabs.len();
                actions.push(Action::SwitchTab((self.active + n - 1) % n));
            }
            if key(Modifiers::CTRL, Key::Tab) {
                actions.push(Action::SwitchTab((self.active + 1) % self.tabs.len()));
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
            if key(Modifiers::SHIFT, Key::F10) {
                actions.push(Action::MenuKey);
            }
            if key(Modifiers::ALT, Key::Enter) {
                actions.push(Action::Properties);
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
                // In the bin, Del is the final delete (asked about first), not another recycle.
                actions.push(if self.tabs[self.active].loc == Loc::Bin {
                    Action::BinDelete
                } else {
                    Action::Recycle
                });
            }
            if key(Modifiers::CTRL, Key::A) {
                actions.push(Action::SelectAll);
            }
            // Ctrl+Shift+C before Ctrl+C, whose pattern would take it too.
            if key(Modifiers::CTRL | Modifiers::SHIFT, Key::C) {
                actions.push(Action::CopyPath);
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
            // Arrow keys, Home and End move the cursor; Shift extends the selection. In the grid,
            // Up and Down move a whole line and Left and Right one tile.
            let grid = self.settings.view == ViewMode::Grid;
            let (line, line_h) = if grid {
                (self.grid_cols.max(1), self.grid_line_h)
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
            if i.consume_key(Modifiers::CTRL, Key::Space) {
                actions.push(Action::ToggleCursor);
            }
            // Plain keys select, Shift extends, Ctrl only moves the cursor (to toggle with
            // Ctrl+Space, as in Explorer).
            let mut target = None;
            // egui lets a pattern without Shift match a key pressed with it, so the Shift
            // pattern goes first.
            for (shift, m) in [
                (Some(true), Modifiers::SHIFT),
                (None, Modifiers::CTRL),
                (Some(false), Modifiers::NONE),
            ] {
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
                match shift {
                    Some(true) => tab.select_range(t),
                    Some(false) => tab.select_only(t),
                    None => tab.cursor = Some(t),
                }
                tab.scroll_to_cursor = true;
            }
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                tab.selected.clear();
            }
        });
    }

    /// Picker mode's Esc, outside any text field: away with the replace question, or cancel.
    /// Read before the browsing keys, which would only clear the selection.
    pub(super) fn pick_keys(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        if self.pick.is_none() {
            return;
        }
        // Esc with a list open (the file types, Sort) only closes the list.
        if ctx.memory(|m| m.any_popup_open()) {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                ctx.memory_mut(|m| m.close_popup());
            }
            return;
        }
        if self.dialog.is_some() || self.menu.is_open() || ctx.wants_keyboard_input() {
            return;
        }
        ctx.input_mut(|i| {
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                actions.push(Action::PickEscape);
            }
        });
    }
}

/// Whether the Menu (application) key went down since the last frame, while this window has focus.
fn menu_key_pressed(ctx: &egui::Context) -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_APPS};
    static WAS_DOWN: AtomicBool = AtomicBool::new(false);
    let down = unsafe { GetAsyncKeyState(i32::from(VK_APPS.0)) } as u16 & 0x8000 != 0;
    let was = WAS_DOWN.swap(down, Ordering::Relaxed);
    down && !was && ctx.input(|i| i.focused)
}
