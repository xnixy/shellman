//! Turning a startup item on or off: writing its StartupApproved value where Windows keeps it.
//! Pure over the registry trait; the elevated fallback is the caller's (`worker`).

use super::approved;
use super::items::Item;
use crate::registry::{Hive, Registry};

/// How far a switch got.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Done,
    /// Windows would not let this process write the machine-wide key; the administrator helper
    /// has to.
    NeedsAdmin,
}

/// Switch `item` on or off. `now` is the FILETIME stamped on a disable.
///
/// Per-user items are written under HKCU. Machine-wide ones keep their approval under HKLM, which
/// is tried directly; that is the only place Windows reads it from (see `approved`). A per-user
/// entry for a machine-wide item (Windows ignores it; something else may have written it) is
/// kept in step, so no tool that reads it shows the opposite.
pub fn apply(reg: &dyn Registry, item: &Item, enable: bool, now: u64) -> Result<Step, String> {
    let area = item.area().ok_or_else(|| "This item has no switch.".to_string())?;
    let write = |hive| approved::write(reg, hive, area, &item.name, enable, now);
    if !item.machine_wide() {
        return write(Hive::Hkcu).map(|()| Step::Done).map_err(|e| format!("Windows did not allow the change ({e})."));
    }
    if reg.get(Hive::Hkcu, &area.key(), &item.name).is_some() {
        write(Hive::Hkcu).map_err(|e| format!("Windows did not allow the change ({e})."))?;
    }
    Ok(match write(Hive::Hklm) {
        Ok(()) => Step::Done,
        Err(_) => Step::NeedsAdmin,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{fake::Fake, Value};
    use crate::startup::approved::Area;
    use crate::startup::items::{collect, FolderFile};
    use std::path::PathBuf;

    const CV: &str = "Software\\Microsoft\\Windows\\CurrentVersion";

    fn items(reg: &Fake) -> Vec<Item> {
        let files = [FolderFile { common: true, path: PathBuf::from("C:\\C\\t.lnk") }];
        collect(reg, &files, &|s| s.to_string(), true)
    }

    fn state(reg: &Fake, hive: Hive, area: Area, name: &str) -> Option<Vec<u8>> {
        match reg.get(hive, &area.key(), name)? {
            Value::Binary(b) => Some(b),
            _ => None,
        }
    }

    #[test]
    fn a_user_item_is_written_under_hkcu() {
        let reg = Fake::default();
        reg.put(Hive::Hkcu, &format!("{CV}\\Run"), "App", Value::Text("a.exe".into()));
        let item = items(&reg).remove(0);
        assert_eq!(apply(&reg, &item, false, 7), Ok(Step::Done));
        let b = state(&reg, Hive::Hkcu, Area::Run, "App").unwrap();
        assert_eq!(b[0], 3);
        assert_eq!(&b[4..], &7u64.to_le_bytes());
        assert_eq!(apply(&reg, &item, true, 8), Ok(Step::Done));
        assert_eq!(state(&reg, Hive::Hkcu, Area::Run, "App").unwrap(), [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert!(state(&reg, Hive::Hklm, Area::Run, "App").is_none(), "never touches HKLM");
    }

    #[test]
    fn a_machine_item_is_written_under_hklm() {
        let reg = Fake::default();
        reg.put(Hive::Hklm, &format!("{CV}\\Run"), "M", Value::Text("m.exe".into()));
        let item = items(&reg).into_iter().find(|i| i.name == "M").unwrap();
        assert_eq!(apply(&reg, &item, false, 1), Ok(Step::Done));
        assert_eq!(state(&reg, Hive::Hklm, Area::Run, "M").unwrap()[0], 3);
        assert!(state(&reg, Hive::Hkcu, Area::Run, "M").is_none());
    }

    #[test]
    fn the_users_own_entry_for_a_machine_item_is_kept_in_step() {
        let reg = Fake::default();
        reg.put(Hive::Hklm, &format!("{CV}\\Run"), "M", Value::Text("m.exe".into()));
        reg.put(Hive::Hkcu, &Area::Run.key(), "M", Value::Binary(vec![2; 12]));
        let item = items(&reg).into_iter().find(|i| i.name == "M").unwrap();
        assert_eq!(apply(&reg, &item, false, 1), Ok(Step::Done));
        assert!(!approved::check(&reg, Hive::Hklm, Area::Run, "M").0);
        assert!(!approved::check(&reg, Hive::Hkcu, Area::Run, "M").0);
    }

    #[test]
    fn a_refused_machine_write_asks_for_the_helper() {
        let reg = Fake::default();
        let item = items(&reg).into_iter().find(|i| i.name == "t.lnk").unwrap();
        reg.refuse.borrow_mut().push("t.lnk".into());
        assert_eq!(apply(&reg, &item, false, 1), Ok(Step::NeedsAdmin));
    }

    #[test]
    fn a_refused_user_write_is_an_error() {
        let reg = Fake::default();
        reg.put(Hive::Hkcu, &format!("{CV}\\Run"), "App", Value::Text("a.exe".into()));
        reg.refuse.borrow_mut().push("App".into());
        let item = items(&reg).remove(0);
        assert!(apply(&reg, &item, false, 1).is_err());
    }

    #[test]
    fn items_without_a_switch_are_refused() {
        let reg = Fake::default();
        reg.put(Hive::Hkcu, &format!("{CV}\\RunOnce"), "O", Value::Text("o.exe".into()));
        let item = items(&reg).into_iter().find(|i| i.name == "O").unwrap();
        assert!(apply(&reg, &item, false, 1).is_err());
    }

    /// Against the real HKCU, with a throwaway value; run by hand (`cargo test -- --ignored`).
    /// Leaves the keys as it found them.
    #[test]
    #[ignore]
    fn real_round_trip_on_a_throwaway_user_item() {
        use crate::registry::Real;
        let name = "shellman-startup-test";
        let run = format!("{CV}\\Run");
        let before = (Real.list(Hive::Hkcu, &run).len(), Real.list(Hive::Hkcu, &Area::Run.key()).len());
        Real.set_text(Hive::Hkcu, &run, name, r"C:\Windows\System32\notepad.exe").unwrap();
        let find = || {
            let rows = crate::startup::worker::read_rows();
            rows.into_iter().find(|r| r.item.name == name).expect("the test item is listed")
        };
        let row = find();
        assert!(row.item.enabled && !row.item.machine_wide() && !row.missing);
        assert_eq!(row.title, "Notepad");
        assert_eq!(apply(&Real, &row.item, false, crate::startup::win::now_filetime()), Ok(Step::Done));
        assert!(!find().item.enabled, "off after switching off");
        assert_eq!(apply(&Real, &row.item, true, 0), Ok(Step::Done));
        assert!(find().item.enabled, "on again");
        Real.delete(Hive::Hkcu, &run, name).unwrap();
        Real.delete(Hive::Hkcu, &Area::Run.key(), name).unwrap();
        let after = (Real.list(Hive::Hkcu, &run).len(), Real.list(Hive::Hkcu, &Area::Run.key()).len());
        assert_eq!(before, after, "keys are back as they were");
    }
}
