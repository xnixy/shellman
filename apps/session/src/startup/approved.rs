//! The enable/disable state Task Manager and Settings keep for startup items, under
//! `...\Explorer\StartupApproved\{Run, Run32, StartupFolder}` in HKCU and HKLM.
//!
//! One binary value per item, named like the Run value (or like the file in a Startup folder).
//! Twelve bytes: the first DWORD says enabled or not, the other eight are the FILETIME at which it
//! was disabled. Enabled is an even first byte (02, 06 seen), disabled an odd one (03 written by
//! Task Manager, 01 seen too). There is no Microsoft documentation; this is observed behaviour:
//! the windowsir.blogspot.com note "Does autostart really mean autostart?" (Jul 2022) gives 02/06
//! as enabled and 03 plus a FILETIME as disabled, and the values on a real machine (04 for an
//! enabled Security Health entry, 01 for disabled Docker Desktop and Discord) agree with "even is
//! enabled". A missing value means enabled. The Run value itself is left in place when disabled.
//! The per-user (HKCU) answer is taken before the machine-wide one: Task Manager writes the
//! user's choice for HKLM items under HKCU.

use crate::registry::{Hive, Registry, Value};

const BASE: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved";

/// Which StartupApproved key an item is judged by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Area {
    Run,
    Run32,
    StartupFolder,
}

impl Area {
    fn key(self) -> String {
        let leaf = match self {
            Area::Run => "Run",
            Area::Run32 => "Run32",
            Area::StartupFolder => "StartupFolder",
        };
        format!("{BASE}\\{leaf}")
    }
}

/// `Some(true)` enabled, `Some(false)` disabled, `None` when there is no byte to read.
pub fn parse(bytes: &[u8]) -> Option<bool> {
    bytes.first().map(|b| b % 2 == 0)
}

/// Whether an item is enabled, and a sentence saying why.
pub fn check(reg: &dyn Registry, area: Area, name: &str) -> (bool, String) {
    let key = area.key();
    for (hive, label) in [(Hive::Hkcu, "HKCU"), (Hive::Hklm, "HKLM")] {
        let Some(value) = reg.get(hive, &key, name) else { continue };
        let leaf = key.rsplit('\\').next().unwrap_or("");
        return match &value {
            Value::Binary(b) => match parse(b) {
                Some(on) => (on, format!("{label} StartupApproved\\{leaf}: first byte {:02x}", b[0])),
                None => (true, format!("{label} StartupApproved\\{leaf}: empty value, treated as enabled")),
            },
            _ => (true, format!("{label} StartupApproved\\{leaf}: not binary, treated as enabled")),
        };
    }
    (true, "no StartupApproved entry".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::fake::Fake;

    fn put(reg: &Fake, hive: Hive, area: Area, name: &str, first: u8) {
        let mut v = vec![first, 0, 0, 0];
        v.extend([0u8; 8]);
        reg.put(hive, &area.key(), name, Value::Binary(v));
    }

    #[test]
    fn even_is_enabled_and_odd_is_disabled() {
        assert_eq!(parse(&[2]), Some(true));
        assert_eq!(parse(&[6, 0, 0, 0]), Some(true));
        assert_eq!(parse(&[4]), Some(true));
        assert_eq!(parse(&[3]), Some(false));
        assert_eq!(parse(&[1]), Some(false));
        assert_eq!(parse(&[]), None);
    }

    #[test]
    fn missing_means_enabled() {
        let reg = Fake::default();
        assert!(check(&reg, Area::Run, "Anything").0);
    }

    #[test]
    fn user_choice_wins_over_machine() {
        let reg = Fake::default();
        put(&reg, Hive::Hklm, Area::Run, "App", 2);
        assert!(check(&reg, Area::Run, "App").0);
        put(&reg, Hive::Hkcu, Area::Run, "App", 3);
        assert!(!check(&reg, Area::Run, "app").0, "names compare without case");
    }

    #[test]
    fn areas_are_separate() {
        let reg = Fake::default();
        put(&reg, Hive::Hkcu, Area::Run32, "Discord", 3);
        assert!(check(&reg, Area::Run, "Discord").0);
        assert!(!check(&reg, Area::Run32, "Discord").0);
    }

    #[test]
    fn odd_values_are_read_as_text_in_the_reason() {
        let reg = Fake::default();
        reg.put(Hive::Hkcu, &Area::Run.key(), "X", Value::Dword(3));
        let (on, why) = check(&reg, Area::Run, "X");
        assert!(on && why.contains("not binary"));
    }
}
