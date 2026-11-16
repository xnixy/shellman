//! Paying an app's first-run cost before the user starts it.
//!
//! Windows Defender scans a program the first time it runs after it was written (an install, an
//! update): measured on 13 Nov, a never-seen `shellman-files.exe` took 1.2 to 2.3 s to reach its
//! first line, and 0.2 s to its window every time after. The process uses no CPU meanwhile; the
//! wait is before it runs. A run that exits at once pays the same cost, so the session (at
//! sign-in) and setup (after an update) run each app that is started on demand once with
//! [`FLAG`], which every such app answers by returning from `main` first thing.
//!
//! The residents (bar, launcher, ...) need none: the session starts them itself.

use std::path::Path;
use std::time::{Duration, Instant};

/// "Exit at once": the first argument of a warming run.
pub const FLAG: &str = "--warm";

/// The apps started on demand, by exe name without `shellman-` and `.exe`.
pub const APPS: [&str; 4] = ["files", "settings", "tasks", "terminal"];

/// Whether this run is a warming one. Checked first in `main`, before anything else.
pub fn asked() -> bool {
    std::env::args_os().nth(1).is_some_and(|a| a == FLAG)
}

/// Run each of [`APPS`] in `dir` once with [`FLAG`], one after another, and say how long each
/// took (or why not). Blocks for as long as the scans take: a worker thread's job.
pub fn run_all(dir: &Path) -> Vec<(&'static str, Result<Duration, String>)> {
    APPS.iter()
        .map(|name| {
            let exe = dir.join(format!("shellman-{name}.exe"));
            let t0 = Instant::now();
            let ran = std::process::Command::new(&exe)
                .arg(FLAG)
                .status()
                .map(|_| t0.elapsed())
                .map_err(|e| e.to_string());
            (*name, ran)
        })
        .collect()
}
