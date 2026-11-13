//! What an install or update will do, computed from listings alone (no file system, no
//! processes): source files with their hashes, installed files with theirs, and which installed
//! files are in use by a running process. Pure, so every rule is tested.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub rel: String,
    pub src: PathBuf,
    pub hash: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Not installed yet.
    Add,
    /// Differs; `running`: a process runs from the installed file (so it goes aside, and the new
    /// build takes effect when that process restarts).
    Replace { running: bool },
    Unchanged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub rel: String,
    pub src: PathBuf,
    pub hash: String,
    pub action: Action,
    /// Processes running from the installed file's old image (before this update).
    pub pids: Vec<u32>,
}

impl Item {
    /// The session is only ever replaced on disk: it is the shell, so it changes at next sign-in.
    pub fn is_session(&self) -> bool {
        self.rel.eq_ignore_ascii_case("shellman-session.exe")
    }
}

/// Keys are lower-case relative names.
pub type Hashes = BTreeMap<String, String>;
pub type Running = BTreeMap<String, Vec<u32>>;

pub fn key(rel: &str) -> String {
    rel.to_ascii_lowercase().replace('/', "\\")
}

/// One action per source file, in source order.
pub fn plan(source: &[Source], installed: &Hashes, running: &Running) -> Vec<Item> {
    source
        .iter()
        .map(|s| {
            let k = key(&s.rel);
            let pids = running.get(&k).cloned().unwrap_or_default();
            let action = match installed.get(&k) {
                None => Action::Add,
                Some(h) if *h == s.hash => Action::Unchanged,
                Some(_) => Action::Replace { running: !pids.is_empty() },
            };
            Item { rel: s.rel.clone(), src: s.src.clone(), hash: s.hash.clone(), action, pids }
        })
        .collect()
}

/// Which installed file a running image belongs to: `rel` (lower-case) when `image` is the file
/// itself, or the same file renamed aside by an update (`name.old<digits>.ext`), below `root`.
pub fn owner(image: &str, root: &Path) -> Option<String> {
    let norm = |s: &str| s.trim_start_matches("\\\\?\\").replace('/', "\\").to_ascii_lowercase();
    let (image, root) = (norm(image), norm(&root.to_string_lossy()));
    let rest = image.strip_prefix(root.trim_end_matches('\\'))?.strip_prefix('\\')?;
    let (dir, name) = match rest.rfind('\\') {
        Some(i) => (&rest[..i + 1], &rest[i + 1..]),
        None => ("", rest),
    };
    let name = crate::manifest::aside_of(name).unwrap_or_else(|| name.to_string());
    Some(format!("{dir}{name}"))
}

/// Group running processes (`pid`, image path) by installed file.
pub fn running_by_file(procs: &[(u32, String)], root: &Path) -> Running {
    let mut out = Running::new();
    for (pid, image) in procs {
        if let Some(k) = owner(image, root) {
            out.entry(k).or_default().push(*pid);
        }
    }
    out
}

/// Processes running from a renamed-aside image (`*.old*.exe`): still on an old build.
pub fn on_old_image(procs: &[(u32, String)], root: &Path) -> Vec<(u32, String)> {
    procs
        .iter()
        .filter(|(_, image)| {
            owner(image, root).is_some()
                && Path::new(image).file_name().is_some_and(|n| crate::manifest::is_aside(&n.to_string_lossy()))
        })
        .cloned()
        .collect()
}

/// The words for one item.
pub fn describe(item: &Item) -> &'static str {
    match item.action {
        Action::Add => "added",
        Action::Unchanged => "unchanged",
        Action::Replace { running: false } => "replaced",
        Action::Replace { running: true } if item.is_session() => "replaced (running: takes effect at next sign-in)",
        Action::Replace { running: true } => "replaced (running: takes effect when the app restarts)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(rel: &str, hash: &str) -> Source {
        Source { rel: rel.into(), src: PathBuf::from(format!("S\\{rel}")), hash: hash.into() }
    }

    fn installed(list: &[(&str, &str)]) -> Hashes {
        list.iter().map(|(r, h)| (key(r), h.to_string())).collect()
    }

    #[test]
    fn decides_per_file() {
        let source = [
            src("shellman-bar.exe", "b2"),
            src("shellman-files.exe", "f1"),
            src("shellman-shelf.exe", "s1"),
            src("shellman-session.exe", "x2"),
            src("conpty.dll", "c1"),
        ];
        let have = installed(&[
            ("shellman-bar.exe", "b1"),
            ("Shellman-Files.exe", "f1"),
            ("shellman-session.exe", "x1"),
            ("conpty.dll", "c0"),
        ]);
        let mut running = Running::new();
        running.insert(key("shellman-bar.exe"), vec![10]);
        running.insert(key("shellman-session.exe"), vec![11]);
        running.insert(key("shellman-files.exe"), vec![12]);
        let items = plan(&source, &have, &running);
        let actions: Vec<_> = items.iter().map(|i| i.action).collect();
        assert_eq!(
            actions,
            [
                Action::Replace { running: true },
                Action::Unchanged, // running but identical: nothing to do
                Action::Add,
                Action::Replace { running: true },
                Action::Replace { running: false },
            ]
        );
        assert_eq!(items[0].pids, [10]);
        assert_eq!(describe(&items[0]), "replaced (running: takes effect when the app restarts)");
        assert_eq!(describe(&items[3]), "replaced (running: takes effect at next sign-in)");
        assert_eq!(describe(&items[1]), "unchanged");
        assert_eq!(describe(&items[2]), "added");
        assert_eq!(describe(&items[4]), "replaced");
    }

    #[test]
    fn an_up_to_date_install_plans_nothing() {
        let source = [src("a.exe", "1"), src("b.dll", "2")];
        let items = plan(&source, &installed(&[("a.exe", "1"), ("b.dll", "2")]), &Running::new());
        assert!(items.iter().all(|i| i.action == Action::Unchanged));
    }

    #[test]
    fn images_map_to_installed_files() {
        let root = Path::new(r"C:\P\shellman");
        assert_eq!(owner(r"c:\p\SHELLMAN\shellman-bar.exe", root).as_deref(), Some("shellman-bar.exe"));
        assert_eq!(owner(r"C:\P\shellman\shellman-bar.old141815.exe", root).as_deref(), Some("shellman-bar.exe"));
        assert_eq!(owner(r"\\?\C:\P\shellman\sub\OpenConsole.old1.exe", root).as_deref(), Some("sub\\openconsole.exe"));
        assert_eq!(owner(r"C:\P\shellman2\shellman-bar.exe", root), None);
        assert_eq!(owner(r"C:\Other\shellman-bar.exe", root), None);
    }

    #[test]
    fn running_processes_are_grouped_and_old_images_found() {
        let root = Path::new(r"C:\P\shellman");
        let procs = vec![
            (1, r"C:\P\shellman\shellman-bar.old100.exe".to_string()),
            (2, r"C:\P\shellman\shellman-bar.exe".to_string()),
            (3, r"C:\P\shellman\shellman-files.exe".to_string()),
            (4, r"C:\Windows\explorer.exe".to_string()),
        ];
        let by = running_by_file(&procs, root);
        assert_eq!(by[&key("shellman-bar.exe")], [1, 2]);
        assert_eq!(by[&key("shellman-files.exe")], [3]);
        assert_eq!(by.len(), 2);
        let old = on_old_image(&procs, root);
        assert_eq!(old.len(), 1);
        assert_eq!(old[0].0, 1);
    }
}
