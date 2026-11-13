//! Opening a terminal: every "Open Terminal here" in shellman goes through [`open_at`].
//!
//! Which terminal is a choice stored in `terminal.json` (the terminal's own settings file, in
//! shellman's config folder): shellman's own (the default), Windows Terminal, or a command line
//! of the user's. Whatever is chosen, a missing one falls back to the next that exists:
//! shellman's terminal (`shellman-terminal.exe` beside the running exe), Windows Terminal, then
//! Command Prompt in a console of its own.
//!
//! Windows Terminal without Explorer (measured on 26300 with shellman as the shell): it is a
//! Store app, and `shell:AppsFolder\…` needs Explorer, but its app alias
//! (`%LOCALAPPDATA%\Microsoft\WindowsApps\wt.exe`, a reparse point the system resolves itself)
//! starts it with `-d <folder>`. Without the alias the activation manager starts the package
//! directly (`apps_index::activate`).

use serde::{Deserialize, Serialize};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The settings file the choice lives in (the terminal's own).
pub const NAME: &str = "terminal";
/// Windows Terminal's app id.
const WINDOWS_TERMINAL: &str = "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App";
/// CREATE_NEW_CONSOLE, for Command Prompt.
const NEW_CONSOLE: u32 = 0x10;
/// In a custom command line, replaced by the folder (quoted).
pub const DIR_TOKEN: &str = "{dir}";

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Which {
    #[default]
    Shellman,
    WindowsTerminal,
    /// `custom_terminal`, a command line.
    Custom,
}

/// The choice, as `terminal.json` keeps it (the terminal's other settings sit beside it).
#[derive(Clone, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Choice {
    /// `"shellman"`, `"windows-terminal"` or `"custom"`.
    pub default_terminal: Which,
    /// For `"custom"`: the command line to run, `{dir}` standing for the folder (it also starts
    /// in that folder), e.g. `"C:\Tools\wezterm.exe" start --cwd {dir}`.
    pub custom_terminal: String,
}

impl Choice {
    pub fn load() -> Self {
        crate::config::load(NAME)
    }
}

/// One way of starting a terminal, in the order they are tried.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Route {
    /// `shellman-terminal.exe` at this path.
    Shellman(PathBuf),
    /// Windows Terminal's app alias at this path.
    WtAlias(PathBuf),
    /// Windows Terminal through the activation manager.
    WtActivate,
    /// The user's command line.
    Custom(String),
    /// Command Prompt in a new console.
    Cmd,
}

/// The order to try for `choice`, given where shellman's terminal and the alias are (None when
/// missing). Pure, for tests.
pub fn plan(choice: &Choice, shellman: Option<PathBuf>, wt_alias: Option<PathBuf>) -> Vec<Route> {
    let own: Vec<Route> = shellman.map(Route::Shellman).into_iter().collect();
    let mut wt: Vec<Route> = wt_alias.map(Route::WtAlias).into_iter().collect();
    wt.push(Route::WtActivate);
    let mut out = Vec::new();
    match choice.default_terminal {
        Which::Shellman => {
            out.extend(own);
            out.extend(wt);
        }
        Which::WindowsTerminal => {
            out.extend(wt);
            out.extend(own);
        }
        Which::Custom => {
            if !choice.custom_terminal.trim().is_empty() {
                out.push(Route::Custom(choice.custom_terminal.trim().to_owned()));
            }
            out.extend(own);
            out.extend(wt);
        }
    }
    out.push(Route::Cmd);
    out
}

/// A folder as one argument. A trailing backslash would escape the closing quote Windows puts
/// around it, so a drive root keeps a dot after it (`D:\.`).
pub fn dir_arg(dir: &Path) -> String {
    let mut d = dir.display().to_string();
    if d.ends_with('\\') {
        d.push('.');
    }
    d
}

/// `dir_arg`, quoted for a command line when it has blanks.
fn quoted_dir(dir: &Path) -> String {
    let d = dir_arg(dir);
    if d.contains([' ', '\t']) {
        format!("\"{d}\"")
    } else {
        d
    }
}

/// The custom command line split into its program and the rest (as typed), with `{dir}`
/// replaced in the rest. None when there is no program.
pub fn custom_parts(line: &str, dir: &Path) -> Option<(String, String)> {
    let line = line.trim();
    let (program, rest) = if let Some(after) = line.strip_prefix('"') {
        let end = after.find('"')?;
        (&after[..end], &after[end + 1..])
    } else {
        match line.find(char::is_whitespace) {
            Some(i) => (&line[..i], &line[i..]),
            None => (line, ""),
        }
    };
    if program.is_empty() {
        return None;
    }
    Some((program.to_owned(), rest.trim().replace(DIR_TOKEN, &quoted_dir(dir))))
}

/// Where Windows Terminal's app alias is, if it is there.
fn wt_alias() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Microsoft\\WindowsApps\\wt.exe");
    p.exists().then_some(p)
}

/// `shellman-terminal.exe` beside the running exe, if it is there.
pub fn shellman_exe() -> Option<PathBuf> {
    let p = std::env::current_exe().ok()?.parent()?.join("shellman-terminal.exe");
    p.is_file().then_some(p)
}

/// Open a terminal in `dir`, as the user chose. Says which one started, or why none would.
/// May block for a moment (the activation manager), so call it off the UI thread.
pub fn open_at(dir: &Path) -> Result<&'static str, String> {
    // The user asked for it: let it come to the front.
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(
            windows::Win32::UI::WindowsAndMessaging::ASFW_ANY,
        );
    }
    let mut last = String::from("nothing to try");
    for route in plan(&Choice::load(), shellman_exe(), wt_alias()) {
        match start(&route, dir) {
            Ok(how) => return Ok(how),
            Err(e) => last = e,
        }
    }
    Err(format!("no terminal would start: {last}"))
}

fn start(route: &Route, dir: &Path) -> Result<&'static str, String> {
    let d = dir_arg(dir);
    let spawned = |c: &mut Command, how: &'static str| c.current_dir(dir).spawn().map(|_| how).map_err(|e| format!("{how}: {e}"));
    match route {
        Route::Shellman(exe) => spawned(Command::new(exe).args(["-d", &d]), "shellman Terminal"),
        Route::WtAlias(exe) => spawned(Command::new(exe).args(["-d", &d]), "Windows Terminal (alias)"),
        Route::WtActivate => {
            let args = format!("-d \"{d}\"");
            if crate::apps_index::activate::activate_with(WINDOWS_TERMINAL, &args) {
                Ok("Windows Terminal (activation)")
            } else {
                Err("Windows Terminal is not installed".into())
            }
        }
        Route::Custom(line) => {
            let (program, rest) = custom_parts(line, dir).ok_or("the custom terminal command is empty")?;
            spawned(Command::new(program).raw_arg(rest), "custom terminal")
        }
        Route::Cmd => spawned(Command::new("cmd.exe").creation_flags(NEW_CONSOLE), "Command Prompt"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(which: Which, custom: &str) -> Choice {
        Choice { default_terminal: which, custom_terminal: custom.into() }
    }

    #[test]
    fn folders_are_one_argument() {
        assert_eq!(dir_arg(Path::new(r"C:\Users\a\Desktop")), r"C:\Users\a\Desktop");
        assert_eq!(dir_arg(Path::new(r"D:\")), r"D:\.");
    }

    #[test]
    fn shellman_first_then_windows_terminal_then_cmd() {
        let own = PathBuf::from(r"C:\p\shellman-terminal.exe");
        let wt = PathBuf::from(r"C:\a\wt.exe");
        let p = plan(&Choice::default(), Some(own.clone()), Some(wt.clone()));
        assert_eq!(p, [Route::Shellman(own.clone()), Route::WtAlias(wt.clone()), Route::WtActivate, Route::Cmd]);
        // Ours missing: Windows Terminal, then cmd.
        assert_eq!(plan(&Choice::default(), None, None), [Route::WtActivate, Route::Cmd]);
        let p = plan(&choice(Which::WindowsTerminal, ""), Some(own.clone()), Some(wt.clone()));
        assert_eq!(p, [Route::WtAlias(wt), Route::WtActivate, Route::Shellman(own.clone()), Route::Cmd]);
        // An empty custom command is skipped.
        assert_eq!(plan(&choice(Which::Custom, "  "), Some(own.clone()), None)[0], Route::Shellman(own.clone()));
        assert_eq!(plan(&choice(Which::Custom, " wezterm "), Some(own), None)[0], Route::Custom("wezterm".into()));
    }

    #[test]
    fn custom_command_lines() {
        let dir = Path::new(r"C:\My Files");
        assert_eq!(
            custom_parts(r#""C:\Tools\wez term.exe" start --cwd {dir}"#, dir),
            Some((r"C:\Tools\wez term.exe".into(), r#"start --cwd "C:\My Files""#.into()))
        );
        assert_eq!(custom_parts("alacritty", dir), Some(("alacritty".into(), String::new())));
        assert_eq!(custom_parts("x --dir {dir}", Path::new(r"D:\")), Some(("x".into(), r"--dir D:\.".into())));
        assert_eq!(custom_parts(r#""" a"#, dir), None);
        assert_eq!(custom_parts(r#""unterminated"#, dir), None);
    }

    #[test]
    fn the_choice_reads_from_json() {
        let c: Choice = serde_json::from_str(r#"{"default_terminal": "windows-terminal", "font_size": 14}"#).unwrap();
        assert_eq!(c.default_terminal, Which::WindowsTerminal);
        let c: Choice = serde_json::from_str(r#"{"default_terminal": "custom", "custom_terminal": "x"}"#).unwrap();
        assert_eq!(c, choice(Which::Custom, "x"));
        assert_eq!(serde_json::from_str::<Choice>("{}").unwrap(), Choice::default());
        assert_eq!(serde_json::to_value(Choice::default()).unwrap()["default_terminal"], "shellman");
    }
}
