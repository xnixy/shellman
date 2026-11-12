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
}

impl Loc {
    pub(super) fn title(&self) -> String {
        match self {
            Loc::Pc => "This PC".into(),
            Loc::Bin => "Recycle Bin".into(),
            Loc::Dir(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.display().to_string()),
        }
    }

    pub(super) fn dir(&self) -> Option<&Path> {
        match self {
            Loc::Pc | Loc::Bin => None,
            Loc::Dir(p) => Some(p),
        }
    }

    pub(super) fn parent(&self) -> Option<Loc> {
        match self {
            Loc::Pc | Loc::Bin => None,
            Loc::Dir(p) => Some(p.parent().map_or(Loc::Pc, |q| Loc::Dir(q.to_path_buf()))),
        }
    }
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
    /// Every entry index, sorted by `sorted_for`. Typing in the filter only walks this, so a
    /// keystroke never re-sorts. Hidden items stay in it and are skipped while filtering.
    order: Vec<usize>,
    /// Sort the cached `order` was built for; `None` means it must be rebuilt.
    sorted_for: Option<(SortKey, bool)>,
    /// Identity of `entries` when `order` was built, so replacing them is noticed even when the
    /// loader assigns the field directly instead of calling `set_entries`.
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
            order: Vec::new(),
            sorted_for: None,
            entries_sig: (0, 0),
            index: HashMap::new(),
            rows: Vec::new(),
        }
    }

    /// Replace the entries (a fresh listing). Forces one re-sort on the next `rebuild_view`.
    pub(super) fn set_entries(&mut self, entries: Vec<Entry>) {
        self.entries = entries;
        self.sorted_for = None;
    }

    /// Re-sorts only when the entries or the sort changed; otherwise just re-filters the cached
    /// order, which is a linear scan with no comparisons.
    pub(super) fn rebuild_view(&mut self, show_hidden: bool) {
        let sig = (self.entries.len(), self.entries.as_ptr() as usize);
        if sig != self.entries_sig {
            self.entries_sig = sig;
            self.sorted_for = None;
        }
        if self.sorted_for != Some((self.sort, self.ascending)) {
            self.resort();
        }
        let needle = self.filter.trim().to_lowercase();
        let entries = &self.entries;
        self.view.clear();
        self.view.extend(self.order.iter().copied().filter(|&i| {
            let e = &entries[i];
            (show_hidden || !e.hidden) && (needle.is_empty() || e.key.contains(&needle))
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
                index
                    .get(p)
                    .is_some_and(|&i| rows.get(i).is_some_and(|&r| r != u32::MAX))
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
        // The path index depends only on the entries, so rebuild it only when they changed.
        if self.index.len() != entries.len() || !self.index_matches() {
            self.index = entries
                .iter()
                .enumerate()
                .map(|(i, e)| (e.path.clone(), i))
                .collect();
        }
    }

    /// Cheap spot check that the index still describes `entries` after a sort-only rebuild.
    fn index_matches(&self) -> bool {
        self.entries
            .first()
            .is_none_or(|e| self.index.get(&e.path) == Some(&0))
            && self
                .entries
                .last()
                .is_none_or(|e| self.index.get(&e.path) == Some(&(self.entries.len() - 1)))
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
