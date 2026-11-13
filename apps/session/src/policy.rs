//! What to do when a supervised app exits. No Win32 here, so every rule is tested.
//!
//! Exit code 0 means the app was ended on purpose (a quit, or a `--background` start that found
//! its resident already running). The apps have no quit UI yet, so nothing else can produce it.
//! Any other code is a crash: restarted with a growing pause, and given up on after `LIMIT`
//! crashes within `WINDOW`, so a broken app is not started in a loop forever.

use std::time::Duration;

/// Pauses before the 1st, 2nd and later restarts in a row.
pub const DELAYS: [Duration; 3] = [Duration::from_millis(500), Duration::from_secs(2), Duration::from_secs(8)];
pub const LIMIT: usize = 5;
pub const WINDOW: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Next {
    /// Start it again after this pause.
    Restart(Duration),
    /// Too many crashes in a short time: leave it stopped.
    GiveUp,
    /// It was ended on purpose: leave it stopped.
    Deliberate,
}

/// When an app crashed, as time since the session started.
#[derive(Default, Debug)]
pub struct Crashes(Vec<Duration>);

impl Crashes {
    pub fn on_exit(&mut self, code: u32, now: Duration) -> Next {
        if code == 0 {
            return Next::Deliberate;
        }
        self.0.retain(|&t| now.saturating_sub(t) < WINDOW);
        self.0.push(now);
        let n = self.0.len();
        if n >= LIMIT {
            return Next::GiveUp;
        }
        Next::Restart(DELAYS[(n - 1).min(DELAYS.len() - 1)])
    }

    /// The process ended because the session asked it to (`restart.rs`): start it again at once.
    /// Not a crash, so it is not counted, never gives up and never waits. Whatever the exit code.
    pub fn on_requested_exit(&self) -> Next {
        Next::Restart(Duration::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn exit_zero_is_deliberate() {
        let mut c = Crashes::default();
        assert_eq!(c.on_exit(0, secs(1)), Next::Deliberate);
        // And it does not count towards giving up.
        assert_eq!(c.on_exit(1, secs(2)), Next::Restart(DELAYS[0]));
    }

    #[test]
    fn backs_off_then_gives_up() {
        let mut c = Crashes::default();
        assert_eq!(c.on_exit(1, secs(1)), Next::Restart(Duration::from_millis(500)));
        assert_eq!(c.on_exit(1, secs(2)), Next::Restart(secs(2)));
        assert_eq!(c.on_exit(0xC000_0005, secs(5)), Next::Restart(secs(8)));
        assert_eq!(c.on_exit(101, secs(14)), Next::Restart(secs(8)));
        assert_eq!(c.on_exit(1, secs(23)), Next::GiveUp);
    }

    #[test]
    fn a_requested_restart_is_not_a_crash() {
        let mut c = Crashes::default();
        // Even right after the budget is nearly spent, and with any exit code, asking changes nothing.
        for t in 1..=4 {
            c.on_exit(1, secs(t));
        }
        for _ in 0..10 {
            assert_eq!(c.on_requested_exit(), Next::Restart(Duration::ZERO));
        }
        // The list still holds the four crashes (the fifth gives up) and nothing was added.
        assert_eq!(c.on_exit(1, secs(5)), Next::GiveUp);
        let mut fresh = Crashes::default();
        fresh.on_requested_exit();
        assert_eq!(fresh.on_exit(1, secs(1)), Next::Restart(DELAYS[0]), "the first real crash is still the first");
    }

    #[test]
    fn old_crashes_stop_counting() {
        let mut c = Crashes::default();
        for t in 1..=4 {
            c.on_exit(1, secs(t));
        }
        // A long quiet time later it is a first crash again.
        assert_eq!(c.on_exit(1, secs(200)), Next::Restart(DELAYS[0]));
    }
}
