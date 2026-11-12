//! The shell swap: making this session the user's shell, and undoing it. The per-user value
//! `HKCU\Software\Microsoft\Windows NT\CurrentVersion\Winlogon\Shell` overrides the machine's
//! (`explorer.exe`) when Windows starts the shell at logon. Nothing here runs at a normal start:
//! only `--install` and `--uninstall` do, and both work on the registry through the trait.
//!
//! Before changing the value the previous one is saved under `HKCU\Software\shellman\session`, so
//! `--uninstall` puts back exactly what was there (usually nothing, which means Explorer).

use crate::registry::{Hive, Registry};
use std::path::Path;

pub const WINLOGON: &str = "Software\\Microsoft\\Windows NT\\CurrentVersion\\Winlogon";
const SAVED: &str = "Software\\shellman\\session";

#[derive(PartialEq, Eq, Debug)]
pub enum Done {
    Installed { previous: Option<String> },
    Updated,
    Restored(String),
    Removed,
}

/// Whether a Shell value is this session's: its exe name, with or without quotes.
fn is_ours(value: &str) -> bool {
    value.trim().trim_matches('"').to_lowercase().ends_with("shellman-session.exe")
}

/// The value to store: quoted when the path has spaces.
fn value_for(exe: &Path) -> String {
    let p = exe.display().to_string();
    if p.contains(' ') { format!("\"{p}\"") } else { p }
}

fn current(reg: &dyn Registry) -> Option<String> {
    reg.get(Hive::Hkcu, WINLOGON, "Shell").and_then(|v| v.text().map(str::to_string))
}

/// `exists` tells whether a file is there (injected for tests).
pub fn install(reg: &dyn Registry, exe: &Path, exists: &dyn Fn(&Path) -> bool) -> Result<Done, String> {
    let bar = exe.with_file_name("shellman-bar.exe");
    for need in [exe, bar.as_path()] {
        if !exists(need) {
            return Err(format!("{} is missing; nothing was changed", need.display()));
        }
    }
    let now = current(reg);
    let value = value_for(exe);
    if now.as_deref().is_some_and(is_ours) {
        // Already installed, maybe from another folder: point it here, keep the saved original.
        reg.set_text(Hive::Hkcu, WINLOGON, "Shell", &value)?;
        return Ok(Done::Updated);
    }
    // Save first: if that fails the shell value is not touched.
    reg.set_text(Hive::Hkcu, SAVED, "PreviousShell", now.as_deref().unwrap_or(""))
        .and_then(|_| reg.set_text(Hive::Hkcu, SAVED, "HadPreviousShell", if now.is_some() { "1" } else { "0" }))
        .map_err(|e| format!("could not save the previous shell value ({e}); nothing was changed"))?;
    reg.set_text(Hive::Hkcu, WINLOGON, "Shell", &value)?;
    if current(reg).as_deref() != Some(value.as_str()) {
        return Err("the value did not stick".into());
    }
    Ok(Done::Installed { previous: now })
}

pub fn uninstall(reg: &dyn Registry) -> Result<Done, String> {
    let Some(now) = current(reg) else { return Err("no per-user Shell value is set: Explorer is already the shell".into()) };
    if !is_ours(&now) {
        return Err(format!("the Shell value is {now:?}, not shellman's; left alone"));
    }
    let had = reg.get(Hive::Hkcu, SAVED, "HadPreviousShell").and_then(|v| v.text().map(|t| t == "1")).unwrap_or(false);
    let previous = reg.get(Hive::Hkcu, SAVED, "PreviousShell").and_then(|v| v.text().map(str::to_string)).unwrap_or_default();
    let done = if had && !previous.trim().is_empty() && !is_ours(&previous) {
        reg.set_text(Hive::Hkcu, WINLOGON, "Shell", &previous)?;
        Done::Restored(previous)
    } else {
        reg.delete(Hive::Hkcu, WINLOGON, "Shell")?;
        Done::Removed
    };
    for name in ["PreviousShell", "HadPreviousShell"] {
        let _ = reg.delete(Hive::Hkcu, SAVED, name);
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{fake::Fake, Value};
    use std::path::PathBuf;

    fn exe() -> PathBuf {
        PathBuf::from("C:\\s\\shellman-session.exe")
    }

    fn there(_: &Path) -> bool {
        true
    }

    fn shell(reg: &Fake) -> Option<String> {
        reg.get(Hive::Hkcu, WINLOGON, "Shell").and_then(|v| v.text().map(str::to_string))
    }

    #[test]
    fn install_sets_the_value_and_uninstall_removes_it() {
        let reg = Fake::default();
        assert_eq!(install(&reg, &exe(), &there), Ok(Done::Installed { previous: None }));
        assert_eq!(shell(&reg).as_deref(), Some("C:\\s\\shellman-session.exe"));
        assert_eq!(uninstall(&reg), Ok(Done::Removed));
        assert_eq!(shell(&reg), None);
    }

    #[test]
    fn the_previous_value_comes_back() {
        let reg = Fake::default();
        reg.put(Hive::Hkcu, WINLOGON, "Shell", Value::Text("other.exe".into()));
        install(&reg, &exe(), &there).unwrap();
        assert_eq!(uninstall(&reg), Ok(Done::Restored("other.exe".into())));
        assert_eq!(shell(&reg).as_deref(), Some("other.exe"));
    }

    #[test]
    fn installing_twice_keeps_the_first_saved_value() {
        let reg = Fake::default();
        reg.put(Hive::Hkcu, WINLOGON, "Shell", Value::Text("other.exe".into()));
        install(&reg, &exe(), &there).unwrap();
        assert_eq!(install(&reg, &PathBuf::from("C:\\n\\shellman-session.exe"), &there), Ok(Done::Updated));
        assert_eq!(uninstall(&reg), Ok(Done::Restored("other.exe".into())));
    }

    #[test]
    fn spaces_are_quoted() {
        let reg = Fake::default();
        install(&reg, &PathBuf::from("C:\\a b\\shellman-session.exe"), &there).unwrap();
        assert_eq!(shell(&reg).as_deref(), Some("\"C:\\a b\\shellman-session.exe\""));
        assert!(uninstall(&reg).is_ok(), "the quoted value is still recognised as ours");
    }

    #[test]
    fn refuses_without_the_files_and_changes_nothing() {
        let reg = Fake::default();
        let only_session = |p: &Path| p.ends_with("shellman-session.exe");
        assert!(install(&reg, &exe(), &only_session).unwrap_err().contains("shellman-bar.exe"));
        assert!(reg.log.borrow().is_empty());
    }

    #[test]
    fn a_failed_save_leaves_the_shell_alone() {
        let reg = Fake::default();
        reg.refuse.borrow_mut().push("PreviousShell".into());
        assert!(install(&reg, &exe(), &there).is_err());
        assert_eq!(shell(&reg), None);
    }

    #[test]
    fn uninstall_never_touches_someone_elses_value() {
        let reg = Fake::default();
        assert!(uninstall(&reg).is_err());
        reg.put(Hive::Hkcu, WINLOGON, "Shell", Value::Text("other.exe".into()));
        assert!(uninstall(&reg).is_err());
        assert_eq!(shell(&reg).as_deref(), Some("other.exe"));
    }
}
