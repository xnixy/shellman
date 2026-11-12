//! One tab: where it is, what it lists, and what is selected.

use crate::fsops::{self, Entry};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone, PartialEq, Debug)]
pub(super) enum Loc {
    Pc,
    Dir(PathBuf),
}

impl Loc {
    pub(super) fn title(&self) -> String {
        match self {
            Loc::Pc => "This PC".into(),
            Loc::Dir(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.display().to_string()),
        }
    }

    pub(super) fn dir(&self) -> Option<&Path> {
        match self {
            Loc::Pc => None,
            Loc::Dir(p) => Some(p),
        }
    }

    pub(super) fn parent(&self) -> Option<Loc> {
        match self {
            Loc::Pc => None,
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
        }
    }

    pub(super) fn rebuild_view(&mut self, show_hidden: bool) {
        let needle = self.filter.trim().to_lowercase();
        let entries = &self.entries;
        let mut view: Vec<usize> = (0..entries.len())
            .filter(|&i| {
                let e = &entries[i];
                (show_hidden || !e.hidden) && (needle.is_empty() || e.key.contains(&needle))
            })
            .collect();
        let (key, asc) = (self.sort, self.ascending);
        view.sort_by(|&a, &b| {
            let (a, b) = (&entries[a], &entries[b]);
            // Folders always come first, whichever way the list is sorted.
            b.is_dir.cmp(&a.is_dir).then_with(|| {
                let ord = match key {
                    SortKey::Name => std::cmp::Ordering::Equal,
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
        self.view = view;
        self.dirty = false;
        self.selected
            .retain(|p| self.view.iter().any(|&i| &entries[i].path == p));
        self.cursor = self.cursor.filter(|&c| c < self.view.len());
        self.anchor = self.anchor.filter(|&c| c < self.view.len());
    }

    pub(super) fn entry(&self, row: usize) -> &Entry {
        &self.entries[self.view[row]]
    }

    pub(super) fn row_of(&self, path: &Path) -> Option<usize> {
        (0..self.view.len()).find(|&r| self.entry(r).path == path)
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
