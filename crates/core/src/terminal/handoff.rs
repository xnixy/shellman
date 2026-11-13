//! "Use shellman Terminal for all console programs": whether Windows hands new consoles to
//! shellman's terminal (its default terminal), read from the registry, and the command that
//! switches it. The switching itself is the terminal exe's own (`--register-default-terminal`,
//! `--unregister-default-terminal`, see `apps/terminal/src/defterm`), so Settings runs that.

use std::path::{Path, PathBuf};

/// shellman Terminal's handoff class (the terminal registers it as `DelegationTerminal`).
pub const CLSID: &str = "{39C0509F-90BF-4A6C-A66D-5CF96BDDC7E2}";
/// "Let Windows decide".
pub const DEFAULT: &str = "{00000000-0000-0000-0000-000000000000}";
/// Windows Terminal's own `DelegationTerminal`.
pub const WT_TERMINAL: &str = "{E12CFF52-A866-4C77-9A90-F570A7AA2C6B}";
pub const STARTUP_KEY: &str = r"Console\%%Startup";
pub const TERMINAL_VALUE: &str = "DelegationTerminal";

/// Who gets new consoles.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum State {
    /// shellman's terminal.
    Shellman,
    /// Windows Terminal.
    WindowsTerminal,
    /// Not set, or "let Windows decide" (the console window, or Windows Terminal where Windows
    /// makes it the default).
    WindowsDecides,
    /// Another terminal, by its CLSID.
    Other(String),
}

impl State {
    /// From the `DelegationTerminal` value (None when it is not there).
    pub fn from_value(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            None | Some("") => State::WindowsDecides,
            Some(v) if v.eq_ignore_ascii_case(CLSID) => State::Shellman,
            Some(v) if v.eq_ignore_ascii_case(DEFAULT) => State::WindowsDecides,
            Some(v) if v.eq_ignore_ascii_case(WT_TERMINAL) => State::WindowsTerminal,
            Some(v) => State::Other(v.to_owned()),
        }
    }

    pub fn is_shellman(&self) -> bool {
        *self == State::Shellman
    }

    /// In words, for Settings.
    pub fn describe(&self) -> String {
        match self {
            State::Shellman => "Console programs open in shellman Terminal.".into(),
            State::WindowsTerminal => "Console programs open in Windows Terminal.".into(),
            State::WindowsDecides => "Windows decides where console programs open.".into(),
            State::Other(clsid) => format!("Console programs open in another terminal ({clsid})."),
        }
    }
}

/// Who gets new consoles now.
pub fn current() -> State {
    use crate::registry::{Hive, Real, Registry};
    let v = Real.get(Hive::Hkcu, STARTUP_KEY, TERMINAL_VALUE).and_then(|v| v.text().map(str::to_owned));
    State::from_value(v.as_deref())
}

/// Whether Windows Terminal is installed for this user (its OpenConsole is what the handoff
/// goes through): its app alias folder is there.
pub fn windows_terminal_installed() -> bool {
    std::env::var_os("LOCALAPPDATA")
        .map(|d| Path::new(&d).join(r"Microsoft\WindowsApps\Microsoft.WindowsTerminal_8wekyb3d8bbwe"))
        .is_some_and(|p| p.is_dir())
}

/// The program and argument that turn it on (`on`) or off: the installed terminal exe's own
/// switch.
pub fn command(terminal_exe: &Path, on: bool) -> (PathBuf, &'static str) {
    let arg = if on { "--register-default-terminal" } else { "--unregister-default-terminal" };
    (terminal_exe.to_path_buf(), arg)
}

/// Run [`command`] without a console window and wait for it; Ok with what it printed, or why it
/// failed. Blocks: call it off the UI thread.
pub fn switch(terminal_exe: &Path, on: bool) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    const NO_WINDOW: u32 = 0x0800_0000;
    let (exe, arg) = command(terminal_exe, on);
    let out = std::process::Command::new(&exe).arg(arg).creation_flags(NO_WINDOW).output().map_err(|e| format!("{}: {e}", exe.display()))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)).trim().to_owned();
    if out.status.success() {
        Ok(text)
    } else {
        Err(if text.is_empty() { format!("exit code {:?}", out.status.code()) } else { text })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_value_says_who_gets_consoles() {
        assert_eq!(State::from_value(Some(CLSID)), State::Shellman);
        assert_eq!(State::from_value(Some(&CLSID.to_lowercase())), State::Shellman);
        assert_eq!(State::from_value(None), State::WindowsDecides);
        assert_eq!(State::from_value(Some(DEFAULT)), State::WindowsDecides);
        assert_eq!(State::from_value(Some(WT_TERMINAL)), State::WindowsTerminal);
        assert_eq!(State::from_value(Some("{B23D10C0-E52E-411E-9D5B-C09FDF709C7D}")), State::Other("{B23D10C0-E52E-411E-9D5B-C09FDF709C7D}".into()));
        assert!(State::from_value(Some(CLSID)).is_shellman());
    }

    #[test]
    fn the_switch_runs_the_terminal_s_own_flags() {
        let exe = Path::new(r"C:\Users\a\AppData\Local\Programs\shellman\shellman-terminal.exe");
        assert_eq!(command(exe, true), (exe.to_path_buf(), "--register-default-terminal"));
        assert_eq!(command(exe, false), (exe.to_path_buf(), "--unregister-default-terminal"));
    }

    /// The real value, read: whatever it is, it reads as one of the states (nothing is written).
    #[test]
    fn the_current_state_reads() {
        let s = current();
        assert!(!s.describe().is_empty());
    }
}
