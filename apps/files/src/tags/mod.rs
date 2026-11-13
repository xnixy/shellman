//! Colour tags on files and folders: six colours, each with a name the user can change, kept in a
//! small file of their own (`files-tags.json` in the settings folder).
//!
//! Each tagged item is kept by its path (so the dots on cards cost a lookup), and, where its
//! volume allows, by its identity on the volume (`FileId`: the volume's serial and the file's
//! 128-bit id), learned the first time it is checked. Tags follow an item when Files renames,
//! moves or copies it ([`Store::rekey`], [`Store::copy_tree`]); a rename or move by anyone else
//! within the volume is found by the identity (see `check`), which also re-learns a new identity
//! when an editor saved by replacing the file. Items whose identity cannot be found and whose
//! path is gone are marked missing, and dropped after a while ([`Store::apply`]).
//!
//! Everything here is plain data; only [`Store::load`] and [`Store::save`] touch the disk, and
//! saving writes a temporary file and renames it over the old one, so a crash never leaves half
//! a file. A file from before identities loads as it is; the identities are learned as items are
//! checked.

pub mod check;

use serde::{Deserialize, Serialize};
use shellman_core::fsops::identity::FileId;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How long an item whose path is gone and whose identity is not found anywhere keeps its tags,
/// in case it comes back (an editor that deletes and writes anew, a move still going on).
pub const MISSING_GRACE_SECS: u64 = 10 * 60;

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

/// One tagged item: its path as last known, its tags as bits (tag `i` is bit `i`), its
/// identity once learned, and since when (seconds since 1970) it has been missing, if it is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub path: PathBuf,
    pub tags: u8,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "id_text")]
    pub id: Option<FileId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing: Option<u64>,
}

impl Item {
    fn new(path: PathBuf, tags: u8) -> Self {
        Self {
            path,
            tags,
            id: None,
            missing: None,
        }
    }
}

/// An identity as text in the file.
mod id_text {
    use super::FileId;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(id: &Option<FileId>, s: S) -> Result<S::Ok, S::Error> {
        match id {
            Some(id) => s.serialize_str(&id.to_text()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<FileId>, D::Error> {
        let s: Option<String> = Option::deserialize(d)?;
        Ok(s.as_deref().and_then(FileId::from_text))
    }
}

/// Seconds since 1970, now.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
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
            self.items
                .entry(k)
                .or_insert_with(|| Item::new(p.to_path_buf(), 0))
                .tags = after;
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
        self.carry(from, to, true)
    }

    /// `from` was copied to `to` (by Files): the copy gets its tags, and so does everything
    /// inside it that was tagged, as Finder does. The copies' identities are learned when they
    /// are checked. True when any tag was copied.
    pub fn copy_tree(&mut self, from: &Path, to: &Path) -> bool {
        key(from) != key(to) && self.carry(from, to, false)
    }

    /// The items at `from` and inside it, at `to`: moved (keeping their identities) or copied.
    fn carry(&mut self, from: &Path, to: &Path, moving: bool) -> bool {
        let old = key(from);
        let inside = format!("{old}\\");
        let found: Vec<String> = self
            .items
            .keys()
            .filter(|k| **k == old || k.starts_with(&inside))
            .cloned()
            .collect();
        for k in &found {
            let item = if moving {
                self.items.remove(k)
            } else {
                self.items.get(k).cloned()
            };
            let Some(item) = item else { continue };
            let path = if *k == old {
                to.to_path_buf()
            } else {
                // The rest of the path below the folder, as it was written: the key matched,
                // so the item's path starts with the folder's, whatever its case.
                let skip = old.chars().count() + 1;
                let rest: String = item.path.to_string_lossy().chars().skip(skip).collect();
                to.join(rest)
            };
            let entry = self
                .items
                .entry(key(&path))
                .or_insert_with(|| Item::new(path.clone(), 0));
            entry.tags |= item.tags;
            entry.missing = None;
            if moving && item.id.is_some() {
                entry.id = item.id;
            }
        }
        !found.is_empty()
    }

    /// The paths with tag `i`, sorted by key; items missing now are left out.
    #[cfg(test)]
    pub fn tagged(&self, i: usize) -> Vec<PathBuf> {
        let bit = 1u8 << i;
        self.items
            .values()
            .filter(|it| it.tags & bit != 0 && it.missing.is_none())
            .map(|it| it.path.clone())
            .collect()
    }

    /// Take in what a check found (see `check`), at `now`: items found elsewhere move there,
    /// new identities are learned, items missing are marked so and dropped once they have been
    /// missing longer than `MISSING_GRACE_SECS`. Results for items changed since the check
    /// began are left out. True when anything changed.
    pub fn apply(&mut self, results: &[(String, PathBuf, check::Verdict)], now: u64) -> bool {
        use check::Verdict;
        let mut changed = false;
        for (k, was, verdict) in results {
            let Some(item) = self.items.get_mut(k) else { continue };
            if item.path != *was {
                continue;
            }
            match verdict {
                Verdict::Same | Verdict::Offline => {
                    if matches!(verdict, Verdict::Same) && item.missing.take().is_some() {
                        changed = true;
                    }
                }
                Verdict::Learn(id) => {
                    changed |= item.id != *id || item.missing.is_some();
                    item.id = *id;
                    item.missing = None;
                }
                Verdict::Moved(to, id) => {
                    let (to, id) = (to.clone(), *id);
                    self.rekey(was, &to);
                    if let Some(it) = self.items.get_mut(&key(&to)) {
                        it.id = id;
                        it.missing = None;
                    }
                    changed = true;
                }
                Verdict::Binned | Verdict::Missing => match item.missing {
                    None => {
                        item.missing = Some(now);
                        changed = true;
                    }
                    Some(since) if matches!(verdict, Verdict::Missing)
                        && now.saturating_sub(since) >= MISSING_GRACE_SECS =>
                    {
                        self.items.remove(k);
                        changed = true;
                    }
                    Some(_) => {}
                },
            }
        }
        changed
    }

    /// Which tags are on anything, as bits.
    pub fn used(&self) -> u8 {
        self.items.values().fold(0, |m, it| m | it.tags)
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
    fn a_copy_gets_the_tags_inside_folders_too_and_the_original_keeps_them() {
        let mut s = Store::default();
        s.set(&p(r"C:\Work\Plan"), 1, true);
        s.set(&p(r"C:\Work\Plan\notes.txt"), 2, true);
        s.items.get_mut(&key(&p(r"C:\Work\Plan"))).unwrap().id = Some(FileId { volume: 1, file: 9 });
        assert!(s.copy_tree(&p(r"C:\Work\Plan"), &p(r"C:\Work\Plan - Copy")));
        assert_eq!(s.mask(&p(r"C:\Work\Plan - Copy")), 0b10);
        assert_eq!(s.mask(&p(r"C:\Work\Plan - Copy\notes.txt")), 0b100);
        assert_eq!(s.mask(&p(r"C:\Work\Plan")), 0b10);
        // The copy is another file: it does not take the original's identity.
        assert_eq!(s.items[&key(&p(r"C:\Work\Plan - Copy"))].id, None);
        assert!(!s.copy_tree(&p(r"C:\Work\Plan"), &p(r"c:\work\plan")), "onto itself");
        assert!(!s.copy_tree(&p(r"C:\Untagged"), &p(r"C:\Else")));
    }

    #[test]
    fn a_move_keeps_the_identity() {
        let mut s = Store::default();
        s.set(&p(r"C:\a.txt"), 0, true);
        let id = Some(FileId { volume: 1, file: 2 });
        s.items.get_mut(&key(&p(r"C:\a.txt"))).unwrap().id = id;
        s.rekey(&p(r"C:\a.txt"), &p(r"C:\b\a.txt"));
        assert_eq!(s.items[&key(&p(r"C:\b\a.txt"))].id, id);
    }

    #[test]
    fn checks_move_learn_and_in_time_drop() {
        use check::Verdict;
        let mut s = Store::default();
        for name in ["moved", "saved", "gone", "binned", "away", "back"] {
            s.set(&p(&format!(r"C:\t\{name}")), 0, true);
        }
        s.set(&p(r"C:\t\moved\inner.txt"), 3, true);
        s.items.get_mut(&key(&p(r"C:\t\back"))).unwrap().missing = Some(5);
        let id = |n| Some(FileId { volume: 7, file: n });
        let r = |name: &str, v: Verdict| (key(&p(&format!(r"C:\t\{name}"))), p(&format!(r"C:\t\{name}")), v);
        let results = vec![
            r("moved", Verdict::Moved(p(r"C:\u\renamed"), id(1))),
            r("saved", Verdict::Learn(id(2))),
            r("gone", Verdict::Missing),
            r("binned", Verdict::Binned),
            r("away", Verdict::Offline),
            r("back", Verdict::Same),
        ];
        assert!(s.apply(&results, 100));
        assert_eq!(s.mask(&p(r"C:\u\renamed")), 1);
        assert_eq!(s.mask(&p(r"C:\u\renamed\inner.txt")), 0b1000, "what was inside goes along");
        assert_eq!(s.items[&key(&p(r"C:\u\renamed"))].id, id(1));
        assert_eq!(s.items[&key(&p(r"C:\t\saved"))].id, id(2));
        assert_eq!(s.items[&key(&p(r"C:\t\gone"))].missing, Some(100));
        assert_eq!(s.items[&key(&p(r"C:\t\binned"))].missing, Some(100));
        assert_eq!(s.items[&key(&p(r"C:\t\away"))].missing, None);
        assert_eq!(s.items[&key(&p(r"C:\t\back"))].missing, None);
        // Missing items are not listed, but keep their tags for a while.
        assert!(!s.tagged(0).contains(&p(r"C:\t\gone")));
        let again = vec![r("gone", Verdict::Missing), r("binned", Verdict::Binned)];
        assert!(!s.apply(&again, 100 + MISSING_GRACE_SECS - 1));
        assert!(s.apply(&again, 100 + MISSING_GRACE_SECS));
        assert_eq!(s.mask(&p(r"C:\t\gone")), 0, "dropped after the grace");
        assert_eq!(s.mask(&p(r"C:\t\binned")), 1, "in the bin: kept while it is there");
        // A result for an item that changed since the check began is left out.
        let stale = vec![(key(&p(r"C:\t\saved")), p(r"C:\elsewhere"), Verdict::Missing)];
        assert!(!s.apply(&stale, 1000));
    }

    #[test]
    fn a_file_from_before_identities_loads_and_keeps_them_after() {
        let old = r#"{"names":["Red","Orange","Yellow","Green","Blue","Purple"],
            "items":{"c:\\temp\\x.png":{"path":"C:\\Temp\\x.png","tags":5}}}"#;
        let mut s = Store::from_json(old);
        assert_eq!(s.mask(&p(r"C:\Temp\x.png")), 5);
        assert_eq!(s.items[&key(&p(r"C:\Temp\x.png"))].id, None);
        let id = Some(FileId { volume: 0xabc, file: 0x1234 });
        s.items.get_mut(&key(&p(r"C:\Temp\x.png"))).unwrap().id = id;
        let json = s.to_json();
        assert!(json.contains("0000000000000abc:"), "{json}");
        assert_eq!(Store::from_json(&json), s);
        // An identity that does not read is dropped, not the tag.
        let bad = json.replace("0000000000000abc:", "zz:");
        assert_eq!(Store::from_json(&bad).mask(&p(r"C:\Temp\x.png")), 5);
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
