//! Starts `shellman-update --auto` on schedule: about 5 minutes after sign-in, then daily
//! (`shellman_update_core::schedule::next_check`, from when `update.json` says the last check
//! was). Everything else (the feed, the signature, waiting out fullscreen apps and file copies,
//! the quiet install) is the helper's, in its own process; this is a thread the session never
//! waits on, so a slow network or a hung download cannot touch the shell.
//!
//! Only for a real install: setup's record (`shellman-install.txt`) and `shellman-update.exe` must
//! be beside this exe, so a build folder or a test session never updates itself. Nothing is
//! started while the session is ending or has handed over to Explorer, or while Settings >
//! About > "Update automatically" is off (looked at again every hour).

use crate::log;
use shellman_update_core::schedule;
use shellman_update_core::state::State;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// What to do when a check is due.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Due {
    Start,
    /// Off in Settings: look again in an hour.
    Off,
    /// Ending, or parked behind Explorer: look again soon.
    NotNow,
}

pub fn decide(enabled: bool, quiet: bool, parked: bool) -> Due {
    if quiet || parked {
        Due::NotNow
    } else if !enabled {
        Due::Off
    } else {
        Due::Start
    }
}

const NOT_NOW: Duration = Duration::from_secs(10 * 60);

pub fn start(t0: Instant) {
    let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) else { return };
    let helper = dir.join("shellman-update.exe");
    if !dir.join("shellman-install.txt").is_file() || !helper.is_file() {
        log::line("auto-update: not an installed copy (no shellman-install.txt or shellman-update.exe here); never checks");
        return;
    }
    let _ = std::thread::Builder::new().name("autoupdate".into()).spawn(move || run(t0, dir, helper));
}

fn run(t0: Instant, dir: PathBuf, helper: PathBuf) {
    let mut said_off = false;
    let mut last_start: Option<Instant> = None;
    loop {
        let st = State::load();
        let due = schedule::next_check(t0.elapsed(), shellman_update_core::state::now(), st.last_check, st.outcome);
        std::thread::sleep(schedule::with_floor(due, last_start.map(|t| t.elapsed())));
        let enabled = shellman_core::settings::shell::ShellSettings::load().auto_update;
        match decide(enabled, crate::shutdown::quiet(), crate::restart::parked()) {
            Due::NotNow => std::thread::sleep(NOT_NOW),
            Due::Off => {
                if !said_off {
                    log::line("auto-update: off in Settings; not checking");
                    said_off = true;
                }
                std::thread::sleep(schedule::OFF_LOOK);
            }
            Due::Start => {
                said_off = false;
                last_start = Some(Instant::now());
                let p = dir.display().to_string();
                match std::process::Command::new(&helper).args(["--auto", "--prefix", p.trim_end_matches('\\')]).spawn() {
                    // The helper logs its own steps; a crash shows as `Working` left behind,
                    // which the schedule treats as a failure (retried in 3 h).
                    Ok(mut child) => {
                        let code = child.wait().ok().and_then(|s| s.code());
                        log::line(&format!("auto-update: check ended ({})", code.map_or("no exit code".into(), |c| c.to_string())));
                    }
                    Err(e) => {
                        log::line(&format!("auto-update: ERROR could not start {}: {e}", helper.display()));
                        std::thread::sleep(schedule::RETRY_FAILED);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_live_session_with_updating_on_starts_a_check() {
        assert_eq!(decide(true, false, false), Due::Start);
        assert_eq!(decide(false, false, false), Due::Off);
        assert_eq!(decide(true, true, false), Due::NotNow, "the session is ending");
        assert_eq!(decide(true, false, true), Due::NotNow, "handed over to Explorer");
    }
}
