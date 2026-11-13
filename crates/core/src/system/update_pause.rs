//! Pausing Windows Update, the way Windows' own Settings does: start and end times (UTC, ISO
//! text) under `HKLM\SOFTWARE\Microsoft\WindowsUpdate\UX\Settings`, a key the Users group may
//! write. The names are the ones the update orchestrator (`MoUsoCoreWorker.exe`) reads. Measured
//! on 26300: after writing them, the next scan sets `UpdatePolicy\Settings`
//! `PausedQualityStatus` and `PausedFeatureStatus` to 1 and offers nothing; after deleting them,
//! the next scan sets both back to 0 and offers the updates again. Resuming deletes the values,
//! which is the state of a machine that was never paused.
//!
//! Written against `registry::Registry`, so the tests use the fake.

use super::update_logic::{iso, pause_end, paused_until, DEFAULT_MAX_PAUSE_DAYS};
use crate::registry::{Hive, Registry, Value};

pub const KEY: &str = r"SOFTWARE\Microsoft\WindowsUpdate\UX\Settings";
const POLICY: &str = r"SOFTWARE\Microsoft\WindowsUpdate\UpdatePolicy\Settings";
const STARTS: [&str; 3] = ["PauseUpdatesStartTime", "PauseFeatureUpdatesStartTime", "PauseQualityUpdatesStartTime"];
const ENDS: [&str; 3] = ["PauseUpdatesExpiryTime", "PauseFeatureUpdatesEndTime", "PauseQualityUpdatesEndTime"];
const MAX_DAYS: &str = "FlightSettingsMaxPauseDays";

/// The pause as stored and as Windows took it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PauseState {
    /// When a pause in force ends (seconds since 1970).
    pub until: Option<i64>,
    /// Windows' own policy state says paused (it catches up at its next scan).
    pub windows_agrees: bool,
    /// The longest pause Windows allows, in days.
    pub max_days: u32,
}

pub fn read(reg: &dyn Registry, now: i64) -> PauseState {
    let text = |name: &str| reg.get(Hive::Hklm, KEY, name).and_then(|v| v.text().map(str::to_owned));
    let dword = |key: &str, name: &str| match reg.get(Hive::Hklm, key, name) {
        Some(Value::Dword(n)) => Some(n),
        _ => None,
    };
    PauseState {
        until: paused_until(text(ENDS[0]).as_deref(), now),
        windows_agrees: dword(POLICY, "PausedQualityStatus") == Some(1),
        max_days: dword(KEY, MAX_DAYS).filter(|d| (1..=365).contains(d)).unwrap_or(DEFAULT_MAX_PAUSE_DAYS),
    }
}

/// Why a pause or resume was not written.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Refused {
    /// Access denied: this PC's key is not writable by this account.
    Denied,
    Failed(String),
}

impl Refused {
    fn from(e: String) -> Refused {
        if e.contains("0x80070005") || e.to_lowercase().contains("access is denied") {
            Refused::Denied
        } else {
            Refused::Failed(e)
        }
    }

    pub fn line(&self) -> String {
        match self {
            Refused::Denied => "Windows refused: pausing updates needs administrator rights on this PC.".to_owned(),
            Refused::Failed(e) => format!("Windows could not change it ({e})."),
        }
    }
}

/// Pause for `weeks` from `now` (a pause in force keeps its start). Returns the end. A write that
/// fails half way is undone, so updates are never left half paused.
pub fn pause(reg: &dyn Registry, now: i64, weeks: u32) -> Result<i64, Refused> {
    let state = read(reg, now);
    let start = state
        .until
        .and_then(|_| reg.get(Hive::Hklm, KEY, STARTS[0]))
        .and_then(|v| v.text().map(str::to_owned))
        .unwrap_or_else(|| iso(now));
    let end = pause_end(now, weeks, state.max_days);
    let end_text = iso(end);
    let writes = STARTS.iter().map(|n| (*n, start.as_str())).chain(ENDS.iter().map(|n| (*n, end_text.as_str())));
    for (name, value) in writes {
        if let Err(e) = reg.set_text(Hive::Hklm, KEY, name, value) {
            let _ = resume(reg);
            return Err(Refused::from(e));
        }
    }
    Ok(end)
}

/// End a pause: delete what a pause wrote (only the values that are there).
pub fn resume(reg: &dyn Registry) -> Result<(), Refused> {
    let mut first_error = None;
    for name in STARTS.iter().chain(ENDS.iter()) {
        if reg.get(Hive::Hklm, KEY, name).is_some() {
            if let Err(e) = reg.delete(Hive::Hklm, KEY, name) {
                first_error.get_or_insert(e);
            }
        }
    }
    first_error.map_or(Ok(()), |e| Err(Refused::from(e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::fake::Fake;

    const NOW: i64 = 1_794_535_000;

    fn get(reg: &Fake, name: &str) -> Option<String> {
        reg.get(Hive::Hklm, KEY, name).and_then(|v| v.text().map(str::to_owned))
    }

    #[test]
    fn an_empty_key_is_not_paused() {
        let reg = Fake::default();
        assert_eq!(read(&reg, NOW), PauseState { until: None, windows_agrees: false, max_days: 35 });
    }

    #[test]
    fn a_pause_writes_all_six_values_and_reads_back() {
        let reg = Fake::default();
        let end = pause(&reg, NOW, 2).unwrap();
        assert_eq!(end, NOW + 14 * 86_400);
        for n in STARTS {
            assert_eq!(get(&reg, n).as_deref(), Some(iso(NOW).as_str()), "{n}");
        }
        for n in ENDS {
            assert_eq!(get(&reg, n).as_deref(), Some(iso(end).as_str()), "{n}");
        }
        assert_eq!(read(&reg, NOW).until, Some(end));
        // Extending keeps the start.
        let later = pause(&reg, NOW + 3600, 3).unwrap();
        assert_eq!(get(&reg, STARTS[0]).as_deref(), Some(iso(NOW).as_str()));
        assert_eq!(read(&reg, NOW).until, Some(later));
    }

    #[test]
    fn resuming_leaves_the_key_as_it_was() {
        let reg = Fake::default();
        reg.put(Hive::Hklm, KEY, "SomethingElse", Value::Dword(1));
        pause(&reg, NOW, 1).unwrap();
        resume(&reg).unwrap();
        assert_eq!(reg.list(Hive::Hklm, KEY), vec![("SomethingElse".to_owned(), Value::Dword(1))]);
        assert_eq!(read(&reg, NOW).until, None);
        // Nothing to delete is fine.
        resume(&reg).unwrap();
    }

    #[test]
    fn windows_maximum_and_policy_state_are_read() {
        let reg = Fake::default();
        reg.put(Hive::Hklm, KEY, MAX_DAYS, Value::Dword(7));
        reg.put(Hive::Hklm, POLICY, "PausedQualityStatus", Value::Dword(1));
        let s = read(&reg, NOW);
        assert_eq!(s.max_days, 7);
        assert!(s.windows_agrees);
        assert_eq!(pause(&reg, NOW, 5).unwrap(), NOW + 7 * 86_400);
        reg.put(Hive::Hklm, KEY, MAX_DAYS, Value::Dword(0));
        assert_eq!(read(&reg, NOW).max_days, 35, "nonsense falls back to the default");
    }

    #[test]
    fn a_pause_that_ended_reads_as_not_paused() {
        let reg = Fake::default();
        reg.put(Hive::Hklm, KEY, ENDS[0], Value::Text(iso(NOW - 10)));
        assert_eq!(read(&reg, NOW).until, None);
    }

    #[test]
    fn a_refused_write_is_undone_and_told() {
        let reg = Fake::default();
        reg.refuse.borrow_mut().push(ENDS[1].to_owned());
        let e = pause(&reg, NOW, 1).unwrap_err();
        assert!(matches!(e, Refused::Failed(_)));
        assert!(reg.list(Hive::Hklm, KEY).is_empty(), "nothing left half paused");
        assert_eq!(Refused::from("Access is denied. (0x80070005)".into()), Refused::Denied);
        assert!(Refused::Denied.line().contains("administrator"));
    }
}
