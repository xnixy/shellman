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

/// What the session does once a fallback (`fallback::run`) is over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum After {
    /// Stay parked, with the watchdog looking for a taskbar.
    Park,
    /// Supervise the apps again, with a fresh crash budget.
    Resume,
}

/// Explorer gave no taskbar even after the host was asked to quit and a retry: supervise the
/// bar again, once per session, so a bar that cannot run does not bounce between the two for
/// ever. Never after Shift (the user's choice). Every other end parks, under the watchdog.
pub fn after_fallback(reason: Reason, outcome: crate::fallback::Outcome, resumed_before: bool) -> After {
    let failed = outcome == crate::fallback::Outcome::NoTaskbar;
    if failed && reason != Reason::Shift && !resumed_before {
        After::Resume
    } else {
        After::Park
    }
}

/// While parked, how often the watchdog looks for a taskbar.
pub const WATCH_EVERY: Duration = Duration::from_secs(60);
/// No taskbar must be seen twice, this far apart, before anything is done (an Explorer that is
/// restarting itself has none for a moment).
pub const CONFIRM: Duration = Duration::from_secs(10);
/// At most this many fallbacks from the watchdog within `RETRY_WINDOW`.
pub const RETRIES: usize = 5;
pub const RETRY_WINDOW: Duration = Duration::from_secs(60 * 60);

/// What the watchdog wants after a look.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    /// Look again after this long.
    Wait(Duration),
    /// No taskbar at all: run the fallback again.
    Retry,
    /// No taskbar, and the retries for this hour are spent: say so loudly, look again later.
    Spent(Duration),
}

/// A parked session must never be the end state with neither a shellman bar nor an Explorer
/// taskbar. This looks for any `Shell_TrayWnd` every minute; when there is none (twice, 10 s
/// apart) it runs the fallback again (free the shell window, start Explorer, check), at most
/// `RETRIES` times an hour.
#[derive(Default, Debug)]
pub struct Watchdog {
    missing_since: Option<Duration>,
    tries: Vec<Duration>,
}

impl Watchdog {
    /// `now`: time since the session started.
    pub fn look(&mut self, taskbar: bool, now: Duration) -> Look {
        if taskbar {
            self.missing_since = None;
            return Look::Wait(WATCH_EVERY);
        }
        let Some(since) = self.missing_since else {
            self.missing_since = Some(now);
            return Look::Wait(CONFIRM);
        };
        if now.saturating_sub(since) < CONFIRM {
            return Look::Wait(CONFIRM - now.saturating_sub(since));
        }
        self.tries.retain(|&t| now.saturating_sub(t) < RETRY_WINDOW);
        if self.tries.len() >= RETRIES {
            // Confirmed afresh when the wait is over.
            self.missing_since = None;
            let oldest = self.tries.iter().min().copied().unwrap_or(now);
            return Look::Spent((oldest + RETRY_WINDOW).saturating_sub(now).max(WATCH_EVERY));
        }
        self.tries.push(now);
        self.missing_since = None;
        Look::Retry
    }
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
    fn resumes_once_when_explorer_gave_no_taskbar() {
        use crate::fallback::Outcome::*;
        assert_eq!(after_fallback(Reason::BarLost, NoTaskbar, false), After::Resume);
        assert_eq!(after_fallback(Reason::NoTray, NoTaskbar, false), After::Resume);
        assert_eq!(after_fallback(Reason::BarLost, NoTaskbar, true), After::Park, "only once");
        assert_eq!(after_fallback(Reason::Shift, NoTaskbar, false), After::Park, "Shift is the user's choice");
        for ok in [Taskbar, AlreadyShell, Ending] {
            assert_eq!(after_fallback(Reason::BarLost, ok, false), After::Park);
        }
    }

    fn s(secs: u64) -> Duration {
        Duration::from_secs(secs)
    }

    #[test]
    fn the_watchdog_leaves_a_taskbar_alone() {
        let mut w = Watchdog::default();
        for t in 0..100 {
            assert_eq!(w.look(true, s(t * 60)), Look::Wait(WATCH_EVERY));
        }
    }

    #[test]
    fn the_watchdog_retries_after_two_looks_without_a_taskbar() {
        let mut w = Watchdog::default();
        assert_eq!(w.look(false, s(100)), Look::Wait(CONFIRM));
        assert_eq!(w.look(false, s(104)), Look::Wait(s(6)), "woken early: the rest of the wait");
        assert_eq!(w.look(false, s(110)), Look::Retry);
        // A taskbar came back for a moment in between: the count starts over.
        assert_eq!(w.look(false, s(200)), Look::Wait(CONFIRM));
        assert_eq!(w.look(true, s(210)), Look::Wait(WATCH_EVERY));
        assert_eq!(w.look(false, s(270)), Look::Wait(CONFIRM));
    }

    #[test]
    fn the_watchdog_is_bounded_but_never_gives_up_for_good() {
        let mut w = Watchdog::default();
        let mut t = s(0);
        let mut retries = 0;
        let mut spent = None;
        for _ in 0..40 {
            match w.look(false, t) {
                Look::Retry => retries += 1,
                Look::Spent(d) => {
                    spent = Some(d);
                    break;
                }
                Look::Wait(_) => {}
            }
            t += CONFIRM;
        }
        assert_eq!(retries, RETRIES);
        let d = spent.expect("spent after RETRIES");
        assert!(d >= WATCH_EVERY && d <= RETRY_WINDOW);
        // An hour after the first retry it tries again.
        let later = t + RETRY_WINDOW;
        assert_eq!(w.look(false, later), Look::Wait(CONFIRM));
        assert_eq!(w.look(false, later + CONFIRM), Look::Retry);
    }
}
