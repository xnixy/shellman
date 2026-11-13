//! When the preview panel is open: a small state machine over what the pointer does, with the
//! clock passed in so it is tested without waiting.
//!
//! - Hovering an icon that has windows opens the panel after `OPEN_DELAY`.
//! - With the panel open, moving to another such icon switches at once ("scrubbing"); moving to
//!   an icon without windows closes it.
//! - Leaving both the dock and the panel closes it after `GRACE`, long enough to travel from an
//!   icon up to the panel.
//!
//! There are no timers here: it answers `deadline` with how long until something is due, and the
//! caller wakes itself for then.

use std::time::{Duration, Instant};
use windows::Win32::Foundation::RECT;

pub const OPEN_DELAY: Duration = Duration::from_millis(350);
pub const GRACE: Duration = Duration::from_millis(200);

/// What is hovered: an app on one monitor's dock.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Key {
    pub device: String,
    pub app_id: String,
}

/// Where the panel goes, in physical pixels.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Anchor {
    /// The hovered icon's centre.
    pub center_x: i32,
    /// The panel's bottom edge, just above the dock.
    pub bottom: i32,
    /// Where it may go: the work area's sides and top, the monitor's bottom.
    pub work: RECT,
    /// Pixels per point of that monitor.
    pub scale: f32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Phase {
    Idle,
    /// Hovering since; not shown yet.
    Pending(Instant),
    /// Shown; with the time the pointer left both dock and panel, if it has.
    Open(Option<Instant>),
}

pub struct Machine {
    phase: Phase,
    key: Option<Key>,
    anchor: Option<Anchor>,
    /// Counts changes of what is shown, so the owner knows to place the panel again.
    epoch: u32,
    /// The pointer is over a dock icon, or over the panel.
    dock_hot: bool,
    panel_hot: bool,
    /// An icon that was just clicked: it does not reopen the panel until the pointer has been
    /// elsewhere.
    suppressed: Option<Key>,
}

impl Machine {
    pub fn new() -> Self {
        Self {
            phase: Phase::Idle,
            key: None,
            anchor: None,
            epoch: 0,
            dock_hot: false,
            panel_hot: false,
            suppressed: None,
        }
    }

    pub fn is_open(&self) -> bool {
        matches!(self.phase, Phase::Open(_))
    }

    pub fn key(&self) -> Option<&Key> {
        self.key.as_ref().filter(|_| self.is_open())
    }

    pub fn anchor(&self) -> Option<Anchor> {
        self.anchor.filter(|_| self.is_open())
    }

    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    fn is_current(&self, key: &Key) -> bool {
        self.phase != Phase::Idle && self.key.as_ref() == Some(key)
    }

    fn reset(&mut self) {
        if self.phase != Phase::Idle {
            self.epoch += 1;
        }
        self.phase = Phase::Idle;
        self.key = None;
        self.anchor = None;
        self.panel_hot = false;
    }

    /// The pointer is over this icon. `anchor` is only asked for when the panel will be about
    /// something new.
    pub fn hover(&mut self, now: Instant, key: Key, has_windows: bool, anchor: impl FnOnce() -> Anchor) {
        self.dock_hot = true;
        if self.suppressed.as_ref() == Some(&key) {
            return;
        }
        self.suppressed = None;
        if !has_windows {
            return self.reset();
        }
        if let Phase::Open(leaving) = &mut self.phase {
            *leaving = None;
        }
        if self.is_current(&key) {
            return;
        }
        self.anchor = Some(anchor());
        self.key = Some(key);
        self.epoch += 1;
        if !self.is_open() {
            self.phase = Phase::Pending(now);
        }
    }

    /// The pointer is not over `device`'s dock window. (Gaps between icons are still over it, and
    /// change nothing.)
    pub fn leave_dock(&mut self, now: Instant, device: &str) {
        if self.suppressed.as_ref().is_some_and(|k| k.device == device) {
            self.suppressed = None;
        }
        if self.key.as_ref().is_none_or(|k| k.device != device) {
            return;
        }
        self.dock_hot = false;
        match self.phase {
            Phase::Pending(_) => self.reset(),
            Phase::Open(None) if !self.panel_hot => self.phase = Phase::Open(Some(now)),
            _ => {}
        }
    }

    /// The panel's window saw the pointer inside it or not.
    pub fn panel_pointer(&mut self, now: Instant, inside: bool) {
        if inside {
            self.panel_hot = true;
            if let Phase::Open(leaving) = &mut self.phase {
                *leaving = None;
            }
        } else if self.panel_hot {
            self.panel_hot = false;
            if self.phase == Phase::Open(None) && !self.dock_hot {
                self.phase = Phase::Open(Some(now));
            }
        }
    }

    /// Close at once. With `clicked`, that icon stays closed until the pointer goes elsewhere.
    pub fn dismiss(&mut self, clicked: Option<Key>) {
        self.suppressed = clicked;
        self.reset();
    }

    /// Move on whatever has come due.
    pub fn tick(&mut self, now: Instant) {
        match self.phase {
            Phase::Pending(since) if now >= since + OPEN_DELAY => self.phase = Phase::Open(None),
            Phase::Open(Some(left)) if now >= left + GRACE => self.reset(),
            _ => {}
        }
    }

    /// How long until `tick` has something to do, if anything is waiting.
    pub fn deadline(&self, now: Instant) -> Option<Duration> {
        let due = match self.phase {
            Phase::Pending(since) => since + OPEN_DELAY,
            Phase::Open(Some(left)) => left + GRACE,
            _ => return None,
        };
        Some(due.saturating_duration_since(now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANCHOR: Anchor = Anchor {
        center_x: 100,
        bottom: 50,
        work: RECT { left: 0, top: 0, right: 1000, bottom: 500 },
        scale: 1.0,
    };

    fn key(app: &str) -> Key {
        key_on("D1", app)
    }

    fn key_on(device: &str, app: &str) -> Key {
        Key { device: device.into(), app_id: app.into() }
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn hover(m: &mut Machine, t: Instant, k: Key) {
        m.hover(t, k, true, || ANCHOR);
    }

    /// Hover `a` and let the delay pass.
    fn opened(t: Instant) -> Machine {
        let mut m = Machine::new();
        hover(&mut m, t, key("a"));
        m.tick(t + OPEN_DELAY);
        assert!(m.is_open());
        m
    }

    #[test]
    fn opens_only_after_the_delay() {
        let t = Instant::now();
        let mut m = Machine::new();
        hover(&mut m, t, key("a"));
        assert!(!m.is_open());
        assert_eq!(m.deadline(t), Some(OPEN_DELAY));
        m.tick(t + ms(349));
        assert!(!m.is_open());
        // More frames over the same icon do not restart the wait.
        hover(&mut m, t + ms(300), key("a"));
        assert_eq!(m.deadline(t + ms(300)), Some(ms(50)));
        m.tick(t + ms(350));
        assert_eq!(m.key(), Some(&key("a")));
        assert_eq!(m.anchor(), Some(ANCHOR));
        assert_eq!(m.deadline(t + ms(350)), None);
    }

    #[test]
    fn moving_to_another_icon_before_it_opens_starts_over() {
        let t = Instant::now();
        let mut m = Machine::new();
        hover(&mut m, t, key("a"));
        hover(&mut m, t + ms(300), key("b"));
        m.tick(t + ms(400));
        assert!(!m.is_open());
        m.tick(t + ms(650));
        assert_eq!(m.key(), Some(&key("b")));
    }

    #[test]
    fn an_icon_without_windows_never_opens_and_closes_an_open_panel() {
        let t = Instant::now();
        let mut m = Machine::new();
        m.hover(t, key("pin"), false, || ANCHOR);
        assert_eq!(m.deadline(t), None);
        let mut m = opened(t);
        let before = m.epoch();
        m.hover(t + ms(400), key("pin"), false, || ANCHOR);
        assert!(!m.is_open());
        assert_ne!(m.epoch(), before);
    }

    #[test]
    fn scrubbing_switches_at_once_and_can_change_monitor() {
        let t = Instant::now();
        let mut m = opened(t);
        let e = m.epoch();
        // The same icon again changes nothing.
        hover(&mut m, t + ms(360), key("a"));
        assert_eq!(m.epoch(), e);
        hover(&mut m, t + ms(370), key("b"));
        assert_eq!(m.key(), Some(&key("b")));
        assert_ne!(m.epoch(), e);
        hover(&mut m, t + ms(380), key_on("D2", "b"));
        assert_eq!(m.key(), Some(&key_on("D2", "b")));
        assert_eq!(m.deadline(t + ms(380)), None);
    }

    #[test]
    fn leaving_the_dock_closes_after_the_grace() {
        let t = Instant::now();
        let mut m = opened(t);
        let t = t + ms(500);
        m.leave_dock(t, "D1");
        assert_eq!(m.deadline(t), Some(GRACE));
        m.tick(t + ms(199));
        assert!(m.is_open());
        m.tick(t + ms(200));
        assert!(!m.is_open());
    }

    #[test]
    fn reaching_the_panel_or_another_icon_in_time_keeps_it() {
        let t = Instant::now();
        let mut m = opened(t);
        m.leave_dock(t, "D1");
        m.panel_pointer(t + ms(100), true);
        m.tick(t + ms(900));
        assert!(m.is_open());
        // Leaving the panel starts the grace again.
        m.panel_pointer(t + ms(1000), false);
        assert_eq!(m.deadline(t + ms(1000)), Some(GRACE));
        // Back onto an icon before it runs out.
        hover(&mut m, t + ms(1100), key("b"));
        m.tick(t + ms(1300));
        assert_eq!(m.key(), Some(&key("b")));
    }

    #[test]
    fn a_dock_frame_with_the_pointer_in_the_panel_does_not_start_the_grace() {
        let t = Instant::now();
        let mut m = opened(t);
        m.panel_pointer(t, true);
        m.leave_dock(t + ms(50), "D1");
        assert_eq!(m.deadline(t + ms(50)), None);
        // And the panel's first frames, before the pointer was ever in it, are not "leaving".
        let mut m = opened(t);
        m.panel_pointer(t, false);
        assert_eq!(m.deadline(t), None);
    }

    #[test]
    fn leaving_the_panel_onto_an_icon_does_not_start_the_grace() {
        let t = Instant::now();
        let mut m = opened(t);
        m.panel_pointer(t, true);
        hover(&mut m, t + ms(10), key("b"));
        m.panel_pointer(t + ms(12), false);
        assert_eq!(m.deadline(t + ms(12)), None);
    }

    #[test]
    fn leaving_before_it_opens_cancels_it_and_other_docks_do_not_count() {
        let t = Instant::now();
        let mut m = Machine::new();
        hover(&mut m, t, key("a"));
        m.leave_dock(t + ms(100), "D2");
        assert_eq!(m.deadline(t + ms(100)), Some(ms(250)));
        m.leave_dock(t + ms(100), "D1");
        assert_eq!(m.deadline(t + ms(100)), None);
        m.tick(t + ms(1000));
        assert!(!m.is_open());
    }

    #[test]
    fn a_clicked_icon_stays_closed_until_the_pointer_goes_elsewhere() {
        let t = Instant::now();
        let mut m = opened(t);
        m.dismiss(Some(key("a")));
        assert!(!m.is_open());
        hover(&mut m, t + ms(500), key("a"));
        m.tick(t + ms(2000));
        assert!(!m.is_open());
        // Another icon, then back: allowed again.
        hover(&mut m, t + ms(2100), key("b"));
        hover(&mut m, t + ms(2200), key("a"));
        m.tick(t + ms(3000));
        assert!(m.is_open());
        // Or leaving the dock.
        m.dismiss(Some(key("a")));
        m.leave_dock(t + ms(3100), "D1");
        hover(&mut m, t + ms(3200), key("a"));
        m.tick(t + ms(4000));
        assert!(m.is_open());
    }
}
