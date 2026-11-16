//! Checking tagged items against the disk: where each one is now. This touches the disk (and
//! maybe the network) once or twice per item, so it runs on a worker; the store takes the
//! results in afterwards (`Store::apply`).
//!
//! The rules, for an item with a path and maybe an identity:
//! - Something is at the path: the tag stays there. If its identity changed (an editor saved by
//!   writing a new file over the old one) the new one is learned.
//! - Nothing is at the path, and the identity is found elsewhere on its volume: the item was
//!   renamed or moved (by anyone), and its tags follow. Found in the Recycle Bin, it counts as
//!   missing but is kept while it is there, in case it is restored.
//! - Nothing at the path and no identity found: missing (dropped after a grace period). An item
//!   on a volume that is not there now (an unplugged drive) is left alone.

use super::{key, Item};
use shellman_core::fsops::identity::{self, FileId, Probe};
use std::path::{Path, PathBuf};

/// What a check found for one item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// At its path, with the identity known.
    Same,
    /// At its path, with this identity (new, or none on this volume).
    Learn(Option<FileId>),
    /// Found at another path.
    Moved(PathBuf, Option<FileId>),
    /// In the Recycle Bin.
    Binned,
    /// Nowhere.
    Missing,
    /// Its volume is not there now.
    Offline,
}

/// The disk, as a check sees it (a fake one in tests).
pub trait Disk {
    fn probe(&self, p: &Path) -> Probe;
    fn find(&self, id: FileId, hint: &Path) -> Option<PathBuf>;
    fn online(&self, p: &Path) -> bool;
}

/// The real disk.
pub struct RealDisk;

impl Disk for RealDisk {
    fn probe(&self, p: &Path) -> Probe {
        identity::probe(p)
    }
    fn find(&self, id: FileId, hint: &Path) -> Option<PathBuf> {
        identity::path_of(id, hint)
    }
    fn online(&self, p: &Path) -> bool {
        identity::volume_online(p)
    }
}

/// Whether a path is inside a Recycle Bin folder.
pub fn in_bin(p: &Path) -> bool {
    key(p).contains("\\$recycle.bin\\")
}

/// Where `item` is now.
pub fn verdict(item: &Item, disk: &impl Disk) -> Verdict {
    match disk.probe(&item.path) {
        Probe::Exists(id) if id == item.id => Verdict::Same,
        Probe::Exists(id) => Verdict::Learn(id),
        Probe::Missing => {
            if !disk.online(&item.path) {
                return Verdict::Offline;
            }
            match item.id.and_then(|id| disk.find(id, &item.path).map(|p| (id, p))) {
                Some((_, p)) if in_bin(&p) => Verdict::Binned,
                Some((id, p)) => Verdict::Moved(p, Some(id)),
                None => Verdict::Missing,
            }
        }
    }
}

/// Check every item of a snapshot (key, item), as `Store::apply` takes the results.
pub fn run(items: Vec<(String, Item)>, disk: &impl Disk) -> Vec<(String, PathBuf, Verdict)> {
    items
        .into_iter()
        .map(|(k, it)| {
            let v = verdict(&it, disk);
            (k, it.path, v)
        })
        .collect()
}

/// Where each item will be once the results are applied: its new path, or none when it is
/// missing. For listing a tag place from the same check.
pub fn now_at(path: &Path, v: &Verdict) -> Option<PathBuf> {
    match v {
        Verdict::Same | Verdict::Learn(_) => Some(path.to_path_buf()),
        Verdict::Moved(to, _) => Some(to.clone()),
        Verdict::Binned | Verdict::Missing | Verdict::Offline => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Fake {
        at: HashMap<String, Option<FileId>>,
        ids: HashMap<FileId, PathBuf>,
        offline: Vec<String>,
    }

    impl Disk for Fake {
        fn probe(&self, p: &Path) -> Probe {
            match self.at.get(&key(p)) {
                Some(id) => Probe::Exists(*id),
                None => Probe::Missing,
            }
        }
        fn find(&self, id: FileId, _hint: &Path) -> Option<PathBuf> {
            self.ids.get(&id).cloned()
        }
        fn online(&self, p: &Path) -> bool {
            !self.offline.iter().any(|o| key(p).starts_with(o))
        }
    }

    fn id(n: u128) -> FileId {
        FileId { volume: 1, file: n }
    }

    fn item(path: &str, id: Option<FileId>) -> Item {
        Item {
            path: PathBuf::from(path),
            tags: 1,
            id,
            missing: None,
        }
    }

    #[test]
    fn the_rules() {
        let disk = Fake {
            at: HashMap::from([
                (key(Path::new(r"C:\same.txt")), Some(id(1))),
                (key(Path::new(r"C:\saved.txt")), Some(id(22))),
                (key(Path::new(r"C:\fat\x.txt")), None),
                (key(Path::new(r"C:\new name.txt")), Some(id(3))),
            ]),
            ids: HashMap::from([
                (id(3), PathBuf::from(r"C:\new name.txt")),
                (id(4), PathBuf::from(r"C:\$Recycle.Bin\S-1-5\$R1.txt")),
            ]),
            offline: vec!["e:".into()],
        };
        assert_eq!(verdict(&item(r"C:\same.txt", Some(id(1))), &disk), Verdict::Same);
        // Saved by replacing: the path wins and the new identity is learned.
        assert_eq!(
            verdict(&item(r"C:\saved.txt", Some(id(2))), &disk),
            Verdict::Learn(Some(id(22)))
        );
        // First time seen, or on a volume without ids.
        assert_eq!(verdict(&item(r"C:\same.txt", None), &disk), Verdict::Learn(Some(id(1))));
        assert_eq!(verdict(&item(r"C:\fat\x.txt", None), &disk), Verdict::Same);
        // Renamed behind our back.
        assert_eq!(
            verdict(&item(r"C:\old name.txt", Some(id(3))), &disk),
            Verdict::Moved(PathBuf::from(r"C:\new name.txt"), Some(id(3)))
        );
        assert_eq!(verdict(&item(r"C:\deleted.txt", Some(id(4))), &disk), Verdict::Binned);
        assert_eq!(verdict(&item(r"C:\gone.txt", Some(id(5))), &disk), Verdict::Missing);
        assert_eq!(verdict(&item(r"C:\gone.txt", None), &disk), Verdict::Missing);
        assert_eq!(verdict(&item(r"E:\usb.txt", Some(id(6))), &disk), Verdict::Offline);
        assert_eq!(now_at(Path::new("x"), &Verdict::Missing), None);
        assert_eq!(now_at(Path::new("x"), &Verdict::Learn(None)), Some(PathBuf::from("x")));
    }

    /// The real disk, in a folder of our own: renames and moves behind Files' back, and a save
    /// that replaces the file.
    #[test]
    fn on_the_real_disk() {
        use super::super::{now_secs, Store};
        let dir = shellman_core::long_temp_dir().join(format!("files-tag-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let (a, doc, folder) = (dir.join("a.txt"), dir.join("doc.txt"), dir.join("folder"));
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&doc, "one").unwrap();
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("inner.txt"), "i").unwrap();
        let mut s = Store::default();
        for p in [&a, &doc, &folder, &folder.join("inner.txt")] {
            s.set(p, 2, true);
        }
        let snapshot = |s: &Store| s.items.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<Vec<_>>();
        // The first check learns identities.
        let r = run(snapshot(&s), &RealDisk);
        s.apply(&r, now_secs());
        if s.items.values().any(|it| it.id.is_none()) {
            // Not a volume with ids: only paths can be followed here.
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        // Behind Files' back: a rename, a move to another folder, a folder renamed, and a save
        // that writes a new file over the old one.
        let renamed = dir.join("renamed.txt");
        std::fs::rename(&a, &renamed).unwrap();
        let moved = dir.join("sub").join("renamed.txt");
        std::fs::rename(&renamed, &moved).unwrap();
        let folder2 = dir.join("folder 2");
        std::fs::rename(&folder, &folder2).unwrap();
        let tmp = dir.join("doc.tmp");
        std::fs::write(&tmp, "two").unwrap();
        std::fs::rename(&tmp, &doc).unwrap();
        let before_save = s.items[&key(&doc)].id;
        let r = run(snapshot(&s), &RealDisk);
        assert!(s.apply(&r, now_secs()));
        let k = |p: &Path| key(p);
        assert_eq!(s.mask(&moved), 4, "{:?}", s.items.keys().collect::<Vec<_>>());
        assert_eq!(s.mask(&a), 0);
        assert_eq!(s.mask(&folder2), 4);
        assert_eq!(s.mask(&folder2.join("inner.txt")), 4);
        assert_eq!(s.mask(&doc), 4, "the saved file keeps its tag");
        assert_ne!(s.items[&k(&doc)].id, before_save, "and its new identity is learned");
        assert!(s.tagged(2).iter().all(|p| p.exists()));
        // A second check finds nothing more to do.
        let r = run(snapshot(&s), &RealDisk);
        assert!(!s.apply(&r, now_secs()));
        // Deleted for good: missing.
        std::fs::remove_file(&moved).unwrap();
        let r = run(snapshot(&s), &RealDisk);
        s.apply(&r, now_secs());
        assert!(s.items[&k(&moved)].missing.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
