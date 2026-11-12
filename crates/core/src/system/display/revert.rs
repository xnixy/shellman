//! "Keep these display settings?": after a change is applied the screen may be unreadable, so
//! unless the user confirms in time the old settings come back. The state machine is pure; the
//! worker feeds it the clock and does what it answers.

use std::time::{Duration, Instant};

pub const WINDOW: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Revert {
    #[default]
    Idle,
    /// A change was applied and awaits an answer until `deadline`.
    Pending { deadline: Instant },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Do {
    Nothing,
    /// Put the old settings back.
    Restore,
    /// The change stands; forget the old settings.
    Forget,
}

impl Revert {
    /// A change was just applied; the window restarts if one was already pending.
    pub fn applied(&mut self, now: Instant) {
        *self = Revert::Pending { deadline: now + WINDOW };
    }

    pub fn keep(&mut self) -> Do {
        match std::mem::take(self) {
            Revert::Pending { .. } => Do::Forget,
            Revert::Idle => Do::Nothing,
        }
    }

    pub fn revert(&mut self) -> Do {
        match std::mem::take(self) {
            Revert::Pending { .. } => Do::Restore,
            Revert::Idle => Do::Nothing,
        }
    }

    /// The clock moved: time is up if the deadline passed.
    pub fn tick(&mut self, now: Instant) -> Do {
        match *self {
            Revert::Pending { deadline } if now >= deadline => self.revert(),
            _ => Do::Nothing,
        }
    }

    pub fn pending(&self) -> Option<Instant> {
        match self {
            Revert::Pending { deadline } => Some(*deadline),
            Revert::Idle => None,
        }
    }
}

/// Whole seconds left, rounded up, for the countdown text.
pub fn seconds_left(deadline: Instant, now: Instant) -> u32 {
    deadline.saturating_duration_since(now).as_millis().div_ceil(1000) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_pending_does_nothing() {
        let mut r = Revert::default();
        let now = Instant::now();
        assert_eq!((r.keep(), r.revert(), r.tick(now + WINDOW * 2)), (Do::Nothing, Do::Nothing, Do::Nothing));
    }

    #[test]
    fn keeping_in_time_forgets_and_a_late_tick_does_nothing() {
        let (mut r, t0) = (Revert::default(), Instant::now());
        r.applied(t0);
        assert_eq!(r.tick(t0 + Duration::from_secs(14)), Do::Nothing);
        assert_eq!(r.keep(), Do::Forget);
        assert_eq!(r.tick(t0 + WINDOW * 2), Do::Nothing);
        assert_eq!(r.pending(), None);
    }

    #[test]
    fn silence_restores_once() {
        let (mut r, t0) = (Revert::default(), Instant::now());
        r.applied(t0);
        assert_eq!(r.tick(t0 + WINDOW), Do::Restore);
        assert_eq!(r.tick(t0 + WINDOW * 2), Do::Nothing);
    }

    #[test]
    fn answering_no_restores_and_a_new_change_restarts_the_window() {
        let (mut r, t0) = (Revert::default(), Instant::now());
        r.applied(t0);
        assert_eq!(r.revert(), Do::Restore);
        r.applied(t0);
        r.applied(t0 + Duration::from_secs(10));
        assert_eq!(r.tick(t0 + Duration::from_secs(20)), Do::Nothing);
        assert_eq!(r.tick(t0 + Duration::from_secs(25)), Do::Restore);
    }

    #[test]
    fn the_countdown_rounds_up() {
        let t = Instant::now();
        assert_eq!(seconds_left(t + Duration::from_millis(14_200), t), 15);
        assert_eq!(seconds_left(t, t + Duration::from_secs(1)), 0);
    }
}
