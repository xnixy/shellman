//! Which tile is selected as keys are pressed. Windows are in most-recently-used order, the one
//! in front first.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// The selection when the switcher opens. Alt+Tab picks the window used before the one in front,
/// so a quick tap swaps the two; Alt+Shift+Tab picks the least recently used. When no app window
/// is in front (the desktop is), the most recent one is the one to go back to.
pub fn first(n: usize, front_is_first: bool, back: bool) -> usize {
    match n {
        0 => 0,
        _ if back => n - 1,
        _ if front_is_first && n > 1 => 1,
        _ => 0,
    }
}

/// Tab and Shift+Tab: the next or previous tile, wrapping around.
pub fn step(sel: usize, n: usize, back: bool) -> usize {
    if n == 0 {
        return 0;
    }
    if back {
        (sel + n - 1) % n
    } else {
        (sel + 1) % n
    }
}

/// Arrow keys in a grid of `cols` per row. Left and right wrap like Tab; up and down stay put at
/// the top and bottom, and down into a shorter last row lands on its last tile.
pub fn arrow(sel: usize, n: usize, cols: usize, dir: Dir) -> usize {
    if n == 0 {
        return 0;
    }
    let cols = cols.max(1);
    match dir {
        Dir::Left => step(sel, n, true),
        Dir::Right => step(sel, n, false),
        Dir::Up => sel.checked_sub(cols).unwrap_or(sel),
        Dir::Down if sel + cols < n => sel + cols,
        Dir::Down if sel / cols < (n - 1) / cols => n - 1,
        Dir::Down => sel,
    }
}

/// After the window list changed under an open switcher: stay on the same window if it is still
/// there, otherwise on the same place, kept in range.
pub fn keep(old: &[isize], sel: usize, new: &[isize]) -> usize {
    if new.is_empty() {
        return 0;
    }
    old.get(sel)
        .and_then(|h| new.iter().position(|n| n == h))
        .unwrap_or(sel.min(new.len() - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_on_the_previous_window() {
        assert_eq!(first(5, true, false), 1);
        assert_eq!(first(5, false, false), 0, "desktop in front: the most recent window");
        assert_eq!(first(1, true, false), 0);
        assert_eq!(first(0, true, false), 0);
        assert_eq!(first(5, true, true), 4, "Alt+Shift+Tab: the oldest");
    }

    #[test]
    fn tab_wraps_both_ways() {
        assert_eq!(step(3, 4, false), 0);
        assert_eq!(step(0, 4, true), 3);
        assert_eq!(step(1, 4, false), 2);
        assert_eq!(step(0, 0, false), 0);
    }

    #[test]
    fn arrows_move_in_the_grid() {
        // Two rows: 0 1 2 / 3 4.
        assert_eq!(arrow(1, 5, 3, Dir::Down), 4);
        assert_eq!(arrow(2, 5, 3, Dir::Down), 4, "into the shorter row: its last tile");
        assert_eq!(arrow(4, 5, 3, Dir::Down), 4, "stays at the bottom");
        assert_eq!(arrow(4, 5, 3, Dir::Up), 1);
        assert_eq!(arrow(1, 5, 3, Dir::Up), 1, "stays at the top");
        assert_eq!(arrow(2, 5, 3, Dir::Right), 3, "right wraps into the next row");
        assert_eq!(arrow(0, 5, 3, Dir::Left), 4);
        assert_eq!(arrow(0, 3, 3, Dir::Down), 0, "one row: down does nothing");
    }

    #[test]
    fn keeps_the_selected_window_when_others_change() {
        assert_eq!(keep(&[10, 20, 30], 1, &[30, 20]), 1);
        assert_eq!(keep(&[10, 20, 30], 2, &[10, 20]), 1, "gone: same place, in range");
        assert_eq!(keep(&[10], 0, &[]), 0);
    }
}
