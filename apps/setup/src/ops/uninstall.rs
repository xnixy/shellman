//! `uninstall`: undo everything an install did. In order: the integrations that only need the
//! registry (dialog hook, font check), then, with `--revert`, the default terminal and the shell
//! (while their exes are still there to do it); then the running apps are closed, the files,
//! shortcuts and (with `--purge-config`) settings removed, and the Apps & features entry last.
//!
//! When shellman is the shell that runs right now, nothing is closed: the registry is put back,
//! and a RunOnce value runs `--finish-command` at the next sign-in (under Explorer) to remove the
//! files. Without `--revert` it refuses while shellman is the shell or the default terminal, or
//! while anything runs from the install folder, as before.

use super::*;
use crate::{shortcuts, stop};

/// What an uninstall would do, for the installer window to show before it starts.
#[derive(Debug, Default)]
pub struct UninstallPlan {
    pub recorded: bool,
    pub files: Vec<String>,
    pub shell: bool,
    pub live_shell: bool,
    pub terminal: bool,
    pub hook: bool,
    pub font_check: bool,
    pub listed: bool,
    pub running: Vec<(u32, String)>,
}

/// The installed files: the record's, else what install would have put there.
fn installed_files(env: &Env) -> (bool, Vec<String>) {
    if let Some(r) = read_record(env) {
        return (true, r.files.iter().cloned().collect());
    }
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&env.prefix) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            let l = n.to_ascii_lowercase();
            if !manifest::is_aside(&n) && ((l.starts_with("shellman-") && l.ends_with(".exe")) || l == integrate::HOOK_DLL) {
                v.push(n);
            }
        }
    }
    (false, v)
}

pub fn plan_uninstall(env: &Env) -> UninstallPlan {
    let (recorded, files) = installed_files(env);
    let procs = sys::processes();
    let reg = &*env.reg;
    UninstallPlan {
        recorded,
        files,
        shell: env.shell_is_shellman(),
        live_shell: stop::shellman_is_live_shell(&procs, &env.prefix),
        terminal: env.terminal_is_shellman(),
        hook: integrate::hook_registered(reg),
        font_check: integrate::font_check_on(reg),
        listed: integrate::app_registered(reg).is_some_and(|p| same_dir(Path::new(&p), &env.prefix)),
        running: stop::running_from(&procs, &env.prefix),
    }
}

pub fn uninstall(o: &Opts, env: &Env) -> Result<(), String> {
    let p = plan_uninstall(env);
    if !o.revert {
        if let Some(v) = env.shell().filter(|v| state::shell_is_shellman(v)) {
            return Err(format!(
                "shellman is the Windows shell ({v}). Refusing to remove it: run `shellman-session --uninstall` first, sign out and in, then run this again (or give --revert)"
            ));
        }
        if p.terminal {
            return Err("shellman Terminal is the default terminal. Run `shellman-terminal --unregister-default-terminal` first, then run this again (or give --revert)".into());
        }
    }
    if !env.prefix.is_dir() && !p.listed {
        return Err(format!("nothing is installed in {}", env.prefix.display()));
    }
    let busy: Vec<&str> = p.running.iter().map(|(_, i)| i.as_str()).collect();
    if !busy.is_empty() && !o.revert {
        return Err(format!(
            "still running from the install folder: {}. Close them (the session ends them at sign-out) and run this again (or give --revert to close them)",
            busy.join(", ")
        ));
    }
    let dry = o.dry_run;
    // Known before anything changes: a live shell needs a way to finish at the next sign-in.
    if p.live_shell && !dry && o.finish_command.is_none() {
        return Err("shellman is the shell that runs now, and no command was given to finish at the next sign-in".into());
    }
    say!("shellman-setup {} uninstalling {}", shellman_buildinfo::STAMP, env.prefix.display());
    if !p.recorded {
        say!("  (no install record: removing shellman-*.exe and shellman_pickhook.dll only)");
    }
    let reg = &*env.reg;
    let _lock = if dry { None } else { Some(sys::lock(&env.prefix)?) };

    // 1. What only needs the registry.
    if p.hook {
        if dry {
            say!("  would give apps Windows' own Open and Save dialogs back");
        } else {
            let n = integrate::unregister_hook(reg)?;
            say!("  removed the Open/Save dialog hook ({n} registrations; apps get Windows' dialogs from their next start)");
        }
    }
    if p.font_check {
        if dry {
            say!("  would remove the sign-in font check");
        } else {
            integrate::remove_font_check(reg)?;
            say!("  removed the sign-in font check");
        }
    }
    // 2. The terminal and the shell, while their exes are still there to put things back.
    if o.revert {
        if p.terminal || reg.key_exists(&format!(r"Software\Classes\CLSID\{}", state::TERMINAL_CLSID)) {
            if dry {
                say!("  would put back the default terminal that was there before shellman's");
            } else if integrate::revert_terminal(reg, &env.prefix)? {
                say!("  put back the default terminal that was there before shellman's");
            }
        }
        if p.shell {
            if dry {
                say!("  would make Explorer (or what was there before) the shell again");
            } else if integrate::revert_shell(reg, &env.prefix)? {
                say!("  Explorer (or what was there before) is the shell again from the next sign-in");
            }
        }
    }
    // 3. The running apps.
    if p.live_shell {
        remove_shortcuts(env, dry)?;
        if dry {
            say!("  shellman is the shell that runs now: would leave it running and remove its files at the next sign-in");
            say!("dry run: nothing was changed.");
        } else if let Some(cmd) = &o.finish_command {
            integrate::finish_at_sign_in(reg, cmd)?;
            say!("shellman is the shell that runs now, so it was left running.");
            say!("Sign out and back in: Explorer starts, and shellman's files are removed then.");
        }
        return Ok(());
    }
    if !p.running.is_empty() {
        if dry {
            say!("  would close: {}", busy.join(", "));
        } else {
            let left = stop::close(&p.running);
            if !left.is_empty() {
                return Err(format!("could not close {}; close them and run this again", left.iter().map(|(_, i)| i.as_str()).collect::<Vec<_>>().join(", ")));
            }
            say!("  closed {} running shellman process(es)", p.running.len());
        }
    }
    // 4. The files.
    let root = env.root();
    let mut dirs = std::collections::BTreeSet::new();
    let mut left = Vec::new();
    for rel in &p.files {
        let path = root.join(rel)?;
        root.guard(&path)?;
        if let Some(d) = path.parent() {
            dirs.insert(d.to_path_buf());
        }
        if dry {
            say!("  would remove {}", path.display());
        } else if path.exists() {
            match remove_or_move(&path) {
                Ok(None) => say!("  removed {}", path.display()),
                Ok(Some(to)) => say!("  moved {} (in use) to {}", rel, to.display()),
                Err(e) => left.push(format!("{rel}: {e}")),
            }
        }
    }
    if !dry {
        let n = fsops::sweep_asides(&root, &p.files);
        if n > 0 {
            say!("  removed {n} old renamed-aside file(s)");
        }
        if left.is_empty() {
            let rec = root.join(state::RECORD)?;
            if rec.exists() {
                root.guard(&rec)?;
                let _ = std::fs::remove_file(&rec);
            }
        }
        // Folders we emptied, deepest first, then the install folder if nothing else is in it.
        let mut dirs: Vec<_> = dirs.into_iter().filter(|d| d != &env.prefix).collect();
        dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
        for d in dirs {
            if root.guard(&d.join("x")).is_ok() {
                let _ = std::fs::remove_dir(&d);
            }
        }
        match std::fs::remove_dir(&env.prefix) {
            Ok(()) => say!("  removed the install folder"),
            Err(_) if env.prefix.is_dir() => say!("  left {} (it holds files that are not shellman's, such as logs)", env.prefix.display()),
            Err(_) => {}
        }
    }
    remove_shortcuts(env, dry)?;
    if o.purge_config {
        for d in &env.config_dirs {
            purge(d, dry)?;
        }
        if let Some(r) = &env.state_root {
            for id in STATE_IDS {
                purge(&r.join(id), dry)?;
            }
        }
    } else {
        say!("  your settings were kept (--purge-config removes them)");
    }
    // 5. The Apps & features entry, once nothing is left that it could uninstall.
    if dry {
        if p.listed {
            say!("  would remove shellman from Apps & features");
        }
    } else if left.is_empty() {
        integrate::unregister_app(reg)?;
        integrate::cancel_finish(reg)?;
        integrate::tidy_saved_values(reg);
        if p.listed {
            say!("  removed shellman from Apps & features");
        }
    }
    if !left.is_empty() {
        for l in &left {
            say!("  could not remove {l}");
        }
        return Err(format!("{} file(s) could not be removed; close what uses them and uninstall again", left.len()));
    }
    say!("{}", if dry { "dry run: nothing was changed." } else { "uninstalled." });
    Ok(())
}

/// Delete a file; one in use (a running exe, the dialog hook loaded in some app) is moved to a
/// folder in `%TEMP%` instead, which works on the same volume. `Ok(Some(to))` when moved.
fn remove_or_move(path: &Path) -> Result<Option<PathBuf>, String> {
    let Err(e) = std::fs::remove_file(path) else { return Ok(None) };
    let dir = std::env::temp_dir().join(format!("shellman-leftovers-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let to = dir.join(path.file_name().unwrap_or_default());
    std::fs::rename(path, &to).map(|_| Some(to)).map_err(|_| e.to_string())
}

fn remove_shortcuts(env: &Env, dry: bool) -> Result<(), String> {
    // Only ours, only by name, only in the Start Menu folder.
    let menu = Root::new(env.start_menu.clone());
    for name in shortcuts::our_files() {
        let p = menu.join(&name)?;
        if p.is_file() {
            if dry {
                say!("  would remove shortcut {}", p.display());
            } else {
                menu.guard(&p)?;
                let _ = std::fs::remove_file(&p);
                say!("  removed shortcut {}", p.display());
            }
        }
    }
    if !dry {
        let _ = std::fs::remove_dir(&env.start_menu);
    }
    Ok(())
}

/// The folders eframe keeps the apps' window state in, under `%APPDATA%` (their app ids).
pub const STATE_IDS: [&str; 10] = [
    "shellman-bar",
    "shellman-control",
    "shellman-files",
    "shellman-installer",
    "shellman-launcher",
    "shellman-settings",
    "shellman-shelf",
    "shellman-switcher",
    "shellman-tasks",
    "shellman-terminal",
];

/// Remove a settings/data folder, but only one that is plainly shellman's: named `shellman` or
/// one of [`STATE_IDS`], and not a drive root or a shallow path.
fn purge(dir: &Path, dry: bool) -> Result<(), String> {
    let named = dir.file_name().is_some_and(|n| {
        let n = n.to_string_lossy();
        n.eq_ignore_ascii_case("shellman") || STATE_IDS.iter().any(|id| n.eq_ignore_ascii_case(id))
    });
    if !named || dir.components().count() < 3 {
        return Err(format!("refusing to purge {}: not a folder of shellman's", dir.display()));
    }
    if !dir.is_dir() {
        return Ok(());
    }
    if dry {
        say!("  would remove {}", dir.display());
    } else {
        std::fs::remove_dir_all(dir).map_err(|e| format!("cannot remove {}: {e}", dir.display()))?;
        say!("  removed {}", dir.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trailing_separator_is_dropped_from_a_folder() {
        assert_eq!(trim_dir(PathBuf::from(r"C:\x\")), PathBuf::from(r"C:\x"));
        assert_eq!(trim_dir(PathBuf::from(r"C:\x")), PathBuf::from(r"C:\x"));
        assert_eq!(trim_dir(PathBuf::from(r"C:\")), PathBuf::from(r"C:\"));
    }

    #[test]
    fn purge_only_touches_a_folder_of_shellmans() {
        for bad in [r"C:\", r"C:\Users", r"C:\Users\u\AppData\Roaming", r"C:\shellmanx", "shellman", r"C:\a\shellman-other"] {
            assert!(purge(Path::new(bad), true).is_err(), "{bad}");
        }
        assert!(purge(Path::new(r"C:\no\such\shellman"), true).is_ok());
        assert!(purge(Path::new(r"C:\no\such\shellman-files"), true).is_ok());
    }
}
