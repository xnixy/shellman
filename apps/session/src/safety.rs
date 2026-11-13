//! The safety net: a session that is the shell and cannot show a bar leaves the user with an
//! empty screen, so in that case it starts Explorer instead. Pure decisions here; the Win32 side
//! (looking for windows, starting explorer.exe) is in `fallback.rs`.
//!
//! A bar that crashes is not lost: the supervisor restarts it (0.5 s, 2 s, 8 s) until it has
//! crashed too often in a short time. The net only fires when the bar is gone for good (its exe
//! missing, given up on, or ended on purpose), or when a bar that runs has not made the tray
//! window within `DEADLINE` of its own latest start. Measuring from the session's start instead
//! made one crash after a long uptime look like "no tray after the deadline" while the restart
//! was still pending, and handed the session to Explorer.

use std::time::Duration;

/// A bar that has not made the tray window this long after it was (re)started is not coming.
pub const DEADLINE: Duration = Duration::from_secs(20);

/// After a fallback, how long Explorer gets to become the shell (own `Shell_TrayWnd`).
pub const EXPLORER_GRACE: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reason {
    /// Shift was held while the session started: the user's way out.
    Shift,
    /// The bar's exe is not there, or it was given up on, or it ended.
    BarLost,
    /// The bar runs, but there is still no tray window after the deadline.
    NoTray,
}

impl Reason {
    pub fn text(self) -> &'static str {
        match self {
            Reason::Shift => "Shift was held at start",
            Reason::BarLost => "the bar is missing, crashed too often, or ended",
            Reason::NoTray => "the bar did not come up in time (no Shell_TrayWnd)",
        }
    }
}

/// Where the bar's slot stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bar {
    /// Its process runs.
    Running,
    /// Not running, and a (re)start is scheduled.
    Pending,
    /// Not running, and nothing will start it: exe missing, given up on, or ended on purpose.
    Lost,
    /// The app list has no bar at all (a test list).
    Absent,
}

impl Bar {
    pub fn of(running: bool, scheduled: bool) -> Bar {
        match (running, scheduled) {
            (true, _) => Bar::Running,
            (false, true) => Bar::Pending,
            (false, false) => Bar::Lost,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Seen {
    pub shift: bool,
    pub bar: Bar,
    pub tray_up: bool,
    /// Since the bar's latest start (since the session's start when there is no bar).
    pub since: Duration,
}

/// `Some(reason)` when Explorer should take over.
pub fn decide(s: &Seen) -> Option<Reason> {
    if s.shift {
        return Some(Reason::Shift);
    }
    match s.bar {
        Bar::Lost => Some(Reason::BarLost),
        // A crash the supervisor is about to answer: wait for the new copy.
        Bar::Pending => None,
        _ if s.tray_up || s.since < DEADLINE => None,
        Bar::Running => Some(Reason::NoTray),
        Bar::Absent => Some(Reason::BarLost),
    }
}

/// How long the loop may sleep before `decide` could change its mind without a process exit
/// waking it: the rest of a running bar's deadline while its tray is not up yet.
pub fn wake_in(s: &Seen) -> Option<Duration> {
    let waiting = matches!(s.bar, Bar::Running | Bar::Absent) && !s.tray_up;
    waiting.then(|| DEADLINE.saturating_sub(s.since) + Duration::from_millis(50))
}

/// After a fallback (not Shift: that one is the user's choice), whether Explorer failed to become
/// the shell, so supervising should start again. That happens when something else already owns
/// the shell window (the modern-app host): Explorer then only opens a folder window, and a parked
/// session would leave the user with neither bar. Only once per session, so a bar that cannot
/// run does not bounce between the two for ever.
pub fn resume(reason: Reason, explorer_is_shell: bool, since_fallback: Duration, resumed_before: bool) -> bool {
    reason != Reason::Shift && !resumed_before && !explorer_is_shell && since_fallback >= EXPLORER_GRACE
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{Crashes, Next};

    fn seen() -> Seen {
        Seen { shift: false, bar: Bar::Running, tray_up: false, since: Duration::from_secs(1) }
    }

    #[test]
    fn a_starting_bar_is_given_time() {
        assert_eq!(decide(&seen()), None);
        assert_eq!(decide(&Seen { since: DEADLINE - Duration::from_millis(1), ..seen() }), None);
    }

    #[test]
    fn shift_always_wins() {
        assert_eq!(decide(&Seen { shift: true, tray_up: true, ..seen() }), Some(Reason::Shift));
    }

    #[test]
    fn a_lost_bar_falls_back_at_once() {
        assert_eq!(decide(&Seen { bar: Bar::Lost, ..seen() }), Some(Reason::BarLost));
    }

    #[test]
    fn no_tray_after_the_deadline_falls_back() {
        assert_eq!(decide(&Seen { since: DEADLINE, ..seen() }), Some(Reason::NoTray));
        assert_eq!(decide(&Seen { since: DEADLINE, bar: Bar::Absent, ..seen() }), Some(Reason::BarLost));
        assert_eq!(decide(&Seen { since: DEADLINE, tray_up: true, ..seen() }), None);
    }

    #[test]
    fn a_restart_pending_at_the_check_is_not_a_loss() {
        // The tray went with the bar, and the session has run far past the deadline.
        let s = Seen { bar: Bar::Pending, tray_up: false, since: Duration::from_secs(4801), ..seen() };
        assert_eq!(decide(&s), None);
        assert_eq!(wake_in(&s), None, "the restart timer wakes the loop, not the deadline");
    }

    #[test]
    fn one_crash_after_a_long_uptime_is_restarted_without_fallback() {
        let mut crashes = Crashes::default();
        let uptime = Duration::from_millis(4_801_203);
        let next = crashes.on_exit(0xffff_ffff, uptime);
        assert_eq!(next, Next::Restart(Duration::from_millis(500)));
        // At the moment of the exit: a restart is scheduled.
        assert_eq!(decide(&Seen { bar: Bar::of(false, true), since: uptime, ..seen() }), None);
        // The new copy runs; its deadline counts from its own start.
        let restarted = Seen { bar: Bar::of(true, false), since: Duration::from_millis(300), ..seen() };
        assert_eq!(decide(&restarted), None);
        assert!(wake_in(&restarted).is_some_and(|d| d < DEADLINE + Duration::from_millis(51)));
        assert_eq!(decide(&Seen { tray_up: true, ..restarted }), None);
    }

    #[test]
    fn crashing_past_the_budget_falls_back() {
        let mut crashes = Crashes::default();
        let mut t = Duration::from_secs(100);
        let mut next = Next::Deliberate;
        for _ in 0..crate::policy::LIMIT {
            next = crashes.on_exit(1, t);
            let scheduled = matches!(next, Next::Restart(_));
            let s = Seen { bar: Bar::of(false, scheduled), since: Duration::ZERO, ..seen() };
            if scheduled {
                assert_eq!(decide(&s), None);
            }
            t += Duration::from_secs(3);
        }
        assert_eq!(next, Next::GiveUp);
        assert_eq!(decide(&Seen { bar: Bar::of(false, false), ..seen() }), Some(Reason::BarLost));
    }

    #[test]
    fn resumes_once_when_explorer_did_not_take_over() {
        let g = EXPLORER_GRACE;
        assert!(resume(Reason::BarLost, false, g, false));
        assert!(resume(Reason::NoTray, false, g, false));
        assert!(!resume(Reason::BarLost, false, g - Duration::from_millis(1), false), "Explorer is given time");
        assert!(!resume(Reason::BarLost, true, g, false), "Explorer is the shell: stay out of its way");
        assert!(!resume(Reason::BarLost, false, g, true), "only once");
        assert!(!resume(Reason::Shift, false, g, false), "Shift is the user's choice");
    }
}
