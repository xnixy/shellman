//! Which tile is selected when Task View opens, and after the window list changes under it.

/// On opening: the window that was in front, so Enter (or Esc) goes straight back to it; the
/// most recent window when none was (the desktop was in front).
pub fn first(windows: &[isize], front: Option<isize>) -> usize {
    front.and_then(|f| windows.iter().position(|&w| w == f)).unwrap_or(0)
}

/// After the list changed: stay on the same window if it is still there, otherwise on the same
/// place, kept in range.
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
    fn opens_on_the_window_in_front() {
        assert_eq!(first(&[10, 20, 30], Some(20)), 1);
        assert_eq!(first(&[10, 20, 30], None), 0);
        assert_eq!(first(&[10, 20, 30], Some(99)), 0, "a window not listed (the desktop)");
        assert_eq!(first(&[], Some(1)), 0);
    }

    #[test]
    fn keeps_the_selected_window_when_others_change() {
        assert_eq!(keep(&[10, 20, 30], 1, &[30, 20]), 1);
        assert_eq!(keep(&[10, 20, 30], 2, &[10, 20]), 1, "closed: same place, in range");
        assert_eq!(keep(&[10], 0, &[]), 0);
    }
}
