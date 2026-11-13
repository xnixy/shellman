//! shellman-setup: install, update, inspect and remove shellman. A console tool with no UI; it
//! works from a plain folder of exes (a release zip) as well as from a build's `target\release`.
//! See `docs/PLAN.md`, "Install and update". It never makes shellman the Windows shell and never
//! restarts anything without `--restart`.

mod cli;
mod fsops;
mod manifest;
mod paths;
mod plan;
mod sess;
mod sha256;
mod shortcuts;
mod state;
mod sys;

use cli::{Cmd, Opts};
use paths::Root;
use plan::{Action, Hashes, Item, Source};
use state::Record;
use std::path::{Path, PathBuf};

const SESSION_EXE: &str = "shellman-session.exe";

/// Where things are, with every place overridable (flags, then environment, then Windows' own).
struct Env {
    prefix: PathBuf,
    start_menu: PathBuf,
    instance: String,
    config_dirs: Vec<PathBuf>,
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

impl Env {
    fn resolve(o: &Opts) -> Result<Env, String> {
        let local = var("LOCALAPPDATA").map(PathBuf::from);
        let roaming = var("APPDATA").map(PathBuf::from);
        let prefix = o
            .prefix
            .clone()
            .or_else(|| var("SHELLMAN_SETUP_PREFIX").map(PathBuf::from))
            .or_else(|| local.as_ref().map(|l| l.join("Programs").join("shellman")))
            .ok_or("cannot find %LOCALAPPDATA%; give --prefix")?;
        let start_menu = o
            .start_menu
            .clone()
            .or_else(|| var("SHELLMAN_SETUP_START_MENU").map(PathBuf::from))
            .or_else(|| roaming.as_ref().map(|r| r.join("Microsoft\\Windows\\Start Menu\\Programs\\shellman")))
            .ok_or("cannot find %APPDATA%; give --start-menu")?;
        let mut config_dirs = Vec::new();
        match var("SHELLMAN_SETUP_CONFIG") {
            Some(c) => config_dirs.push(PathBuf::from(c)),
            None => config_dirs.extend(roaming.map(|r| r.join("shellman"))),
        }
        match var("SHELLMAN_SETUP_DATA") {
            Some(c) => config_dirs.push(PathBuf::from(c)),
            None => config_dirs.extend(local.map(|l| l.join("shellman"))),
        }
        Ok(Env { prefix, start_menu, instance: var("SHELLMAN_INSTANCE").unwrap_or_default(), config_dirs })
    }

    fn root(&self) -> Root {
        Root::new(self.prefix.clone())
    }

    /// The Winlogon `Shell` value (a stand-in from `SHELLMAN_SETUP_SHELL` for tests).
    fn shell(&self) -> Option<String> {
        match var("SHELLMAN_SETUP_SHELL") {
            Some(v) if v == "none" => None,
            Some(v) => Some(v),
            None => sys::hkcu_string(r"Software\Microsoft\Windows NT\CurrentVersion\Winlogon", "Shell"),
        }
    }

    fn terminal(&self) -> Option<String> {
        match var("SHELLMAN_SETUP_TERMINAL") {
            Some(v) if v == "none" => None,
            Some(v) => Some(v),
            None => sys::hkcu_string(r"Console\%%Startup", "DelegationTerminal"),
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match cli::parse(&args) {
        Err(e) => {
            eprintln!("shellman-setup: {e}\n\n{}", cli::HELP);
            2
        }
        Ok((Cmd::Help, _)) => {
            println!("{}", cli::HELP);
            0
        }
        Ok((Cmd::Version, _)) => {
            println!("shellman-setup {}", shellman_buildinfo::STAMP);
            0
        }
        Ok((cmd, o)) => match Env::resolve(&o).and_then(|env| run(cmd, &o, &env)) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("shellman-setup: {e}");
                1
            }
        },
    };
    std::process::exit(code);
}

fn run(cmd: Cmd, o: &Opts, env: &Env) -> Result<(), String> {
    match cmd {
        Cmd::Install => install(o, env),
        Cmd::Update => update(o, env),
        Cmd::Status => status(o, env),
        Cmd::Uninstall => uninstall(o, env),
        Cmd::Version | Cmd::Help => Ok(()),
    }
}

fn exe_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)).unwrap_or_default()
}

/// The folder to install from: `--from`, else the folder this tool is in.
fn source_dir(o: &Opts) -> PathBuf {
    o.from.clone().unwrap_or_else(exe_dir)
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let n = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().trim_start_matches("\\\\?\\").to_ascii_lowercase();
    n(a) == n(b)
}

/// The manifest for a source folder, hashed.
fn load_source(o: &Opts, env: &Env) -> Result<(PathBuf, Vec<Source>, Vec<String>), String> {
    let dir = source_dir(o);
    if !dir.is_dir() {
        return Err(format!("source folder {} not found", dir.display()));
    }
    if same_dir(&dir, &env.prefix) {
        return Err(format!(
            "the source folder is the install folder ({}); give --from <folder of new files> (in the repo: target\\release)",
            dir.display()
        ));
    }
    let built = manifest::build(&dir, &o.extras);
    if !built.entries.iter().any(|e| e.rel.to_ascii_lowercase().starts_with("shellman-") && e.rel.to_ascii_lowercase().ends_with(".exe")) {
        return Err(format!("no shellman-*.exe in {}", dir.display()));
    }
    let mut sources = Vec::new();
    for e in built.entries {
        let hash = sha256::of_file(&e.src).map_err(|err| format!("cannot read {}: {err}", e.src.display()))?;
        sources.push(Source { rel: e.rel, src: e.src, hash });
    }
    Ok((dir, sources, built.warnings))
}

/// Hashes of the installed files named in `rels`.
fn installed_hashes(root: &Root, rels: &[String]) -> Hashes {
    let mut h = Hashes::new();
    for rel in rels {
        if let Ok(p) = root.join(rel) {
            if p.is_file() {
                if let Ok(x) = sha256::of_file(&p) {
                    h.insert(plan::key(rel), x);
                }
            }
        }
    }
    h
}

fn read_record(env: &Env) -> Option<Record> {
    std::fs::read_to_string(env.prefix.join(state::RECORD)).ok().map(|t| Record::parse(&t))
}

fn write_record(env: &Env, rec: &Record) -> Result<(), String> {
    let root = env.root();
    let p = root.join(state::RECORD)?;
    root.guard(&p)?;
    std::fs::write(&p, rec.render()).map_err(|e| format!("cannot write {}: {e}", p.display()))
}

fn short(h: Option<&String>) -> String {
    h.map_or("-".to_string(), |h| h[..8.min(h.len())].to_string())
}

fn print_warnings(w: &[String]) {
    for w in w {
        println!("  warning: {w}");
    }
}

/// Copy what the plan says; returns the plan with `running` corrected by what the rename found.
fn apply(env: &Env, items: Vec<Item>) -> Vec<(Item, Result<String, String>)> {
    let root = env.root();
    let stamp = sys::hhmmss();
    let mut out = Vec::new();
    for mut item in items {
        if item.action == Action::Unchanged {
            out.push((item, Ok(String::new())));
            continue;
        }
        let r = fsops::place(&fsops::RealFs, &root, &item.rel, &item.src, &item.hash, &stamp);
        let r = r.map(|placed| {
            if let (fsops::Placed::Replaced { aside: Some(_) }, Action::Replace { .. }) = (&placed, item.action) {
                item.action = Action::Replace { running: true };
            }
            String::new()
        });
        out.push((item, r));
    }
    out
}

fn report(out: &[(Item, Result<String, String>)], env: &Env, old_stamps: &std::collections::BTreeMap<String, String>) -> usize {
    let mut failed = 0;
    for (item, r) in out {
        match r {
            Err(e) => {
                failed += 1;
                println!("  FAILED     {}  {e}", item.rel);
            }
            Ok(_) => {
                let new = sys::file_stamp(&env.prefix.join(&item.rel)).or_else(|| sys::file_stamp(&item.src));
                let change = match (old_stamps.get(&plan::key(&item.rel)), &new) {
                    (Some(o), Some(n)) if o != n => format!("  [{o} -> {n}]"),
                    (None, Some(n)) if item.action != Action::Unchanged => format!("  [{n}]"),
                    _ => String::new(),
                };
                println!("  {:<10} {}{change}", plan::describe(item).split(" (").next().unwrap_or(""), item.rel);
                if let Action::Replace { running: true } = item.action {
                    println!("             ({})", plan::describe(item).split(" (").nth(1).unwrap_or("").trim_end_matches(')'));
                }
            }
        }
    }
    failed
}

fn install(o: &Opts, env: &Env) -> Result<(), String> {
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
    println!("shellman-setup {} installing into {}", shellman_buildinfo::STAMP, env.prefix.display());
    println!("from {}", src_dir.display());
    print_warnings(&warnings);
    if o.dry_run {
        for i in &items {
            println!("  would copy {} ({})", i.rel, short(Some(&i.hash)));
        }
        if !o.no_shortcuts {
            print_shortcut_steps(&steps, true);
        }
        println!("dry run: nothing was changed.");
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
    println!("installed {} files.", rec.files.len());
    println!();
    println!("shellman is NOT the Windows shell yet. That is a separate, explicit step:");
    println!("    {}\\shellman-session.exe --install      (takes effect at the next sign-in)", env.prefix.display());
    println!("Undo it with `shellman-session --uninstall`. Until then, `shellman-session` can also run beside Explorer.");
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
            Create(sc) => println!("  {} shortcut {}", if dry { "would create" } else { "created" }, sc.lnk.display()),
            Retarget(sc) => println!("  {} shortcut {} -> {}", if dry { "would repoint" } else { "repointed" }, sc.lnk.display(), sc.target.display()),
            UpToDate(sc) => println!("  shortcut {} is up to date", sc.lnk.display()),
            Skip { name, why } => println!("  no shortcut {name}: {why}"),
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

fn update(o: &Opts, env: &Env) -> Result<(), String> {
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

    println!("shellman-setup {} updating {}", shellman_buildinfo::STAMP, env.prefix.display());
    println!("from {}", src_dir.display());
    print_warnings(&warnings);

    if o.dry_run {
        for i in &items {
            println!("  would be {:<10} {}", plan::describe(i), i.rel);
        }
        if o.restart {
            let names: Vec<_> = items
                .iter()
                .filter(|i| matches!(i.action, Action::Replace { running: true }) && !i.is_session())
                .filter_map(|i| sess::app_name(&i.rel))
                .collect();
            println!("  with --restart it would ask the session to restart: {}", if names.is_empty() { "(nothing)".into() } else { names.join(", ") });
            println!("  (apps the session does not supervise are listed, never closed)");
        }
        println!("dry run: nothing was changed.");
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
    println!("{changed} file(s) changed, {} unchanged{}.", out.len() - changed - failed, if swept > 0 { format!(", {swept} old file(s) cleaned up") } else { String::new() });

    // Replaced files that a process still runs.
    let stale: Vec<&Item> = out.iter().filter(|(i, r)| r.is_ok() && matches!(i.action, Action::Replace { running: true })).map(|(i, _)| i).collect();
    let mut left: Vec<&Item> = Vec::new();
    for item in &stale {
        if item.is_session() {
            println!("{}: replaced on disk; the running session keeps the old build until the next sign-in.", item.rel);
        } else {
            left.push(item);
        }
    }
    if o.restart {
        left = restart_apps(env, left);
    } else if !left.is_empty() {
        println!("Not restarted (no --restart). Supervised apps: `shellman-setup update --restart`.");
    }
    if !left.is_empty() {
        println!("Still on the old build (restart them when convenient; setup never closes these):");
        for item in left {
            let name = sess::app_name(&item.rel).unwrap_or_else(|| item.rel.clone());
            let live: Vec<u32> = plan::running_by_file(&sys::processes(), &env.prefix).get(&plan::key(&item.rel)).cloned().unwrap_or_default();
            let pids = if live.is_empty() { item.pids.clone() } else { live };
            let titles = sys::window_titles(&pids);
            if titles.is_empty() {
                println!("  {name}: pid {}, no visible window", pids.iter().map(u32::to_string).collect::<Vec<_>>().join(", "));
            }
            for (pid, t) in titles {
                println!("  {name}: window \"{t}\" (pid {pid})");
            }
        }
    }
    if o.restart {
        // Apps that were restarted no longer hold their old images: those can go now.
        let more = fsops::sweep_asides(&root, &rels);
        if more > 0 {
            println!("{more} more old file(s) cleaned up.");
        }
    }
    if failed > 0 {
        return Err(format!("{failed} file(s) could not be updated; see above"));
    }
    Ok(())
}

/// Ask the session to restart the supervised apps among `items`; returns the ones left on the
/// old build (not supervised, refused, or no session).
fn restart_apps<'a>(env: &Env, items: Vec<&'a Item>) -> Vec<&'a Item> {
    let mut left = Vec::new();
    if sess::find(&env.instance).is_none() && !items.is_empty() {
        println!("No shellman session is running{}: nothing can be restarted from here.", if env.instance.is_empty() { String::new() } else { format!(" (instance {})", env.instance) });
        return items;
    }
    for item in items {
        let Some(name) = sess::app_name(&item.rel) else {
            left.push(item);
            continue;
        };
        match sess::ask_restart(&env.instance, &name) {
            Ok(sess::Answer::Accepted) => match wait_restarted(env, item) {
                Some(pid) => println!("{}: restarted by the session (new pid {pid})", item.rel),
                None => println!("{}: the session was asked to restart it; not back after 15 s (see %LOCALAPPDATA%\\shellman\\session.log)", item.rel),
            },
            Ok(sess::Answer::Unknown) => {
                println!("{}: not supervised by the session (on demand): left running", item.rel);
                left.push(item);
            }
            Ok(sess::Answer::Refused) => {
                println!("{}: the session is ending or has handed over to Explorer: not restarted", item.rel);
                left.push(item);
            }
            Err(e) => {
                println!("{}: {e}", item.rel);
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

fn status(o: &Opts, env: &Env) -> Result<(), String> {
    println!("shellman-setup {}", shellman_buildinfo::STAMP);
    println!("install folder: {}{}", env.prefix.display(), if env.prefix.is_dir() { "" } else { "  (does not exist)" });
    let record = read_record(env);
    match &record {
        Some(r) => println!("recorded by setup: {} files, last set up by {}", r.files.len(), r.stamp),
        None => println!("no install record (not installed by shellman-setup)"),
    }
    let (source, sources) = match load_source(o, env) {
        Ok((d, s, w)) => {
            print_warnings(&w);
            println!("source folder: {}", d.display());
            (Some(d), s)
        }
        Err(e) => {
            println!("source folder: none ({e})");
            (None, Vec::new())
        }
    };
    let root = env.root();
    let mut rels: std::collections::BTreeMap<String, String> = Default::default();
    for s in &sources {
        rels.insert(plan::key(&s.rel), s.rel.clone());
    }
    if let Some(r) = &record {
        for f in &r.files {
            rels.entry(plan::key(f)).or_insert_with(|| f.clone());
        }
    }
    if let Ok(rd) = std::fs::read_dir(&env.prefix) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.to_ascii_lowercase().starts_with("shellman") && !manifest::is_aside(&n) && !n.to_ascii_lowercase().ends_with(".log") && n != state::RECORD {
                rels.entry(plan::key(&n)).or_insert(n);
            }
        }
    }
    let names: Vec<String> = rels.values().cloned().collect();
    let have = installed_hashes(&root, &names);
    let procs = sys::processes();
    let running = plan::running_by_file(&procs, &env.prefix);
    println!();
    println!("  {:<28} {:<34} {:<9} {:<9} state", "file", "installed build", "installed", "source");
    for rel in &names {
        let k = plan::key(rel);
        let stamp = sys::file_stamp(&env.prefix.join(rel)).unwrap_or_else(|| "-".into());
        let src = sources.iter().find(|s| plan::key(&s.rel) == k);
        let state = match (have.get(&k), src) {
            (None, Some(_)) => "not installed".to_string(),
            (None, None) => "missing".to_string(),
            (Some(_), None) if source.is_some() => "not in source".to_string(),
            (Some(a), Some(s)) if *a == s.hash => "up to date".to_string(),
            (Some(_), Some(_)) => "DIFFERS from source".to_string(),
            (Some(_), None) => "installed".to_string(),
        };
        let run = running.get(&k).map_or(String::new(), |p| format!("  running pid {}", p.iter().map(u32::to_string).collect::<Vec<_>>().join(",")));
        println!("  {:<28} {:<34} {:<9} {:<9} {state}{run}", rel, stamp, short(have.get(&k)), short(src.map(|s| &s.hash)));
    }
    for s in &sources {
        if !names.iter().any(|n| plan::key(n) == plan::key(&s.rel)) {
            println!("  {:<28} {:<34} {:<9} {:<9} not installed", s.rel, "-", "-", short(Some(&s.hash)));
        }
    }
    println!();
    let old = plan::on_old_image(&procs, &env.prefix);
    if old.is_empty() {
        println!("processes on an old image: none");
    } else {
        println!("processes on an old image (replaced while running; restart them):");
        for (pid, image) in old {
            println!("  pid {pid}  {image}");
        }
    }
    match env.shell() {
        Some(v) if state::shell_is_shellman(&v) => println!("Windows shell: shellman ({v}), per-user Winlogon Shell"),
        Some(v) => println!("Windows shell: {v} (per-user Winlogon Shell; not shellman)"),
        None => println!("Windows shell: Explorer (no per-user Winlogon Shell value)"),
    }
    println!("default terminal: {}", state::terminal_state(env.terminal().as_deref()));
    println!("session: {}", if sess::find(&env.instance).is_some() { "running" } else { "not running" });
    Ok(())
}

fn uninstall(o: &Opts, env: &Env) -> Result<(), String> {
    if let Some(v) = env.shell().filter(|v| state::shell_is_shellman(v)) {
        return Err(format!(
            "shellman is the Windows shell ({v}). Refusing to remove it: run `shellman-session --uninstall` first, sign out and in, then run this again"
        ));
    }
    if state::terminal_is_shellman(env.terminal().as_deref()) {
        return Err("shellman Terminal is the default terminal. Run `shellman-terminal --unregister-default-terminal` first, then run this again".into());
    }
    let root = env.root();
    if !env.prefix.is_dir() {
        return Err(format!("nothing is installed in {}", env.prefix.display()));
    }
    let record = read_record(env);
    let rels: Vec<String> = match &record {
        Some(r) => r.files.iter().cloned().collect(),
        None => {
            // Installed by hand, before this tool: what install would have put there.
            let mut v = Vec::new();
            if let Ok(rd) = std::fs::read_dir(&env.prefix) {
                for e in rd.flatten() {
                    let n = e.file_name().to_string_lossy().into_owned();
                    let l = n.to_ascii_lowercase();
                    if !manifest::is_aside(&n) && ((l.starts_with("shellman-") && l.ends_with(".exe")) || l == "shellman_pickhook.dll") {
                        v.push(n);
                    }
                }
            }
            v
        }
    };
    let running = plan::running_by_file(&sys::processes(), &env.prefix);
    let busy: Vec<_> = rels.iter().filter(|r| running.contains_key(&plan::key(r))).collect();
    if !busy.is_empty() {
        return Err(format!(
            "still running from the install folder: {}. Close them (the session ends them at sign-out) and run this again",
            busy.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }
    println!("shellman-setup {} uninstalling {}", shellman_buildinfo::STAMP, env.prefix.display());
    if record.is_none() {
        println!("  (no install record: removing shellman-*.exe and shellman_pickhook.dll only)");
    }
    let dry = o.dry_run;
    let mut dirs = std::collections::BTreeSet::new();
    for rel in &rels {
        let p = root.join(rel)?;
        root.guard(&p)?;
        if let Some(d) = p.parent() {
            dirs.insert(d.to_path_buf());
        }
        if dry {
            println!("  would remove {}", p.display());
        } else if p.exists() {
            std::fs::remove_file(&p).map_err(|e| format!("cannot remove {}: {e}", p.display()))?;
            println!("  removed {}", p.display());
        }
    }
    if !dry {
        let n = fsops::sweep_asides(&root, &rels);
        if n > 0 {
            println!("  removed {n} old renamed-aside file(s)");
        }
        let rec = root.join(state::RECORD)?;
        if rec.exists() {
            root.guard(&rec)?;
            let _ = std::fs::remove_file(&rec);
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
            Ok(()) => println!("  removed the install folder"),
            Err(_) => println!("  left {} (it holds files that are not shellman's, such as logs)", env.prefix.display()),
        }
    }
    // Shortcuts: only ours, only by name, only in the Start Menu folder.
    let menu = Root::new(env.start_menu.clone());
    for name in shortcuts::our_files() {
        let p = menu.join(&name)?;
        if p.is_file() {
            if dry {
                println!("  would remove shortcut {}", p.display());
            } else {
                menu.guard(&p)?;
                let _ = std::fs::remove_file(&p);
                println!("  removed shortcut {}", p.display());
            }
        }
    }
    if !dry {
        let _ = std::fs::remove_dir(&env.start_menu);
    }
    if o.purge_config {
        for d in &env.config_dirs {
            purge(d, dry)?;
        }
    } else {
        println!("  your settings were kept (--purge-config removes them)");
    }
    println!("{}", if dry { "dry run: nothing was changed." } else { "uninstalled." });
    Ok(())
}

/// Remove a settings/data folder, but only one that is plainly shellman's: named `shellman`, and
/// not a drive root or a shallow path.
fn purge(dir: &Path, dry: bool) -> Result<(), String> {
    let ok = dir.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("shellman")) && dir.components().count() >= 3;
    if !ok {
        return Err(format!("refusing to purge {}: not a folder named 'shellman'", dir.display()));
    }
    if !dir.is_dir() {
        return Ok(());
    }
    if dry {
        println!("  would remove {}", dir.display());
    } else {
        std::fs::remove_dir_all(dir).map_err(|e| format!("cannot remove {}: {e}", dir.display()))?;
        println!("  removed {}", dir.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purge_only_touches_a_folder_named_shellman() {
        for bad in [r"C:\", r"C:\Users", r"C:\Users\u\AppData\Roaming", r"C:\shellmanx", "shellman"] {
            assert!(purge(Path::new(bad), true).is_err(), "{bad}");
        }
        assert!(purge(Path::new(r"C:\no\such\shellman"), true).is_ok());
    }
}
