#![windows_subsystem = "windows"]
//! Shellman's session: the one process Windows starts as the user's shell. It starts the other
//! apps, restarts them when they crash, runs the user's startup items, and ends them in order at
//! logoff. Plain Win32, no egui, nothing that can wait on anything else: the most robust piece.
//!
//! Beside Explorer nothing changes: this is only started by hand (for tests) or, later, by
//! Winlogon after `--install`.

mod apps;
mod cli;
mod fallback;
mod find;
mod log;
mod policy;
mod proc;
mod registry;
mod safety;
mod shutdown;
mod single;
mod startup;
mod supervisor;
mod swap;
mod transparency;
mod window;

use cli::Mode;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let startup = match cli::parse(&args) {
        Mode::Run { startup } => startup,
        Mode::DryRun => return print_mode(startup::dry_run),
        Mode::Install => return print_mode(install),
        Mode::Uninstall => return print_mode(uninstall),
        Mode::Help => return print_mode(cli::help),
    };
    // Held until exit; a second session in this logon session does nothing.
    let Some(_single) = single::claim() else { return };
    let (apps, test) = apps::load();
    // The safety net is for the real shell. A test run may switch it on to try it.
    let safety = !test || std::env::var("SHELLMAN_SESSION_SAFETY").is_ok_and(|v| v == "on");
    log::line(&format!("session starts, {} apps{}", apps.len(), if test { " (test list)" } else { "" }));
    // Before any app: glass made while DWM does not allow transparency stays flat.
    transparency::start(test);
    let Some(_window) = window::create() else {
        log::line("could not create the session window, stopping");
        return;
    };
    // Shift at start is the user's way out: Explorer, and no shellman apps at all.
    if safety && fallback::shift_held() {
        log::line(&format!("falling back to Explorer: {}", safety::Reason::Shift.text()));
        fallback::start_explorer(test);
        run(supervisor::Fleet::new(Vec::new()), false, test, true);
        return;
    }
    let fleet = supervisor::Fleet::new(apps);
    if startup && !test {
        startup::start(fleet.adopted());
    }
    run(fleet, safety, test, false);
}

/// The message loop: supervise until the window closes.
fn run(mut fleet: supervisor::Fleet, safety: bool, test: bool, park: bool) {
    if park {
        fleet.park();
    }
    // The last fallback, when it was, and whether supervising was already resumed once.
    let mut fell_back: Option<(safety::Reason, std::time::Instant)> = None;
    let mut resumed = false;
    loop {
        fleet.step();
        let mut extra = None;
        if safety && !fleet.parked() {
            let (bar, since) = fleet.bar();
            let seen = safety::Seen { shift: false, bar, tray_up: fallback::tray_up(), since };
            if let Some(reason) = safety::decide(&seen) {
                log::line(&format!("falling back to Explorer: {}", reason.text()));
                fallback::start_explorer(test);
                fleet.park();
                fell_back = Some((reason, std::time::Instant::now()));
            } else {
                extra = safety::wake_in(&seen);
            }
        }
        if let Some((reason, at)) = fell_back.filter(|_| fleet.parked() && !resumed) {
            let explorer = shellman_core::app_host::explorer_is_shell();
            if safety::resume(reason, explorer, at.elapsed(), resumed) {
                log::line("fallback: Explorer did not become the shell; supervising the apps again (once)");
                resumed = true;
                fleet.resume();
            } else if !explorer {
                extra = Some(safety::EXPLORER_GRACE.saturating_sub(at.elapsed()) + Duration::from_millis(50));
            }
        }
        if !fleet.wait(extra) {
            break;
        }
    }
    shutdown::stop();
    log::line("session ends");
}

fn print_mode(f: impl FnOnce()) {
    cli::attach_console();
    f();
}

fn install() {
    let exe = std::env::current_exe().unwrap_or_default();
    match swap::install(&registry::Real, &exe, &|p| p.exists()) {
        Ok(done) => println!("installed: {done:?}. Takes effect at the next sign-in. Undo: shellman-session --uninstall"),
        Err(e) => println!("not installed: {e}"),
    }
}

fn uninstall() {
    match swap::uninstall(&registry::Real) {
        Ok(done) => println!("uninstalled: {done:?}. Explorer is the shell again at the next sign-in."),
        Err(e) => println!("nothing changed: {e}"),
    }
}
