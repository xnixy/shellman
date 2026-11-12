//! The supervisor's loop: look, decide (`decide`), act, then sleep in one kernel wait until the
//! host ends or a setting changes. Its thread lives as long as the process.

use super::decide::{self, Explorer, Failures, Owner, Seen, Step};
use super::win::{self, Handle, SettingsWatch, Wait, HOST_EXE};
use std::time::{Duration, Instant};

/// How long the host may take to own the shell window (measured: 0.2 to 6 s).
const TAKE_OVER: Duration = Duration::from_secs(20);
/// After the host ended, how long Winlogon's Explorer may take to appear (measured: under 0.2 s).
const RESPAWN: Duration = Duration::from_secs(8);
/// An Explorer counts as Winlogon's replacement when it started this soon after the host ended
/// (in 100 ns units).
const RESPAWN_WINDOW: u64 = 30 * 10_000_000;
/// How long a cleanly asked host may take to go (measured: 0.2 to 8 s).
const QUIT: Duration = Duration::from_secs(12);

struct State {
    path: Option<std::path::PathBuf>,
    watch: Option<SettingsWatch>,
    begun: Instant,
    failures: Failures,
    /// When the host last ended, on `win::now`'s clock, while Winlogon's answer is awaited.
    death: Option<u64>,
    stopped_cleanly: bool,
    enabled: bool,
    /// The last line logged, so waking up again to the same state logs nothing.
    last: String,
}

impl State {
    fn say(&mut self, text: String) {
        if text != self.last {
            super::log::line(&text);
            self.last = text;
        }
    }
}

pub fn run() {
    let mut st = State {
        path: win::host_path(),
        watch: crate::config::config_dir().and_then(|d| SettingsWatch::new(&d)),
        begun: Instant::now(),
        failures: Failures::default(),
        death: None,
        stopped_cleanly: false,
        enabled: super::enabled(),
        last: String::new(),
    };
    let up = match &st.path {
        Some(p) => format!("supervisor up; host {}", p.display()),
        None => "supervisor up; ShellAppRuntime.exe is not on this system, nothing to run".into(),
    };
    st.say(up);
    loop {
        let seen = observe(&st);
        match decide::next(&seen, &st.failures, st.begun.elapsed()) {
            Step::Watch(pid) => watch(&mut st, pid, seen.owner),
            Step::Start(pause) => start(&mut st, pause),
            Step::StopExplorer(pid) => {
                let gone = win::terminate(pid);
                st.say(format!("stopped Winlogon's explorer.exe {pid}: {gone}"));
                // Apps moved their tray icons to that Explorer; send them back to the bar.
                win::announce_taskbar();
                // Any Explorer seen from now on is someone's choice, not Winlogon's reflex.
                st.death = None;
                win::restore_taskman();
            }
            Step::StopHost(pid) => {
                let asked = win::quit_shell(pid);
                let gone = asked && Handle::watch(pid).is_none_or(|h| h.wait(QUIT) == Wait::Process);
                st.say(format!("setting off: asked host {pid} to quit: {asked}, gone: {gone}"));
                st.stopped_cleanly = true;
                win::restore_taskman();
                rest(&mut st);
            }
            Step::Idle(why) => {
                st.say(format!("idle: {why:?}"));
                rest(&mut st);
            }
        }
    }
}

/// Look at the session.
fn observe(st: &State) -> Seen {
    let procs = win::session_processes();
    let host = procs.iter().find(|p| p.name == HOST_EXE).map(|p| p.pid);
    let name_of = |pid: u32| procs.iter().find(|p| p.pid == pid).map(|p| p.name.as_str());
    let owner = match win::shell_window_pid() {
        None => Owner::Nobody,
        Some(pid) => match name_of(pid) {
            Some(HOST_EXE) => Owner::Host,
            Some("explorer.exe") => Owner::Explorer,
            _ => Owner::Other,
        },
    };
    let mut explorer = Explorer::None;
    for p in procs.iter().filter(|p| p.name == "explorer.exe") {
        // Winlogon's: started just after the host ended, by userinit (which has usually exited
        // by now). Anything else is left alone.
        let by_userinit = matches!(name_of(p.parent), None | Some("userinit.exe"));
        let after_death = st.death.zip(win::started(p.pid)).is_some_and(|(death, started)| {
            started + 10_000_000 >= death && started <= death + RESPAWN_WINDOW
        });
        if by_userinit && after_death {
            explorer = Explorer::Respawned(p.pid);
            break;
        }
        explorer = Explorer::Running;
    }
    Seen {
        owner,
        host,
        explorer,
        installed: st.path.is_some(),
        enabled: st.enabled,
        stopped_cleanly: st.stopped_cleanly,
    }
}

/// Wait on a running host until it ends or a setting changes.
fn watch(st: &mut State, pid: u32, owner: Owner) {
    let Some(host) = Handle::watch(pid) else {
        st.say(format!("cannot open host {pid} to watch it"));
        return rest(st);
    };
    st.say(format!("watching host {pid} (owns the shell window: {})", owner == Owner::Host));
    match win::wait_any(Some(&host), st.watch.as_ref(), None) {
        Wait::Process => ended(st, pid, host.exit_code()),
        Wait::Settings => {
            settings_changed(st);
            // It may have ended meanwhile; that must still be seen.
            if host.wait(Duration::ZERO) == Wait::Process {
                ended(st, pid, host.exit_code());
            }
        }
        // Only when the wait itself failed; never spin on that.
        Wait::Timeout => std::thread::sleep(Duration::from_secs(5)),
    }
}

/// The host ended. Its exit code does not tell a crash from a clean stop (both were 1 here), but
/// Winlogon does: after a crash or a kill it starts Explorer as the shell within a moment, after
/// a clean stop (WM_QUIT, someone's choice) it does nothing. So: a replacement Explorer means a
/// failure, stopped and followed by a new host; none means the stop was meant, and it is left so.
fn ended(st: &mut State, pid: u32, code: Option<u32>) {
    st.say(format!("host {pid} ended (exit code {code:?})"));
    st.death = Some(win::now());
    // Bounded: a few looks over a few seconds, not a polling loop.
    let until = Instant::now() + RESPAWN;
    while Instant::now() < until {
        if matches!(observe(st).explorer, Explorer::Respawned(_)) {
            st.failures.add(st.begun.elapsed());
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    st.death = None;
    st.stopped_cleanly = true;
    win::restore_taskman();
    st.say("no Explorer replaced it, so it was asked to stop: leaving it stopped".into());
}

/// Start the host and wait for it to own the shell window, or to exit (it does at once when a
/// shell window or another copy already exists).
fn start(st: &mut State, pause: Duration) {
    if !pause.is_zero() {
        st.say(format!("starting the host again in {pause:?}"));
        if win::wait_any(None, st.watch.as_ref(), Some(pause)) == Wait::Settings {
            return settings_changed(st);
        }
    }
    let Some(path) = st.path.clone() else { return };
    let begun = Instant::now();
    let Some((pid, host)) = win::start_host(&path) else {
        st.say("could not start the host".into());
        return st.failures.add(st.begun.elapsed());
    };
    loop {
        if win::shell_window_pid() == Some(pid) {
            return st.say(format!("host {pid} owns the shell window after {} ms", begun.elapsed().as_millis()));
        }
        if host.wait(Duration::from_millis(200)) == Wait::Process {
            st.say(format!("host {pid} exited at start (exit code {:?})", host.exit_code()));
            return st.failures.add(st.begun.elapsed());
        }
        if begun.elapsed() > TAKE_OVER {
            // Still running: watched like any host; if it never works, modern apps stay off.
            return st.say(format!("host {pid} has not taken the shell window after {TAKE_OVER:?}"));
        }
    }
}

/// Sleep until a setting changes (forever, without a settings folder).
fn rest(st: &mut State) {
    match win::wait_any(None, st.watch.as_ref(), None) {
        Wait::Settings => settings_changed(st),
        // The wait itself failed; never spin on that.
        _ => std::thread::sleep(Duration::from_secs(5)),
    }
}

/// Any settings file was written; only the shell setting matters here. Turning it back on is a
/// deliberate retry: past failures and a clean stop are forgotten.
fn settings_changed(st: &mut State) {
    if let Some(w) = &st.watch {
        w.rearm();
    }
    // A writer renames its file into place; let it finish.
    std::thread::sleep(Duration::from_millis(100));
    let enabled = super::enabled();
    if enabled != st.enabled {
        st.say(format!("modern apps setting: {}", if enabled { "on" } else { "off" }));
        if enabled {
            st.failures.clear();
            st.stopped_cleanly = false;
        }
        st.enabled = enabled;
    }
}
