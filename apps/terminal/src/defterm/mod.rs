//! Windows' "default terminal application": where a console program shows when it starts with
//! no terminal (a `.bat` double-clicked, Win+R `cmd`, a program spawning `powershell`).
//!
//! How it works (microsoft/terminal: `DelegationConfig.cpp`, `srvinit.cpp`, `ITerminalHandoff.idl`):
//! the inbox `conhost.exe` reads `HKCU\Console\%%Startup` `DelegationConsole` /
//! `DelegationTerminal` (CLSIDs), hands the new console to the delegation console (a COM server
//! implementing `IConsoleHandoff`: Windows Terminal's OpenConsole), which runs it as a
//! pseudo-console and calls the delegation terminal's `ITerminalHandoff3::EstablishPtyHandoff`
//! (`CLSCTX_LOCAL_SERVER`). Windows' Settings only lists packaged pairs (app extensions
//! `com.microsoft.windows.console.host` / `.terminal.host` from one package), but the values
//! themselves take any CLSID, and OpenConsole creates whatever the terminal value names.
//!
//! So shellman registers, per user and without admin rights:
//! - its own class `CLSID` under `HKCU\Software\Classes\CLSID`, `LocalServer32` being this exe
//!   (COM starts it with `-Embedding`; see `server`);
//! - `DelegationConsole` = Windows Terminal's OpenConsole (needs Windows Terminal installed: its
//!   OpenConsole and its proxy-stub for the handoff interfaces are packaged COM, visible to us),
//!   `DelegationTerminal` = ours;
//! - the values that were there before, under `HKCU\Software\shellman\Terminal`, which
//!   `--unregister-default-terminal` puts back.
//!
//! Everything that decides what to write is [`register_plan`] / [`unregister_plan`] (pure, tested);
//! [`apply`] only carries it out.

pub mod server;

use shellman_core::registry::{Hive, Real, Registry};

/// shellman Terminal's handoff class (Settings reads it too).
pub const CLSID: &str = shellman_core::terminal::handoff::CLSID;
/// Windows Terminal's OpenConsole (`IConsoleHandoff`), the console host we hand to.
pub const WT_CONSOLE: &str = "{2EACA947-7F5F-4CFA-BA87-8F7FBEEFBE69}";
/// "Let Windows decide".
pub const DEFAULT: &str = shellman_core::terminal::handoff::DEFAULT;

pub const STARTUP_KEY: &str = r"Console\%%Startup";
pub const CONSOLE_VALUE: &str = "DelegationConsole";
pub const TERMINAL_VALUE: &str = "DelegationTerminal";
pub const SAVED_KEY: &str = r"Software\shellman\Terminal";
pub const SAVED_CONSOLE: &str = "SavedDelegationConsole";
pub const SAVED_TERMINAL: &str = "SavedDelegationTerminal";

pub fn class_key() -> String {
    format!(r"Software\Classes\CLSID\{CLSID}")
}

/// A `DelegationConsole` / `DelegationTerminal` pair.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Pair {
    pub console: String,
    pub terminal: String,
}

impl Pair {
    fn new(console: &str, terminal: &str) -> Self {
        Self { console: console.to_owned(), terminal: terminal.to_owned() }
    }
    fn is_ours(&self) -> bool {
        self.terminal.eq_ignore_ascii_case(CLSID)
    }
}

/// One registry change, all under HKCU.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Op {
    Set { key: String, name: String, value: String },
    DeleteValue { key: String, name: String },
    /// A whole key and what is under it.
    DeleteKey { key: String },
}

fn set(key: &str, name: &str, value: &str) -> Op {
    Op::Set { key: key.into(), name: name.into(), value: value.into() }
}

/// Make shellman's terminal the default: `exe` is this program's full path; `current` the pair
/// there now (None when unset), `saved` what an earlier registration kept.
pub fn register_plan(exe: &str, current: Option<Pair>, saved: Option<Pair>) -> Vec<Op> {
    let mut ops = Vec::new();
    // Keep what was there, once: registering again must not save our own values over it.
    if saved.is_none() {
        let before = current.filter(|c| !c.is_ours()).unwrap_or_else(|| Pair::new(DEFAULT, DEFAULT));
        ops.push(set(SAVED_KEY, SAVED_CONSOLE, &before.console));
        ops.push(set(SAVED_KEY, SAVED_TERMINAL, &before.terminal));
    }
    let class = class_key();
    ops.push(set(&class, "", "shellman Terminal (console handoff)"));
    ops.push(set(&format!(r"{class}\LocalServer32"), "", &format!("\"{exe}\"")));
    ops.push(set(STARTUP_KEY, CONSOLE_VALUE, WT_CONSOLE));
    ops.push(set(STARTUP_KEY, TERMINAL_VALUE, CLSID));
    ops
}

/// Undo [`register_plan`]: the saved pair goes back if shellman is still the default (if the
/// user has since picked another terminal, that choice stays), and shellman's keys go.
pub fn unregister_plan(current: Option<Pair>, saved: Option<Pair>) -> Vec<Op> {
    let mut ops = Vec::new();
    if current.as_ref().is_some_and(Pair::is_ours) {
        let back = saved.filter(|s| !s.is_ours()).unwrap_or_else(|| Pair::new(DEFAULT, DEFAULT));
        ops.push(set(STARTUP_KEY, CONSOLE_VALUE, &back.console));
        ops.push(set(STARTUP_KEY, TERMINAL_VALUE, &back.terminal));
    }
    ops.push(Op::DeleteValue { key: SAVED_KEY.into(), name: SAVED_CONSOLE.into() });
    ops.push(Op::DeleteValue { key: SAVED_KEY.into(), name: SAVED_TERMINAL.into() });
    ops.push(Op::DeleteKey { key: class_key() });
    ops
}

fn read_pair(reg: &dyn Registry, key: &str, console: &str, terminal: &str) -> Option<Pair> {
    let get = |n: &str| reg.get(Hive::Hkcu, key, n).and_then(|v| v.text().map(str::to_owned));
    match (get(console), get(terminal)) {
        (None, None) => None,
        (c, t) => Some(Pair { console: c.unwrap_or_else(|| DEFAULT.into()), terminal: t.unwrap_or_else(|| DEFAULT.into()) }),
    }
}

/// The pair Windows uses now.
pub fn current() -> Option<Pair> {
    read_pair(&Real, STARTUP_KEY, CONSOLE_VALUE, TERMINAL_VALUE)
}

fn saved() -> Option<Pair> {
    read_pair(&Real, SAVED_KEY, SAVED_CONSOLE, SAVED_TERMINAL)
}

/// Carry out `ops`; stops at the first failure (a missing value or key to delete is fine).
pub fn apply(ops: &[Op]) -> Result<(), String> {
    for op in ops {
        match op {
            Op::Set { key, name, value } => Real.set_text(Hive::Hkcu, key, name, value).map_err(|e| format!("{key}\\{name}: {e}"))?,
            Op::DeleteValue { key, name } => {
                let _ = Real.delete(Hive::Hkcu, key, name);
            }
            Op::DeleteKey { key } => {
                use windows::core::HSTRING;
                use windows::Win32::System::Registry::{RegDeleteTreeW, HKEY_CURRENT_USER};
                let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(key.as_str())) };
            }
        }
    }
    Ok(())
}

use shellman_core::terminal::handoff::windows_terminal_installed;

fn show(p: &Option<Pair>) -> String {
    match p {
        Some(p) => format!("console {} terminal {}", p.console, p.terminal),
        None => "(not set: let Windows decide)".into(),
    }
}

/// `--register-default-terminal`. Returns the process exit code.
pub fn register() -> i32 {
    if !windows_terminal_installed() {
        eprintln!("shellman-terminal: Windows Terminal is not installed; its OpenConsole is needed to hand consoles over. Nothing changed.");
        return 1;
    }
    let Ok(exe) = std::env::current_exe() else { return 1 };
    let before = current();
    match apply(&register_plan(&exe.display().to_string(), before.clone(), saved())) {
        Ok(()) => {
            println!("before: {}\nnow:    {}", show(&before), show(&current()));
            0
        }
        Err(e) => {
            eprintln!("shellman-terminal: {e}");
            1
        }
    }
}

/// `--unregister-default-terminal`. Returns the process exit code.
pub fn unregister() -> i32 {
    let before = current();
    match apply(&unregister_plan(before.clone(), saved())) {
        Ok(()) => {
            println!("before: {}\nnow:    {}", show(&before), show(&current()));
            0
        }
        Err(e) => {
            eprintln!("shellman-terminal: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WT_TERMINAL: &str = "{E12CFF52-A866-4C77-9A90-F570A7AA2C6B}";
    const EXE: &str = r"C:\Users\a\AppData\Local\Programs\shellman\shellman-terminal.exe";

    fn sets(ops: &[Op]) -> Vec<(String, String, String)> {
        ops.iter()
            .filter_map(|o| match o {
                Op::Set { key, name, value } => Some((key.clone(), name.clone(), value.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn registering_saves_what_was_there_and_points_at_us() {
        let ops = register_plan(EXE, Some(Pair::new(WT_CONSOLE, WT_TERMINAL)), None);
        let s = sets(&ops);
        assert!(s.contains(&(SAVED_KEY.into(), SAVED_CONSOLE.into(), WT_CONSOLE.into())));
        assert!(s.contains(&(SAVED_KEY.into(), SAVED_TERMINAL.into(), WT_TERMINAL.into())));
        assert!(s.contains(&(format!(r"{}\LocalServer32", class_key()), String::new(), format!("\"{EXE}\""))));
        assert_eq!(&s[s.len() - 2..], [
            (STARTUP_KEY.into(), CONSOLE_VALUE.into(), WT_CONSOLE.into()),
            (STARTUP_KEY.into(), TERMINAL_VALUE.into(), CLSID.into()),
        ]);
        // Nothing set before: "let Windows decide" is what goes back.
        let s = sets(&register_plan(EXE, None, None));
        assert!(s.contains(&(SAVED_KEY.into(), SAVED_TERMINAL.into(), DEFAULT.into())));
    }

    #[test]
    fn registering_twice_keeps_the_first_saved_pair() {
        let ops = register_plan(EXE, Some(Pair::new(WT_CONSOLE, CLSID)), Some(Pair::new(DEFAULT, DEFAULT)));
        assert!(!sets(&ops).iter().any(|(k, _, _)| k == SAVED_KEY));
        // Ours already set but nothing saved (a lost key): never save ourselves as "before".
        let ops = register_plan(EXE, Some(Pair::new(WT_CONSOLE, &CLSID.to_lowercase())), None);
        assert!(sets(&ops).contains(&(SAVED_KEY.into(), SAVED_TERMINAL.into(), DEFAULT.into())));
    }

    #[test]
    fn unregistering_restores_and_removes() {
        let ops = unregister_plan(Some(Pair::new(WT_CONSOLE, CLSID)), Some(Pair::new(WT_CONSOLE, WT_TERMINAL)));
        assert_eq!(&sets(&ops), &[
            (STARTUP_KEY.into(), CONSOLE_VALUE.into(), WT_CONSOLE.into()),
            (STARTUP_KEY.into(), TERMINAL_VALUE.into(), WT_TERMINAL.into()),
        ]);
        assert!(ops.contains(&Op::DeleteKey { key: class_key() }));
        assert!(ops.contains(&Op::DeleteValue { key: SAVED_KEY.into(), name: SAVED_CONSOLE.into() }));
        // Nothing saved: back to "let Windows decide".
        let s = sets(&unregister_plan(Some(Pair::new(WT_CONSOLE, CLSID)), None));
        assert_eq!(s[1].2, DEFAULT);
    }

    #[test]
    fn a_later_choice_of_the_user_is_left_alone() {
        let ops = unregister_plan(Some(Pair::new(WT_CONSOLE, WT_TERMINAL)), Some(Pair::new(DEFAULT, DEFAULT)));
        assert!(sets(&ops).is_empty());
        assert!(ops.contains(&Op::DeleteKey { key: class_key() }));
    }

    #[test]
    fn the_class_is_a_guid() {
        assert_eq!(CLSID.len(), 38);
        assert!(windows::core::GUID::try_from(&CLSID[1..37]).is_ok());
    }
}
