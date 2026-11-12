//! The safety net: a session that is the shell and cannot show a bar leaves the user with an
//! empty screen, so in that case it starts Explorer instead. Pure decisions here; the Win32 side
//! (looking for windows, starting explorer.exe) is in `fallback.rs`.

use std::time::Duration;

/// A bar that has not made the tray window by now is not coming.
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

#[derive(Clone, Copy, Debug)]
pub struct Seen {
    pub shift: bool,
    pub bar_lost: bool,
    pub bar_alive: bool,
    pub tray_up: bool,
    pub since_start: Duration,
}

/// `Some(reason)` when Explorer should take over.
pub fn decide(s: &Seen) -> Option<Reason> {
    if s.shift {
        return Some(Reason::Shift);
    }
    if s.bar_lost {
        return Some(Reason::BarLost);
    }
    if s.since_start >= DEADLINE && !s.tray_up {
        return Some(if s.bar_alive { Reason::NoTray } else { Reason::BarLost });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen() -> Seen {
        Seen { shift: false, bar_lost: false, bar_alive: true, tray_up: false, since_start: Duration::from_secs(1) }
    }

    #[test]
    fn a_starting_bar_is_given_time() {
        assert_eq!(decide(&seen()), None);
        assert_eq!(decide(&Seen { since_start: DEADLINE - Duration::from_millis(1), ..seen() }), None);
    }

    #[test]
    fn shift_always_wins() {
        assert_eq!(decide(&Seen { shift: true, tray_up: true, ..seen() }), Some(Reason::Shift));
    }

    #[test]
    fn a_lost_bar_falls_back_at_once() {
        assert_eq!(decide(&Seen { bar_lost: true, bar_alive: false, ..seen() }), Some(Reason::BarLost));
    }

    #[test]
    fn no_tray_after_the_deadline_falls_back() {
        assert_eq!(decide(&Seen { since_start: DEADLINE, ..seen() }), Some(Reason::NoTray));
        assert_eq!(decide(&Seen { since_start: DEADLINE, bar_alive: false, ..seen() }), Some(Reason::BarLost));
        assert_eq!(decide(&Seen { since_start: DEADLINE, tray_up: true, ..seen() }), None);
    }
}
