//! The keyboard: the terminal's own shortcuts (tabs, copy and paste, zoom), then keys, typed text
//! and pastes to the shown tab's shell.
//!
//! Text comes as Windows composed it (`WM_CHAR`: dead keys, AltGr, any layout), so characters
//! are never worked out from key codes here; key codes only matter for keys with sequences of
//! their own and for Ctrl and Alt combinations. AltGr reaches egui without Ctrl and Alt (winit
//! takes them off on layouts that have AltGr), so its characters go as text.

use crate::keys::{self, Key, Local, Mods};
use crate::select;
use alacritty_terminal::term::TermMode;

#[derive(Default)]
pub struct KeyState {
    /// A key pressed with Alt whose text has not come yet: the text gets ESC before it.
    alt_key: Option<char>,
}

/// A shortcut the terminal keeps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shortcut {
    NewTab,
    /// Ctrl+Shift+1..9: a new tab of the n-th profile (from 0).
    NewTabOf(usize),
    CloseTab,
    NextTab,
    PrevTab,
    /// Ctrl+Alt+1..9.
    TabNumber(usize),
    ZoomIn,
    ZoomOut,
    ZoomReset,
    /// Ctrl+Shift+F: find in the scrollback.
    Find,
}

/// The shortcut for a key, if it is one. `physical` is the key's place on the keyboard, for the
/// digit row (Shift turns it into punctuation on most layouts).
pub fn shortcut(key: egui::Key, physical: Option<egui::Key>, m: Mods) -> Option<Shortcut> {
    use egui::Key as K;
    let digit = |k: Option<K>| -> Option<usize> {
        Some(match k? {
            K::Num1 => 1,
            K::Num2 => 2,
            K::Num3 => 3,
            K::Num4 => 4,
            K::Num5 => 5,
            K::Num6 => 6,
            K::Num7 => 7,
            K::Num8 => 8,
            K::Num9 => 9,
            _ => return None,
        })
    };
    let ctrl_shift = m.ctrl && m.shift && !m.alt;
    let ctrl_only = m.ctrl && !m.shift && !m.alt;
    Some(match key {
        K::T if ctrl_shift => Shortcut::NewTab,
        K::W if ctrl_shift => Shortcut::CloseTab,
        K::F if ctrl_shift => Shortcut::Find,
        K::Tab if m.ctrl && !m.alt => {
            if m.shift {
                Shortcut::PrevTab
            } else {
                Shortcut::NextTab
            }
        }
        K::Equals | K::Plus if m.ctrl && !m.alt => Shortcut::ZoomIn,
        K::Minus if ctrl_only => Shortcut::ZoomOut,
        K::Num0 if ctrl_only => Shortcut::ZoomReset,
        _ => {
            let n = digit(physical.or(Some(key)))?;
            if m.ctrl && m.alt && !m.shift {
                Shortcut::TabNumber(n)
            } else if ctrl_shift {
                Shortcut::NewTabOf(n - 1)
            } else {
                return None;
            }
        }
    })
}

/// egui's key for a virtual-key code, for the keys the terminal's shortcuts use.
fn egui_key(vk: u16) -> Option<egui::Key> {
    use egui::Key as K;
    const LETTERS: [K; 26] = [
        K::A, K::B, K::C, K::D, K::E, K::F, K::G, K::H, K::I, K::J, K::K, K::L, K::M, K::N, K::O, K::P, K::Q, K::R, K::S, K::T, K::U, K::V,
        K::W, K::X, K::Y, K::Z,
    ];
    const DIGITS: [K; 10] = [K::Num0, K::Num1, K::Num2, K::Num3, K::Num4, K::Num5, K::Num6, K::Num7, K::Num8, K::Num9];
    Some(match vk {
        0x41..=0x5A => LETTERS[(vk - 0x41) as usize],
        0x30..=0x39 => DIGITS[(vk - 0x30) as usize],
        0x60..=0x69 => DIGITS[(vk - 0x60) as usize],
        0x09 => K::Tab,
        0xBB => K::Equals,
        0x6B => K::Plus,
        0xBD | 0x6D => K::Minus,
        _ => return None,
    })
}

/// Whether the terminal keeps a key while the console takes whole key events (win32-input-mode):
/// its shortcuts, paste, copying its own selection (Ctrl+C, Ctrl+Insert, Enter, Escape only while
/// there is one), paging through the history, and Alt+F4 / Alt+Space for the window. Everything
/// else goes to the console. `vk` is the virtual-key code.
pub fn keeps(vk: u16, m: Mods, selection: bool) -> bool {
    const ENTER: u16 = 0x0D;
    const ESCAPE: u16 = 0x1B;
    const SPACE: u16 = 0x20;
    const PAGE_UP: u16 = 0x21;
    const PAGE_DOWN: u16 = 0x22;
    const INSERT: u16 = 0x2D;
    const F4: u16 = 0x73;
    let only = |shift: bool, ctrl: bool, alt: bool| m == Mods { shift, ctrl, alt };
    if let Some(k) = egui_key(vk) {
        if shortcut(k, Some(k), m).is_some() {
            return true;
        }
    }
    match vk {
        // Paste.
        0x56 if only(false, true, false) || only(true, true, false) => true,
        INSERT if only(true, false, false) => true,
        // Copy: Ctrl+Shift+C always; Ctrl+C and Ctrl+Insert when there is a selection.
        0x43 if only(true, true, false) => true,
        0x43 | INSERT if only(false, true, false) => selection,
        ENTER | ESCAPE if only(false, false, false) => selection,
        PAGE_UP | PAGE_DOWN if only(true, false, false) => true,
        F4 | SPACE if only(false, false, true) => true,
        _ => false,
    }
}

/// The open tab whose id is `id`: where input typed into it goes. None once it closed (input
/// for it is dropped, never sent to whichever tab is shown now).
pub fn typing_tab<T>(tabs: &crate::tabs::Tabs<T>, id: u64, id_of: impl Fn(&T) -> u64) -> Option<&T> {
    tabs.iter().find(|t| id_of(t) == id)
}

impl super::TerminalApp {    /// Keys, typed text and pastes go to the shell of the tab they were typed into: the one
    /// shown when the frame began (`typed_into`, its id), which a tab closing or a tab handed
    /// over may have replaced as the shown one since; if that tab is gone they are dropped.
    /// After one of the terminal's own shortcuts (which may show another tab) they go to the tab
    /// shown then.
    pub(super) fn input(&mut self, ctx: &egui::Context, typed_into: u64) {
        if self.strip.menu_open() {
            // The profile menu has the keyboard (Escape closes it).
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.strip.close_menu();
            }
            return;
        }
        if self.find.has_focus(ctx) {
            // The find pill has the keyboard (it takes Enter and Escape itself); the terminal's
            // own shortcuts still work.
            let events = ctx.input(|i| i.events.clone());
            for event in events {
                if let egui::Event::Key { key, physical_key, pressed: true, modifiers, .. } = event {
                    let mods = Mods { shift: modifiers.shift, ctrl: modifiers.ctrl, alt: modifiers.alt };
                    if let Some(s) = shortcut(key, physical_key, mods) {
                        self.run(ctx, s);
                    }
                }
            }
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        let mut target = typed_into;
        let mut out = Vec::new();
        let mut typed = false;
        // Keys the console took whole (win32-input-mode) came first, through the window
        // procedure; egui then only sees the terminal's own keys (`keeps`).
        if let Some(gate) = &self.win32_keys {
            let keys = gate.take();
            if !keys.is_empty() {
                out.extend(keys);
                typed = true;
            }
        }
        let whole_keys = self.win32_keys.as_ref().is_some_and(|g| g.active());
        for event in events {
            // The tab typed into, if it is still open.
            let Some(tab) = typing_tab(&self.tabs, target, |t| t.id) else {
                // Gone: only the terminal's own shortcuts still count.
                if let egui::Event::Key { key, physical_key, pressed: true, modifiers, .. } = event {
                    let mods = Mods { shift: modifiers.shift, ctrl: modifiers.ctrl, alt: modifiers.alt };
                    if let Some(s) = shortcut(key, physical_key, mods) {
                        out.clear();
                        self.run(ctx, s);
                        target = self.tabs.current().id;
                    }
                }
                continue;
            };
            let session = &tab.session;
            let mode = session.mode();
            let app_cursor = mode.contains(TermMode::APP_CURSOR);
            match event {
                // Typed text came as key events already.
                egui::Event::Text(_) if whole_keys => {}
                egui::Event::Text(text) | egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                    let alt = self.keys.alt_key.take().is_some();
                    out.extend(keys::text_bytes(&text, alt));
                    typed = true;
                }
                egui::Event::Key { key, physical_key, pressed: true, modifiers, .. } => {
                    // A key whose Alt text never came (Windows sends none for some): ESC and
                    // the key itself.
                    if let Some(c) = self.keys.alt_key.take() {
                        out.extend(keys::text_bytes(&c.to_string(), true));
                    }
                    let mods = Mods { shift: modifiers.shift, ctrl: modifiers.ctrl, alt: modifiers.alt };
                    if let Some(s) = shortcut(key, physical_key, mods) {
                        // What was typed before goes first, to the tab it was typed in.
                        self.flush(&mut out, &mut typed, target);
                        self.run(ctx, s);
                        target = self.tabs.current().id;
                        continue;
                    }
                    let has_selection = session.with_term(select::has);
                    // Enter copies a selection (and Escape drops it), as in Windows Terminal.
                    if has_selection && mods == Mods::NONE && matches!(key, egui::Key::Enter | egui::Key::Escape) {
                        if key == egui::Key::Enter {
                            if let Some(text) = session.with_term(select::text).filter(|t| !t.is_empty()) {
                                ctx.copy_text(text);
                            }
                        }
                        session.with_term_mut(|t| t.selection = None);
                        continue;
                    }
                    if whole_keys {
                        // Only the terminal's own keys reach egui then (paging the history).
                        if let Some(Local::ScrollPage(dir)) = map_key(key, mods).and_then(|k| keys::local(k, mods)) {
                            session.scroll(dir * (session.size().rows as i32 - 1).max(1));
                        }
                        continue;
                    }
                    let Some(k) = map_key(key, mods) else { continue };
                    if let Key::Char(c) = k {
                        if mods.alt && !mods.ctrl {
                            self.keys.alt_key = Some(if mods.shift { c } else { c.to_ascii_lowercase() });
                            continue;
                        }
                    }
                    match keys::local(k, mods) {
                        Some(Local::ScrollPage(dir)) => {
                            session.scroll(dir * (session.size().rows as i32 - 1).max(1));
                        }
                        None => {
                            out.extend(keys::key_bytes(k, mods, app_cursor).unwrap_or_default());
                            typed = true;
                        }
                    }
                }
                egui::Event::Paste(text) => {
                    out.extend(keys::paste_bytes(&text, mode.contains(TermMode::BRACKETED_PASTE)));
                    typed = true;
                }
                // egui turns Ctrl+C (and Ctrl+Shift+C, Ctrl+Insert) into copy: a selection is
                // copied; without one, Ctrl+C is the interrupt it always was.
                egui::Event::Copy => {
                    let m = ctx.input(|i| i.modifiers);
                    if session.with_term(select::has) {
                        if let Some(text) = session.with_term(select::text).filter(|t| !t.is_empty()) {
                            ctx.copy_text(text);
                        }
                        if !m.shift {
                            session.with_term_mut(|t| t.selection = None);
                        }
                    } else if m.ctrl && !m.shift && !whole_keys {
                        out.push(0x03);
                        typed = true;
                    }
                }
                egui::Event::Cut if whole_keys => {}
                egui::Event::Cut => {
                    let m = ctx.input(|i| i.modifiers);
                    if m.ctrl {
                        out.push(0x18);
                    } else {
                        // Shift+Delete.
                        out.extend(keys::key_bytes(Key::Delete, Mods { shift: true, ..Mods::NONE }, app_cursor).unwrap_or_default());
                    }
                    typed = true;
                }
                _ => {}
            }
        }
        if let Some(c) = self.keys.alt_key.take() {
            out.extend(keys::text_bytes(&c.to_string(), true));
            typed = true;
        }
        self.flush(&mut out, &mut typed, target);
    }

    /// Send what was typed to tab `target` (by id), if it is still open; typing drops a
    /// selection and shows the cursor.
    fn flush(&mut self, out: &mut Vec<u8>, typed: &mut bool, target: u64) {
        let bytes = std::mem::take(out);
        let was_typed = std::mem::take(typed);
        let Some(tab) = typing_tab(&self.tabs, target, |t| t.id) else { return };
        let s = &tab.session;
        if was_typed && s.with_term(|t| t.selection.is_some()) {
            s.with_term_mut(|t| t.selection = None);
        }
        s.write(bytes);
        if was_typed && target == self.tabs.current().id {
            self.view.restart_blink();
        }
    }
    /// Put the selection on the clipboard.
    pub(super) fn copy_selection(&self, ctx: &egui::Context) -> bool {
        match self.tabs.current().session.with_term(select::text) {
            Some(text) if !text.is_empty() => {
                ctx.copy_text(text);
                true
            }
            _ => false,
        }
    }

    fn run(&mut self, ctx: &egui::Context, s: Shortcut) {
        match s {
            Shortcut::NewTab => self.new_tab(None),
            Shortcut::NewTabOf(i) => {
                if i < self.profiles.len() {
                    self.new_tab(Some(i));
                }
            }
            Shortcut::CloseTab => self.close_tab(ctx, self.tabs.active()),
            Shortcut::NextTab | Shortcut::PrevTab => {
                let to = self.tabs.stepped(s == Shortcut::NextTab);
                self.select_tab(to);
            }
            Shortcut::TabNumber(n) => self.select_tab(self.tabs.numbered(n)),
            Shortcut::ZoomIn => self.zoom(Some(1.0)),
            Shortcut::ZoomOut => self.zoom(Some(-1.0)),
            Shortcut::ZoomReset => self.zoom(None),
            Shortcut::Find => self.find.show(),
        }
    }
}

/// egui's key as the terminal's: the keys with sequences of their own, and character keys
/// (which matter only with Ctrl or Alt; plain typing comes as text).
pub fn map_key(key: egui::Key, mods: Mods) -> Option<Key> {
    use egui::Key as E;
    Some(match key {
        E::Enter => Key::Enter,
        E::Backspace => Key::Backspace,
        E::Tab => Key::Tab,
        E::Escape => Key::Escape,
        // Plain Space types a space as text.
        E::Space if mods.ctrl => Key::Space,
        E::ArrowUp => Key::Up,
        E::ArrowDown => Key::Down,
        E::ArrowLeft => Key::Left,
        E::ArrowRight => Key::Right,
        E::Home => Key::Home,
        E::End => Key::End,
        E::PageUp => Key::PageUp,
        E::PageDown => Key::PageDown,
        E::Insert => Key::Insert,
        E::Delete => Key::Delete,
        E::F1 => Key::F(1),
        E::F2 => Key::F(2),
        E::F3 => Key::F(3),
        E::F4 => Key::F(4),
        E::F5 => Key::F(5),
        E::F6 => Key::F(6),
        E::F7 => Key::F(7),
        E::F8 => Key::F(8),
        E::F9 => Key::F(9),
        E::F10 => Key::F(10),
        E::F11 => Key::F(11),
        E::F12 => Key::F(12),
        _ if mods.ctrl || mods.alt => {
            let mut s = key.symbol_or_name().chars();
            match (s.next(), s.next()) {
                (Some(c), None) => Key::Char(c),
                _ => return None,
            }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Key as K;

    /// A frame's keys follow the tab they were typed into, not whichever tab is shown once
    /// closed tabs are reaped and handed-over tabs open (both happen before input is read).
    #[test]
    fn typed_keys_never_reach_another_tab() {
        use crate::tabs::Tabs;
        let id = |t: &u64| *t;
        // Tabs 1 and 2, 1 shown: the frame begins with keys typed into 1.
        let mut tabs = Tabs::new(1u64);
        tabs.open(2);
        tabs.select(0);
        let typed_into = *tabs.current();
        // A tab handed over opens and is shown: the keys still go to 1.
        tabs.open(3);
        assert_eq!(*tabs.current(), 3);
        assert_eq!(typing_tab(&tabs, typed_into, id), Some(&1));
        // Tab 1's shell exited and it closed: the keys go nowhere, not to the tab shown now.
        tabs.select(0);
        assert!(tabs.retain(|&t| t != 1));
        assert_ne!(*tabs.current(), 1);
        assert_eq!(typing_tab(&tabs, typed_into, id), None);
    }

    const CTRL_SHIFT: Mods = Mods { shift: true, ctrl: true, alt: false };
    const CTRL_ALT: Mods = Mods { shift: false, ctrl: true, alt: true };
    const CTRL: Mods = Mods { shift: false, ctrl: true, alt: false };

    #[test]
    fn tab_shortcuts() {
        assert_eq!(shortcut(K::T, None, CTRL_SHIFT), Some(Shortcut::NewTab));
        assert_eq!(shortcut(K::W, None, CTRL_SHIFT), Some(Shortcut::CloseTab));
        assert_eq!(shortcut(K::Tab, None, CTRL), Some(Shortcut::NextTab));
        assert_eq!(shortcut(K::Tab, None, CTRL_SHIFT), Some(Shortcut::PrevTab));
        // Ctrl+T and Ctrl+W alone are the shell's (readline's transpose, delete word).
        assert_eq!(shortcut(K::T, None, CTRL), None);
        assert_eq!(shortcut(K::W, None, CTRL), None);
        assert_eq!(shortcut(K::F, None, CTRL_SHIFT), Some(Shortcut::Find));
        assert_eq!(shortcut(K::F, None, CTRL), None);
    }

    #[test]
    fn digits_by_their_place_whatever_shift_makes_of_them() {
        assert_eq!(shortcut(K::Num3, Some(K::Num3), CTRL_ALT), Some(Shortcut::TabNumber(3)));
        // Shift+1 is "!" on US, but its place is still 1.
        assert_eq!(shortcut(K::Exclamationmark, Some(K::Num1), CTRL_SHIFT), Some(Shortcut::NewTabOf(0)));
        assert_eq!(shortcut(K::Num1, Some(K::Num1), CTRL), None);
        assert_eq!(shortcut(K::Num1, Some(K::Num1), Mods::NONE), None);
    }

    #[test]
    fn zoom_keys() {
        assert_eq!(shortcut(K::Equals, None, CTRL), Some(Shortcut::ZoomIn));
        assert_eq!(shortcut(K::Plus, None, CTRL_SHIFT), Some(Shortcut::ZoomIn));
        assert_eq!(shortcut(K::Minus, None, CTRL), Some(Shortcut::ZoomOut));
        assert_eq!(shortcut(K::Num0, Some(K::Num0), CTRL), Some(Shortcut::ZoomReset));
    }

    #[test]
    fn altgr_characters_are_text_not_keys() {
        // AltGr+Q ('@' on a German layout) reaches egui as Q with no modifiers and the text "@".
        assert_eq!(map_key(K::Q, Mods::NONE), None);
        assert_eq!(shortcut(K::Q, Some(K::Q), Mods::NONE), None);
        // Left Ctrl + left Alt is AltGr to Windows too: no control code goes for it.
        assert_eq!(keys::key_bytes(Key::Char('q'), CTRL_ALT, false), None);
        // AltGr+7 is '{' on German: not Ctrl+Alt+7 (switch to tab 7), since AltGr carries no
        // Ctrl or Alt.
        assert_eq!(shortcut(K::Num7, Some(K::Num7), Mods::NONE), None);
    }

    #[test]
    fn with_whole_key_events_the_terminal_keeps_only_its_own_keys() {
        const SHIFT: Mods = Mods { shift: true, ctrl: false, alt: false };
        const ALT: Mods = Mods { shift: false, ctrl: false, alt: true };
        // Its shortcuts, by key code.
        assert!(keeps(0x54, CTRL_SHIFT, false)); // Ctrl+Shift+T
        assert!(keeps(0x09, CTRL, false)); // Ctrl+Tab
        assert!(keeps(0x33, CTRL_ALT, false)); // Ctrl+Alt+3
        assert!(keeps(0xBB, CTRL, false)); // Ctrl+=
        assert!(keeps(0x21, SHIFT, false)); // Shift+PgUp
        assert!(keeps(0x56, CTRL, false) && keeps(0x2D, SHIFT, false)); // paste
        assert!(keeps(0x73, ALT, false)); // Alt+F4
        // Ctrl+C is the console's (PSReadLine copies its own selection, or it interrupts),
        // unless the terminal has a selection.
        assert!(!keys_kept(0x43, CTRL));
        assert!(keeps(0x43, CTRL, true));
        assert!(keeps(0x43, CTRL_SHIFT, false));
        assert!(!keeps(0x0D, Mods::NONE, false) && keeps(0x0D, Mods::NONE, true));
        // Everything else goes whole: Shift+arrows, Ctrl+Alt letters, plain typing.
        assert!(!keys_kept(0x25, SHIFT));
        assert!(!keys_kept(0x41, CTRL_ALT));
        assert!(!keys_kept(0x41, Mods::NONE));
        assert!(!keys_kept(0x54, CTRL)); // Ctrl+T alone is the shell's
    }

    fn keys_kept(vk: u16, m: Mods) -> bool {
        keeps(vk, m, false)
    }

    #[test]
    fn ctrl_letters_map_to_their_keys() {
        assert_eq!(map_key(K::C, CTRL), Some(Key::Char('C')));
        assert_eq!(map_key(K::ArrowUp, Mods::NONE), Some(Key::Up));
        assert_eq!(map_key(K::A, Mods::NONE), None);
    }
}
