//! The user's startup items, which Explorer normally runs at logon and nobody does without it:
//! the Run and RunOnce keys, packaged apps' startup tasks, and the Startup folders, honouring
//! what Task Manager and Settings switched off (`approved`, and each task's `State`).
//!
//! shellman owns this run for the whole logon, also when the modern-app host (ShellAppRuntime)
//! is started later: `guard` takes Windows' own once-per-logon marker so the host does not run
//! everything a second time (see there).

mod dry;
mod guard;
mod runner;
mod win;

// The reading of the items is shared with Settings' startup page.
use shellman_core::startup::{items, packaged_win, win as files};

pub use dry::dry_run;

use crate::registry::{Real, Registry};
use crate::{log, shutdown};
use std::time::Duration;

/// Every item, in the order they start. Needs COM on the calling thread (the packages).
fn collect(reg: &dyn Registry) -> Vec<items::Item> {
    let mut list = items::collect(reg, &files::startup_files(), &files::expand, win::is_elevated());
    items::insert_packaged(&mut list, &packaged_win::list(reg));
    list
}

/// Run the startup items once for this logon, on a thread of its own. `adopted` is how many
/// running apps the session found at its start.
pub fn start(adopted: usize) {
    let _ = std::thread::Builder::new().name("startup items".into()).spawn(move || {
        win::init_com();
        let session = win::session_id();
        match guard::claim(&Real, adopted, session) {
            guard::Verdict::Run => {}
            other => {
                log::line(&format!("startup items not run: {other:?}"));
                return;
            }
        }
        if !guard::windows_marker(&Real, session) {
            log::line("startup items: could not set Windows' RunStuffHasBeenRun marker; ShellAppRuntime, if started later, will run them again");
        }
        let items = collect(&Real);
        log::line(&format!("startup items: {} found, {} enabled", items.len(), items.iter().filter(|i| i.enabled).count()));
        let wait = |d: Duration| shutdown::sleep(d);
        let n = runner::run(&items, &Real, &win::Real, &wait, &log::line);
        log::line(&format!("startup items: {n} started"));
    });
}
