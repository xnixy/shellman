//! What an install hooks into Windows beyond its files, and how each is undone. All per user
//! (HKCU), all through [`Registry`] so the stand-in registry covers them:
//!
//! - the Apps & features entry (`...\CurrentVersion\Uninstall\shellman`), written by every install
//!   and update, removed last by uninstall;
//! - shellman as the Windows shell (Winlogon `Shell`): the session's own `--install` /
//!   `--uninstall` do it on the real registry (they save and restore what was there); the stand-in
//!   gets the same values written here. If the session's exe is gone, [`revert_shell`] restores
//!   from the session's saved value itself;
//! - shellman Terminal as the default terminal: `shellman-terminal --register-default-terminal` /
//!   `--unregister-default-terminal`, likewise;
//! - the Open/Save dialog hook: the per-user CLSIDs `apps/pickhook/register.ps1` writes, done
//!   here instead (same keys, same rule: never over a registration that is not ours);
//! - Settings' "shellman font check" Run value, which only uninstall removes;
//! - a RunOnce value that finishes an uninstall at the next sign-in, when shellman was the running
//!   shell and its files are still in use.

use crate::reg::Registry;
use crate::state;
use std::path::Path;

pub const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\shellman";
pub const WINLOGON: &str = r"Software\Microsoft\Windows NT\CurrentVersion\Winlogon";
const SESSION_SAVED: &str = r"Software\shellman\session";
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const RUNONCE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\RunOnce";
pub const FONT_CHECK_VALUE: &str = "shellman font check";
pub const FINISH_VALUE: &str = "shellman uninstall";
pub const STARTUP_KEY: &str = r"Console\%%Startup";
const TERMINAL_SAVED: &str = r"Software\shellman\Terminal";
/// "Let Windows decide" for the delegation values.
const DEFAULT_PAIR: &str = "{00000000-0000-0000-0000-000000000000}";
/// Windows Terminal's OpenConsole, the console host shellman Terminal hands to.
const WT_CONSOLE: &str = "{2EACA947-7F5F-4CFA-BA87-8F7FBEEFBE69}";
pub const HOOK_DLL: &str = "shellman_pickhook.dll";
/// CLSID_FileOpenDialog and CLSID_FileSaveDialog, as `register.ps1` names them.
pub const HOOK_CLASSES: [(&str, &str); 2] = [
    ("{DC1C5A9C-E88A-4dde-A5A1-60F82A20AEF7}", "File Open Dialog (shellman picker hook)"),
    ("{C0B4E2F3-BA21-4773-8DBA-335EC946EB8B}", "File Save Dialog (shellman picker hook)"),
];
pub const INSTALLER_EXE: &str = "shellman-installer.exe";

fn quoted(p: &Path) -> String {
    format!("\"{}\"", p.display())
}

/// The version Apps & features shows: the stamp's first word (`0.1.0+g1234abc`).
pub fn display_version(stamp: &str) -> String {
    stamp.split_whitespace().next().unwrap_or(stamp).to_string()
}

/// The uninstall command for an install: the installer stub when it is there, else the console
/// tool.
pub fn uninstall_command(prefix: &Path, quiet: bool) -> String {
    let stub = prefix.join(INSTALLER_EXE);
    let p = quoted(prefix);
    if stub.is_file() {
        format!("{} --uninstall --prefix {p}{}", quoted(&stub), if quiet { " --quiet" } else { "" })
    } else {
        format!("{} uninstall --revert --prefix {p}", quoted(&prefix.join("shellman-setup.exe")))
    }
}

/// Write (or refresh) the Apps & features entry.
pub fn register_app(reg: &dyn Registry, prefix: &Path, stamp: &str, size_kb: u32, today: &str) -> Result<(), String> {
    let k = UNINSTALL_KEY;
    let icon = ["shellman-settings.exe", INSTALLER_EXE, "shellman-session.exe"].iter().map(|e| prefix.join(e)).find(|p| p.is_file());
    reg.set(k, "DisplayName", "shellman")?;
    reg.set(k, "DisplayVersion", &display_version(stamp))?;
    reg.set(k, "Publisher", "shellman")?;
    reg.set(k, "Comments", "A replacement for the Windows shell in one visual style")?;
    reg.set(k, "InstallLocation", &prefix.display().to_string())?;
    if let Some(icon) = icon {
        reg.set(k, "DisplayIcon", &format!("{},0", icon.display()))?;
    }
    reg.set(k, "UninstallString", &uninstall_command(prefix, false))?;
    if prefix.join(INSTALLER_EXE).is_file() {
        reg.set(k, "QuietUninstallString", &uninstall_command(prefix, true))?;
    } else {
        reg.delete_value(k, "QuietUninstallString")?;
    }
    reg.set(k, "InstallDate", today)?;
    reg.set_dword(k, "EstimatedSize", size_kb)?;
    reg.set_dword(k, "NoModify", 1)?;
    reg.set_dword(k, "NoRepair", 1)?;
    Ok(())
}

pub fn unregister_app(reg: &dyn Registry) -> Result<(), String> {
    reg.delete_tree(UNINSTALL_KEY)
}

pub fn app_registered(reg: &dyn Registry) -> Option<String> {
    reg.get(UNINSTALL_KEY, "InstallLocation")
}

// ---- the picker hook ----

fn hook_key(clsid: &str) -> String {
    format!(r"Software\Classes\CLSID\{clsid}")
}

/// Whether a registration's server is the hook dll.
fn is_hook(server: &str) -> bool {
    server.trim().trim_matches('"').to_ascii_lowercase().ends_with(HOOK_DLL)
}

/// What each dialog class is registered as, per user: `None` (Windows' own), or the server path.
pub fn hook_state(reg: &dyn Registry) -> Vec<Option<String>> {
    HOOK_CLASSES
        .iter()
        .map(|(c, _)| {
            let k = hook_key(c);
            reg.key_exists(&k).then(|| reg.get(&format!(r"{k}\InprocServer32"), "").unwrap_or_default())
        })
        .collect()
}

pub fn hook_registered(reg: &dyn Registry) -> bool {
    hook_state(reg).iter().any(|s| s.as_deref().is_some_and(is_hook))
}

/// Point both dialog classes at `prefix`'s hook dll. Refuses, changing nothing, if either class
/// has a per-user registration that is not the hook.
pub fn register_hook(reg: &dyn Registry, prefix: &Path) -> Result<(), String> {
    let dll = prefix.join(HOOK_DLL);
    if !dll.is_file() {
        return Err(format!("{} is not installed", dll.display()));
    }
    if !prefix.join("shellman-files.exe").is_file() {
        return Err("the picker (shellman-files.exe) is not installed beside the hook".into());
    }
    for ((c, _), now) in HOOK_CLASSES.iter().zip(hook_state(reg)) {
        if let Some(s) = now.filter(|s| !is_hook(s)) {
            return Err(format!("HKCU\\{} is already registered to {s:?}; not touching it", hook_key(c)));
        }
    }
    for (c, name) in HOOK_CLASSES {
        let k = hook_key(c);
        reg.set(&k, "", name)?;
        reg.set(&format!(r"{k}\InprocServer32"), "", &dll.display().to_string())?;
        reg.set(&format!(r"{k}\InprocServer32"), "ThreadingModel", "Apartment")?;
    }
    Ok(())
}

/// Remove the hook's registrations (only those that are the hook). Returns how many went.
pub fn unregister_hook(reg: &dyn Registry) -> Result<usize, String> {
    let mut n = 0;
    for ((c, _), now) in HOOK_CLASSES.iter().zip(hook_state(reg)) {
        if now.as_deref().is_some_and(is_hook) {
            reg.delete_tree(&hook_key(c))?;
            n += 1;
        }
    }
    Ok(n)
}

// ---- Run values ----

pub fn font_check_on(reg: &dyn Registry) -> bool {
    reg.get(RUN_KEY, FONT_CHECK_VALUE).is_some()
}

pub fn remove_font_check(reg: &dyn Registry) -> Result<bool, String> {
    let had = font_check_on(reg);
    reg.delete_value(RUN_KEY, FONT_CHECK_VALUE)?;
    Ok(had)
}

/// The most a Run / RunOnce command line may be (Windows ignores longer ones).
pub const RUN_MAX: usize = 260;

/// Have Windows run `command` once at the next sign-in (to finish an uninstall). `command`
/// starts with a quoted exe path; when it is too long for RunOnce, it goes into `finish.cmd`
/// beside that exe and RunOnce runs the script.
pub fn finish_at_sign_in(reg: &dyn Registry, command: &str) -> Result<(), String> {
    reg.set(RUNONCE_KEY, FINISH_VALUE, &run_once_value(command)?)
}

pub fn run_once_value(command: &str) -> Result<String, String> {
    if command.len() < RUN_MAX {
        return Ok(command.to_string());
    }
    let exe = command.strip_prefix('"').and_then(|r| r.split('"').next()).ok_or("the finish command has no quoted program")?;
    let script = Path::new(exe).with_file_name("finish.cmd");
    std::fs::write(&script, format!("@{command}\r\n")).map_err(|e| format!("cannot write {}: {e}", script.display()))?;
    let v = quoted(&script);
    if v.len() >= RUN_MAX {
        return Err(format!("the path {} is too long for a RunOnce value", script.display()));
    }
    Ok(v)
}

pub fn cancel_finish(reg: &dyn Registry) -> Result<(), String> {
    reg.delete_value(RUNONCE_KEY, FINISH_VALUE)
}

// ---- the shell ----

pub fn shell_value(reg: &dyn Registry) -> Option<String> {
    reg.get(WINLOGON, "Shell")
}

pub fn shell_is_shellman(reg: &dyn Registry) -> bool {
    shell_value(reg).is_some_and(|v| state::shell_is_shellman(&v))
}

/// Run one of the installed apps with a flag, its output captured; `Ok(output)` on exit code 0.
fn run_app(exe: &Path, arg: &str) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new(exe)
        .arg(arg)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("cannot run {}: {e}", exe.display()))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)).trim().to_string();
    if out.status.success() {
        Ok(text)
    } else {
        Err(if text.is_empty() { format!("{} {arg} failed", exe.display()) } else { text })
    }
}

/// Make shellman the shell from the next sign-in.
pub fn make_shell(reg: &dyn Registry, prefix: &Path) -> Result<(), String> {
    let session = prefix.join("shellman-session.exe");
    if reg.is_real() {
        run_app(&session, "--install")?;
    } else {
        // The stand-in: what `swap::install` writes.
        if !shell_is_shellman(reg) {
            let now = shell_value(reg);
            reg.set(SESSION_SAVED, "PreviousShell", now.as_deref().unwrap_or(""))?;
            reg.set(SESSION_SAVED, "HadPreviousShell", if now.is_some() { "1" } else { "0" })?;
        }
        let p = session.display().to_string();
        reg.set(WINLOGON, "Shell", &if p.contains(' ') { format!("\"{p}\"") } else { p })?;
    }
    if !shell_is_shellman(reg) {
        return Err("the Winlogon Shell value is not shellman's after `shellman-session --install`".into());
    }
    Ok(())
}

/// Put back the shell that was there before shellman (usually Explorer). Does nothing when the
/// shell is not shellman's.
pub fn revert_shell(reg: &dyn Registry, prefix: &Path) -> Result<bool, String> {
    if !shell_is_shellman(reg) {
        return Ok(false);
    }
    let session = prefix.join("shellman-session.exe");
    if reg.is_real() && session.is_file() {
        run_app(&session, "--uninstall")?;
    }
    if shell_is_shellman(reg) {
        // The stand-in, or no session exe to ask: what `swap::uninstall` does.
        let had = reg.get(SESSION_SAVED, "HadPreviousShell").is_some_and(|v| v == "1");
        let previous = reg.get(SESSION_SAVED, "PreviousShell").unwrap_or_default();
        if had && !previous.trim().is_empty() && !state::shell_is_shellman(&previous) {
            reg.set(WINLOGON, "Shell", &previous)?;
        } else {
            reg.delete_value(WINLOGON, "Shell")?;
        }
        reg.delete_value(SESSION_SAVED, "PreviousShell")?;
        reg.delete_value(SESSION_SAVED, "HadPreviousShell")?;
    }
    if shell_is_shellman(reg) {
        return Err("the Winlogon Shell value is still shellman's".into());
    }
    Ok(true)
}

// ---- the default terminal ----

pub fn terminal_value(reg: &dyn Registry) -> Option<String> {
    reg.get(STARTUP_KEY, "DelegationTerminal")
}

pub fn terminal_is_shellman(reg: &dyn Registry) -> bool {
    state::terminal_is_shellman(terminal_value(reg).as_deref())
}

fn terminal_class_key() -> String {
    format!(r"Software\Classes\CLSID\{}", state::TERMINAL_CLSID)
}

pub fn make_default_terminal(reg: &dyn Registry, prefix: &Path) -> Result<(), String> {
    let exe = prefix.join("shellman-terminal.exe");
    if reg.is_real() {
        run_app(&exe, "--register-default-terminal")?;
    } else {
        // The stand-in: what `defterm::register_plan` writes.
        if reg.get(TERMINAL_SAVED, "SavedDelegationTerminal").is_none() {
            let (c, t) = if terminal_is_shellman(reg) {
                (DEFAULT_PAIR.to_string(), DEFAULT_PAIR.to_string())
            } else {
                (
                    reg.get(STARTUP_KEY, "DelegationConsole").unwrap_or_else(|| DEFAULT_PAIR.into()),
                    terminal_value(reg).unwrap_or_else(|| DEFAULT_PAIR.into()),
                )
            };
            reg.set(TERMINAL_SAVED, "SavedDelegationConsole", &c)?;
            reg.set(TERMINAL_SAVED, "SavedDelegationTerminal", &t)?;
        }
        let k = terminal_class_key();
        reg.set(&k, "", "shellman Terminal (console handoff)")?;
        reg.set(&format!(r"{k}\LocalServer32"), "", &quoted(&exe))?;
        reg.set(STARTUP_KEY, "DelegationConsole", WT_CONSOLE)?;
        reg.set(STARTUP_KEY, "DelegationTerminal", state::TERMINAL_CLSID)?;
    }
    if !terminal_is_shellman(reg) {
        return Err("shellman Terminal is not the default terminal after registering it".into());
    }
    Ok(())
}

/// Put back the terminal that was the default before, and remove shellman Terminal's handoff
/// class. Returns whether it was the default.
pub fn revert_terminal(reg: &dyn Registry, prefix: &Path) -> Result<bool, String> {
    let was = terminal_is_shellman(reg);
    let exe = prefix.join("shellman-terminal.exe");
    if reg.is_real() && exe.is_file() && (was || reg.key_exists(&terminal_class_key())) {
        run_app(&exe, "--unregister-default-terminal")?;
    }
    if terminal_is_shellman(reg) {
        // The stand-in, or no terminal exe to ask: what `defterm::unregister_plan` does.
        let c = reg.get(TERMINAL_SAVED, "SavedDelegationConsole").unwrap_or_else(|| DEFAULT_PAIR.into());
        let t = reg.get(TERMINAL_SAVED, "SavedDelegationTerminal").filter(|t| !state::terminal_is_shellman(Some(t)));
        let (c, t) = match t {
            Some(t) => (c, t),
            None => (DEFAULT_PAIR.into(), DEFAULT_PAIR.into()),
        };
        reg.set(STARTUP_KEY, "DelegationConsole", &c)?;
        reg.set(STARTUP_KEY, "DelegationTerminal", &t)?;
    }
    reg.delete_value(TERMINAL_SAVED, "SavedDelegationConsole")?;
    reg.delete_value(TERMINAL_SAVED, "SavedDelegationTerminal")?;
    reg.delete_tree(&terminal_class_key())?;
    if terminal_is_shellman(reg) {
        return Err("shellman Terminal is still the default terminal".into());
    }
    Ok(was)
}

/// `HKCU\Software\shellman` once nothing is left under it (the saved values above live there).
pub fn tidy_saved_values(reg: &dyn Registry) {
    for k in [SESSION_SAVED, TERMINAL_SAVED] {
        if reg.key_exists(k) && reg.get(k, "PreviousShell").is_none() && reg.get(k, "SavedDelegationTerminal").is_none() {
            let _ = reg.delete_tree(k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reg::FileReg;
    use std::path::PathBuf;

    fn tmp_prefix(name: &str, files: &[&str]) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shellman-integrate-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for f in files {
            std::fs::write(d.join(f), b"x").unwrap();
        }
        d
    }

    #[test]
    fn a_long_finish_command_goes_into_a_script() {
        let short = r#""C:\t\x.exe" --uninstall"#;
        assert_eq!(run_once_value(short).unwrap(), short);
        let dir = tmp_prefix("finish", &[]);
        let long = format!("\"{}\" --uninstall --prefix \"{}\"", dir.join("x.exe").display(), "p".repeat(300));
        let v = run_once_value(&long).unwrap();
        assert!(v.len() < RUN_MAX && v.ends_with("finish.cmd\""), "{v}");
        assert!(std::fs::read_to_string(dir.join("finish.cmd")).unwrap().contains("--uninstall"));
        assert!(run_once_value(&"x".repeat(300)).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_shell_goes_in_and_comes_back_out() {
        let reg = FileReg::memory();
        let p = PathBuf::from(r"C:\x\shellman");
        reg.set(WINLOGON, "Shell", "other.exe").unwrap();
        make_shell(&reg, &p).unwrap();
        assert!(shell_is_shellman(&reg));
        make_shell(&reg, &p).unwrap();
        assert!(revert_shell(&reg, &p).unwrap());
        assert_eq!(shell_value(&reg).as_deref(), Some("other.exe"), "the earlier value is back");
        assert!(!revert_shell(&reg, &p).unwrap(), "nothing to do the second time");
        let reg = FileReg::memory();
        make_shell(&reg, &p).unwrap();
        revert_shell(&reg, &p).unwrap();
        assert_eq!(shell_value(&reg), None, "no value before means none after");
    }

    #[test]
    fn the_terminal_goes_in_and_comes_back_out() {
        let reg = FileReg::memory();
        let p = PathBuf::from(r"C:\x\shellman");
        reg.set(STARTUP_KEY, "DelegationConsole", "{C}").unwrap();
        reg.set(STARTUP_KEY, "DelegationTerminal", "{T}").unwrap();
        make_default_terminal(&reg, &p).unwrap();
        assert!(terminal_is_shellman(&reg));
        assert!(revert_terminal(&reg, &p).unwrap());
        assert_eq!(terminal_value(&reg).as_deref(), Some("{T}"));
        assert_eq!(reg.get(STARTUP_KEY, "DelegationConsole").as_deref(), Some("{C}"));
        assert!(!reg.key_exists(&terminal_class_key()));
    }

    #[test]
    fn the_hook_never_replaces_a_registration_that_is_not_ours() {
        let p = tmp_prefix("hook", &[HOOK_DLL, "shellman-files.exe"]);
        let reg = FileReg::memory();
        register_hook(&reg, &p).unwrap();
        assert!(hook_registered(&reg));
        register_hook(&reg, &p).unwrap();
        assert_eq!(unregister_hook(&reg).unwrap(), 2);
        assert!(!hook_registered(&reg));
        let other = hook_key(HOOK_CLASSES[1].0);
        reg.set(&format!(r"{other}\InprocServer32"), "", r"C:\other\thing.dll").unwrap();
        assert!(register_hook(&reg, &p).is_err());
        assert!(!hook_registered(&reg), "a refusal changes nothing");
        assert_eq!(unregister_hook(&reg).unwrap(), 0);
        assert!(reg.key_exists(&other), "someone else's registration stays");
        let _ = std::fs::remove_dir_all(p);
    }

    #[test]
    fn the_apps_entry_points_at_the_stub_when_it_is_installed() {
        let p = tmp_prefix("entry", &[INSTALLER_EXE, "shellman-settings.exe"]);
        let reg = FileReg::memory();
        register_app(&reg, &p, "0.1.0+gabc 2026-11-15", 1234, "20261115").unwrap();
        let u = reg.get(UNINSTALL_KEY, "UninstallString").unwrap();
        assert!(u.contains(INSTALLER_EXE) && u.contains("--uninstall") && u.contains("--prefix"), "{u}");
        assert!(reg.get(UNINSTALL_KEY, "QuietUninstallString").unwrap().ends_with("--quiet"));
        assert_eq!(reg.get(UNINSTALL_KEY, "DisplayVersion").as_deref(), Some("0.1.0+gabc"));
        assert_eq!(app_registered(&reg).as_deref(), Some(p.display().to_string().as_str()));
        unregister_app(&reg).unwrap();
        assert!(!reg.key_exists(UNINSTALL_KEY));
        std::fs::remove_file(p.join(INSTALLER_EXE)).unwrap();
        assert!(uninstall_command(&p, false).contains("shellman-setup.exe\" uninstall --revert"));
        let _ = std::fs::remove_dir_all(p);
    }
}
