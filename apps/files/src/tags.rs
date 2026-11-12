//! Colour tags on files and folders: six colours, each with a name the user can change, kept in a
//! small file of their own (`files-tags.json` in the settings folder) by path.
//!
//! Tags follow an item when Files itself renames or moves it ([`Store::rekey`]). A change made
//! anywhere else (Explorer, a program, the command line) is not seen, and the tag stays with the
//! old path until [`Store::prune`] drops paths that are gone.
//!
//! Everything here is plain data; only [`Store::load`] and [`Store::save`] touch the disk, and
//! saving writes a temporary file and renames it over the old one, so a crash never leaves half
//! a file.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How many tags there are.
pub const COUNT: usize = 6;

/// The tags' colours, in order.
pub const COLOURS: [[u8; 3]; COUNT] = [
    [239, 68, 68],
    [245, 158, 11],
    [234, 200, 30],
    [34, 197, 94],
    [59, 130, 246],
    [168, 85, 247],
];

const DEFAULT_NAMES: [&str; COUNT] = ["Red", "Orange", "Yellow", "Green", "Blue", "Purple"];

/// One tagged item: its path as it was given, and its tags as bits (tag `i` is bit `i`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub path: PathBuf,
    pub tags: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    /// The tags' names, by colour.
    pub names: Vec<String>,
    /// Tagged items by `key` of their path.
    pub items: BTreeMap<String, Item>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            names: DEFAULT_NAMES.iter().map(|s| (*s).to_owned()).collect(),
            items: BTreeMap::new(),
        }
    }
}

/// A path as a key: Windows paths ignore case, separators may be either way, and a trailing
/// separator says nothing.
pub fn key(p: &Path) -> String {
    p.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

impl Store {
    /// Tag `i`'s name.
    pub fn name(&self, i: usize) -> &str {
        self.names.get(i).map_or("", String::as_str)
    }

    /// Give tag `i` a new name; an empty one goes back to the colour's own name.
    pub fn rename(&mut self, i: usize, name: &str) -> bool {
        if i >= COUNT {
            return false;
        }
        self.names.resize(COUNT, String::new());
        let name = name.trim();
        let name = if name.is_empty() { DEFAULT_NAMES[i] } else { name };
        if self.names[i] == name {
            return false;
        }
        self.names[i] = name.to_owned();
        true
    }

    /// The tags on `p`, as bits.
    pub fn mask(&self, p: &Path) -> u8 {
        if self.items.is_empty() {
            return 0;
        }
        self.items.get(&key(p)).map_or(0, |i| i.tags)
    }

    /// Put tag `i` on `p`, or take it off. True when that changed anything.
    pub fn set(&mut self, p: &Path, i: usize, on: bool) -> bool {
        if i >= COUNT {
            return false;
        }
        let k = key(p);
        let bit = 1u8 << i;
        let before = self.items.get(&k).map_or(0, |it| it.tags);
        let after = if on { before | bit } else { before & !bit };
        if after == before {
            return false;
        }
        if after == 0 {
            self.items.remove(&k);
        } else {
            self.items.insert(
                k,
                Item {
                    path: p.to_path_buf(),
                    tags: after,
                },
            );
        }
        true
    }

    /// Flip tag `i` on `p`.
    pub fn toggle(&mut self, p: &Path, i: usize) -> bool {
        let on = self.mask(p) & (1 << i) == 0;
        self.set(p, i, on)
    }

    /// Every tag off `p`.
    pub fn clear(&mut self, p: &Path) -> bool {
        self.items.remove(&key(p)).is_some()
    }

    /// `from` is now at `to` (Files renamed or moved it): its tags go along, and so do those of
    /// everything inside it when it is a folder. True when any tag moved.
    pub fn rekey(&mut self, from: &Path, to: &Path) -> bool {
        let old = key(from);
        let inside = format!("{old}\\");
        let moving: Vec<String> = self
            .items
            .keys()
            .filter(|k| **k == old || k.starts_with(&inside))
            .cloned()
            .collect();
        for k in &moving {
            let Some(item) = self.items.remove(k) else { continue };
            let path = if *k == old {
                to.to_path_buf()
            } else {
                // The rest of the path below the folder, as it was written: the key matched,
                // so the item's path starts with the folder's, whatever its case.
                let skip = old.chars().count() + 1;
                let rest: String = item.path.to_string_lossy().chars().skip(skip).collect();
                to.join(rest)
            };
            let entry = self.items.entry(key(&path)).or_insert(Item {
                path: path.clone(),
                tags: 0,
            });
            entry.tags |= item.tags;
        }
        !moving.is_empty()
    }

    /// The paths with tag `i`, sorted by key.
    pub fn tagged(&self, i: usize) -> Vec<PathBuf> {
        let bit = 1u8 << i;
        self.items
            .values()
            .filter(|it| it.tags & bit != 0)
            .map(|it| it.path.clone())
            .collect()
    }

    /// Which tags are on anything, as bits.
    pub fn used(&self) -> u8 {
        self.items.values().fold(0, |m, it| m | it.tags)
    }

    /// Drop the paths `exists` says are gone. Returns how many went.
    pub fn prune(&mut self, exists: impl Fn(&Path) -> bool) -> usize {
        let before = self.items.len();
        self.items.retain(|_, it| exists(&it.path));
        before - self.items.len()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// A store from its JSON; a broken file gives an empty store, with the names filled up to six.
    pub fn from_json(json: &str) -> Self {
        let mut s: Store = serde_json::from_str(json.trim_start_matches('\u{feff}')).unwrap_or_default();
        for (i, name) in DEFAULT_NAMES.iter().enumerate() {
            if s.names.len() <= i {
                s.names.push((*name).to_owned());
            }
        }
        s.names.truncate(COUNT);
        // Keys are re-made, in case the file was edited by hand.
        s.items = s
            .items
            .into_values()
            .filter(|it| it.tags != 0)
            .map(|it| (key(&it.path), it))
            .collect();
        s
    }

    /// Where the store is kept.
    pub fn file() -> Option<PathBuf> {
        shellman_core::config::config_dir().map(|d| d.join("files-tags.json"))
    }

    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .map(|s| Self::from_json(&s))
            .unwrap_or_default()
    }

    /// Write the store whole: a temporary file, then a rename over the old one.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, self.to_json())?;
        std::fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn tags_go_on_and_off() {
        let mut s = Store::default();
        assert!(s.set(&p(r"C:\a\b.txt"), 0, true));
        assert!(!s.set(&p(r"c:\A\B.TXT"), 0, true), "the same path, other case");
        assert!(s.toggle(&p(r"C:\a\b.txt"), 4));
        assert_eq!(s.mask(&p(r"C:\a\b.txt")), 0b1_0001);
        assert!(s.toggle(&p(r"C:\a\b.txt"), 0));
        assert!(s.set(&p(r"C:\a\b.txt"), 4, false));
        assert!(s.items.is_empty(), "an item with no tags is dropped");
        assert!(!s.set(&p(r"C:\x"), 9, true), "no tag 9");
    }

    #[test]
    fn names_change_and_fall_back() {
        let mut s = Store::default();
        assert_eq!(s.name(3), "Green");
        assert!(s.rename(3, "  Later "));
        assert_eq!(s.name(3), "Later");
        assert!(!s.rename(3, "Later"));
        assert!(s.rename(3, ""));
        assert_eq!(s.name(3), "Green");
        assert!(!s.rename(7, "x"));
    }

    #[test]
    fn a_rename_or_move_takes_the_tags_along_inside_folders_too() {
        let mut s = Store::default();
        s.set(&p(r"C:\Work\Plan"), 1, true);
        s.set(&p(r"C:\Work\Plan\notes.txt"), 2, true);
        s.set(&p(r"C:\Work\Planet.txt"), 3, true);
        assert!(s.rekey(&p(r"C:\Work\Plan"), &p(r"D:\Archive\Plan 2026")));
        assert_eq!(s.mask(&p(r"D:\Archive\Plan 2026")), 0b10);
        assert_eq!(s.mask(&p(r"D:\Archive\Plan 2026\notes.txt")), 0b100);
        assert_eq!(s.tagged(2), [p(r"D:\Archive\Plan 2026\notes.txt")]);
        // A sibling that only starts with the same letters stays.
        assert_eq!(s.mask(&p(r"C:\Work\Planet.txt")), 0b1000);
        assert_eq!(s.mask(&p(r"C:\Work\Plan")), 0);
        assert!(!s.rekey(&p(r"C:\Nothing"), &p(r"C:\Else")));
    }

    #[test]
    fn rekeying_onto_a_tagged_path_keeps_both_tags() {
        let mut s = Store::default();
        s.set(&p(r"C:\a.txt"), 0, true);
        s.set(&p(r"C:\b.txt"), 1, true);
        s.rekey(&p(r"C:\a.txt"), &p(r"C:\b.txt"));
        assert_eq!(s.mask(&p(r"C:\b.txt")), 0b11);
        assert_eq!(s.items.len(), 1);
    }

    #[test]
    fn queries_list_tagged_items_and_used_tags() {
        let mut s = Store::default();
        s.set(&p(r"C:\b"), 5, true);
        s.set(&p(r"C:\a"), 5, true);
        s.set(&p(r"C:\a"), 0, true);
        assert_eq!(s.tagged(5), [p(r"C:\a"), p(r"C:\b")]);
        assert!(s.tagged(2).is_empty());
        assert_eq!(s.used(), 0b10_0001);
    }

    #[test]
    fn missing_paths_are_pruned() {
        let mut s = Store::default();
        s.set(&p(r"C:\here"), 0, true);
        s.set(&p(r"C:\gone"), 0, true);
        assert_eq!(s.prune(|q| q == Path::new(r"C:\here")), 1);
        assert_eq!(s.tagged(0), [p(r"C:\here")]);
    }

    #[test]
    fn json_round_trip_and_broken_files() {
        let mut s = Store::default();
        s.rename(0, "Urgent");
        s.set(&p(r"C:\Temp\x.png"), 0, true);
        s.set(&p(r"C:\Temp\y"), 4, true);
        assert_eq!(Store::from_json(&s.to_json()), s);
        assert_eq!(Store::from_json("{ broken"), Store::default());
        // Short name lists are filled up; zero-tag items dropped; keys re-made.
        let odd = r#"{"names":["One"],"items":{"WRONG":{"path":"C:\\Q","tags":2},"z":{"path":"C:\\Z","tags":0}}}"#;
        let t = Store::from_json(odd);
        assert_eq!(t.names.len(), COUNT);
        assert_eq!(t.name(0), "One");
        assert_eq!(t.mask(&p(r"c:\q")), 2);
        assert_eq!(t.items.len(), 1);
    }

    #[test]
    fn saving_is_whole_and_loads_back() {
        let dir = std::env::temp_dir().join(format!("files-tags-test-{}", std::process::id()));
        let file = dir.join("files-tags.json");
        let mut s = Store::default();
        s.set(&p(r"C:\Temp\x.png"), 2, true);
        s.save(&file).unwrap();
        assert!(!file.with_extension("tmp").exists());
        assert_eq!(Store::load(&file), s);
        assert_eq!(Store::load(&dir.join("missing.json")), Store::default());
        let _ = std::fs::remove_dir_all(dir);
    }
}
