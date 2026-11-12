//! May this process be ended, and how carefully: the rule behind the End task button.
//!
//! Critical processes keep Windows (or shellman's session) running; ending one crashes or signs
//! out the session, so the button is off for them. Protected ones are parts of Windows or of
//! shellman whose loss is felt at once (no sound, no top bar); they can be ended, after a
//! stronger confirm. Processes this unelevated program cannot open are not ended at all, and the
//! reason is shown.
//!
//! Elevation seam: ending an elevated process would go through `shellman-elevate.exe` (see
//! `crate::elevate`, which has a fixed list of operations and asks UAC each time). There is no
//! "end process" operation there yet; adding one is the route, and the window says so instead of
//! elevating by itself.

use super::{Access, Proc};

/// What the End task button may do for a process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// End it, after the usual inline confirm (background) or none (a window's close).
    Allowed,
    /// End it only after a confirm that says what will break.
    Protected(&'static str),
    /// Never: it keeps the session alive.
    Critical(&'static str),
    /// This program is not allowed to: it would need administrator rights.
    Denied(&'static str),
}

impl Verdict {
    pub fn can_end(&self) -> bool {
        matches!(self, Verdict::Allowed | Verdict::Protected(_))
    }

    /// Why the button is off or careful; None when nothing needs saying.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            Verdict::Allowed => None,
            Verdict::Protected(r) | Verdict::Critical(r) | Verdict::Denied(r) => Some(r),
        }
    }
}

/// Windows' own processes that end the session (or crash the machine) when they end.
const CRITICAL: [&str; 12] = [
    "system",
    "registry",
    "memory compression",
    "secure system",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "services.exe",
    "lsass.exe",
    "lsaiso.exe",
    "winlogon.exe",
    "dwm.exe",
];

/// Parts of Windows and of shellman that can be ended, but are missed at once.
const PROTECTED: [&str; 12] = [
    "svchost.exe",
    "sihost.exe",
    "ctfmon.exe",
    "fontdrvhost.exe",
    "audiodg.exe",
    "spoolsv.exe",
    "taskhostw.exe",
    "explorer.exe",
    "shellman-bar.exe",
    "shellman-desktop.exe",
    "shellman-switcher.exe",
    "shellman-launcher.exe",
];

/// shellman's session supervisor: ending it ends the shell, like winlogon's shell.
const SUPERVISOR: &str = "shellman-session.exe";

/// The verdict for `proc`. `own_pid` is this program's own PID: it may close itself.
pub fn may_end(proc: &Proc, own_pid: u32) -> Verdict {
    let name = proc.name.to_ascii_lowercase();
    if proc.key.pid <= 4 || CRITICAL.contains(&name.as_str()) {
        return Verdict::Critical("Windows needs this process; ending it would end the session.");
    }
    if name == SUPERVISOR {
        return Verdict::Critical("shellman's session runs the shell; ending it would sign you out.");
    }
    if proc.key.pid == own_pid {
        return Verdict::Allowed;
    }
    match proc.access {
        Access::Full => {}
        Access::ReadOnly | Access::None => {
            return Verdict::Denied("It runs with rights this window does not have (administrator or another user).");
        }
    }
    if name.starts_with("shellman-") && PROTECTED.contains(&name.as_str()) {
        return Verdict::Protected("Part of shellman: it goes away until the session starts it again.");
    }
    if PROTECTED.contains(&name.as_str()) {
        return Verdict::Protected("Part of Windows: things may stop working until you sign in again.");
    }
    Verdict::Allowed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procs::Key;

    fn proc(pid: u32, name: &str, access: Access) -> Proc {
        Proc { key: Key { pid, created: 1 }, name: name.into(), access, ..Default::default() }
    }

    #[test]
    fn critical_processes_are_never_ended() {
        for name in ["csrss.exe", "WinLogon.exe", "lsass.exe", "dwm.exe", "services.exe", "Registry"] {
            let v = may_end(&proc(900, name, Access::Full), 1);
            assert!(matches!(v, Verdict::Critical(_)), "{name}");
            assert!(!v.can_end());
        }
        assert!(matches!(may_end(&proc(4, "System", Access::None), 1), Verdict::Critical(_)));
        assert!(matches!(may_end(&proc(0, "", Access::None), 1), Verdict::Critical(_)));
        assert!(matches!(may_end(&proc(700, "shellman-session.exe", Access::Full), 1), Verdict::Critical(_)));
    }

    #[test]
    fn protected_ones_need_a_stronger_confirm() {
        let v = may_end(&proc(800, "svchost.exe", Access::Full), 1);
        assert!(matches!(v, Verdict::Protected(_)) && v.can_end() && v.reason().is_some());
        let bar = may_end(&proc(801, "shellman-bar.exe", Access::Full), 1);
        assert!(bar.reason().unwrap().contains("shellman"));
    }

    #[test]
    fn what_cannot_be_opened_is_denied_with_a_reason() {
        let v = may_end(&proc(1200, "elevated.exe", Access::ReadOnly), 1);
        assert!(matches!(v, Verdict::Denied(_)) && !v.can_end());
        // Critical wins over denied: the reason given is the stronger one.
        assert!(matches!(may_end(&proc(1300, "lsass.exe", Access::None), 1), Verdict::Critical(_)));
    }

    #[test]
    fn ordinary_apps_and_itself_are_allowed() {
        assert_eq!(may_end(&proc(5000, "notepad.exe", Access::Full), 1), Verdict::Allowed);
        assert_eq!(may_end(&proc(5000, "shellman-tasks.exe", Access::Full), 5000), Verdict::Allowed);
        assert_eq!(Verdict::Allowed.reason(), None);
    }
}
