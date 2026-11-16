//! When to check, and when to hold off. Pure: the caller gathers the facts.
//!
//! - The first check is [`FIRST`] after the session starts (sign-in is busy enough), then one a
//!   day, counted from the last check (`update.json`), so restarting the session does not check
//!   again. A failed check is tried again after [`RETRY_FAILED`], a held one after [`RETRY_HELD`].
//! - Updating automatically off: nothing is started; the session looks again in [`OFF_LOOK`] in
//!   case it was turned on.
//! - A release is installed only while no app is fullscreen in front (a game, a video, a
//!   presentation) and no file copy, move or delete is under way. Held, the helper looks again
//!   every [`HOLD_POLL`] for at most [`HOLD_MAX`], then gives up until the next check.
//! - Nothing here ever signs the user out: the session itself switches at the next sign-in.

use crate::state::Outcome;
use std::time::Duration;

pub const FIRST: Duration = Duration::from_secs(5 * 60);
pub const DAILY: Duration = Duration::from_secs(24 * 3600);
pub const RETRY_FAILED: Duration = Duration::from_secs(3 * 3600);
pub const RETRY_HELD: Duration = Duration::from_secs(3600);
pub const OFF_LOOK: Duration = Duration::from_secs(3600);
pub const HOLD_POLL: Duration = Duration::from_secs(30);
pub const HOLD_MAX: Duration = Duration::from_secs(2 * 3600);

/// How long from now until the next check. `uptime`: since the session started; `now` and
/// `last_check`: Unix seconds (`last_check` 0 = never); `outcome`: what the last one found.
pub fn next_check(uptime: Duration, now: u64, last_check: u64, outcome: Outcome) -> Duration {
    let first = FIRST.saturating_sub(uptime);
    if last_check == 0 || last_check > now {
        // Never checked, or the clock went back: the regular first check.
        return first;
    }
    let every = match outcome {
        // `Working` left behind means a helper died mid-way: like a failure.
        Outcome::Failed | Outcome::Working => RETRY_FAILED,
        Outcome::Held => RETRY_HELD,
        _ => DAILY,
    };
    let since = Duration::from_secs(now - last_check);
    every.saturating_sub(since).max(first)
}

/// The least time between two checks the session starts, whatever `update.json` says. A helper
/// that ends without recording a check (it found another check under way, it crashed, the file
/// could not be written) would otherwise make `next_check` say "now" again at once.
pub const MIN_GAP: Duration = Duration::from_secs(3600);

/// [`next_check`]'s answer, held back to [`MIN_GAP`] after the last start (`None`: none yet).
pub fn with_floor(next: Duration, since_last_start: Option<Duration>) -> Duration {
    match since_last_start {
        Some(since) => next.max(MIN_GAP.saturating_sub(since)),
        None => next,
    }
}

/// What is happening that a new release must not interrupt.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Busy {
    /// The window in front covers its whole monitor.
    pub fullscreen: bool,
    /// A file copy, move or delete is under way (Windows' progress dialog is up, or a shellman
    /// window says it has file work).
    pub file_work: bool,
}

/// Why to hold off now, or `None` to go ahead.
pub fn hold(b: Busy) -> Option<&'static str> {
    if b.fullscreen {
        Some("an app was fullscreen")
    } else if b.file_work {
        Some("files were being copied")
    } else {
        None
    }
}

/// Whether to keep waiting after `waited` held off.
pub fn keep_waiting(waited: Duration) -> bool {
    waited < HOLD_MAX
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: u64 = 3600;
    const NOW: u64 = 1_800_000_000;

    #[test]
    fn the_first_check_is_five_minutes_after_sign_in() {
        assert_eq!(next_check(Duration::ZERO, NOW, 0, Outcome::Never), FIRST);
        assert_eq!(next_check(Duration::from_secs(60), NOW, 0, Outcome::Never), Duration::from_secs(240));
        assert_eq!(next_check(Duration::from_secs(600), NOW, 0, Outcome::Never), Duration::ZERO);
    }

    #[test]
    fn then_daily_from_the_last_check() {
        // Checked an hour ago, session just restarted: not again for 23 h.
        assert_eq!(next_check(Duration::ZERO, NOW, NOW - H, Outcome::UpToDate), Duration::from_secs(23 * H));
        // Checked two days ago: due, but never before five minutes into the session.
        assert_eq!(next_check(Duration::ZERO, NOW, NOW - 48 * H, Outcome::Installed), FIRST);
        assert_eq!(next_check(Duration::from_secs(3600), NOW, NOW - 48 * H, Outcome::UpToDate), Duration::ZERO);
    }

    #[test]
    fn failures_and_holds_retry_sooner() {
        let up = Duration::from_secs(3600);
        assert_eq!(next_check(up, NOW, NOW - H, Outcome::Failed), Duration::from_secs(2 * H));
        assert_eq!(next_check(up, NOW, NOW - H / 2, Outcome::Held), Duration::from_secs(H / 2));
        assert_eq!(next_check(up, NOW, NOW - H, Outcome::Working), Duration::from_secs(2 * H));
    }

    #[test]
    fn a_clock_that_went_back_does_not_wait_for_years() {
        assert_eq!(next_check(Duration::from_secs(3600), NOW, NOW + 365 * 24 * H, Outcome::UpToDate), Duration::ZERO);
    }

    #[test]
    fn a_helper_that_records_nothing_is_not_restarted_at_once() {
        // The state file still says "never checked" right after a start: wait out the gap.
        let due = next_check(Duration::from_secs(600), NOW, 0, Outcome::Never);
        assert_eq!(due, Duration::ZERO);
        assert_eq!(with_floor(due, Some(Duration::from_secs(5))), MIN_GAP - Duration::from_secs(5));
        assert_eq!(with_floor(due, None), Duration::ZERO, "the first start is not held back");
        assert_eq!(with_floor(Duration::from_secs(20 * 3600), Some(Duration::ZERO)), Duration::from_secs(20 * 3600));
        assert_eq!(with_floor(due, Some(2 * MIN_GAP)), Duration::ZERO);
    }

    #[test]
    fn holds() {
        assert_eq!(hold(Busy::default()), None);
        assert!(hold(Busy { fullscreen: true, file_work: false }).unwrap().contains("fullscreen"));
        assert!(hold(Busy { fullscreen: false, file_work: true }).unwrap().contains("copied"));
        assert!(keep_waiting(Duration::from_secs(60)));
        assert!(!keep_waiting(HOLD_MAX));
    }
}
