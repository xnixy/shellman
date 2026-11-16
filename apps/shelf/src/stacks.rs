//! What each stack holds: its folder's items, newest first. Folders are read on worker threads
//! and read again when Windows says they changed (a folder watch, never a timer); nothing is
//! read or watched while the shelf is not shown.

use crate::folders::Folders;
use shellman_core::settings::desktop::Stack;
use shellman_core::transfer::path_key;
use shellman_core::watch::Watcher;
use std::collections::HashSet;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::SystemTime;

/// One thing in a stack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub path: PathBuf,
    pub name: String,
    /// None for a folder.
    pub size: Option<u64>,
    /// When it arrived or was last used, in seconds since 1970 (UTC).
    pub time: i64,
}

impl Item {
    pub fn is_dir(&self) -> bool {
        self.size.is_none()
    }
}

/// Recent items are shortcuts; past this many, older ones are not looked at.
const RECENT_MAX: usize = 400;

const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;

/// Whether a folder entry stays off the shelf: hidden and system files (`desktop.ini`), and
/// downloads still in progress.
pub fn skip(name: &str, attributes: u32) -> bool {
    if attributes & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) != 0 {
        return true;
    }
    let lower = name.to_lowercase();
    lower == "desktop.ini"
        || [".crdownload", ".part", ".partial", ".download", ".opdownload"].iter().any(|e| lower.ends_with(e))
}

/// Newest first; the same time is broken by name, so the order never flickers.
pub fn newest_first(items: &mut [Item]) {
    items.sort_by(|a, b| b.time.cmp(&a.time).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
}

/// Each path once, keeping the first (the newest, after `newest_first`).
pub fn dedupe(items: Vec<Item>) -> Vec<Item> {
    let mut seen = HashSet::new();
    items.into_iter().filter(|i| seen.insert(path_key(&i.path))).collect()
}

/// What a stack's fan shows: its `n` newest items, and how many it leaves out.
pub fn fan(items: &[Item], n: usize) -> (&[Item], usize) {
    let shown = &items[..items.len().min(n)];
    (shown, items.len() - shown.len())
}

fn secs(t: SystemTime) -> i64 {
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

/// The items of one or more folders (the user's and the common Desktop), newest first. An item
/// counts from when it arrived (created) or was last changed, whichever is later: a download
/// keeps the server's date as its change time.
pub fn read_folders(dirs: &[PathBuf]) -> Vec<Item> {
    let mut items = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            let name = entry.file_name().to_string_lossy().into_owned();
            if skip(&name, meta.file_attributes()) {
                continue;
            }
            let time = meta.created().ok().max(meta.modified().ok()).map_or(0, secs);
            items.push(Item { path: entry.path(), name, size: (!meta.is_dir()).then_some(meta.len()), time });
        }
    }
    newest_first(&mut items);
    dedupe(items)
}

/// Windows' recent items: the shortcuts in the Recent folder, newest first, each resolved to
/// what it points at. Only files that still exist; each once.
pub fn read_recent(dir: &Path) -> Vec<Item> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut links: Vec<(PathBuf, i64)> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")))
        .filter_map(|e| Some((e.path(), secs(e.metadata().ok()?.modified().ok()?))))
        .collect();
    links.sort_by(|a, b| b.1.cmp(&a.1));
    links.truncate(RECENT_MAX);
    let mut items = Vec::new();
    for (link, time) in links {
        let Some(target) = shellman_core::fsops::link_target(&link) else { continue };
        let Ok(meta) = std::fs::metadata(&target) else { continue };
        if meta.is_dir() {
            continue;
        }
        let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if skip(&name, meta.file_attributes()) {
            continue;
        }
        items.push(Item { path: target, name, size: Some(meta.len()), time });
    }
    newest_first(&mut items);
    dedupe(items)
}

/// Every stack's items, kept up to date while active.
pub struct Stacks {
    folders: Folders,
    lists: [Vec<Item>; 3],
    /// Bumped for every read asked for, per stack; results for an older one are dropped.
    asked: [u64; 3],
    tx: Sender<(usize, u64, Vec<Item>)>,
    rx: Receiver<(usize, u64, Vec<Item>)>,
    watcher: Watcher,
    ctx: egui::Context,
    active: bool,
}

fn index(stack: Stack) -> usize {
    match stack {
        Stack::Desktop => 0,
        Stack::Downloads => 1,
        Stack::Recent => 2,
    }
}

impl Stacks {
    pub fn new(ctx: &egui::Context) -> Self {
        let (tx, rx) = channel();
        Self {
            folders: Folders::default(),
            lists: Default::default(),
            asked: [0; 3],
            tx,
            rx,
            watcher: Watcher::new(ctx.clone()),
            ctx: ctx.clone(),
            active: false,
        }
    }

    /// Start or stop following the folders. Stopping forgets the items and stops the watch.
    pub fn set_active(&mut self, on: bool, folders: &Folders) {
        if on && (!self.active || self.folders != *folders) {
            self.folders = folders.clone();
            self.watcher.set_dirs(folders.watched());
            for s in Stack::ALL {
                self.read(s);
            }
        } else if !on && self.active {
            self.watcher.set_dirs(Vec::new());
            self.lists = Default::default();
            self.asked = self.asked.map(|n| n + 1);
        }
        self.active = on;
    }

    /// Take in changed folders and finished reads. True when any stack changed.
    pub fn poll(&mut self) -> bool {
        for dir in self.watcher.poll() {
            for s in Stack::ALL {
                if self.dirs(s).iter().any(|d| path_key(d) == path_key(&dir)) {
                    self.read(s);
                }
            }
        }
        let mut changed = false;
        while let Ok((i, n, items)) = self.rx.try_recv() {
            if self.active && n == self.asked[i] && self.lists[i] != items {
                self.lists[i] = items;
                changed = true;
            }
        }
        changed
    }

    /// Read a stack again now (after the shelf put something there).
    pub fn refresh(&mut self, stack: Stack) {
        if self.active {
            self.read(stack);
        }
    }

    pub fn items(&self, stack: Stack) -> &[Item] {
        &self.lists[index(stack)]
    }

    /// The folder "Open in Files" opens.
    pub fn folder(&self, stack: Stack) -> Option<PathBuf> {
        match stack {
            Stack::Desktop => self.folders.desktop.clone(),
            Stack::Downloads => self.folders.downloads.clone(),
            Stack::Recent => self.folders.recent.clone(),
        }
    }

    fn dirs(&self, stack: Stack) -> Vec<PathBuf> {
        let f = &self.folders;
        match stack {
            Stack::Desktop => [&f.desktop, &f.public_desktop].into_iter().flatten().cloned().collect(),
            Stack::Downloads => f.downloads.iter().cloned().collect(),
            Stack::Recent => f.recent.iter().cloned().collect(),
        }
    }

    fn read(&mut self, stack: Stack) {
        let i = index(stack);
        self.asked[i] += 1;
        let (n, dirs, tx, ctx) = (self.asked[i], self.dirs(stack), self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let items = match stack {
                Stack::Recent => dirs.first().map(|d| read_recent(d)).unwrap_or_default(),
                _ => read_folders(&dirs),
            };
            if tx.send((i, n, items)).is_ok() {
                // Named: from another thread a plain repaint goes to whichever window was last drawn.
                ctx.request_repaint_of(egui::ViewportId::ROOT);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str, time: i64) -> Item {
        Item { path: PathBuf::from(format!("C:\\d\\{name}")), name: name.into(), size: Some(1), time }
    }

    #[test]
    fn hidden_system_and_partial_files_are_skipped() {
        assert!(skip("desktop.ini", 0x2 | 0x4));
        assert!(skip("Desktop.ini", 0));
        assert!(skip(".secret", 0x2));
        assert!(skip("movie.mkv.crdownload", 0));
        assert!(skip("ISO.part", 0));
        assert!(!skip("report.pdf", 0x20));
        assert!(!skip("New folder", 0x10));
    }

    #[test]
    fn newest_first_then_by_name() {
        let mut v = vec![item("b", 10), item("a", 10), item("c", 30), item("old", 1)];
        newest_first(&mut v);
        let names: Vec<_> = v.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["c", "a", "b", "old"]);
    }

    #[test]
    fn the_fan_shows_six_and_counts_the_rest() {
        let v: Vec<Item> = (0..20).map(|i| item(&format!("f{i}"), 100 - i)).collect();
        let (shown, more) = fan(&v, 6);
        assert_eq!(shown.len(), 6);
        assert_eq!(shown[0].name, "f0");
        assert_eq!(more, 14);
        let (shown, more) = fan(&v[..3], 6);
        assert_eq!((shown.len(), more), (3, 0));
    }

    #[test]
    fn a_path_shows_once() {
        let mut a = item("x", 5);
        a.path = PathBuf::from("C:\\D\\X");
        let v = dedupe(vec![item("x", 9), a, item("y", 1)]);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].time, 9);
    }

    #[test]
    fn folders_are_read_newest_first_without_hidden_files() {
        let root = shellman_core::long_temp_dir().join(format!("shellman-shelf-read-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("a.txt"), b"hello").unwrap();
        std::fs::write(root.join("x.crdownload"), b"").unwrap();
        let items = read_folders(&[root.clone(), root.join("missing")]);
        let names: HashSet<_> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, HashSet::from(["a.txt", "sub"]));
        assert_eq!(items.iter().find(|i| i.name == "a.txt").unwrap().size, Some(5));
        assert!(items.iter().find(|i| i.name == "sub").unwrap().is_dir());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn recent_items_resolve_and_skip_what_is_gone() {
        let root = shellman_core::long_temp_dir().join(format!("shellman-shelf-recent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (files, recent) = (root.join("files"), root.join("Recent"));
        std::fs::create_dir_all(&files).unwrap();
        std::fs::create_dir_all(&recent).unwrap();
        let kept = files.join("kept.txt");
        let gone = files.join("gone.txt");
        std::fs::write(&kept, b"1").unwrap();
        std::fs::write(&gone, b"2").unwrap();
        shellman_core::fsops::make_link(&kept, &recent.join("kept.txt.lnk")).unwrap();
        shellman_core::fsops::make_link(&gone, &recent.join("gone.txt.lnk")).unwrap();
        shellman_core::fsops::make_link(&files, &recent.join("files.lnk")).unwrap();
        std::fs::remove_file(&gone).unwrap();
        let items = read_recent(&recent);
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0].path, kept);
        let _ = std::fs::remove_dir_all(root);
    }
}
