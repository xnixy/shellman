//! `install` and `update`: the files (see `plan`, `fsops`), the Start Menu shortcuts, the Apps &
//! features entry, and the integrations asked for (`--shell`, `--default-terminal`,
//! `--picker-hook`).

use super::*;
use crate::{sess, shortcuts};

pub fn install(o: &Opts, env: &Env) -> Result<(), String> {
    let (src_dir, sources, warnings) = load_source(o, env)?;
    for need in [SESSION_EXE, "shellman-bar.exe"] {
        if !sources.iter().any(|s| s.rel.eq_ignore_ascii_case(need)) {
            return Err(format!("{need} is not in {}; refusing to install an incomplete shellman", src_dir.display()));
        }
    }
    if read_record(env).is_some() {
        return Err(format!("shellman is already installed in {}; use `shellman-setup update`", env.prefix.display()));
    }
    let root = env.root();
    let rels: Vec<String> = sources.iter().map(|s| s.rel.clone()).collect();
    let items = plan::plan(&sources, &installed_hashes(&root, &rels), &plan::Running::new());
    let steps = shortcuts_plan(env, &rels);
    say!("shellman-setup {} installing into {}", shellman_buildinfo::STAMP, env.prefix.display());
    say!("from {}", src_dir.display());
    print_warnings(&warnings);
    if o.dry_run {
        for i in &items {
            say!("  would copy {} ({})", i.rel, short(Some(&i.hash)));
        }
        if !o.no_shortcuts {
            print_shortcut_steps(&steps, true);
        }
        say!("  would add shellman to Apps & features");
        dry_integrations(o);
        say!("dry run: nothing was changed.");
        return Ok(());
    }
    let _lock = sys::lock(&env.prefix)?;
    std::fs::create_dir_all(&env.prefix).map_err(|e| format!("cannot create {}: {e}", env.prefix.display()))?;
    let out = apply(env, items);
    let failed = report(&out, env, &Default::default());
    let mut rec = Record { stamp: shellman_buildinfo::STAMP.to_string(), files: Default::default() };
    for (i, r) in &out {
        if r.is_ok() {
            rec.files.insert(i.rel.clone());
        }
    }
    write_record(env, &rec)?;
    if failed > 0 {
        return Err(format!("{failed} file(s) could not be installed; see above (the ones that were copied are recorded)"));
    }
    if !o.no_shortcuts {
        apply_shortcuts(&steps)?;
    }
    say!("installed {} files.", rec.files.len());
    register_entry(env, &rec)?;
    integrations(o, env)?;
    if !env.shell_is_shellman() {
        say!();
        say!("shellman is NOT the Windows shell. That is a separate, explicit step:");
        say!("    {}\\shellman-session.exe --install      (takes effect at the next sign-in)", env.prefix.display());
        say!("Undo it with `shellman-session --uninstall`. Until then, `shellman-session` can also run beside Explorer.");
    }
    Ok(())
}

fn shortcuts_plan(env: &Env, rels: &[String]) -> Vec<shortcuts::Step> {
    shortcuts::plan(
        &env.start_menu,
        &env.prefix,
        &|exe| rels.iter().any(|r| r.eq_ignore_ascii_case(exe)),
        &shortcuts::read_target,
    )
}

fn print_shortcut_steps(steps: &[shortcuts::Step], dry: bool) {
    use shortcuts::Step::*;
    for s in steps {
        match s {
            Create(sc) => say!("  {} shortcut {}", if dry { "would create" } else { "created" }, sc.lnk.display()),
            Retarget(sc) => say!("  {} shortcut {} -> {}", if dry { "would repoint" } else { "repointed" }, sc.lnk.display(), sc.target.display()),
            UpToDate(sc) => say!("  shortcut {} is up to date", sc.lnk.display()),
            Skip { name, why } => say!("  no shortcut {name}: {why}"),
        }
    }
}

fn apply_shortcuts(steps: &[shortcuts::Step]) -> Result<(), String> {
    use shortcuts::Step::*;
    for s in steps {
        if let Create(sc) | Retarget(sc) = s {
            shortcuts::write(sc).map_err(|e| format!("shortcut {}: {e}", sc.lnk.display()))?;
        }
    }
    print_shortcut_steps(steps, false);
    Ok(())
}

/// The Apps & features entry, sized from the record.
fn register_entry(env: &Env, rec: &Record) -> Result<(), String> {
    let bytes: u64 = rec.files.iter().filter_map(|f| std::fs::metadata(env.prefix.join(f)).ok()).map(|m| m.len()).sum();
    integrate::register_app(&*env.reg, &env.prefix, &rec.stamp, (bytes / 1024).min(u32::MAX as u64) as u32, &sys::yyyymmdd())?;
    say!("listed in Apps & features (uninstall from there or with `shellman-setup uninstall`).");
    Ok(())
}

fn dry_integrations(o: &Opts) {
    if o.picker_hook {
        say!("  would make apps' Open and Save dialogs use shellman Files");
    }
    if o.default_terminal {
        say!("  would make shellman Terminal the default terminal");
    }
    if o.shell {
        say!("  would make shellman the Windows shell from the next sign-in");
    }
}

/// The opt-in integrations, in order of how easily they are undone.
fn integrations(o: &Opts, env: &Env) -> Result<(), String> {
    let reg = &*env.reg;
    if o.picker_hook {
        integrate::register_hook(reg, &env.prefix).map_err(|e| format!("the Open/Save dialog hook: {e}"))?;
        say!("apps' Open and Save dialogs now use shellman Files (in programs started from now on).");
    }
    if o.default_terminal {
        integrate::make_default_terminal(reg, &env.prefix).map_err(|e| format!("the default terminal: {e}"))?;
        say!("shellman Terminal is now the default terminal.");
    }
    if o.shell {
        integrate::make_shell(reg, &env.prefix).map_err(|e| format!("the shell: {e}"))?;
        say!("shellman is the Windows shell from the next sign-in (hold Shift while signing in to get Explorer).");
    }
    Ok(())
}

pub fn update(o: &Opts, env: &Env) -> Result<(), String> {
    if !env.prefix.is_dir() {
        return Err(format!("nothing is installed in {}; run `shellman-setup install` first", env.prefix.display()));
    }
    let (src_dir, sources, warnings) = load_source(o, env)?;
    let root = env.root();
    let rels: Vec<String> = sources.iter().map(|s| s.rel.clone()).collect();
    let procs = sys::processes();
    let running = plan::running_by_file(&procs, &env.prefix);
    let items = plan::plan(&sources, &installed_hashes(&root, &rels), &running);
    let old_stamps: std::collections::BTreeMap<String, String> =
        rels.iter().filter_map(|r| sys::file_stamp(&env.prefix.join(r)).map(|s| (plan::key(r), s))).collect();

    say!("shellman-setup {} updating {}", shellman_buildinfo::STAMP, env.prefix.display());
    say!("from {}", src_dir.display());
    print_warnings(&warnings);

    if o.dry_run {
        for i in &items {
            say!("  would be {:<10} {}", plan::describe(i), i.rel);
        }
        if o.restart {
            let names: Vec<_> = items
                .iter()
                .filter(|i| matches!(i.action, Action::Replace { running: true }) && !i.is_session())
                .filter_map(|i| sess::app_name(&i.rel))
                .collect();
            say!("  with --restart it would ask the session to restart: {}", if names.is_empty() { "(nothing)".into() } else { names.join(", ") });
            say!("  (apps the session does not supervise are listed, never closed)");
        }
        dry_integrations(o);
        say!("dry run: nothing was changed.");
        return Ok(());
    }

    let _lock = sys::lock(&env.prefix)?;
    let out = apply(env, items);
    let failed = report(&out, env, &old_stamps);

    let mut rec = read_record(env).unwrap_or_default();
    rec.stamp = shellman_buildinfo::STAMP.to_string();
    for (i, r) in &out {
        if r.is_ok() {
            rec.files.insert(i.rel.clone());
        }
    }
    write_record(env, &rec)?;
    let swept = fsops::sweep_asides(&root, &rels);
    let changed = out.iter().filter(|(i, r)| r.is_ok() && i.action != Action::Unchanged).count();
    say!("{changed} file(s) changed, {} unchanged{}.", out.len() - changed - failed, if swept > 0 { format!(", {swept} old file(s) cleaned up") } else { String::new() });
    if !o.no_shortcuts && o.shortcuts_on_update {
        apply_shortcuts(&shortcuts_plan(env, &rec.files.iter().cloned().collect::<Vec<_>>()))?;
    }
    if failed == 0 {
        register_entry(env, &rec)?;
    }

    warm(env, &out);

    // Replaced files that a process still runs.
    let stale: Vec<&Item> = out.iter().filter(|(i, r)| r.is_ok() && matches!(i.action, Action::Replace { running: true })).map(|(i, _)| i).collect();
    let mut left: Vec<&Item> = Vec::new();
    for item in &stale {
        if item.is_session() {
            say!("{}: replaced on disk; the running session keeps the old build until the next sign-in.", item.rel);
        } else {
            left.push(item);
        }
    }
    if o.restart {
        left = restart_apps(env, left);
    } else if !left.is_empty() {
        say!("Not restarted (no --restart). Supervised apps: `shellman-setup update --restart`.");
    }
    if !left.is_empty() {
        say!("Still on the old build (restart them when convenient; setup never closes these):");
        for item in left {
            let name = sess::app_name(&item.rel).unwrap_or_else(|| item.rel.clone());
            let live: Vec<u32> = plan::running_by_file(&sys::processes(), &env.prefix).get(&plan::key(&item.rel)).cloned().unwrap_or_default();
            let pids = if live.is_empty() { item.pids.clone() } else { live };
            let titles = sys::window_titles(&pids);
            if titles.is_empty() {
                say!("  {name}: pid {}, no visible window", pids.iter().map(u32::to_string).collect::<Vec<_>>().join(", "));
            }
            for (pid, t) in titles {
                say!("  {name}: window \"{t}\" (pid {pid})");
            }
        }
    }
    if o.restart {
        // Apps that were restarted no longer hold their old images: those can go now.
        let more = fsops::sweep_asides(&root, &rels);
        if more > 0 {
            say!("{more} more old file(s) cleaned up.");
        }
    }
    if failed > 0 {
        return Err(format!("{failed} file(s) could not be updated; see above"));
    }
    integrations(o, env)
}

/// Run each app that is started on demand and was just written once with `--warm` (it returns
/// from `main` at once): Windows Defender scans a new exe the first time it runs, 1 to 2 s, and
/// this way that is not the user's first click on it (`shellman_core::warm`, whose list and
/// flag these are).
fn warm(env: &Env, out: &[(Item, Result<String, String>)]) {
    const APPS: [&str; 4] = ["shellman-files.exe", "shellman-settings.exe", "shellman-tasks.exe", "shellman-terminal.exe"];
    let written: Vec<&Item> = out
        .iter()
        .filter(|(i, r)| r.is_ok() && i.action != Action::Unchanged && APPS.iter().any(|a| i.rel.eq_ignore_ascii_case(a)))
        .map(|(i, _)| i)
        .collect();
    if written.is_empty() {
        return;
    }
    let t0 = std::time::Instant::now();
    for item in &written {
        let _ = std::process::Command::new(env.prefix.join(&item.rel)).arg("--warm").status();
    }
    say!("{} app(s) run once so their first start is quick ({} ms).", written.len(), t0.elapsed().as_millis());
}

/// Ask the session to restart the supervised apps among `items`; returns the ones left on the
/// old build (not supervised, refused, or no session).
fn restart_apps<'a>(env: &Env, items: Vec<&'a Item>) -> Vec<&'a Item> {
    let mut left = Vec::new();
    if sess::find(&env.instance).is_none() && !items.is_empty() {
        say!("No shellman session is running{}: nothing can be restarted from here.", if env.instance.is_empty() { String::new() } else { format!(" (instance {})", env.instance) });
        return items;
    }
    for item in items {
        let Some(name) = sess::app_name(&item.rel) else {
            left.push(item);
            continue;
        };
        match sess::ask_restart(&env.instance, &name) {
            Ok(sess::Answer::Accepted) => match wait_restarted(env, item) {
                Some(pid) => say!("{}: restarted by the session (new pid {pid})", item.rel),
                None => say!("{}: the session was asked to restart it; not back after 15 s (see %LOCALAPPDATA%\\shellman\\session.log)", item.rel),
            },
            Ok(sess::Answer::Unknown) => {
                say!("{}: not supervised by the session (on demand): left running", item.rel);
                left.push(item);
            }
            Ok(sess::Answer::Refused) => {
                say!("{}: the session is ending or has handed over to Explorer: not restarted", item.rel);
                left.push(item);
            }
            Err(e) => {
                say!("{}: {e}", item.rel);
                left.push(item);
            }
        }
    }
    left
}

/// Wait up to 15 s for a new process running the installed (new) file; its pid.
fn wait_restarted(env: &Env, item: &Item) -> Option<u32> {
    let want = env.prefix.join(&item.rel).to_string_lossy().replace('/', "\\").to_ascii_lowercase();
    for _ in 0..60 {
        std::thread::sleep(std::time::Duration::from_millis(250));
        let found = sys::processes().into_iter().find(|(pid, image)| {
            !item.pids.contains(pid) && image.trim_start_matches("\\\\?\\").to_ascii_lowercase() == want
        });
        if let Some((pid, _)) = found {
            return Some(pid);
        }
    }
    None
}
