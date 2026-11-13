//! The command line:
//!
//! ```text
//! shellman-terminal [-d|--dir <folder>] [-p|--profile <name>] [--hold] [-e <command line> | -- <command line>]
//! shellman-terminal <command line>                 (as before: runs it in the first tab)
//! shellman-terminal --register-default-terminal    (Windows' default terminal: see `defterm`)
//! shellman-terminal --unregister-default-terminal
//! shellman-terminal -Embedding                     (started by COM for a console handoff)
//! ```
//!
//! The command line after `-e` or `--` reaches CreateProcess exactly as typed (quotes and all),
//! so it is taken from the raw command line by offset, not re-joined from split arguments.
//! What a second launch hands to a running window (`instance`) is a [`Launch`], checked again
//! with [`Launch::check`] on the receiving side.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Longest command line Windows takes (in UTF-16 units; bytes here, which is stricter).
const MAX_COMMAND: usize = 32_000;
const MAX_DIR: usize = 32_000;
const MAX_PROFILE: usize = 256;

/// What a new window, or a tab handed to a running one, starts with.
#[derive(Clone, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Launch {
    /// The folder the first tab starts in (WSL gets it as `--cd`).
    pub dir: Option<PathBuf>,
    /// The profile to run, by name; None for the default.
    pub profile: Option<String>,
    /// A command line to run instead of the profile's shell.
    pub command: Option<String>,
    /// Keep the tab open after `command` exits.
    pub hold: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Mode {
    Window(Launch),
    /// COM started this to take a console handoff (`defterm`).
    Embedding,
    RegisterDefault,
    UnregisterDefault,
}

impl Launch {
    /// Within limits, with no NULs: what is accepted from the command line or from another
    /// process.
    pub fn check(&self) -> Result<(), String> {
        let bad = |s: &str| s.contains('\0');
        if let Some(d) = &self.dir {
            let s = d.to_string_lossy();
            if s.trim().is_empty() || s.len() > MAX_DIR || bad(&s) {
                return Err("the folder is not a usable path".into());
            }
        }
        if let Some(p) = &self.profile {
            if p.trim().is_empty() || p.len() > MAX_PROFILE || bad(p) {
                return Err("the profile name is not usable".into());
            }
        }
        if let Some(c) = &self.command {
            if c.trim().is_empty() || c.len() > MAX_COMMAND || bad(c) {
                return Err("the command line is empty or too long".into());
            }
        }
        if self.hold && self.command.is_none() {
            return Err("--hold needs a command (-e or --)".into());
        }
        Ok(())
    }
}

/// One argument, and where it started and ended in the raw line.
#[derive(Debug, PartialEq, Eq)]
struct Token {
    text: String,
    start: usize,
    end: usize,
}

/// Split `line` as Windows programs do (`CommandLineToArgvW`'s rules for everything after the
/// program: blanks separate, quotes group, `\"` is a quote, 2n backslashes before a quote are n,
/// `""` inside quotes is a quote), keeping each argument's position.
fn tokens(line: &str) -> Vec<Token> {
    let b: Vec<(usize, char)> = line.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i].1 == ' ' || b[i].1 == '\t') {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        let start = b[i].0;
        let mut text = String::new();
        let mut quoted = false;
        while i < b.len() {
            let c = b[i].1;
            if !quoted && (c == ' ' || c == '\t') {
                break;
            }
            if c == '\\' {
                let mut n = 0;
                while i < b.len() && b[i].1 == '\\' {
                    n += 1;
                    i += 1;
                }
                if i < b.len() && b[i].1 == '"' {
                    text.extend(std::iter::repeat_n('\\', n / 2));
                    if n % 2 == 1 {
                        text.push('"');
                        i += 1;
                    }
                } else {
                    text.extend(std::iter::repeat_n('\\', n));
                }
                continue;
            }
            if c == '"' {
                if quoted && i + 1 < b.len() && b[i + 1].1 == '"' {
                    text.push('"');
                    i += 2;
                    continue;
                }
                quoted = !quoted;
                i += 1;
                continue;
            }
            text.push(c);
            i += 1;
        }
        let end = if i < b.len() { b[i].0 } else { line.len() };
        out.push(Token { text, start, end });
    }
    out
}

/// Parse everything after the exe.
pub fn parse(line: &str) -> Result<Mode, String> {
    let toks = tokens(line);
    // COM adds `-Embedding` (once, or after one already in the registered line).
    if !toks.is_empty() && toks.iter().all(|t| t.text.eq_ignore_ascii_case("-embedding") || t.text.eq_ignore_ascii_case("/embedding")) {
        return Ok(Mode::Embedding);
    }
    if let [only] = toks.as_slice() {
        match only.text.as_str() {
            "--register-default-terminal" => return Ok(Mode::RegisterDefault),
            "--unregister-default-terminal" => return Ok(Mode::UnregisterDefault),
            _ => {}
        }
    }
    let mut launch = Launch::default();
    let mut i = 0;
    let value = |i: usize, name: &str| -> Result<String, String> {
        toks.get(i + 1).map(|t| t.text.clone()).ok_or_else(|| format!("{name} needs a value"))
    };
    while i < toks.len() {
        let t = &toks[i];
        let lower = t.text.to_ascii_lowercase();
        match lower.as_str() {
            "-d" | "--dir" | "--startingdirectory" => {
                launch.dir = Some(PathBuf::from(value(i, &t.text)?));
                i += 2;
            }
            "-p" | "--profile" => {
                launch.profile = Some(value(i, &t.text)?);
                i += 2;
            }
            "--hold" => {
                launch.hold = true;
                i += 1;
            }
            "-e" | "--command" | "--" => {
                let rest = line[t.end..].trim();
                if rest.is_empty() {
                    return Err(format!("{} needs a command line", t.text));
                }
                launch.command = Some(rest.to_owned());
                break;
            }
            // As before: a command line on its own (`shellman-terminal cmd /k ver`).
            _ if i == 0 && !t.text.starts_with('-') => {
                launch.command = Some(line[t.start..].trim().to_owned());
                break;
            }
            _ => return Err(format!("unknown argument: {}", t.text)),
        }
    }
    launch.check()?;
    Ok(Mode::Window(launch))
}

pub const USAGE: &str = "usage: shellman-terminal [-d <folder>] [-p <profile>] [--hold] [-e <command line> | -- <command line>]\n       shellman-terminal --register-default-terminal | --unregister-default-terminal";

#[cfg(test)]
mod tests {
    use super::*;

    fn window(line: &str) -> Launch {
        match parse(line) {
            Ok(Mode::Window(l)) => l,
            other => panic!("{line}: {other:?}"),
        }
    }

    #[test]
    fn splitting_follows_windows_rules() {
        let t: Vec<String> = tokens(r#"a "b c" d\"e "f""g" h\\"i j" \\k"#).into_iter().map(|t| t.text).collect();
        assert_eq!(t, ["a", "b c", "d\"e", "f\"g", r"h\i j", r"\\k"]);
        let t = tokens(r#"  -d   "C:\x y"  "#);
        assert_eq!((t[1].start, t[1].end), (7, 15));
    }

    #[test]
    fn nothing_is_the_default_shell() {
        assert_eq!(window(""), Launch::default());
        assert_eq!(window("   "), Launch::default());
    }

    #[test]
    fn folders_and_profiles() {
        let l = window(r#"-d "C:\My Files" --profile Debian"#);
        assert_eq!(l.dir, Some(PathBuf::from(r"C:\My Files")));
        assert_eq!(l.profile.as_deref(), Some("Debian"));
        assert_eq!(window(r"--dir D:\.").dir, Some(PathBuf::from(r"D:\.")));
        assert_eq!(window(r#"-p "Windows PowerShell""#).profile.as_deref(), Some("Windows PowerShell"));
    }

    #[test]
    fn commands_keep_their_quoting() {
        let l = window(r#"-d C:\x -- "C:\Program Files\Git\bin\bash.exe" -c "echo hi""#);
        assert_eq!(l.command.as_deref(), Some(r#""C:\Program Files\Git\bin\bash.exe" -c "echo hi""#));
        assert_eq!(l.dir, Some(PathBuf::from(r"C:\x")));
        let l = window(r#"--hold -e cmd /c "echo -d x""#);
        assert_eq!((l.command.as_deref(), l.hold), (Some(r#"cmd /c "echo -d x""#), true));
        // Options after the command are the command's.
        assert_eq!(window("-- ping -p 1").command.as_deref(), Some("ping -p 1"));
        // A bare command line, as stage 1 took it.
        assert_eq!(window(r"cmd /k ver").command.as_deref(), Some("cmd /k ver"));
    }

    #[test]
    fn bad_lines_are_refused() {
        for line in ["-d", "-p", "--", "-e   ", "--bogus", "--hold", "-p \"\""] {
            assert!(parse(line).is_err(), "{line}");
        }
        assert!(parse(&format!("-- {}", "x".repeat(MAX_COMMAND + 1))).is_err());
    }

    #[test]
    fn modes() {
        assert_eq!(parse("-Embedding"), Ok(Mode::Embedding));
        assert_eq!(parse("/Embedding"), Ok(Mode::Embedding));
        assert_eq!(parse("-Embedding -Embedding"), Ok(Mode::Embedding));
        assert_eq!(parse("--register-default-terminal"), Ok(Mode::RegisterDefault));
        assert_eq!(parse("--unregister-default-terminal"), Ok(Mode::UnregisterDefault));
        assert!(parse("-d x --register-default-terminal").is_err());
        assert!(parse("-Embedding -d x").is_err());
    }

    #[test]
    fn handed_over_launches_are_checked() {
        let ok: Launch = serde_json::from_str(r#"{"dir":"C:\\x","command":"cmd","hold":true}"#).unwrap();
        assert!(ok.check().is_ok());
        assert!(serde_json::from_str::<Launch>(r#"{"exe":"evil"}"#).is_err());
        let nul = Launch { command: Some("a\0b".into()), ..Default::default() };
        assert!(nul.check().is_err());
    }
}
