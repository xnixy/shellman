//! Keyboard movement in a menu, as plain functions over which rows can be landed on.

/// The next row the keyboard lands on going down (or up) from `from`, wrapping around and
/// skipping separators and notes. From nowhere, the first (or last) one.
pub(super) fn step(selectable: &[bool], from: Option<usize>, down: bool) -> Option<usize> {
    let n = selectable.len();
    if n == 0 {
        return None;
    }
    let start = match (from, down) {
        (Some(f), true) => f + 1,
        (Some(f), false) => f + n - 1,
        (None, true) => 0,
        (None, false) => n - 1,
    };
    (0..n)
        .map(|k| if down { (start + k) % n } else { (start + n - k) % n })
        .find(|&i| selectable[i])
}

/// The row whose letter is `ch`, among enabled ones.
pub(super) fn mnemonic(rows: impl Iterator<Item = (Option<char>, bool)>, ch: char) -> Option<usize> {
    let ch = ch.to_ascii_lowercase();
    rows.enumerate()
        .find(|(_, (m, enabled))| *enabled && *m == Some(ch))
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_skip_separators_and_wrap() {
        // A row, a separator, a row, a note.
        let rows = [true, false, true, false];
        assert_eq!(step(&rows, None, true), Some(0));
        assert_eq!(step(&rows, None, false), Some(2));
        assert_eq!(step(&rows, Some(0), true), Some(2));
        assert_eq!(step(&rows, Some(2), true), Some(0));
        assert_eq!(step(&rows, Some(0), false), Some(2));
        assert_eq!(step(&[false, false], None, true), None);
        assert_eq!(step(&[], None, true), None);
    }

    #[test]
    fn letters_pick_enabled_rows() {
        let rows = [(Some('o'), false), (Some('o'), true), (None, true)];
        assert_eq!(mnemonic(rows.into_iter(), 'O'), Some(1));
        assert_eq!(mnemonic(rows.into_iter(), 'x'), None);
    }
}
