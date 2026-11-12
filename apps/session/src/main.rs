#![windows_subsystem = "windows"]
//! Shellman's session: the one process Windows starts as the user's shell. It starts the other
//! apps, restarts them when they crash, runs the user's startup items, and ends them in order at
//! logoff. Plain Win32, no egui, nothing that can wait on anything else: the most robust piece.
//!
//! Beside Explorer nothing changes: this is only started by hand (for tests) or, later, by
//! Winlogon after `--install`.

mod apps;
mod log;
mod policy;
mod proc;
mod find;
mod shutdown;
mod single;
mod supervisor;
mod window;

fn main() {
    // Held until exit; a second session in this logon session does nothing.
    let Some(_single) = single::claim() else { return };
    let (apps, _test) = apps::load();
    log::line(&format!("session starts, {} apps", apps.len()));
    let Some(_window) = window::create() else {
        log::line("could not create the session window, stopping");
        return;
    };
    let mut fleet = supervisor::Fleet::new(apps);
    loop {
        fleet.step();
        if !fleet.wait(None) {
            break;
        }
    }
    log::line("session ends");
}
