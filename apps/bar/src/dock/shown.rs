//! What the bars show of the window list, to tell whether a fresh read changed any of it.
//!
//! The window list is read again after every event that may matter, and many of those change
//! nothing on screen (a dialog's owner came back to the front, a window was hidden and shown).
//! Each bar window's frame costs several milliseconds, so only the bars whose picture changed are
//! repainted: the docks draw the icons and which app is in front; the top bars, the window in
//! front and its title.

use super::items::Item;
use shellman_core::winlist::AppWindow;

#[derive(Clone, PartialEq, Debug)]
pub struct Shown {
    items: Vec<Item>,
    front: Option<isize>,
    front_window: Option<AppWindow>,
}

/// Which bars must repaint.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Change {
    pub docks: bool,
    pub tops: bool,
}

impl Shown {
    pub fn of(items: &[Item], front: Option<isize>, front_window: Option<&AppWindow>) -> Self {
        Self { items: items.to_vec(), front, front_window: front_window.cloned() }
    }

    pub fn change(&self, now: &Shown) -> Change {
        Change {
            docks: self.items != now.items || self.front != now.front,
            tops: self.front_window != now.front_window,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dock::items::Section;

    fn item(app: &str, windows: &[isize]) -> Item {
        Item { section: Section::Running, app_id: app.into(), exe: None, windows: windows.to_vec() }
    }

    fn window(hwnd: isize, title: &str) -> AppWindow {
        AppWindow { hwnd, title: title.into(), app_id: "a".into(), exe: None, minimized: false }
    }

    #[test]
    fn nothing_changed_wakes_nothing() {
        let w = window(1, "Doc");
        let a = Shown::of(&[item("a", &[1])], Some(1), Some(&w));
        assert_eq!(a.change(&a.clone()), Change::default());
    }

    #[test]
    fn a_new_window_wakes_the_docks_only() {
        let w = window(1, "Doc");
        let a = Shown::of(&[item("a", &[1])], Some(1), Some(&w));
        let b = Shown::of(&[item("a", &[1]), item("b", &[2])], Some(1), Some(&w));
        assert_eq!(a.change(&b), Change { docks: true, tops: false });
    }

    #[test]
    fn a_new_title_wakes_the_top_bars_only() {
        let a = Shown::of(&[item("a", &[1])], Some(1), Some(&window(1, "Doc")));
        let b = Shown::of(&[item("a", &[1])], Some(1), Some(&window(1, "Doc *")));
        assert_eq!(a.change(&b), Change { docks: false, tops: true });
    }

    #[test]
    fn another_app_in_front_wakes_both() {
        let items = [item("a", &[1]), item("b", &[2])];
        let a = Shown::of(&items, Some(1), Some(&window(1, "A")));
        let b = Shown::of(&items, Some(2), Some(&window(2, "B")));
        assert_eq!(a.change(&b), Change { docks: true, tops: true });
    }
}
