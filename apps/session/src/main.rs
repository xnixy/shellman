#![windows_subsystem = "windows"]
//! Shellman's session: the one process Windows starts as the user's shell. It starts the other
//! apps, restarts them when they crash, runs the user's startup items, and ends them in order at
//! logoff. Plain Win32, no egui, nothing that can wait on anything else: the most robust piece.
//!
//! Winlogon starts it as the user's shell after `--install`; by hand (for tests) it runs beside
//! Explorer.

mod apps;
mod cli;
mod fallback;
mod find;
mod leftovers;
mod log;
mod policy;
mod proc;
mod restart;
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
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

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
    if !test {
        if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(std::path::Path::to_path_buf)) {
            log::line(&format!("removed {} leftover *.old*.exe files", leftovers::sweep(&dir)));
        }
    }
    // Before any app: glass made while DWM does not allow transparency stays flat.
    transparency::start(test);
    let Some(window) = window::create() else {
        log::line("could not create the session window, stopping");
        return;
    };
    transparency::listen(window);
    let net = Net { shell: fallback::for_run(test), window: window.0 as isize, t0: Instant::now() };
    // Shift at start is the user's way out: Explorer, and no shellman apps at all.
    if safety && fallback::shift_held() {
        run(supervisor::Fleet::new(Vec::new()), Some(net), Some(safety::Reason::Shift));
        return;
    }
    let fleet = supervisor::Fleet::new(apps);
    if startup && !test {
        startup::start(fleet.adopted());
    }
    run(fleet, safety.then_some(net), None);
}

/// The safety net's part of the loop.
struct Net {
    shell: Arc<dyn fallback::Shell + Send + Sync>,
    window: isize,
    t0: Instant,
}

/// Where the safety net stands.
enum State {
    /// Supervising; the net watches the bar.
    Watching,
    /// A fallback runs on its thread (`fallback::spawn`); the fleet is parked meanwhile.
    FallingBack(safety::Reason, Receiver<fallback::Outcome>),
    /// Parked; the watchdog looks for a taskbar at `next`.
    Parked { reason: safety::Reason, next: Instant },
}

impl Net {
    fn fall_back(&self, reason: safety::Reason, fleet: &mut supervisor::Fleet) -> State {
        log::line(&format!("falling back to Explorer: {}", reason.text()));
        fleet.park();
        let (tx, rx) = channel();
        fallback::spawn(self.shell.clone(), self.window, tx);
        State::FallingBack(reason, rx)
    }
}

/// The message loop: supervise until the window closes. `net` is the safety net (None: off).
fn run(mut fleet: supervisor::Fleet, net: Option<Net>, shift: Option<safety::Reason>) {
    let mut state = State::Watching;
    // Whether supervising was already resumed once after a fallback that gave no taskbar.
    let mut resumed = false;
    // One for the whole session, so its bound (5 fallbacks an hour) holds across parks.
    let mut dog = safety::Watchdog::default();
    if let (Some(net), Some(reason)) = (&net, shift) {
        state = net.fall_back(reason, &mut fleet);
    }
    loop {
        fleet.step();
        let mut extra = None;
        if let Some(net) = &net {
            state = match state {
                State::Watching => {
                    let (bar, since) = fleet.bar();
                    let seen = safety::Seen { shift: false, bar, tray_up: fallback::tray_up(), since };
                    match safety::decide(&seen) {
                        Some(reason) => net.fall_back(reason, &mut fleet),
                        None => {
                            extra = safety::wake_in(&seen);
                            State::Watching
                        }
                    }
                }
                State::FallingBack(reason, rx) => match rx.try_recv() {
                    Ok(outcome) => match safety::after_fallback(reason, outcome, resumed) {
                        safety::After::Resume => {
                            log::line("fallback: no taskbar from Explorer; supervising the apps again (once)");
                            resumed = true;
                            fleet.resume();
                            State::Watching
                        }
                        safety::After::Park => {
                            log::line(&format!("fallback: {outcome:?}; parked, the watchdog looks for a taskbar every {} s", safety::WATCH_EVERY.as_secs()));
                            let next = Instant::now() + safety::WATCH_EVERY;
                            State::Parked { reason, next }
                        }
                    },
                    Err(TryRecvError::Empty) => State::FallingBack(reason, rx),
                    Err(TryRecvError::Disconnected) => {
                        log::line("fallback: ERROR its thread ended without an outcome; parked under the watchdog");
                        State::Parked { reason, next: Instant::now() }
                    }
                },
                State::Parked { reason, mut next } => {
                    let now = Instant::now();
                    if now < next || shutdown::quiet() {
                        State::Parked { reason, next }
                    } else {
                        match dog.look(net.shell.any_taskbar(), net.t0.elapsed()) {
                            safety::Look::Retry => {
                                log::line("watchdog: ERROR no taskbar at all (no bar, no Explorer); running the fallback again");
                                net.fall_back(reason, &mut fleet)
                            }
                            safety::Look::Wait(d) => {
                                next = now + d;
                                State::Parked { reason, next }
                            }
                            safety::Look::Spent(d) => {
                                log::line(&format!("watchdog: ERROR still no taskbar and {} fallbacks this hour; next try in {} s. By hand: Ctrl+Alt+Del, Task Manager, Run new task, explorer.exe", safety::RETRIES, d.as_secs()));
                                next = now + d;
                                State::Parked { reason, next }
                            }
                        }
                    }
                }
            };
            // Parked, the fleet waits on nothing: the watchdog's next look must wake the loop.
            // (While the session is being asked to end it is not due; a cancel is a message.)
            if let State::Parked { next, .. } = &state {
                let left = next.saturating_duration_since(Instant::now());
                extra = Some(if left.is_zero() { safety::WATCH_EVERY } else { left + Duration::from_millis(50) });
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
