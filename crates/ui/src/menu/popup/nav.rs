//! Moving through a popup menu with the keyboard and the pointer, as plain state: which row is
//! lit at each level, which submenu is open, and which level the keyboard is in. No egui here,
//! so it is tested on its own.

use super::model::{Row, RowKind};
use crate::menu::keys;

/// How long the pointer rests on a row before its submenu opens (or another one closes): long
/// enough that a diagonal move toward an open submenu does not close it on the way.
pub const DWELL: f64 = 0.2;

/// A key the menu reacts to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Key {
    Up,
    Down,
    Home,
    End,
    Left,
    Right,
    /// Enter or Space.
    Enter,
    Escape,
    Letter(char),
}

/// What a key or click comes to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    Nothing,
    /// The caller's value number `usize` was picked; the menu closes.
    Pick(usize),
    /// Escape on the menu itself: close without a pick.
    Close,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Nav {
    /// The lit row of the menu (0) and of the submenu (1).
    pub hot: [Option<usize>; 2],
    /// The menu row whose submenu is open.
    pub sub: Option<usize>,
    /// Where the keyboard is: 0 the menu, 1 the submenu.
    pub level: usize,
    /// The menu row the pointer rests on, and since when (seconds).
    dwell: Option<(usize, f64)>,
}

impl Nav {
    /// The rows of the submenu that is open, if one is.
    pub fn sub_rows<'a>(&self, rows: &'a [Row]) -> Option<&'a [Row]> {
        rows.get(self.sub?)?.submenu()
    }

    fn level_rows<'a>(&self, rows: &'a [Row], level: usize) -> &'a [Row] {
        if level == 0 {
            rows
        } else {
            self.sub_rows(rows).unwrap_or(&[])
        }
    }

    pub fn key(&mut self, rows: &[Row], key: Key) -> Outcome {
        if self.level == 1 && self.sub_rows(rows).is_none() {
            self.level = 0;
        }
        let level = self.level;
        let here = self.level_rows(rows, level);
        let selectable: Vec<bool> = here.iter().map(Row::selectable).collect();
        let hot = self.hot[level];
        let moved = match key {
            Key::Down => keys::step(&selectable, hot, true),
            Key::Up => keys::step(&selectable, hot, false),
            Key::Home => keys::step(&selectable, None, true),
            Key::End => keys::step(&selectable, None, false),
            _ => None,
        };
        if moved.is_some() {
            self.hot[level] = moved;
            self.dwell = None;
            return Outcome::Nothing;
        }
        match key {
            Key::Escape | Key::Left if level == 1 => {
                self.close_sub();
                Outcome::Nothing
            }
            Key::Escape => Outcome::Close,
            Key::Right if level == 0 => match hot {
                Some(i) if rows[i].submenu().is_some() => self.activate(rows, 0, i),
                _ => Outcome::Nothing,
            },
            Key::Enter => match hot {
                Some(i) => self.activate(rows, level, i),
                None => Outcome::Nothing,
            },
            Key::Letter(ch) => self.letter(rows, level, ch),
            _ => Outcome::Nothing,
        }
    }

    /// A letter: the row marked with it is chosen; else the rows starting with it are stepped
    /// through, and one alone is chosen.
    fn letter(&mut self, rows: &[Row], level: usize, ch: char) -> Outcome {
        let ch = ch.to_ascii_lowercase();
        let here = self.level_rows(rows, level);
        let marked = keys::mnemonic(here.iter().map(|r| (r.mnemonic, r.enabled && r.selectable())), ch);
        if let Some(i) = marked {
            return self.activate(rows, level, i);
        }
        let matching: Vec<usize> = here
            .iter()
            .enumerate()
            .filter(|(_, r)| r.selectable() && r.enabled && r.letter() == Some(ch))
            .map(|(i, _)| i)
            .collect();
        match matching.as_slice() {
            [] => Outcome::Nothing,
            [only] => self.activate(rows, level, *only),
            many => {
                let after = self.hot[level].map_or(0, |h| h + 1);
                let next = many.iter().copied().find(|&i| i >= after).unwrap_or(many[0]);
                self.hot[level] = Some(next);
                Outcome::Nothing
            }
        }
    }

    /// Choose row `i` of `level`: open its submenu (the keyboard goes in) or pick it.
    fn activate(&mut self, rows: &[Row], level: usize, i: usize) -> Outcome {
        let Some(row) = self.level_rows(rows, level).get(i) else { return Outcome::Nothing };
        self.hot[level] = Some(i);
        if !row.enabled {
            return Outcome::Nothing;
        }
        match &row.kind {
            RowKind::Item(id) => Outcome::Pick(*id),
            RowKind::Submenu(sub) if level == 0 => {
                self.sub = Some(i);
                self.level = 1;
                let selectable: Vec<bool> = sub.iter().map(Row::selectable).collect();
                self.hot[1] = keys::step(&selectable, None, true);
                self.dwell = None;
                Outcome::Nothing
            }
            _ => Outcome::Nothing,
        }
    }

    fn close_sub(&mut self) {
        self.sub = None;
        self.hot[1] = None;
        self.level = 0;
        self.dwell = None;
    }

    /// The pointer is over row `row` of `level` (None: over that level but on no row, such as a
    /// separator or the padding) at time `now`.
    pub fn hover(&mut self, rows: &[Row], level: usize, row: Option<usize>, now: f64) {
        let row = row.filter(|&i| self.level_rows(rows, level).get(i).is_some_and(Row::selectable));
        if level == 1 {
            if self.sub_rows(rows).is_some() {
                self.hot[1] = row;
                self.level = 1;
                // Back in the submenu: whatever was about to close it is forgotten.
                self.dwell = None;
            }
            return;
        }
        if self.hot[0] != row || self.level != 0 {
            self.hot[0] = row.or(self.sub);
            self.level = 0;
            if self.sub.is_some() {
                self.hot[1] = None;
            }
        }
        match row {
            Some(i) if self.dwell.map(|d| d.0) != Some(i) => {
                // The first submenu opens at once when none is open, as a native menu does after
                // its delay; switching from an open one waits.
                self.dwell = Some((i, now));
            }
            Some(_) => {}
            None => self.dwell = None,
        }
    }

    /// Let a resting pointer open or close a submenu. Returns how long until it would, if it is
    /// waiting, so the caller can ask for a frame then.
    pub fn tick(&mut self, rows: &[Row], now: f64) -> Option<f64> {
        let (i, since) = self.dwell?;
        let left = DWELL - (now - since);
        if left > 0.0 {
            return Some(left);
        }
        self.dwell = None;
        let row = rows.get(i)?;
        match row.submenu() {
            Some(_) if row.enabled && self.sub != Some(i) => {
                self.sub = Some(i);
                self.hot[1] = None;
            }
            Some(_) => {}
            None if self.sub.is_some() => {
                self.sub = None;
                self.hot[1] = None;
            }
            None => {}
        }
        None
    }

    /// A click (button released) on row `row` of `level`.
    pub fn click(&mut self, rows: &[Row], level: usize, row: usize) -> Outcome {
        if level == 0 && self.sub == Some(row) {
            return Outcome::Nothing;
        }
        let out = self.activate(rows, level, row);
        if level == 0 && self.sub == Some(row) {
            // Opened by the pointer: the keyboard stays where it was asked to be, on the menu.
            self.level = 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::{flatten, Entry};
    use super::*;

    /// New ▸ (Folder, Text), Paste (disabled), ─, Open in Files, Open Terminal, ─, Wallpaper.
    fn rows() -> Vec<Row> {
        let entries = vec![
            Entry::submenu("&New", vec![Entry::item(0, "&Folder"), Entry::separator(), Entry::item(1, "&Text document")]),
            Entry::item(2, "&Paste").enabled(false),
            Entry::separator(),
            Entry::item(3, "Open in Files"),
            Entry::item(4, "Open Terminal here"),
            Entry::heading("Look"),
            Entry::item(5, "Change wallpaper"),
        ];
        flatten(entries, &mut Vec::new(), 0)
    }

    #[test]
    fn arrows_skip_separators_headings_and_wrap() {
        let r = rows();
        let mut n = Nav::default();
        n.key(&r, Key::Down);
        assert_eq!(n.hot[0], Some(0));
        n.key(&r, Key::Down);
        // Disabled rows can be lit (as in Windows), separators cannot.
        assert_eq!(n.hot[0], Some(1));
        n.key(&r, Key::Down);
        assert_eq!(n.hot[0], Some(3));
        n.key(&r, Key::End);
        assert_eq!(n.hot[0], Some(6));
        n.key(&r, Key::Down);
        assert_eq!(n.hot[0], Some(0));
        n.key(&r, Key::Up);
        assert_eq!(n.hot[0], Some(6));
        n.key(&r, Key::Home);
        assert_eq!(n.hot[0], Some(0));
    }

    #[test]
    fn right_and_left_open_and_close_the_submenu() {
        let r = rows();
        let mut n = Nav::default();
        n.key(&r, Key::Down);
        assert_eq!(n.key(&r, Key::Right), Outcome::Nothing);
        assert_eq!((n.sub, n.level, n.hot[1]), (Some(0), 1, Some(0)));
        n.key(&r, Key::Down);
        assert_eq!(n.hot[1], Some(2));
        assert_eq!(n.key(&r, Key::Enter), Outcome::Pick(1));
        n.key(&r, Key::Left);
        assert_eq!((n.sub, n.level), (None, 0));
        assert_eq!(n.hot[0], Some(0));
        // Escape in a submenu closes only the submenu; on the menu, the menu.
        n.key(&r, Key::Right);
        assert_eq!(n.key(&r, Key::Escape), Outcome::Nothing);
        assert_eq!(n.sub, None);
        assert_eq!(n.key(&r, Key::Escape), Outcome::Close);
    }

    #[test]
    fn right_on_an_item_does_nothing_and_disabled_rows_never_pick() {
        let r = rows();
        let mut n = Nav { hot: [Some(3), None], ..Default::default() };
        assert_eq!(n.key(&r, Key::Right), Outcome::Nothing);
        assert_eq!(n.key(&r, Key::Enter), Outcome::Pick(3));
        let mut n = Nav { hot: [Some(1), None], ..Default::default() };
        assert_eq!(n.key(&r, Key::Enter), Outcome::Nothing);
        assert_eq!(n.key(&r, Key::Letter('p')), Outcome::Nothing);
    }

    #[test]
    fn letters_pick_marked_rows_and_step_through_first_letters() {
        let r = rows();
        let mut n = Nav::default();
        // Two rows start with O: the first letter steps through them.
        assert_eq!(n.key(&r, Key::Letter('o')), Outcome::Nothing);
        assert_eq!(n.hot[0], Some(3));
        assert_eq!(n.key(&r, Key::Letter('O')), Outcome::Nothing);
        assert_eq!(n.hot[0], Some(4));
        assert_eq!(n.key(&r, Key::Letter('o')), Outcome::Nothing);
        assert_eq!(n.hot[0], Some(3));
        // One row starts with C: picked.
        assert_eq!(n.key(&r, Key::Letter('c')), Outcome::Pick(5));
        // A marked letter opens a submenu, and works inside it.
        assert_eq!(n.key(&r, Key::Letter('n')), Outcome::Nothing);
        assert_eq!(n.level, 1);
        assert_eq!(n.key(&r, Key::Letter('t')), Outcome::Pick(1));
        assert_eq!(n.key(&r, Key::Letter('z')), Outcome::Nothing);
    }

    #[test]
    fn resting_on_a_submenu_row_opens_it_and_on_another_row_closes_it() {
        let r = rows();
        let mut n = Nav::default();
        n.hover(&r, 0, Some(0), 10.0);
        assert!(n.tick(&r, 10.05).is_some());
        assert_eq!(n.sub, None);
        assert_eq!(n.tick(&r, 10.25), None);
        assert_eq!(n.sub, Some(0));
        // Into the submenu: it stays.
        n.hover(&r, 1, Some(2), 10.3);
        assert_eq!((n.level, n.hot[1]), (1, Some(2)));
        // Passing over another row on the way back does not close it at once...
        n.hover(&r, 0, Some(3), 11.0);
        n.tick(&r, 11.1);
        assert_eq!(n.sub, Some(0));
        // ...resting there does.
        n.tick(&r, 11.3);
        assert_eq!(n.sub, None);
    }

    #[test]
    fn a_diagonal_move_back_into_the_submenu_keeps_it() {
        let r = rows();
        let mut n = Nav { sub: Some(0), hot: [Some(0), None], ..Default::default() };
        n.hover(&r, 0, Some(3), 1.0);
        n.hover(&r, 1, Some(0), 1.1);
        n.tick(&r, 2.0);
        assert_eq!(n.sub, Some(0));
    }

    #[test]
    fn clicks_pick_items_and_open_submenus() {
        let r = rows();
        let mut n = Nav::default();
        assert_eq!(n.click(&r, 0, 0), Outcome::Nothing);
        assert_eq!(n.sub, Some(0));
        assert_eq!(n.click(&r, 1, 0), Outcome::Pick(0));
        assert_eq!(n.click(&r, 0, 1), Outcome::Nothing);
        assert_eq!(n.click(&r, 0, 6), Outcome::Pick(5));
        // Separators and headings are not rows the pointer can light.
        n.hover(&r, 0, Some(2), 0.0);
        assert_ne!(n.hot[0], Some(2));
    }
}
