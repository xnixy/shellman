//! `status`: each installed file against the source folder, old images still running, and what
//! the install hooks into (shell, default terminal, dialog hook, Apps & features).

use super::*;
use crate::sess;

pub fn status(o: &Opts, env: &Env) -> Result<(), String> {
    say!("shellman-setup {}", shellman_buildinfo::STAMP);
    say!("install folder: {}{}", env.prefix.display(), if env.prefix.is_dir() { "" } else { "  (does not exist)" });
    let record = read_record(env);
    match &record {
        Some(r) => say!("recorded by setup: {} files, last set up by {}", r.files.len(), r.stamp),
        None => say!("no install record (not installed by shellman-setup)"),
    }
    let (source, sources) = match load_source(o, env) {
        Ok((d, s, w)) => {
            print_warnings(&w);
            say!("source folder: {}", d.display());
            (Some(d), s)
        }
        Err(e) => {
            say!("source folder: none ({e})");
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
    say!();
    say!("  {:<28} {:<34} {:<9} {:<9} state", "file", "installed build", "installed", "source");
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
        say!("  {:<28} {:<34} {:<9} {:<9} {state}{run}", rel, stamp, short(have.get(&k)), short(src.map(|s| &s.hash)));
    }
    for s in &sources {
        if !names.iter().any(|n| plan::key(n) == plan::key(&s.rel)) {
            say!("  {:<28} {:<34} {:<9} {:<9} not installed", s.rel, "-", "-", short(Some(&s.hash)));
        }
    }
    say!();
    let old = plan::on_old_image(&procs, &env.prefix);
    if old.is_empty() {
        say!("processes on an old image: none");
    } else {
        say!("processes on an old image (replaced while running; restart them):");
        for (pid, image) in old {
            say!("  pid {pid}  {image}");
        }
    }
    match env.shell() {
        Some(v) if state::shell_is_shellman(&v) => say!("Windows shell: shellman ({v}), per-user Winlogon Shell"),
        Some(v) => say!("Windows shell: {v} (per-user Winlogon Shell; not shellman)"),
        None => say!("Windows shell: Explorer (no per-user Winlogon Shell value)"),
    }
    say!("default terminal: {}", state::terminal_state(env.terminal().as_deref()));
    let reg = &*env.reg;
    say!("Open/Save dialog hook: {}", if integrate::hook_registered(reg) { "registered (apps get shellman Files' picker)" } else { "not registered" });
    say!("sign-in font check: {}", if integrate::font_check_on(reg) { "on" } else { "off" });
    say!(
        "Apps & features: {}",
        match integrate::app_registered(reg) {
            Some(p) if same_dir(Path::new(&p), &env.prefix) => "listed".to_string(),
            Some(p) => format!("listed for another folder ({p})"),
            None => "not listed".to_string(),
        }
    );
    if !reg.is_real() {
        say!("(registry: the stand-in file from SHELLMAN_SETUP_REGISTRY)");
    }
    say!("session: {}", if sess::find(&env.instance).is_some() { "running" } else { "not running" });
    Ok(())
}
