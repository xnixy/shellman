//! The enable/disable state Task Manager and Settings keep for startup items, under
//! `...\Explorer\StartupApproved\{Run, Run32, StartupFolder}` in HKCU and HKLM.
//!
//! One binary value per item, named like the Run value (or like the file in a Startup folder).
//! Twelve bytes as Windows writes them: a DWORD, then the FILETIME at which the item was turned
//! off (zero when it never was). There is no Microsoft documentation. The rule here is read from
//! the shell's own code, `shell\lib\runonce` (the copy in ShellAppRuntime.exe 10.0.26100.9818,
//! which Explorer shares; see PLAN.md, "Startup items"):
//!
//! - the value is read with `RegGetValueW(.., RRF_RT_REG_BINARY | RRF_ZEROONFAILURE)` into a
//!   zeroed 12-byte buffer, and only the low bit of the first little-endian DWORD counts:
//!   set (01, 03, 07, ...) is disabled, clear (00, 02, 04, 06, ...) is enabled;
//! - the FILETIME is never looked at, so `01 00 .. 00` (as Docker Desktop's value reads) is as
//!   disabled as Task Manager's `03 <time>`;
//! - a missing value, a missing key, a value of another type and any read error all count as
//!   enabled;
//! - an item is judged only in the StartupApproved key of its own hive: HKCU Run items under HKCU,
//!   HKLM Run and Run32 items and the common Startup folder under HKLM.
//!
//! The behaviour agrees: across three sign-ins with Explorer on this machine (Shell-Core
//! Operational events 9707) Explorer ran every HKCU Run item but Docker Desktop (`01`, zero
//! time). The Run value itself is left in place when an item is disabled.

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

/// `Some(true)` enabled, `Some(false)` disabled, `None` when there is no byte to read (which
/// Windows counts as enabled). Only the low bit of the first DWORD matters.
pub fn parse(bytes: &[u8]) -> Option<bool> {
    bytes.first().map(|b| b & 1 == 0)
}

/// Whether an item from `hive` is enabled, and a sentence saying why. Only `hive`'s own
/// StartupApproved key is read, as Windows does.
pub fn check(reg: &dyn Registry, hive: Hive, area: Area, name: &str) -> (bool, String) {
    let key = area.key();
    let label = if hive == Hive::Hkcu { "HKCU" } else { "HKLM" };
    let leaf = key.rsplit('\\').next().unwrap_or("");
    let Some(value) = reg.get(hive, &key, name) else {
        return (true, format!("no {label} StartupApproved\\{leaf} entry"));
    };
    match &value {
        Value::Binary(b) => match parse(b) {
            Some(on) => {
                let word = if on { "even: enabled" } else { "odd: disabled" };
                (on, format!("{label} StartupApproved\\{leaf}: first byte {:02x}, {word}", b[0]))
            }
            None => (true, format!("{label} StartupApproved\\{leaf}: empty value, treated as enabled")),
        },
        _ => (true, format!("{label} StartupApproved\\{leaf}: not binary, treated as enabled")),
    }
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
        assert_eq!(parse(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]), Some(true), "00: enabled");
        assert_eq!(parse(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]), Some(false), "01 with no time: disabled");
        assert_eq!(parse(&[1, 0, 0, 0, 3, 0x72, 0xaa, 0x19, 0x41, 0x72, 0xdd, 1]), Some(false), "01 with a time");
        assert_eq!(parse(&[2]), Some(true));
        assert_eq!(parse(&[3, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8]), Some(false));
        assert_eq!(parse(&[4]), Some(true));
        assert_eq!(parse(&[6, 0, 0, 0]), Some(true));
        assert_eq!(parse(&[7]), Some(false));
        // The rest of the DWORD does not count.
        assert_eq!(parse(&[2, 1, 0, 0]), Some(true));
        assert_eq!(parse(&[0x81]), Some(false));
        assert_eq!(parse(&[]), None);
    }

    #[test]
    fn docker_desktops_own_value_is_disabled() {
        let reg = Fake::default();
        reg.put(Hive::Hkcu, &Area::Run.key(), "Docker Desktop", Value::Binary(vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        let (on, why) = check(&reg, Hive::Hkcu, Area::Run, "Docker Desktop");
        assert!(!on);
        assert!(why.contains("first byte 01"), "{why}");
    }

    #[test]
    fn written_bytes_follow_windows_layout() {
        assert_eq!(bytes(true, 99), [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(bytes(false, 0x0102), [3, 0, 0, 0, 2, 1, 0, 0, 0, 0, 0, 0]);
        let reg = Fake::default();
        write(&reg, Hive::Hkcu, Area::Run32, "X", false, 5).unwrap();
        assert!(!check(&reg, Hive::Hkcu, Area::Run32, "X").0);
        write(&reg, Hive::Hkcu, Area::Run32, "x", true, 5).unwrap();
        assert!(check(&reg, Hive::Hkcu, Area::Run32, "X").0);
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
        assert!(check(&reg, Hive::Hkcu, Area::Run, "Anything").0);
        assert!(check(&reg, Hive::Hklm, Area::Run, "Anything").0);
    }

    #[test]
    fn each_hive_judges_only_its_own_items() {
        let reg = Fake::default();
        put(&reg, Hive::Hklm, Area::Run, "App", 2);
        put(&reg, Hive::Hkcu, Area::Run, "App", 3);
        assert!(check(&reg, Hive::Hklm, Area::Run, "App").0, "the user's value does not reach an HKLM item");
        assert!(!check(&reg, Hive::Hkcu, Area::Run, "app").0, "names compare without case");
        put(&reg, Hive::Hklm, Area::Run, "Other", 3);
        assert!(check(&reg, Hive::Hkcu, Area::Run, "Other").0, "nor the machine's an HKCU item");
    }

    #[test]
    fn areas_are_separate() {
        let reg = Fake::default();
        put(&reg, Hive::Hkcu, Area::Run32, "Discord", 3);
        assert!(check(&reg, Hive::Hkcu, Area::Run, "Discord").0);
        assert!(!check(&reg, Hive::Hkcu, Area::Run32, "Discord").0);
    }

    #[test]
    fn odd_values_are_read_as_text_in_the_reason() {
        let reg = Fake::default();
        reg.put(Hive::Hkcu, &Area::Run.key(), "X", Value::Dword(3));
        let (on, why) = check(&reg, Hive::Hkcu, Area::Run, "X");
        assert!(on && why.contains("not binary"));
    }
}
