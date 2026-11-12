//! The check at sign-in: a per-user Run value that starts `shellman-settings --check-fonts`. The
//! session starts the Run values at sign-in as Explorer would, so this needs nothing of its own
//! there. It sets window text again if a theme change reset it, and opens Settings on the Fonts
//! page if Windows put Segoe UI back.
//!
//! Fontman's own value (`Fontman`, running `fontman.exe check`) is only read, never changed:
//! once this one is on, that one can be turned off in Fontman or removed by hand.

use crate::reg;
use crate::settings::{self, read_json, write_json};

/// The flag `shellman-settings` takes for the check.
pub const CHECK_FLAG: &str = "--check-fonts";
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE: &str = "shellman font check";
/// Fontman's value, read to tell the user it is still on.
const FONTMAN_VALUE: &str = "Fontman";
const SETTINGS_EXE: &str = "shellman-settings.exe";

/// In a dry run the value lives in a file instead of the registry.
fn dry_path() -> std::path::PathBuf {
    settings::user_dir().join("dry-run-value.json")
}

fn get() -> Option<String> {
    if settings::dry_run() {
        return read_json(&dry_path());
    }
    reg::get(reg::HKEY_CURRENT_USER, RUN_KEY, VALUE)
}

pub fn is_enabled() -> bool {
    get().is_some()
}

/// Whether Fontman's own sign-in check is still on.
pub fn fontman_check_on() -> bool {
    reg::get(reg::HKEY_CURRENT_USER, RUN_KEY, FONTMAN_VALUE).is_some()
}

pub fn set_enabled(on: bool) -> Result<(), String> {
    if settings::dry_run() {
        return if on {
            write_json(&dry_path(), &command()?)
        } else {
            let _ = std::fs::remove_file(dry_path());
            Ok(())
        };
    }
    if on {
        reg::set(reg::HKEY_CURRENT_USER, RUN_KEY, VALUE, &command()?)
    } else {
        reg::delete(reg::HKEY_CURRENT_USER, RUN_KEY, VALUE)
    }
}

/// The command line: Settings beside the running exe, with the check flag.
fn command() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let dir = exe.parent().ok_or("The program has no folder.")?;
    Ok(command_for(&dir.join(SETTINGS_EXE)))
}

pub fn command_for(settings_exe: &std::path::Path) -> String {
    format!("\"{}\" {CHECK_FLAG}", settings_exe.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_quotes_the_exe_and_adds_the_flag() {
        let cmd = command_for(std::path::Path::new("C:\\Program Files\\shellman\\shellman-settings.exe"));
        assert_eq!(cmd, "\"C:\\Program Files\\shellman\\shellman-settings.exe\" --check-fonts");
    }
}
