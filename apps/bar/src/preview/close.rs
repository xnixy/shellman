//! The one authoritative answer to "should the open panel still be open?", as a pure function.
//!
//! The hover machine closes the panel from pointer events seen by the dock's and the panel's own
//! frames. Those frames stop coming when the dock is hidden for a fullscreen app, when a game
//! captures or hides the pointer, or when the dock goes away, and then nothing would close the
//! panel. So while a panel is open the host also asks this, on a timer, from facts read straight
//! from Windows (the cursor, the windows' rectangles and visibility, the foreground window).

use std::time::{Duration, Instant};
use windows::Win32::Foundation::RECT;

/// How often an open panel is checked. Only while a panel is open.
pub const WATCH_PERIOD: Duration = Duration::from_millis(100);

/// What is true right now.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Facts {
    /// How long the cursor has been outside the dock window and the panel; `None` while inside.
    pub outside_for: Option<Duration>,
    /// The hovered app's dock window is hidden (stepping aside) or gone (monitor removed).
    pub dock_hidden: bool,
    /// A fullscreen window covers the dock's monitor.
    pub covered: bool,
    /// The app has no windows left.
    pub windows_gone: bool,
    /// The foreground window is another one than when the panel opened (an app was started or
    /// activated).
    pub foreground_changed: bool,
    /// The panel's window was shown and is no longer visible (hidden from outside, or destroyed).
    pub panel_hidden: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    PointerLeft,
    DockHidden,
    Covered,
    WindowsGone,
    ForegroundChanged,
    PanelHidden,
}

/// Why the panel should close now, if it should.
pub fn decide(f: &Facts, grace: Duration) -> Option<Reason> {
    if f.dock_hidden {
        Some(Reason::DockHidden)
    } else if f.covered {
        Some(Reason::Covered)
    } else if f.panel_hidden {
        Some(Reason::PanelHidden)
    } else if f.windows_gone {
        Some(Reason::WindowsGone)
    } else if f.foreground_changed {
        Some(Reason::ForegroundChanged)
    } else if f.outside_for.is_some_and(|d| d >= grace) {
        Some(Reason::PointerLeft)
    } else {
        None
    }
}

fn contains(r: &RECT, (x, y): (i32, i32)) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

/// Is the cursor over any of the zones (physical pixels)?
pub fn inside(zones: &[RECT], cursor: (i32, i32)) -> bool {
    zones.iter().any(|z| contains(z, cursor))
}

/// Tracks since when the cursor has been outside the zones.
#[derive(Default)]
pub struct Outside(Option<Instant>);

impl Outside {
    pub fn update(&mut self, now: Instant, inside: bool) -> Option<Duration> {
        if inside {
            self.0 = None;
            None
        } else {
            Some(now.saturating_duration_since(*self.0.get_or_insert(now)))
        }
    }

    pub fn reset(&mut self) {
        self.0 = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: Duration = Duration::from_millis(200);

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn nothing_wrong_stays_open() {
        assert_eq!(decide(&Facts::default(), G), None);
        assert_eq!(decide(&Facts { outside_for: Some(ms(199)), ..Default::default() }, G), None);
    }

    #[test]
    fn pointer_outside_closes_after_the_grace() {
        assert_eq!(decide(&Facts { outside_for: Some(ms(200)), ..Default::default() }, G), Some(Reason::PointerLeft));
    }

    #[test]
    fn each_reason_closes_at_once() {
        let f = Facts::default();
        assert_eq!(decide(&Facts { dock_hidden: true, ..f }, G), Some(Reason::DockHidden));
        assert_eq!(decide(&Facts { covered: true, ..f }, G), Some(Reason::Covered));
        assert_eq!(decide(&Facts { windows_gone: true, ..f }, G), Some(Reason::WindowsGone));
        assert_eq!(decide(&Facts { foreground_changed: true, ..f }, G), Some(Reason::ForegroundChanged));
        assert_eq!(decide(&Facts { panel_hidden: true, ..f }, G), Some(Reason::PanelHidden));
    }

    #[test]
    fn a_captured_pointer_still_closes_through_the_other_facts() {
        // The pointer never moves out (a game holds it) but the dock was hidden for the game.
        let f = Facts { outside_for: None, dock_hidden: true, covered: true, ..Default::default() };
        assert!(decide(&f, G).is_some());
    }

    #[test]
    fn zones_are_half_open_rectangles() {
        let dock = RECT { left: 0, top: 100, right: 100, bottom: 140 };
        let panel = RECT { left: 20, top: 0, right: 80, bottom: 90 };
        assert!(inside(&[dock, panel], (50, 120)));
        assert!(inside(&[dock, panel], (50, 10)));
        assert!(!inside(&[dock, panel], (50, 95)));
        assert!(!inside(&[dock, panel], (100, 120)));
        assert!(!inside(&[], (0, 0)));
    }

    #[test]
    fn outside_tracks_from_the_first_miss_and_resets_inside() {
        let t = Instant::now();
        let mut o = Outside::default();
        assert_eq!(o.update(t, true), None);
        assert_eq!(o.update(t + ms(10), false), Some(ms(0)));
        assert_eq!(o.update(t + ms(150), false), Some(ms(140)));
        assert_eq!(o.update(t + ms(160), true), None);
        assert_eq!(o.update(t + ms(170), false), Some(ms(0)));
    }
}
