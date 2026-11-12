//! Startup items run once per logon, not once per session process. A named kernel object held by
//! the session would die with it, which is exactly when a restarted session must not run them
//! again. So the marker is a volatile registry key under HKCU: it survives the session process
//! and is removed by Windows when the user logs off. (Creating a key and learning whether it
//! was new is one atomic call, so two sessions cannot both win.) A session that adopted running
//! apps was restarted by definition and skips them too.

use crate::registry::{Hive, Registry};

pub const KEY: &str = "Software\\shellman\\session-startup";

#[derive(PartialEq, Eq, Debug)]
pub enum Verdict {
    Run,
    AlreadyRan,
    /// Apps were adopted: this is a restarted session.
    Restarted,
    /// The marker could not be made; better to run nothing than to run twice.
    Unknown(String),
}

/// Claim this logon's startup run. Only the first caller gets `Run`.
pub fn claim(reg: &dyn Registry, adopted: usize) -> Verdict {
    match reg.create_volatile(Hive::Hkcu, KEY) {
        Ok(true) if adopted > 0 => Verdict::Restarted,
        Ok(true) => Verdict::Run,
        Ok(false) => Verdict::AlreadyRan,
        Err(e) => Verdict::Unknown(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::fake::Fake;

    #[test]
    fn only_the_first_claim_runs() {
        let reg = Fake::default();
        assert_eq!(claim(&reg, 0), Verdict::Run);
        // The same logon, a session that came back after a crash.
        assert_eq!(claim(&reg, 0), Verdict::AlreadyRan);
    }

    #[test]
    fn adopting_apps_means_a_restart() {
        // A restarted session whose marker is somehow gone still does not run them again.
        assert_eq!(claim(&Fake::default(), 2), Verdict::Restarted);
    }
}
