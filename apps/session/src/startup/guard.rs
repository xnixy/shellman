//! Startup items run once per logon, not once per session process. A named kernel object held by
//! the session would die with it, which is exactly when a restarted session must not run them
//! again. So the marker is a volatile registry key under HKCU: it survives the session process
//! and is removed by Windows when the user logs off. (Creating a key and learning whether it
//! was new is one atomic call, so two sessions cannot both win.) A session that adopted running
//! apps was restarted by definition and skips them too.
//!
//! Windows' own shells keep the same kind of marker, and shellman must hold theirs as well:
//! Explorer and ShellAppRuntime (the modern-app host) run the startup items, packaged startup
//! tasks included, only when they are the ones to create the volatile key
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\SessionInfo\<session id>\
//! RunStuffHasBeenRun` (read from ShellAppRuntime.exe 10.0.26100.9818: it creates the key with
//! REG_OPTION_VOLATILE and runs the Run keys, the startup tasks and the Startup folders only on
//! REG_CREATED_NEW_KEY; HKCU RunOnce it processes every time). Without it, turning on the
//! modern-app setting mid-session started everything a second time, and ShellAppRuntime does not
//! apply StartupApproved there, so items the user had switched off (Docker Desktop) came up too.
//! So shellman creates that key when it takes the run; if it already exists on a fresh logon,
//! Explorer or the host has the run and shellman stays out of it.

use crate::registry::{Hive, Registry};

pub const KEY: &str = "Software\\shellman\\session-startup";

const WINDOWS_LEAF: &str = "RunStuffHasBeenRun";

fn session_info(session: u32) -> String {
    format!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\SessionInfo\\{session}")
}

/// Windows' marker for this logon session.
pub fn windows_key(session: u32) -> String {
    format!("{}\\{WINDOWS_LEAF}", session_info(session))
}

/// Whether Windows' marker is there: Explorer or the host would not run the items again.
pub fn windows_marker(reg: &dyn Registry, session: u32) -> bool {
    reg.subkeys(Hive::Hkcu, &session_info(session)).iter().any(|k| k.eq_ignore_ascii_case(WINDOWS_LEAF))
}

#[derive(PartialEq, Eq, Debug)]
pub enum Verdict {
    Run,
    AlreadyRan,
    /// Apps were adopted: this is a restarted session.
    Restarted,
    /// Explorer or ShellAppRuntime had already claimed this logon's run.
    WindowsRan,
    /// The marker could not be made; better to run nothing than to run twice.
    Unknown(String),
}

/// Claim this logon's startup run. Only the first caller gets `Run`. Windows' marker is claimed
/// in every case but `Unknown`, so a host started later in this logon does not run them again.
pub fn claim(reg: &dyn Registry, adopted: usize, session: u32) -> Verdict {
    let ours = reg.create_volatile(Hive::Hkcu, KEY);
    let windows = match &ours {
        Err(_) => None,
        Ok(_) => Some(reg.create_volatile(Hive::Hkcu, &windows_key(session))),
    };
    match (ours, windows) {
        (Err(e), _) => Verdict::Unknown(e),
        (Ok(true), _) if adopted > 0 => Verdict::Restarted,
        (Ok(true), Some(Ok(false))) => Verdict::WindowsRan,
        // Windows' marker could not be made: run anyway, as before it was known (logged).
        (Ok(true), _) => Verdict::Run,
        (Ok(false), _) => Verdict::AlreadyRan,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::fake::Fake;

    #[test]
    fn only_the_first_claim_runs() {
        let reg = Fake::default();
        assert_eq!(claim(&reg, 0, 1), Verdict::Run);
        // The same logon, a session that came back after a crash.
        assert_eq!(claim(&reg, 0, 1), Verdict::AlreadyRan);
    }

    #[test]
    fn adopting_apps_means_a_restart() {
        // A restarted session whose marker is somehow gone still does not run them again.
        assert_eq!(claim(&Fake::default(), 2, 1), Verdict::Restarted);
    }

    #[test]
    fn windows_marker_is_taken_so_the_host_does_not_run_them_again() {
        let reg = Fake::default();
        assert_eq!(claim(&reg, 0, 3), Verdict::Run);
        // ShellAppRuntime, started later in the logon, finds the key and skips its run.
        assert_eq!(reg.create_volatile(Hive::Hkcu, &windows_key(3)), Ok(false));
    }

    #[test]
    fn a_shell_that_ran_first_keeps_the_run() {
        let reg = Fake::default();
        // The host (or Explorer) came up before the session's startup thread.
        assert_eq!(reg.create_volatile(Hive::Hkcu, &windows_key(1)), Ok(true));
        assert_eq!(claim(&reg, 0, 1), Verdict::WindowsRan);
        assert_eq!(claim(&reg, 0, 1), Verdict::AlreadyRan);
    }

    #[test]
    fn the_key_is_per_logon_session() {
        assert_eq!(
            windows_key(1),
            "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\SessionInfo\\1\\RunStuffHasBeenRun"
        );
    }
}
