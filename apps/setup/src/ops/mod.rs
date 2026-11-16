//! The commands themselves, shared by the command line (`main.rs`) and the installer window
//! (`apps/installer`). Everything is reported through `say!` (see `out`).

mod install;
mod status;
mod uninstall;

pub use install::{install, update};
pub use status::status;
pub use uninstall::{plan_uninstall, uninstall, UninstallPlan};

use crate::cli::{Cmd, Opts};
use crate::paths::Root;
use crate::plan::{self, Action, Hashes, Item, Source};
use crate::reg::{self, Registry};
use crate::state::{self, Record};
use crate::{fsops, integrate, manifest, sha256, sys};
use std::path::{Path, PathBuf};

pub const SESSION_EXE: &str = "shellman-session.exe";

/// Where things are, with every place overridable (flags, then environment, then Windows' own),
/// and the registry (HKCU, or the stand-in file `SHELLMAN_SETUP_REGISTRY`).
pub struct Env {
    pub prefix: PathBuf,
    pub start_menu: PathBuf,
    pub instance: String,
    pub config_dirs: Vec<PathBuf>,
    /// Where eframe keeps each app's window state (`<this>\shellman-<app>\data`): `%APPDATA%`.
    pub state_root: Option<PathBuf>,
    pub reg: Box<dyn Registry>,
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// A folder without a trailing separator (`"C:\x\"` would end a quoted argument with `\"`),
/// except a drive root.
pub fn trim_dir(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    let t = s.trim_end_matches(['\\', '/']);
    if t.len() == s.len() || t.len() < 3 {
        p
    } else {
        PathBuf::from(t)
    }
}

/// The default install folder, `%LOCALAPPDATA%\Programs\shellman` (or `SHELLMAN_SETUP_PREFIX`).
pub fn default_prefix() -> Option<PathBuf> {
    var("SHELLMAN_SETUP_PREFIX").map(PathBuf::from).or_else(|| var("LOCALAPPDATA").map(|l| PathBuf::from(l).join("Programs").join("shellman")))
}

impl Env {
    pub fn resolve(o: &Opts) -> Result<Env, String> {
        let local = var("LOCALAPPDATA").map(PathBuf::from);
        let roaming = var("APPDATA").map(PathBuf::from);
        let prefix = trim_dir(o.prefix.clone().or_else(default_prefix).ok_or("cannot find %LOCALAPPDATA%; give --prefix")?);
        let start_menu = o
            .start_menu
            .clone()
            .or_else(|| var("SHELLMAN_SETUP_START_MENU").map(PathBuf::from))
            .or_else(|| roaming.as_ref().map(|r| r.join("Microsoft\\Windows\\Start Menu\\Programs\\shellman")))
            .ok_or("cannot find %APPDATA%; give --start-menu")?;
        let mut config_dirs = Vec::new();
        let config = var("SHELLMAN_SETUP_CONFIG").map(PathBuf::from).or_else(|| roaming.as_ref().map(|r| r.join("shellman")));
        let state_root = config.as_ref().and_then(|c| c.parent().map(Path::to_path_buf));
        config_dirs.extend(config);
        match var("SHELLMAN_SETUP_DATA") {
            Some(c) => config_dirs.push(PathBuf::from(c)),
            None => config_dirs.extend(local.map(|l| l.join("shellman"))),
        }
        Ok(Env { prefix, start_menu, instance: var("SHELLMAN_INSTANCE").unwrap_or_default(), config_dirs, state_root, reg: reg::open() })
    }

    pub fn root(&self) -> Root {
        Root::new(self.prefix.clone())
    }

    /// The Winlogon `Shell` value (`SHELLMAN_SETUP_SHELL` stands in for it, read-only).
    pub fn shell(&self) -> Option<String> {
        match var("SHELLMAN_SETUP_SHELL") {
            Some(v) if v == "none" => None,
            Some(v) => Some(v),
            None => integrate::shell_value(&*self.reg),
        }
    }

    pub fn terminal(&self) -> Option<String> {
        match var("SHELLMAN_SETUP_TERMINAL") {
            Some(v) if v == "none" => None,
            Some(v) => Some(v),
            None => integrate::terminal_value(&*self.reg),
        }
    }

    pub fn shell_is_shellman(&self) -> bool {
        self.shell().is_some_and(|v| state::shell_is_shellman(&v))
    }

    pub fn terminal_is_shellman(&self) -> bool {
        state::terminal_is_shellman(self.terminal().as_deref())
    }

    /// Whether shellman is installed here (by setup: there is a record).
    pub fn installed(&self) -> bool {
        read_record(self).is_some()
    }
}

pub fn run(cmd: Cmd, o: &Opts, env: &Env) -> Result<(), String> {
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

pub fn same_dir(a: &Path, b: &Path) -> bool {
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

pub fn read_record(env: &Env) -> Option<Record> {
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
        say!("  warning: {w}");
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
                say!("  FAILED     {}  {e}", item.rel);
            }
            Ok(_) => {
                let new = sys::file_stamp(&env.prefix.join(&item.rel)).or_else(|| sys::file_stamp(&item.src));
                let change = match (old_stamps.get(&plan::key(&item.rel)), &new) {
                    (Some(o), Some(n)) if o != n => format!("  [{o} -> {n}]"),
                    (None, Some(n)) if item.action != Action::Unchanged => format!("  [{n}]"),
                    _ => String::new(),
                };
                say!("  {:<10} {}{change}", plan::describe(item).split(" (").next().unwrap_or(""), item.rel);
                if let Action::Replace { running: true } = item.action {
                    say!("             ({})", plan::describe(item).split(" (").nth(1).unwrap_or("").trim_end_matches(')'));
                }
            }
        }
    }
    failed
}
