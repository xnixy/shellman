//! One tab: where it is, what it lists, and what is selected.

use shellman_core::fsops::{self, Entry};
use shellman_core::recycle_bin::BinItem;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Clone, PartialEq, Debug)]
pub(super) enum Loc {
    Pc,
    Dir(PathBuf),
    /// The Recycle Bin: a place of its own, not a folder, so it has no path.
    Bin,
    /// Every item with this tag (from 0), wherever it is: a list, like search results.
    Tag(u8),
}

impl Loc {
    pub(super) fn title(&self) -> String {
        match self {
            Loc::Pc => "This PC".into(),
            Loc::Bin => "Recycle Bin".into(),
            Loc::Tag(i) => super::tag_ui::name_of(*i as usize),
            Loc::Dir(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.display().to_string()),
        }
    }

    pub(super) fn dir(&self) -> Option<&Path> {
        match self {
            Loc::Pc | Loc::Bin | Loc::Tag(_) => None,
            Loc::Dir(p) => Some(p),
        }
    }

    pub(super) fn parent(&self) -> Option<Loc> {
        match self {
            Loc::Pc | Loc::Bin | Loc::Tag(_) => None,
            Loc::Dir(p) => Some(p.parent().map_or(Loc::Pc, |q| Loc::Dir(q.to_path_buf()))),
        }
    }
}

/// In picker mode, which items a folder shows: folders always (to get around), and files only
/// when they match the chosen file type, and not at all when folders are being picked.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Only {
    pub(super) folders: bool,
    pub(super) files: crate::pick::filter::Patterns,
}

impl Only {
    fn shows(&self, e: &Entry) -> bool {
        e.is_dir || (!self.folders && self.files.matches(&e.name))
    }
}

/// A rubber band in progress: where it began, in content coordinates (see `select::Layout`),
/// and what was selected before it, kept when Ctrl was held.
#[derive(Clone, Debug)]
pub(super) struct Band {
    pub(super) from: egui::Pos2,
    pub(super) before: std::collections::BTreeSet<usize>,
    pub(super) add: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum SortKey {
    Name,
    Modified,
    Kind,
    Size,
}

pub(super) struct Tab {
    pub(super) id: u64,
    pub(super) loc: Loc,
    pub(super) back: Vec<Loc>,
    pub(super) fwd: Vec<Loc>,
    pub(super) entries: Vec<Entry>,
    /// Indexes into `entries`, filtered and sorted.
    pub(super) view: Vec<usize>,
    pub(super) dirty: bool,
    pub(super) loading: bool,
    pub(super) error: Option<String>,
    pub(super) gen: u64,
    pub(super) selected: HashSet<PathBuf>,
    pub(super) anchor: Option<usize>,
    pub(super) cursor: Option<usize>,
    pub(super) filter: String,
    pub(super) sort: SortKey,
    pub(super) ascending: bool,
    pub(super) scroll_to_cursor: bool,
    pub(super) list_offset: f32,
    pub(super) list_height: f32,
    /// After the next load, select this item, and maybe start renaming it.
    pub(super) select_after_load: Option<(PathBuf, bool)>,
    /// The Recycle Bin's items by their stored path, for the original location and the handles
    /// restore and delete need. Empty in every other place.
    pub(super) bin: HashMap<PathBuf, BinItem>,
    /// The sort to go back to when the tab leaves the bin, which opens newest first.
    pub(super) sort_before_bin: Option<(SortKey, bool)>,
    /// What a picker shows of the folder; None when browsing.
    pub(super) only: Option<Only>,
    /// A rectangle being dragged out over the items to select them.
    pub(super) band: Option<Band>,
    /// Where the scrolled view showed, on screen, last frame: rubber band coordinates start here.
    pub(super) view_rect: egui::Rect,
    /// A scroll the rubber band asked for, applied in the next frame.
    pub(super) scroll_to: Option<f32>,
    /// Every entry index, sorted by `sorted_for`. Typing in the filter only walks this, so a
    /// keystroke never re-sorts. Hidden items stay in it and are skipped while filtering.
    order: Vec<usize>,
    /// Sort the cached `order` was built for; `None` means it must be rebuilt.
    sorted_for: Option<(SortKey, bool)>,
    /// Identity of `entries` when `order` and `index` were built, so replacing them is noticed
    /// even when the loader assigns the field directly instead of calling `set_entries`.
    entries_sig: (usize, usize),
    /// Entry index by path, for O(1) selection stats and `row_of`.
    index: HashMap<PathBuf, usize>,
    /// Row in `view` for each entry index, `u32::MAX` when filtered out.
    rows: Vec<u32>,
}

impl Tab {
    pub(super) fn new(id: u64, loc: Loc) -> Self {
        Self {
            id,
            loc,
            back: Vec::new(),
            fwd: Vec::new(),
            entries: Vec::new(),
            view: Vec::new(),
            dirty: false,
            loading: false,
            error: None,
            gen: 0,
            selected: HashSet::new(),
            anchor: None,
            cursor: None,
            filter: String::new(),
            sort: SortKey::Name,
            ascending: true,
            scroll_to_cursor: false,
            list_offset: 0.0,
            list_height: 0.0,
            select_after_load: None,
            bin: HashMap::new(),
            sort_before_bin: None,
            only: None,
            band: None,
            view_rect: egui::Rect::NOTHING,
            scroll_to: None,
            order: Vec::new(),
            sorted_for: None,
            entries_sig: (0, 0),
            index: HashMap::new(),
            rows: Vec::new(),
        }
    }

    /// Replace the entries (a fresh listing). Forces one re-sort and a new path index on the
    /// next `rebuild_view`.
    pub(super) fn set_entries(&mut self, entries: Vec<Entry>) {
        self.entries = entries;
        self.sorted_for = None;
        self.entries_sig = (usize::MAX, 0);
    }

    /// Re-sorts only when the entries or the sort changed; otherwise just re-filters the cached
    /// order, which is a linear scan with no comparisons.
    pub(super) fn rebuild_view(&mut self, show_hidden: bool) {
        let sig = (self.entries.len(), self.entries.as_ptr() as usize);
        if sig != self.entries_sig {
            self.entries_sig = sig;
            self.sorted_for = None;
            // Every time the entries change: a rename in the middle of a folder changes neither
            // the count nor the first or last path, and a stale index loses the renamed item and
            // keeps a ghost selection.
            self.index = self
                .entries
                .iter()
                .enumerate()
                .map(|(i, e)| (e.path.clone(), i))
                .collect();
        }
        if self.sorted_for != Some((self.sort, self.ascending)) {
            self.resort();
        }
        let needle = self.filter.trim().to_lowercase();
        let entries = &self.entries;
        let only = self.only.as_ref();
        self.view.clear();
        self.view.extend(self.order.iter().copied().filter(|&i| {
            let e = &entries[i];
            (show_hidden || !e.hidden)
                && (needle.is_empty() || e.key.contains(&needle))
                && only.is_none_or(|o| o.shows(e))
        }));
        self.rows.clear();
        self.rows.resize(entries.len(), u32::MAX);
        for (r, &i) in self.view.iter().enumerate() {
            self.rows[i] = r as u32;
        }
        self.dirty = false;
        if !self.selected.is_empty() {
            let (index, rows) = (&self.index, &self.rows);
            self.selected.retain(|p| {
                index.get(p).is_some_and(|&i| {
                    entries.get(i).is_some_and(|e| e.path == *p) && rows.get(i).is_some_and(|&r| r != u32::MAX)
                })
            });
        }
        self.cursor = self.cursor.filter(|&c| c < self.view.len());
        self.anchor = self.anchor.filter(|&c| c < self.view.len());
    }

    fn resort(&mut self) {
        let entries = &self.entries;
        let (key, asc) = (self.sort, self.ascending);
        let mut order: Vec<usize> = (0..entries.len()).collect();
        order.sort_by(|&a, &b| {
            let (a, b) = (&entries[a], &entries[b]);
            // Folders always come first, whichever way the list is sorted.
            b.is_dir.cmp(&a.is_dir).then_with(|| {
                let ord = match key {
                    SortKey::Name => Ordering::Equal,
                    SortKey::Modified => a.modified.cmp(&b.modified),
                    SortKey::Kind => a.kind.cmp(&b.kind),
                    SortKey::Size => a.size.cmp(&b.size),
                }
                .then_with(|| fsops::natural_cmp(&a.key, &b.key));
                if asc {
                    ord
                } else {
                    ord.reverse()
                }
            })
        });
        self.order = order;
        self.sorted_for = Some((key, asc));
    }

    pub(super) fn entry(&self, row: usize) -> &Entry {
        &self.entries[self.view[row]]
    }

    pub(super) fn row_of(&self, path: &Path) -> Option<usize> {
        let fast = self
            .index
            .get(path)
            .and_then(|&i| self.rows.get(i))
            .copied();
        match fast {
            // Verify, in case the entries were swapped since the index was built.
            Some(r)
                if r != u32::MAX
                    && self
                        .view
                        .get(r as usize)
                        .is_some_and(|_| self.entry(r as usize).path == path) =>
            {
                Some(r as usize)
            }
            _ => (0..self.view.len()).find(|&r| self.entry(r).path == path),
        }
    }

    /// (count, total bytes of selected files) without cloning entries. Cost is proportional to
    /// the selection, not the folder, so it is safe to call every frame.
    pub(super) fn selection_stats(&self) -> (usize, u64) {
        let mut bytes = 0;
        for p in &self.selected {
            if let Some(e) = self.index.get(p).and_then(|&i| self.entries.get(i)) {
                if !e.is_dir {
                    bytes += e.size;
                }
            }
        }
        (self.selected.len(), bytes)
    }

    /// Selected items in list order.
    pub(super) fn selection(&self) -> Vec<Entry> {
        (0..self.view.len())
            .map(|r| self.entry(r))
            .filter(|e| self.selected.contains(&e.path))
            .cloned()
            .collect()
    }

    /// The selection as rows (see `select`). Costs as much as the selection.
    pub(super) fn sel(&self) -> super::select::Sel {
        super::select::Sel {
            rows: self.selected.iter().filter_map(|p| self.row_of(p)).collect(),
            anchor: self.anchor,
            cursor: self.cursor,
        }
    }

    /// Take a selection worked out as rows.
    pub(super) fn set_sel(&mut self, sel: super::select::Sel) {
        let n = self.view.len();
        self.selected = sel
            .rows
            .iter()
            .filter(|&&r| r < n)
            .map(|&r| self.entry(r).path.clone())
            .collect();
        self.anchor = sel.anchor.filter(|&r| r < n);
        self.cursor = sel.cursor.filter(|&r| r < n);
    }

    /// Files renamed `old` to `new`: the selection keeps it under its new name, so the next
    /// listing shows it still selected. True when it is all that is selected (then the cursor
    /// should land on it too).
    pub(super) fn renamed(&mut self, old: &Path, new: &Path) -> bool {
        if self.selected.remove(old) {
            self.selected.insert(new.to_path_buf());
        }
        self.selected.is_empty() || (self.selected.len() == 1 && self.selected.contains(new))
    }

    pub(super) fn select_only(&mut self, row: usize) {
        self.selected.clear();
        self.selected.insert(self.entry(row).path.clone());
        self.cursor = Some(row);
        self.anchor = Some(row);
    }

    pub(super) fn select_range(&mut self, to: usize) {
        let from = self.anchor.unwrap_or(to);
        self.selected.clear();
        for r in from.min(to)..=from.max(to) {
            self.selected.insert(self.entry(r).path.clone());
        }
        self.cursor = Some(to);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(dir: &str, name: &str, size: u64) -> Entry {
        Entry {
            name: name.into(),
            key: name.to_lowercase(),
            path: Path::new(dir).join(name),
            is_dir: false,
            size,
            modified: None,
            modified_text: String::new(),
            kind: "File".into(),
            hidden: false,
        }
    }

    fn listing(names: &[(&str, u64)]) -> Vec<Entry> {
        names.iter().map(|(n, s)| entry(r"C:\d", n, *s)).collect()
    }

    #[test]
    fn a_rename_in_the_middle_leaves_no_ghost_and_finds_the_new_name() {
        let mut t = Tab::new(1, Loc::Dir(PathBuf::from(r"C:\d")));
        t.set_entries(listing(&[("a", 1), ("b", 10), ("c", 100)]));
        t.rebuild_view(false);
        let b = PathBuf::from(r"C:\d\b");
        t.selected.insert(b.clone());
        assert_eq!(t.selection_stats(), (1, 10));
        // "b" renamed to "bb": same count, same first and last item. The listing comes back in
        // the disk's order, with the item where "b" was.
        t.set_entries(listing(&[("a", 1), ("bb", 20), ("c", 100)]));
        t.rebuild_view(false);
        assert!(t.selected.is_empty(), "the old name is gone, not a ghost: {:?}", t.selected);
        assert_eq!(t.selection_stats(), (0, 0));
        let bb = PathBuf::from(r"C:\d\bb");
        // Found through the index, not a scan.
        assert_eq!(t.index.get(&bb), Some(&1));
        assert_eq!(t.row_of(&bb), Some(1));
        t.select_only(1);
        assert_eq!(t.selection_stats(), (1, 20));
    }

    #[test]
    fn a_rename_by_files_keeps_the_selection() {
        let mut t = Tab::new(1, Loc::Dir(PathBuf::from(r"C:\d")));
        t.set_entries(listing(&[("a", 1), ("b", 10), ("c", 100)]));
        t.rebuild_view(false);
        let (a, b, bb) = (PathBuf::from(r"C:\d\a"), PathBuf::from(r"C:\d\b"), PathBuf::from(r"C:\d\bb"));
        t.selected.extend([a.clone(), b.clone()]);
        assert!(!t.renamed(&b, &bb), "two selected: the cursor is left alone");
        t.set_entries(listing(&[("a", 1), ("bb", 10), ("c", 100)]));
        t.rebuild_view(false);
        assert_eq!(t.selected, HashSet::from([a.clone(), bb.clone()]));
        assert_eq!(t.selection_stats(), (2, 11));
        t.selected = HashSet::from([bb.clone()]);
        assert!(t.renamed(&bb, Path::new(r"C:\d\z")));
    }

    #[test]
    fn the_index_follows_entries_assigned_directly() {
        let mut t = Tab::new(1, Loc::Dir(PathBuf::from(r"C:\d")));
        t.entries = listing(&[("a", 1), ("b", 2), ("c", 3)]);
        t.rebuild_view(false);
        t.entries = listing(&[("a", 1), ("x", 2), ("c", 3)]);
        t.rebuild_view(false);
        assert_eq!(t.index.get(Path::new(r"C:\d\x")), Some(&1));
        assert!(!t.index.contains_key(Path::new(r"C:\d\b")));
    }
}
