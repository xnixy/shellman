//! Command line: what was asked for, parsed without touching anything.

use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    Install,
    Update,
    Status,
    Uninstall,
    Version,
    Help,
}

#[derive(Debug, PartialEq, Eq, Default)]
pub struct Opts {
    pub from: Option<PathBuf>,
    pub prefix: Option<PathBuf>,
    pub start_menu: Option<PathBuf>,
    pub extras: Vec<PathBuf>,
    pub restart: bool,
    pub dry_run: bool,
    pub purge_config: bool,
    pub no_shortcuts: bool,
    /// install / update: make shellman the Windows shell from the next sign-in.
    pub shell: bool,
    /// install / update: make shellman Terminal the default terminal.
    pub default_terminal: bool,
    /// install / update: give apps' Open and Save dialogs shellman Files' picker.
    pub picker_hook: bool,
    /// uninstall: put the shell and default terminal back and close running apps, instead of
    /// refusing.
    pub revert: bool,
    /// uninstall: the command a RunOnce value runs at the next sign-in when shellman is the shell
    /// that runs now (the command line fills it in; the installer passes its own).
    pub finish_command: Option<String>,
    /// update: also create or repoint the Start Menu shortcuts (the installer window's update).
    pub shortcuts_on_update: bool,
}

pub fn parse(args: &[String]) -> Result<(Cmd, Opts), String> {
    let mut it = args.iter();
    let cmd = match it.next().map(String::as_str) {
        None | Some("help" | "--help" | "-h" | "/?") => return Ok((Cmd::Help, Opts::default())),
        Some("--version" | "-V" | "version") => return Ok((Cmd::Version, Opts::default())),
        Some("install") => Cmd::Install,
        Some("update") => Cmd::Update,
        Some("status") => Cmd::Status,
        Some("uninstall") => Cmd::Uninstall,
        Some(other) => return Err(format!("unknown command '{other}' (try: install, update, status, uninstall, --version)")),
    };
    let mut o = Opts::default();
    let value = |flag: &str, it: &mut std::slice::Iter<String>| -> Result<PathBuf, String> {
        it.next().filter(|v| !v.starts_with("--")).map(PathBuf::from).ok_or(format!("{flag} needs a value"))
    };
    while let Some(a) = it.next() {
        match a.as_str() {
            "--from" => o.from = Some(value("--from", &mut it)?),
            "--prefix" => o.prefix = Some(value("--prefix", &mut it)?),
            "--start-menu" => o.start_menu = Some(value("--start-menu", &mut it)?),
            "--extras" => o.extras.push(value("--extras", &mut it)?),
            "--restart" if cmd == Cmd::Update => o.restart = true,
            "--dry-run" if cmd != Cmd::Status => o.dry_run = true,
            "--purge-config" if cmd == Cmd::Uninstall => o.purge_config = true,
            "--no-shortcuts" if cmd == Cmd::Install => o.no_shortcuts = true,
            "--shell" if matches!(cmd, Cmd::Install | Cmd::Update) => o.shell = true,
            "--default-terminal" if matches!(cmd, Cmd::Install | Cmd::Update) => o.default_terminal = true,
            "--picker-hook" if matches!(cmd, Cmd::Install | Cmd::Update) => o.picker_hook = true,
            "--revert" if cmd == Cmd::Uninstall => o.revert = true,
            other => return Err(format!("unexpected argument '{other}' for this command")),
        }
    }
    Ok((cmd, o))
}

pub const HELP: &str = "shellman-setup: install, update and remove shellman.

  install   [--from <dir>] [--extras <dir>]... [--no-shortcuts] [--dry-run] [integrations]
            first install into %LOCALAPPDATA%\\Programs\\shellman from a folder of built files
            (default: the folder this tool is in); creates the Start Menu shortcuts and the Apps
            & features entry. Without --shell it does NOT make shellman the Windows shell.
  update    [--from <dir>] [--extras <dir>]... [--restart] [--dry-run] [integrations]
            copies the files that differ; a running exe is renamed aside, and takes effect when
            its app restarts. --restart asks the session to restart the supervised apps it
            replaced (the session itself: next sign-in; on-demand apps are never closed).
  status    [--from <dir>]
            installed build per file against the source folder, old images still running, whether
            shellman is the shell, default terminal state.
  uninstall [--revert] [--purge-config] [--dry-run]
            removes what install put there, the shortcuts, the dialog hook, the sign-in font check
            and the Apps & features entry. Refuses while shellman is the shell or the default
            terminal, or while its apps run, unless --revert: then it puts the shell and terminal
            back and closes the apps (if shellman is the shell running now, its files go at the
            next sign-in instead).
  integrations (install, update; all off unless given):
            --shell             shellman is the Windows shell from the next sign-in
            --default-terminal  shellman Terminal is the default terminal (needs Windows Terminal)
            --picker-hook       apps' Open and Save dialogs use shellman Files
  --version

Everywhere: --prefix <dir> (install folder), --start-menu <dir> (shortcut folder).
Environment: SHELLMAN_SETUP_PREFIX, SHELLMAN_SETUP_START_MENU, SHELLMAN_SETUP_CONFIG,
SHELLMAN_SETUP_DATA, SHELLMAN_INSTANCE (test session), SHELLMAN_SETUP_REGISTRY (a file that
stands in for HKCU for every registry read and write), SHELLMAN_SETUP_SHELL and
SHELLMAN_SETUP_TERMINAL (read-only stand-ins for the two values, `none` for absent),
SHELLMAN_SETUP_EXPLORER=none (as if Explorer had no taskbar).";

#[cfg(test)]
mod tests {
    use super::*;

    fn a(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn commands_and_flags() {
        let (c, o) = parse(&a(&["update", "--from", "target\\release", "--restart", "--prefix", "C:\\t"])).unwrap();
        assert_eq!(c, Cmd::Update);
        assert_eq!(o.from, Some(PathBuf::from("target\\release")));
        assert_eq!(o.prefix, Some(PathBuf::from("C:\\t")));
        assert!(o.restart && !o.dry_run);
        let (c, o) = parse(&a(&["install", "--extras", "a", "--extras", "b", "--dry-run"])).unwrap();
        assert_eq!((c, o.extras.len(), o.dry_run), (Cmd::Install, 2, true));
        assert_eq!(parse(&a(&["--version"])).unwrap().0, Cmd::Version);
        assert_eq!(parse(&a(&[])).unwrap().0, Cmd::Help);
        assert!(parse(&a(&["uninstall", "--purge-config"])).unwrap().1.purge_config);
        let (_, o) = parse(&a(&["install", "--shell", "--default-terminal", "--picker-hook"])).unwrap();
        assert!(o.shell && o.default_terminal && o.picker_hook);
        assert!(parse(&a(&["uninstall", "--revert"])).unwrap().1.revert);
    }

    #[test]
    fn mistakes_are_refused() {
        for bad in [
            &["frobnicate"][..],
            &["install", "--restart"],
            &["update", "--purge-config"],
            &["status", "--dry-run"],
            &["update", "--from"],
            &["update", "--from", "--restart"],
            &["install", "stray"],
            &["uninstall", "--shell"],
            &["install", "--revert"],
        ] {
            assert!(parse(&a(bad)).is_err(), "{bad:?}");
        }
    }
}
