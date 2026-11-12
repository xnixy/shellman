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
    pub const ALL: [Area; 3] = [Area::Run, Area::Run32, Area::StartupFolder];

    /// The registry key (below the hive) that holds this area's approvals.
    pub fn key(self) -> String {
        let leaf = match self {
            Area::Run => "Run",
            Area::Run32 => "Run32",
            Area::StartupFolder => "StartupFolder",
        };
        format!("{BASE}\\{leaf}")
    }

    /// The word the administrator helper's command line uses for this area.
    pub fn word(self) -> &'static str {
        match self {
            Area::Run => "run",
            Area::Run32 => "run32",
            Area::StartupFolder => "folder",
        }
    }

    pub fn from_word(w: &str) -> Option<Area> {
        Area::ALL.into_iter().find(|a| a.word() == w)
    }
}

/// The twelve bytes Windows keeps for an item: 02 and zeros for enabled; 03 and the FILETIME of
/// the moment it was turned off for disabled.
pub fn bytes(enabled: bool, filetime: u64) -> [u8; 12] {
    let mut b = [0u8; 12];
    if enabled {
        b[0] = 2;
    } else {
        b[0] = 3;
        b[4..].copy_from_slice(&filetime.to_le_bytes());
    }
    b
}

/// Record that an item is on or off, in `hive`'s StartupApproved key for `area`.
pub fn write(reg: &dyn Registry, hive: Hive, area: Area, name: &str, enabled: bool, filetime: u64) -> Result<(), String> {
    reg.set_binary(hive, &area.key(), name, &bytes(enabled, filetime))
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
    fn written_bytes_follow_windows_layout() {
        assert_eq!(bytes(true, 99), [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(bytes(false, 0x0102), [3, 0, 0, 0, 2, 1, 0, 0, 0, 0, 0, 0]);
        let reg = Fake::default();
        write(&reg, Hive::Hkcu, Area::Run32, "X", false, 5).unwrap();
        assert!(!check(&reg, Area::Run32, "X").0);
        write(&reg, Hive::Hkcu, Area::Run32, "x", true, 5).unwrap();
        assert!(check(&reg, Area::Run32, "X").0);
    }

    #[test]
    fn areas_have_words_that_round_trip() {
        for a in Area::ALL {
            assert_eq!(Area::from_word(a.word()), Some(a));
        }
        assert_eq!(Area::from_word("Run"), None);
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
