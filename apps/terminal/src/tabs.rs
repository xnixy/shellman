//! The window's tabs as a list: which is shown, where a new one goes, which is shown after one
//! closes, moving one, and the keyboard's ways through them. Pure, so the rules are tested
//! without shells or a window.

pub struct Tabs<T> {
    items: Vec<T>,
    active: usize,
}

impl<T> Tabs<T> {
    pub fn new(first: T) -> Self {
        Self { items: vec![first], active: 0 }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn active(&self) -> usize {
        self.active
    }

    pub fn current(&self) -> &T {
        &self.items[self.active]
    }

    pub fn current_mut(&mut self) -> &mut T {
        &mut self.items[self.active]
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }

    pub fn get(&self, i: usize) -> Option<&T> {
        self.items.get(i)
    }

    /// A new tab right after the shown one, shown (as browsers and Windows Terminal do).
    pub fn open(&mut self, item: T) -> usize {
        let at = (self.active + 1).min(self.items.len());
        self.items.insert(at, item);
        self.active = at;
        at
    }

    /// Close tab `i`. Returns it, or None when `i` is the last tab left (the window closes
    /// instead). Closing the shown tab shows the one after it, or before it at the end; closing
    /// another keeps the same tab shown.
    pub fn close(&mut self, i: usize) -> Option<T> {
        if i >= self.items.len() || self.items.len() == 1 {
            return None;
        }
        let item = self.items.remove(i);
        if i < self.active || (i == self.active && self.active == self.items.len()) {
            self.active -= 1;
        }
        Some(item)
    }

    /// Show tab `i` (ignored when there is none).
    pub fn select(&mut self, i: usize) {
        if i < self.items.len() {
            self.active = i;
        }
    }

    /// Ctrl+Tab and Ctrl+Shift+Tab: the next or previous tab, round the ends.
    pub fn stepped(&self, forward: bool) -> usize {
        let n = self.items.len();
        if forward { (self.active + 1) % n } else { (self.active + n - 1) % n }
    }

    /// Ctrl+Alt+1..9: the n-th tab (1-based); 9 is always the last, as in browsers. The shown
    /// one when there is no such tab.
    pub fn numbered(&self, n: usize) -> usize {
        match n {
            9 => self.items.len() - 1,
            1..=8 if n <= self.items.len() => n - 1,
            _ => self.active,
        }
    }

    /// Move tab `from` to `to`, the shown tab staying the same tab.
    pub fn move_tab(&mut self, from: usize, to: usize) {
        let n = self.items.len();
        if from >= n || to >= n || from == to {
            return;
        }
        let item = self.items.remove(from);
        self.items.insert(to, item);
        self.active = if self.active == from {
            to
        } else if from < self.active && to >= self.active {
            self.active - 1
        } else if from > self.active && to <= self.active {
            self.active + 1
        } else {
            self.active
        };
    }

    /// Take out every tab `pred` says is done (its shell exited). Returns whether any are left.
    /// The shown tab stays shown; if it is taken out, the tab after it slides into its place (or
    /// the one before, at the end).
    pub fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) -> bool {
        let mut before = 0;
        let mut kept = Vec::with_capacity(self.items.len());
        for (k, item) in std::mem::take(&mut self.items).into_iter().enumerate() {
            if keep(&item) {
                if k < self.active {
                    before += 1;
                }
                kept.push(item);
            }
        }
        self.items = kept;
        self.active = before.min(self.items.len().saturating_sub(1));
        !self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs(n: usize, active: usize) -> Tabs<usize> {
        let mut t = Tabs::new(0);
        for i in 1..n {
            t.items.push(i);
        }
        t.active = active;
        t
    }

    fn order(t: &Tabs<usize>) -> Vec<usize> {
        t.iter().copied().collect()
    }

    #[test]
    fn a_new_tab_goes_after_the_shown_one_and_is_shown() {
        let mut t = tabs(3, 0);
        assert_eq!(t.open(9), 1);
        assert_eq!(order(&t), [0, 9, 1, 2]);
        assert_eq!(*t.current(), 9);
    }

    #[test]
    fn closing_the_shown_tab_shows_the_next_or_the_one_before_at_the_end() {
        let mut t = tabs(3, 1);
        assert_eq!(t.close(1), Some(1));
        assert_eq!(*t.current(), 2);
        let mut t = tabs(3, 2);
        t.close(2);
        assert_eq!(*t.current(), 1);
    }

    #[test]
    fn closing_another_tab_keeps_the_shown_one() {
        let mut t = tabs(4, 2);
        t.close(0);
        assert_eq!(*t.current(), 2);
        t.close(2);
        assert_eq!(*t.current(), 2);
        assert_eq!(order(&t), [1, 2]);
    }

    #[test]
    fn the_last_tab_is_not_closed_here() {
        let mut t = tabs(1, 0);
        assert_eq!(t.close(0), None);
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn stepping_wraps_round() {
        let t = tabs(3, 2);
        assert_eq!(t.stepped(true), 0);
        assert_eq!(t.stepped(false), 1);
        assert_eq!(tabs(3, 0).stepped(false), 2);
    }

    #[test]
    fn numbers_pick_tabs_and_nine_is_the_last() {
        let t = tabs(12, 0);
        assert_eq!(t.numbered(3), 2);
        assert_eq!(t.numbered(9), 11);
        assert_eq!(tabs(2, 1).numbered(5), 1, "no fifth tab: nothing changes");
    }

    #[test]
    fn moving_keeps_the_same_tab_shown() {
        let mut t = tabs(4, 1);
        t.move_tab(1, 3);
        assert_eq!(order(&t), [0, 2, 3, 1]);
        assert_eq!(*t.current(), 1);
        let mut t = tabs(4, 2);
        t.move_tab(0, 3);
        assert_eq!(*t.current(), 2);
        assert_eq!(t.active(), 1);
        let mut t = tabs(4, 1);
        t.move_tab(3, 0);
        assert_eq!(*t.current(), 1);
        assert_eq!(t.active(), 2);
    }

    #[test]
    fn exited_shells_leave_and_the_shown_tab_follows_the_rules() {
        let mut t = tabs(4, 2);
        assert!(t.retain(|&x| x != 0));
        assert_eq!(*t.current(), 2);
        // The shown one exits: the next takes its place.
        let mut t = tabs(4, 1);
        assert!(t.retain(|&x| x != 1));
        assert_eq!(*t.current(), 2);
        // The shown one is last and exits: the one before.
        let mut t = tabs(3, 2);
        assert!(t.retain(|&x| x != 2));
        assert_eq!(*t.current(), 1);
        let mut t = tabs(2, 0);
        assert!(!t.retain(|_| false));
    }
}
