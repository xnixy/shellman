//! Moving through a popup menu with the keyboard and the pointer, as plain state: which row is
//! lit at each level, which submenus are open, and which level the keyboard is in. No egui here,
//! so it is tested on its own.

use super::model::{Row, RowKind, MAX_LEVELS};
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

/// Level 0 is the menu, level 1 the submenu open from it, and so on.
#[derive(Clone, Debug, PartialEq)]
pub struct Nav {
    /// The lit row of each open level: always one more than `chain`.
    pub hot: Vec<Option<usize>>,
    /// The row of each level whose submenu is open, from the menu down.
    pub chain: Vec<usize>,
    /// Where the keyboard is.
    pub level: usize,
    /// The row the pointer rests on (level, row), and since when (seconds).
    dwell: Option<(usize, usize, f64)>,
}

impl Default for Nav {
    fn default() -> Self {
        Self { hot: vec![None], chain: Vec::new(), level: 0, dwell: None }
    }
}

/// The rows of level `level` with the submenus of `chain` open.
pub fn level_rows<'a>(rows: &'a [Row], chain: &[usize], level: usize) -> Option<&'a [Row]> {
    let mut here = rows;
    for &i in chain.get(..level)? {
        here = here.get(i)?.submenu()?;
    }
    Some(here)
}

impl Nav {
    /// How many levels are open.
    pub fn levels(&self) -> usize {
        self.chain.len() + 1
    }

    /// Drop what no longer exists in `rows` (the caller updated the menu): submenus that are
    /// gone, lit rows past the end.
    pub fn fit(&mut self, rows: &[Row]) {
        let mut keep = 0;
        while keep < self.chain.len() {
            let ok = level_rows(rows, &self.chain, keep)
                .and_then(|r| r.get(self.chain[keep]))
                .is_some_and(|r| r.submenu().is_some());
            if !ok {
                break;
            }
            keep += 1;
        }
        self.chain.truncate(keep);
        self.hot.resize(keep + 1, None);
        for (l, hot) in self.hot.iter_mut().enumerate() {
            let len = level_rows(rows, &self.chain, l).map_or(0, <[Row]>::len);
            if hot.is_some_and(|h| h >= len) {
                *hot = None;
            }
        }
        self.level = self.level.min(keep);
        if self.dwell.is_some_and(|(l, _, _)| l > keep) {
            self.dwell = None;
        }
    }

    pub fn key(&mut self, rows: &[Row], key: Key) -> Outcome {
        self.fit(rows);
        let level = self.level;
        let Some(here) = level_rows(rows, &self.chain, level) else { return Outcome::Nothing };
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
            // The keyboard is here: whatever deeper level was open stays, but is not lit.
            self.dwell = None;
            return Outcome::Nothing;
        }
        match key {
            Key::Escape | Key::Left if level > 0 => {
                self.close_from(level);
                Outcome::Nothing
            }
            Key::Escape => Outcome::Close,
            Key::Right => match hot {
                Some(i) if here[i].submenu().is_some() => self.activate(rows, level, i),
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
        let Some(here) = level_rows(rows, &self.chain, level) else { return Outcome::Nothing };
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
        let Some(row) = level_rows(rows, &self.chain, level).and_then(|r| r.get(i)) else { return Outcome::Nothing };
        self.hot[level] = Some(i);
        if !row.enabled {
            return Outcome::Nothing;
        }
        match &row.kind {
            RowKind::Item(id) | RowKind::Picture(id, ..) => Outcome::Pick(*id),
            RowKind::Submenu(sub) if level + 1 < MAX_LEVELS => {
                self.open_sub(level, i);
                let selectable: Vec<bool> = sub.iter().map(Row::selectable).collect();
                self.hot[level + 1] = keys::step(&selectable, None, true);
                self.level = level + 1;
                Outcome::Nothing
            }
            _ => Outcome::Nothing,
        }
    }

    /// Open the submenu of row `i` of `level`, closing whatever was open from that level.
    fn open_sub(&mut self, level: usize, i: usize) {
        self.chain.truncate(level);
        self.chain.push(i);
        self.hot.truncate(level + 1);
        self.hot[level] = Some(i);
        self.hot.push(None);
        self.dwell = None;
    }

    /// Close level `level` and everything under it; the keyboard goes to the level above.
    fn close_from(&mut self, level: usize) {
        let level = level.max(1);
        self.chain.truncate(level - 1);
        self.hot.truncate(level);
        self.level = level - 1;
        self.dwell = None;
    }

    /// The pointer is over row `row` of `level` (None: over that level but on no row, such as a
    /// separator or the padding) at time `now`.
    pub fn hover(&mut self, rows: &[Row], level: usize, row: Option<usize>, now: f64) {
        if level > self.chain.len() {
            return;
        }
        let row = row.filter(|&i| level_rows(rows, &self.chain, level).and_then(|r| r.get(i)).is_some_and(Row::selectable));
        // Into a deeper level: whatever was about to change a shallower one is forgotten (a
        // diagonal move toward a submenu does not close it).
        if self.dwell.is_some_and(|(l, _, _)| l < level) {
            self.dwell = None;
        }
        if self.hot[level] != row || self.level != level {
            // The row whose submenu is open stays lit while the pointer is on no row.
            self.hot[level] = row.or(self.chain.get(level).copied());
            self.level = level;
            for h in self.hot.iter_mut().skip(level + 1) {
                *h = None;
            }
        }
        match row {
            Some(i) if self.dwell.map(|(l, r, _)| (l, r)) != Some((level, i)) => self.dwell = Some((level, i, now)),
            Some(_) => {}
            None => self.dwell = None,
        }
    }

    /// Let a resting pointer open or close a submenu. Returns how long until it would, if it is
    /// waiting, so the caller can ask for a frame then.
    pub fn tick(&mut self, rows: &[Row], now: f64) -> Option<f64> {
        let (level, i, since) = self.dwell?;
        let left = DWELL - (now - since);
        if left > 0.0 {
            return Some(left);
        }
        self.dwell = None;
        let row = level_rows(rows, &self.chain, level)?.get(i)?;
        match row.submenu() {
            Some(_) if row.enabled && level + 1 < MAX_LEVELS && self.chain.get(level) != Some(&i) => {
                self.open_sub(level, i);
                self.level = level;
            }
            Some(_) => {}
            None if self.chain.len() > level => {
                self.chain.truncate(level);
                self.hot.truncate(level + 1);
            }
            None => {}
        }
        None
    }

    /// A click (button released) on row `row` of `level`.
    pub fn click(&mut self, rows: &[Row], level: usize, row: usize) -> Outcome {
        if level > self.chain.len() || self.chain.get(level) == Some(&row) {
            return Outcome::Nothing;
        }
        let out = self.activate(rows, level, row);
        if self.chain.get(level) == Some(&row) {
            // Opened by the pointer: the keyboard goes in too, as in Windows.
            self.level = level + 1;
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

    /// A ▸ (B ▸ (C ▸ (leaf), b2), a2), z.
    fn deep() -> Vec<Row> {
        let entries = vec![
            Entry::submenu(
                "A",
                vec![
                    Entry::submenu("B", vec![Entry::submenu("C", vec![Entry::item(0, "leaf")]), Entry::item(1, "b2")]),
                    Entry::item(2, "a2"),
                ],
            ),
            Entry::item(3, "z"),
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
        assert_eq!((n.chain.as_slice(), n.level, n.hot[1]), (&[0][..], 1, Some(0)));
        n.key(&r, Key::Down);
        assert_eq!(n.hot[1], Some(2));
        assert_eq!(n.key(&r, Key::Enter), Outcome::Pick(1));
        n.key(&r, Key::Left);
        assert_eq!((n.chain.len(), n.level), (0, 0));
        assert_eq!(n.hot[0], Some(0));
        // Escape in a submenu closes only the submenu; on the menu, the menu.
        n.key(&r, Key::Right);
        assert_eq!(n.key(&r, Key::Escape), Outcome::Nothing);
        assert!(n.chain.is_empty());
        assert_eq!(n.key(&r, Key::Escape), Outcome::Close);
    }

    #[test]
    fn the_keyboard_goes_down_three_levels_and_back() {
        let r = deep();
        let mut n = Nav::default();
        n.key(&r, Key::Down);
        n.key(&r, Key::Right);
        n.key(&r, Key::Right);
        n.key(&r, Key::Right);
        assert_eq!((n.chain.as_slice(), n.level, n.levels()), (&[0, 0, 0][..], 3, 4));
        assert_eq!(n.key(&r, Key::Enter), Outcome::Pick(0));
        n.key(&r, Key::Left);
        assert_eq!((n.chain.as_slice(), n.level), (&[0, 0][..], 2));
        n.key(&r, Key::Escape);
        n.key(&r, Key::Down);
        assert_eq!(n.hot[1], Some(1));
        assert_eq!(n.key(&r, Key::Enter), Outcome::Pick(2));
    }

    #[test]
    fn right_on_an_item_does_nothing_and_disabled_rows_never_pick() {
        let r = rows();
        let mut n = Nav { hot: vec![Some(3)], ..Default::default() };
        assert_eq!(n.key(&r, Key::Right), Outcome::Nothing);
        assert_eq!(n.key(&r, Key::Enter), Outcome::Pick(3));
        let mut n = Nav { hot: vec![Some(1)], ..Default::default() };
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
        assert!(n.chain.is_empty());
        assert_eq!(n.tick(&r, 10.25), None);
        assert_eq!(n.chain, [0]);
        // Into the submenu: it stays.
        n.hover(&r, 1, Some(2), 10.3);
        assert_eq!((n.level, n.hot[1]), (1, Some(2)));
        // Passing over another row on the way back does not close it at once...
        n.hover(&r, 0, Some(3), 11.0);
        n.tick(&r, 11.1);
        assert_eq!(n.chain, [0]);
        // ...resting there does.
        n.tick(&r, 11.3);
        assert!(n.chain.is_empty());
    }

    #[test]
    fn resting_opens_deeper_levels_one_by_one() {
        let r = deep();
        let mut n = Nav::default();
        n.hover(&r, 0, Some(0), 0.0);
        n.tick(&r, 0.3);
        n.hover(&r, 1, Some(0), 0.4);
        n.tick(&r, 0.7);
        assert_eq!(n.chain, [0, 0]);
        // Back on the first submenu's other row: the deepest closes after the rest.
        n.hover(&r, 1, Some(1), 1.0);
        n.tick(&r, 1.3);
        assert_eq!(n.chain, [0]);
        assert_eq!(n.hot.len(), 2);
    }

    #[test]
    fn a_diagonal_move_back_into_the_submenu_keeps_it() {
        let r = rows();
        let mut n = Nav { chain: vec![0], hot: vec![Some(0), None], ..Default::default() };
        n.hover(&r, 0, Some(3), 1.0);
        n.hover(&r, 1, Some(0), 1.1);
        n.tick(&r, 2.0);
        assert_eq!(n.chain, [0]);
    }

    #[test]
    fn clicks_pick_items_and_open_submenus() {
        let r = rows();
        let mut n = Nav::default();
        assert_eq!(n.click(&r, 0, 0), Outcome::Nothing);
        assert_eq!(n.chain, [0]);
        assert_eq!(n.click(&r, 1, 0), Outcome::Pick(0));
        assert_eq!(n.click(&r, 0, 1), Outcome::Nothing);
        assert_eq!(n.click(&r, 0, 6), Outcome::Pick(5));
        // Separators and headings are not rows the pointer can light.
        n.hover(&r, 0, Some(2), 0.0);
        assert_ne!(n.hot[0], Some(2));
    }

    #[test]
    fn an_update_that_removes_the_open_submenu_closes_it() {
        let r = deep();
        let mut n = Nav::default();
        n.key(&r, Key::Down);
        n.key(&r, Key::Right);
        n.key(&r, Key::Right);
        assert_eq!(n.level, 2);
        let flat = flatten(vec![Entry::item(0, "only")], &mut Vec::new(), 0);
        n.fit(&flat);
        assert_eq!((n.chain.len(), n.hot.len(), n.level), (0, 1, 0));
        // A lit row past the new end is let go.
        let mut n = Nav { hot: vec![Some(1)], ..Default::default() };
        n.fit(&flat);
        assert_eq!(n.hot[0], None);
    }
}
