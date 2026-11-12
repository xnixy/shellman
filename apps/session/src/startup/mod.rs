//! The user's startup items, which Explorer normally runs at logon and nobody does without it:
//! the Run and RunOnce keys, the Startup folders, honouring what Task Manager and Settings
//! switched off (`approved`).
//!
//! Packaged (Store) apps' startup tasks are left out. Their on/off state sits under
//! `HKCU\Software\Classes\Local Settings\...\AppModel\SystemAppData\<package>\<task id>` (State), but
//! what starts them is the immersive shell, there is no public call to run a package's startup
//! task, and starting them ourselves would mean reading each package's manifest. Windows'
//! ShellAppRuntime, which the desktop already runs for modern apps, is the one that does logon
//! tasks (see PLAN.md), so they may come up without any help from here.

mod approved;
mod dry;
mod guard;
mod items;
mod runner;
mod win;

pub use dry::dry_run;

use crate::registry::{Real, Registry};
use crate::{log, shutdown};
use std::time::Duration;

fn collect(reg: &dyn Registry) -> Vec<items::Item> {
    items::collect(reg, &win::startup_files(), &win::expand, win::is_elevated())
}

/// Run the startup items once for this logon, on a thread of its own. `adopted` is how many
/// running apps the session found at its start.
pub fn start(adopted: usize) {
    let _ = std::thread::Builder::new().name("startup items".into()).spawn(move || {
        match guard::claim(&Real, adopted) {
            guard::Verdict::Run => {}
            other => {
                log::line(&format!("startup items not run: {other:?}"));
                return;
            }
        }
        let items = collect(&Real);
        log::line(&format!("startup items: {} found, {} enabled", items.len(), items.iter().filter(|i| i.enabled).count()));
        let wait = |d: Duration| shutdown::sleep(d);
        let n = runner::run(&items, &Real, &win::Real, &wait, &log::line);
        log::line(&format!("startup items: {n} started"));
    });
}
