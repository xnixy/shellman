//! What the supervisor does next, from what it sees. No Win32 here, so every rule is tested.

use std::time::Duration;

/// Who owns the shell window (`GetShellWindow`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Owner {
    Nobody,
    /// ShellAppRuntime.
    Host,
    Explorer,
    /// Anything else: some other shell. Never fought.
    Other,
}

/// Whether an `explorer.exe` runs in this session, and whose it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Explorer {
    None,
    /// Running, and not one Winlogon started because the host died: Explorer is the shell (or
    /// the user started it), and shellman runs beside it.
    Running,
    /// Started by Winlogon right after the host died: this one pid is stopped.
    Respawned(u32),
}

/// One look at the session.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Seen {
    pub owner: Owner,
    /// A ShellAppRuntime running in this session.
    pub host: Option<u32>,
    pub explorer: Explorer,
    /// `ShellAppRuntime.exe` is in System32 (it is undocumented, so other builds may lack it).
    pub installed: bool,
    /// The user's setting (and the environment override).
    pub enabled: bool,
    /// The host last exited cleanly (someone asked it to): it is left stopped.
    pub stopped_cleanly: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    /// A host runs: watch it.
    Watch(u32),
    /// Start the host, after this pause.
    Start(Duration),
    /// Stop Winlogon's replacement Explorer, so the host can be the shell again.
    StopExplorer(u32),
    /// Ask the host to quit (the setting was turned off).
    StopHost(u32),
    /// Nothing to do until the setting changes.
    Idle(Why),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Why {
    Off,
    ExplorerIsShell,
    OtherShell,
    NotInstalled,
    StoppedCleanly,
    TooManyFailures,
}

/// Failures (the host exiting with an error, or not starting) within `WINDOW` count; after
/// `LIMIT` of them the supervisor gives up instead of looping.
pub const LIMIT: usize = 3;
pub const WINDOW: Duration = Duration::from_secs(10 * 60);

/// When the host failed, as time since the supervisor started.
#[derive(Default, Debug)]
pub struct Failures(Vec<Duration>);

impl Failures {
    pub fn add(&mut self, at: Duration) {
        self.0.push(at);
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    fn recent(&self, now: Duration) -> usize {
        self.0.iter().filter(|&&t| now.saturating_sub(t) < WINDOW).count()
    }

    /// No pause the first time; then 2, 4, 8 s: a host that dies at once is not hammered.
    fn pause(&self, now: Duration) -> Duration {
        match self.recent(now) {
            0 => Duration::ZERO,
            n => Duration::from_secs(1 << n.min(4)),
        }
    }
}

pub fn next(seen: &Seen, failures: &Failures, now: Duration) -> Step {
    if !seen.enabled {
        return match seen.host {
            Some(pid) if seen.owner == Owner::Host => Step::StopHost(pid),
            _ => Step::Idle(Why::Off),
        };
    }
    // Before anything else: Winlogon's Explorer would otherwise keep the shell window.
    if let Explorer::Respawned(pid) = seen.explorer {
        return Step::StopExplorer(pid);
    }
    if let Some(pid) = seen.host {
        return Step::Watch(pid);
    }
    if seen.explorer == Explorer::Running || seen.owner == Owner::Explorer {
        return Step::Idle(Why::ExplorerIsShell);
    }
    if seen.owner != Owner::Nobody {
        return Step::Idle(Why::OtherShell);
    }
    if !seen.installed {
        return Step::Idle(Why::NotInstalled);
    }
    if seen.stopped_cleanly {
        return Step::Idle(Why::StoppedCleanly);
    }
    if failures.recent(now) >= LIMIT {
        return Step::Idle(Why::TooManyFailures);
    }
    Step::Start(failures.pause(now))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen() -> Seen {
        Seen {
            owner: Owner::Nobody,
            host: None,
            explorer: Explorer::None,
            installed: true,
            enabled: true,
            stopped_cleanly: false,
        }
    }

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn starts_when_nobody_is_the_shell() {
        assert_eq!(next(&seen(), &Failures::default(), secs(0)), Step::Start(Duration::ZERO));
    }

    #[test]
    fn watches_a_running_host() {
        let s = Seen { owner: Owner::Host, host: Some(7), ..seen() };
        assert_eq!(next(&s, &Failures::default(), secs(0)), Step::Watch(7));
        // Still starting up: it does not own the shell window yet, but it runs.
        let s = Seen { host: Some(7), ..seen() };
        assert_eq!(next(&s, &Failures::default(), secs(0)), Step::Watch(7));
    }

    #[test]
    fn leaves_explorer_and_other_shells_alone() {
        let f = Failures::default();
        let s = Seen { owner: Owner::Explorer, explorer: Explorer::Running, ..seen() };
        assert_eq!(next(&s, &f, secs(0)), Step::Idle(Why::ExplorerIsShell));
        // Explorer running but without its shell window yet (signing in): still its turn.
        let s = Seen { explorer: Explorer::Running, ..seen() };
        assert_eq!(next(&s, &f, secs(0)), Step::Idle(Why::ExplorerIsShell));
        let s = Seen { owner: Owner::Other, ..seen() };
        assert_eq!(next(&s, &f, secs(0)), Step::Idle(Why::OtherShell));
    }

    #[test]
    fn stops_only_winlogons_explorer_then_starts_again() {
        let f = Failures::default();
        let s = Seen { owner: Owner::Explorer, explorer: Explorer::Respawned(42), ..seen() };
        assert_eq!(next(&s, &f, secs(0)), Step::StopExplorer(42));
        // Once it is gone the shell window is free again.
        assert_eq!(next(&seen(), &f, secs(0)), Step::Start(Duration::ZERO));
    }

    #[test]
    fn missing_host_does_nothing() {
        let s = Seen { installed: false, ..seen() };
        assert_eq!(next(&s, &Failures::default(), secs(0)), Step::Idle(Why::NotInstalled));
    }

    #[test]
    fn backs_off_then_gives_up() {
        let mut f = Failures::default();
        f.add(secs(10));
        assert_eq!(next(&seen(), &f, secs(11)), Step::Start(secs(2)));
        f.add(secs(20));
        assert_eq!(next(&seen(), &f, secs(21)), Step::Start(secs(4)));
        f.add(secs(30));
        assert_eq!(next(&seen(), &f, secs(31)), Step::Idle(Why::TooManyFailures));
        // Old failures stop counting.
        assert_eq!(next(&seen(), &f, secs(30) + WINDOW), Step::Start(Duration::ZERO));
    }

    #[test]
    fn setting_off_stops_the_host_and_rests() {
        let f = Failures::default();
        let s = Seen { enabled: false, owner: Owner::Host, host: Some(9), ..seen() };
        assert_eq!(next(&s, &f, secs(0)), Step::StopHost(9));
        let s = Seen { enabled: false, ..seen() };
        assert_eq!(next(&s, &f, secs(0)), Step::Idle(Why::Off));
        // Off never touches Explorer, even one Winlogon started.
        let s = Seen { enabled: false, explorer: Explorer::Respawned(3), ..seen() };
        assert_eq!(next(&s, &f, secs(0)), Step::Idle(Why::Off));
    }

    #[test]
    fn a_clean_exit_is_respected() {
        let s = Seen { stopped_cleanly: true, ..seen() };
        assert_eq!(next(&s, &Failures::default(), secs(0)), Step::Idle(Why::StoppedCleanly));
    }
}
