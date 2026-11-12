//! Which window events can change the window list, decided in the event callback before anything
//! is woken.
//!
//! Hooks set for every process hear every top-level window of every app: each tooltip, menu,
//! popup and drop-down a browser or editor shows, hides or destroys raises the same events as an
//! app window. Waking the list (and every bar drawing it) for those cost a busy machine most of a
//! core. So an event counts only if it is about a window already in the list (hidden, destroyed,
//! cloaked or minimized), or about one that would now join it (shown, uncloaked, titled).

use std::sync::Mutex;

pub const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
pub const EVENT_SYSTEM_MINIMIZESTART: u32 = 0x0016;
pub const EVENT_SYSTEM_MINIMIZEEND: u32 = 0x0017;
pub const EVENT_OBJECT_DESTROY: u32 = 0x8001;
pub const EVENT_OBJECT_SHOW: u32 = 0x8002;
pub const EVENT_OBJECT_HIDE: u32 = 0x8003;
pub const EVENT_OBJECT_LOCATIONCHANGE: u32 = 0x800B;
pub const EVENT_OBJECT_NAMECHANGE: u32 = 0x800C;
pub const EVENT_OBJECT_CLOAKED: u32 = 0x8017;
pub const EVENT_OBJECT_UNCLOAKED: u32 = 0x8018;

/// What an event means for the window list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// Nothing anyone shows can have changed.
    Ignore,
    /// The list (or the window in front) may have changed: read it again.
    List,
    /// The window in front changed its title.
    FrontRenamed,
    /// The window in front moved or resized.
    FrontMoved,
}

/// Decide what `event` means. `known` says the window is in the list as last read, `front` that
/// it is the window in front, and `could_list` (asked only when needed, it costs a few calls)
/// whether it passes the list's rules now.
pub fn verdict(event: u32, known: bool, front: bool, could_list: impl Fn() -> bool) -> Verdict {
    match event {
        EVENT_SYSTEM_FOREGROUND => Verdict::List,
        EVENT_OBJECT_LOCATIONCHANGE if front => Verdict::FrontMoved,
        EVENT_OBJECT_NAMECHANGE if front => Verdict::FrontRenamed,
        // A window that gets its first title can join the list; other windows' titles are not
        // shown until they come to the front.
        EVENT_OBJECT_NAMECHANGE if !known && could_list() => Verdict::List,
        EVENT_OBJECT_DESTROY | EVENT_OBJECT_HIDE | EVENT_OBJECT_CLOAKED if known => Verdict::List,
        EVENT_OBJECT_SHOW | EVENT_OBJECT_UNCLOAKED | EVENT_SYSTEM_MINIMIZESTART | EVENT_SYSTEM_MINIMIZEEND
            if known || could_list() =>
        {
            Verdict::List
        }
        _ => Verdict::Ignore,
    }
}

/// The windows in the list as last read, sorted; `None` until the first read, when every event
/// counts.
static KNOWN: Mutex<Option<Vec<isize>>> = Mutex::new(None);

/// Remember the windows a list read found (called by `WindowList::refresh`).
pub fn set_known(mut hwnds: Vec<isize>) {
    hwnds.sort_unstable();
    *KNOWN.lock().unwrap() = Some(hwnds);
}

/// Whether `hwnd` was in the list as last read (true before any read).
pub fn is_known(hwnd: isize) -> bool {
    KNOWN.lock().unwrap().as_ref().is_none_or(|k| k.binary_search(&hwnd).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn never() -> bool {
        panic!("not asked when the answer is already known")
    }

    #[test]
    fn popups_of_other_apps_are_ignored() {
        // A tooltip or menu: not in the list, and not an app window when shown.
        for event in [EVENT_OBJECT_SHOW, EVENT_OBJECT_UNCLOAKED, EVENT_SYSTEM_MINIMIZEEND, EVENT_OBJECT_NAMECHANGE] {
            assert_eq!(verdict(event, false, false, || false), Verdict::Ignore, "{event:#x}");
        }
        for event in [EVENT_OBJECT_HIDE, EVENT_OBJECT_DESTROY, EVENT_OBJECT_CLOAKED] {
            assert_eq!(verdict(event, false, false, never), Verdict::Ignore, "{event:#x}");
        }
        assert_eq!(verdict(EVENT_OBJECT_LOCATIONCHANGE, false, false, never), Verdict::Ignore);
    }

    #[test]
    fn listed_windows_going_away_count() {
        for event in [EVENT_OBJECT_HIDE, EVENT_OBJECT_DESTROY, EVENT_OBJECT_CLOAKED, EVENT_SYSTEM_MINIMIZESTART] {
            assert_eq!(verdict(event, true, false, never), Verdict::List, "{event:#x}");
        }
    }

    #[test]
    fn new_app_windows_count() {
        assert_eq!(verdict(EVENT_OBJECT_SHOW, false, false, || true), Verdict::List);
        assert_eq!(verdict(EVENT_OBJECT_UNCLOAKED, false, false, || true), Verdict::List);
        // Shown untitled, then titled.
        assert_eq!(verdict(EVENT_OBJECT_NAMECHANGE, false, false, || true), Verdict::List);
    }

    #[test]
    fn background_titles_and_moves_are_ignored() {
        assert_eq!(verdict(EVENT_OBJECT_NAMECHANGE, true, false, never), Verdict::Ignore);
        assert_eq!(verdict(EVENT_OBJECT_LOCATIONCHANGE, true, false, never), Verdict::Ignore);
    }

    #[test]
    fn the_front_window() {
        assert_eq!(verdict(EVENT_OBJECT_NAMECHANGE, true, true, never), Verdict::FrontRenamed);
        assert_eq!(verdict(EVENT_OBJECT_LOCATIONCHANGE, true, true, never), Verdict::FrontMoved);
        assert_eq!(verdict(EVENT_SYSTEM_FOREGROUND, false, true, never), Verdict::List);
    }
}
