//! The process list as the window shows it, kept apart from drawing so it is tested with made-up
//! samples: groups (an app and its helpers), the Apps and Background sections, the sort, the
//! search, which groups are open, and the selection, kept by process key across samples.
//!
//! While the pointer is over the list the order is held: rows keep their places, new ones come
//! at the end of their section, and the real order comes back when the pointer leaves.

use shellman_core::procs::group::{self, Group, Totals};
use shellman_core::procs::sort::{self, SortBy, Values};
use shellman_core::procs::{search, Key, Proc, Status};
use std::collections::{HashMap, HashSet};

/// Where a row sits, for holding the order: in a section, or in an open group.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Parent {
    Apps,
    Background,
    Group(Key),
}

/// One line of the list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Line {
    /// "Apps (5)" or "Background processes (212)".
    Heading { apps: bool, count: usize },
    /// A group's row (a lone process is a group of one); `group` indexes [`List::groups`].
    /// `open`: its processes are listed under it.
    Group { group: usize, open: bool },
    /// A process inside an open group; `proc` indexes [`List::procs`].
    Member { group: usize, proc: usize },
    /// The details of the selected process, under its row.
    Details { proc: usize },
}

/// What a command acts on: one process, or a whole group when its row is selected.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Target {
    pub proc: usize,
    /// Set when a group of several processes is selected by its row.
    pub group: Option<usize>,
}

#[derive(Default)]
pub struct List {
    pub procs: Vec<Proc>,
    pub groups: Vec<Group>,
    pub totals: Vec<Totals>,
    pub lines: Vec<Line>,
    pub sort: SortBy,
    pub query: String,
    pub expanded: HashSet<Key>,
    pub selected: Option<Key>,
    /// The selection was made on a row inside an open group (a group's root has its group's
    /// row and its own row inside; only the group's row stands for the whole group).
    pub as_member: bool,
    /// Whether the details row is open under the selection.
    pub details: bool,
    /// Scroll the selection (and its details) into view on the next frame.
    pub reveal: bool,
    /// A row has the keyboard this frame (set while drawing).
    pub row_focus: bool,
    /// A row had the keyboard last frame: the newly selected row takes it over.
    pub take_focus: bool,
    /// The arrows moved the selection while a row had the keyboard, and the selected row has not
    /// taken it yet (it may be drawn only once scrolled into view).
    pub focus_pending: bool,
    /// With nothing selected, the row that Tab stops at (the first one), so the list is reachable.
    pub tab_stop: Option<Key>,
    held: bool,
    shown: HashMap<Parent, Vec<Key>>,
    by_key: HashMap<Key, usize>,
}

impl List {
    /// Take a new sample. The selection stays on its process while that runs.
    pub fn set_procs(&mut self, procs: Vec<Proc>) {
        self.procs = procs;
        self.by_key = self.procs.iter().enumerate().map(|(i, p)| (p.key, i)).collect();
        self.groups = group::group(&self.procs);
        self.totals = self.groups.iter().map(|g| group::totals(&self.procs, g)).collect();
        if self.selected.is_some_and(|k| !self.by_key.contains_key(&k)) {
            self.selected = None;
            self.details = false;
        }
        self.expanded.retain(|k| self.by_key.contains_key(k));
        self.rebuild();
    }

    /// Hold the order (the pointer is over the list) or let it go.
    pub fn hold(&mut self, held: bool) {
        if self.held != held {
            self.held = held;
            if !held {
                self.rebuild();
            }
        }
    }

    pub fn set_sort(&mut self, sort: SortBy) {
        self.sort = sort;
        // A new sort is asked for: show it even under the pointer.
        self.shown.clear();
        self.rebuild();
    }

    pub fn set_query(&mut self, query: &str) {
        if self.query != query {
            self.query = query.to_owned();
            self.shown.clear();
            self.rebuild();
        }
    }

    pub fn toggle(&mut self, root: Key) {
        if !self.expanded.remove(&root) {
            self.expanded.insert(root);
        }
        self.rebuild();
    }

    /// Select a process by its group's row (or its only row), or nothing.
    pub fn select(&mut self, key: Option<Key>) {
        match key {
            Some(k) => self.select_row(k, false),
            None => {
                self.selected = None;
                self.details = false;
                self.rebuild();
            }
        }
    }

    /// Select the row of `key` that was clicked: inside its group (`member`) or the group's.
    pub fn select_row(&mut self, key: Key, member: bool) {
        if self.selected != Some(key) || self.as_member != member {
            if self.selected != Some(key) {
                self.details = false;
            }
            self.selected = Some(key);
            self.as_member = member;
            self.rebuild();
        }
    }

    /// Whether `proc` heads a group of more than itself.
    fn heads_group(&self, proc: usize) -> bool {
        self.groups.iter().any(|g| g.root == proc && !g.members.is_empty())
    }

    pub fn toggle_details(&mut self) {
        self.details = !self.details && self.selected.is_some();
        self.reveal |= self.details;
        self.rebuild();
    }

    pub fn index(&self, key: Key) -> Option<usize> {
        self.by_key.get(&key).copied()
    }

    /// What the selection's commands act on.
    pub fn target(&self) -> Option<Target> {
        let proc = self.index(self.selected?)?;
        // Selected by its group's row: the whole group, when it has more than the one process.
        let group = self
            .groups
            .iter()
            .position(|g| g.root == proc && !g.members.is_empty())
            .filter(|_| !self.as_member);
        Some(Target { proc, group })
    }

    /// The processes a target stands for: the group's, or the one.
    pub fn target_procs(&self, t: Target) -> Vec<usize> {
        match t.group {
            Some(g) => self.groups[g].all().collect(),
            None => vec![t.proc],
        }
    }

    /// The process and every process it started, by parent PID, children before parents. A
    /// child must have started after its parent (a reused PID is not a parent).
    pub fn tree(&self, root: usize) -> Vec<usize> {
        let children = |k: Key| -> Vec<Key> {
            self.procs
                .iter()
                .filter(|p| p.parent == k.pid && p.key != k && p.key.created >= k.created)
                .map(|p| p.key)
                .collect()
        };
        shellman_core::procs::end::tree_order(self.procs[root].key, &children)
            .into_iter()
            .filter_map(|k| self.index(k))
            .collect()
    }

    /// What a group's row is called: its window's title for an app, else its friendly name.
    pub fn group_label(&self, g: usize) -> &str {
        let group = &self.groups[g];
        if group.app {
            if let Some(t) = group.all().find_map(|i| self.procs[i].title()) {
                return t;
            }
        }
        &self.procs[group.root].description
    }

    /// A group's status: not responding if any of it is, suspended if all of it is.
    pub fn group_status(&self, g: usize) -> Status {
        let statuses: Vec<Status> = self.groups[g].all().map(|i| self.procs[i].status()).collect();
        if statuses.contains(&Status::NotResponding) {
            Status::NotResponding
        } else if statuses.iter().all(|s| *s == Status::Suspended) {
            Status::Suspended
        } else {
            Status::Running
        }
    }

    fn group_values(&self, g: usize) -> Values {
        let t = self.totals[g];
        Values {
            name: self.group_label(g).to_owned(),
            cpu: t.cpu,
            memory: t.memory,
            disk: t.disk,
            gpu: t.gpu.unwrap_or(0.0),
            pid: self.procs[self.groups[g].root].key.pid,
            status: self.group_status(g),
        }
    }

    fn proc_values(&self, i: usize) -> Values {
        let p = &self.procs[i];
        Values {
            name: p.title().unwrap_or(&p.description).to_owned(),
            cpu: p.cpu,
            memory: p.memory,
            disk: p.disk,
            gpu: p.gpu.unwrap_or(0.0),
            pid: p.key.pid,
            status: p.status(),
        }
    }

    /// Sort `items` (keys with their values) and hold them in place if the order is held.
    fn order(&mut self, parent: Parent, mut items: Vec<(Key, Values)>) -> Vec<Key> {
        items.sort_by(|a, b| sort::compare(&a.1, &b.1, self.sort));
        let sorted: Vec<Key> = items.into_iter().map(|(k, _)| k).collect();
        let keys = match (self.held, self.shown.get(&parent)) {
            (true, Some(shown)) => sort::hold(shown, &sorted),
            _ => sorted,
        };
        self.shown.insert(parent, keys.clone());
        keys
    }

    /// Lay the lines out again from the processes and the state.
    pub fn rebuild(&mut self) {
        let searching = !self.query.trim().is_empty();
        let hit: Vec<bool> = self.procs.iter().map(|p| !searching || search::matches(p, &self.query)).collect();
        let group_of_root: HashMap<Key, usize> =
            self.groups.iter().enumerate().map(|(g, gr)| (self.procs[gr.root].key, g)).collect();
        let mut lines = Vec::new();
        for apps in [true, false] {
            let parent = if apps { Parent::Apps } else { Parent::Background };
            let items: Vec<(Key, Values)> = (0..self.groups.len())
                .filter(|&g| self.groups[g].app == apps && self.groups[g].all().any(|i| hit[i]))
                .map(|g| (self.procs[self.groups[g].root].key, self.group_values(g)))
                .collect();
            if items.is_empty() {
                continue;
            }
            lines.push(Line::Heading { apps, count: items.len() });
            for root in self.order(parent, items) {
                let g = group_of_root[&root];
                let open = !self.groups[g].members.is_empty()
                    && (self.expanded.contains(&root) || (searching && self.groups[g].members.iter().any(|&m| hit[m])));
                lines.push(Line::Group { group: g, open });
                self.details_line(&mut lines, self.groups[g].root, false);
                if !open {
                    continue;
                }
                // The root is listed again inside, so each process of the group has a row.
                let items: Vec<(Key, Values)> = self.groups[g]
                    .all()
                    .filter(|&m| hit[m])
                    .map(|m| (self.procs[m].key, self.proc_values(m)))
                    .collect();
                for key in self.order(Parent::Group(root), items) {
                    let proc = self.by_key[&key];
                    lines.push(Line::Member { group: g, proc });
                    self.details_line(&mut lines, proc, true);
                }
            }
        }
        self.lines = lines;
    }

    /// The details line, after the selected row: `member` says which kind of row was just added.
    fn details_line(&self, lines: &mut Vec<Line>, proc: usize, member: bool) {
        if !self.details || self.selected != Some(self.procs[proc].key) {
            return;
        }
        // A group's root has two rows; the details go under the one that was clicked.
        if self.heads_group(proc) && member != self.as_member {
            return;
        }
        lines.push(Line::Details { proc });
    }
}

/// Each line's top, from the top of the list, and the list's height. Details lines are taller.
pub fn tops(lines: &[Line], row: f32, details: f32) -> (Vec<f32>, f32) {
    let mut y = 0.0;
    let tops = lines
        .iter()
        .map(|l| {
            let top = y;
            y += if matches!(l, Line::Details { .. }) { details } else { row };
            top
        })
        .collect();
    (tops, y)
}

/// Which lines overlap `from..to` (offsets from the list's top), given each line's top.
pub fn visible(tops: &[f32], height: f32, from: f32, to: f32) -> std::ops::Range<usize> {
    let first = tops.partition_point(|&t| t <= from).saturating_sub(1);
    let end = tops.partition_point(|&t| t < to);
    if tops.is_empty() || from >= height {
        return tops.len()..tops.len();
    }
    first..end.max(first)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use shellman_core::procs::{Window, sort::Column};

    pub fn proc(pid: u32, parent: u32, name: &str) -> Proc {
        Proc {
            key: Key { pid, created: pid as i64 },
            parent,
            name: name.into(),
            description: name.trim_end_matches(".exe").into(),
            session: 1,
            ..Default::default()
        }
    }

    pub fn app(pid: u32, parent: u32, name: &str, title: &str) -> Proc {
        let mut p = proc(pid, parent, name);
        p.windows.push(Window { hwnd: pid as isize * 10, title: title.into(), hung: false });
        p
    }

    /// Firefox with two helpers, Notepad, and two background processes.
    pub fn sample() -> Vec<Proc> {
        let mut v = vec![
            proc(10, 1, "svchost.exe"),
            app(100, 10, "firefox.exe", "Islands — Mozilla Firefox"),
            proc(101, 100, "firefox.exe"),
            proc(102, 100, "firefox.exe"),
            app(200, 10, "notepad.exe", "notes.txt - Notepad"),
            proc(300, 10, "audiodg.exe"),
        ];
        v[2].cpu = 30.0;
        v[4].cpu = 5.0;
        v[5].cpu = 50.0;
        v
    }

    fn kinds(l: &List) -> Vec<String> {
        l.lines
            .iter()
            .map(|line| match *line {
                Line::Heading { apps, count } => format!("{}{count}", if apps { "A" } else { "B" }),
                Line::Group { group, .. } => format!("g{}", l.procs[l.groups[group].root].key.pid),
                Line::Member { proc, .. } => format!("m{}", l.procs[proc].key.pid),
                Line::Details { proc } => format!("d{}", l.procs[proc].key.pid),
            })
            .collect()
    }

    #[test]
    fn apps_first_then_background_by_name() {
        let mut l = List::default();
        l.set_procs(sample());
        // Apps by title: "Islands — Mozilla Firefox" before "notes.txt"; background by name.
        assert_eq!(kinds(&l), ["A2", "g100", "g200", "B2", "g300", "g10"]);
        assert_eq!(l.group_label(1), "Islands — Mozilla Firefox");
    }

    #[test]
    fn a_group_opens_to_its_processes() {
        let mut l = List::default();
        l.set_procs(sample());
        l.toggle(Key { pid: 100, created: 100 });
        // Inside, by name: the helpers ("firefox") before the window ("Islands — ...").
        assert_eq!(kinds(&l)[..5], ["A2", "g100", "m101", "m102", "m100"]);
        l.set_sort(SortBy::default().clicked(Column::Cpu));
        // Firefox's group adds to 30, Notepad 5; inside, the busy helper first, ties by name.
        assert_eq!(kinds(&l)[..6], ["A2", "g100", "m101", "m102", "m100", "g200"]);
    }

    #[test]
    fn the_order_is_held_under_the_pointer() {
        let mut l = List::default();
        l.set_sort(SortBy { column: Column::Cpu, descending: true });
        l.set_procs(sample());
        assert_eq!(kinds(&l)[3..], ["B2", "g300", "g10"]);
        l.hold(true);
        let mut next = sample();
        next[0].cpu = 90.0; // svchost now busiest
        next.push(proc(400, 10, "zz.exe"));
        l.set_procs(next.clone());
        assert_eq!(kinds(&l)[3..], ["B3", "g300", "g10", "g400"], "held: the new one at the end");
        l.hold(false);
        assert_eq!(kinds(&l)[3..], ["B3", "g10", "g300", "g400"]);
    }

    #[test]
    fn the_selection_follows_its_process() {
        let mut l = List::default();
        l.set_procs(sample());
        l.select(Some(Key { pid: 300, created: 300 }));
        l.toggle_details();
        assert!(kinds(&l).contains(&"d300".to_owned()));
        let mut next = sample();
        next.retain(|p| p.key.pid != 10);
        l.set_procs(next);
        assert_eq!(l.selected, Some(Key { pid: 300, created: 300 }));
        // The process ends: the selection goes with it.
        let mut gone = sample();
        gone.retain(|p| p.key.pid != 300);
        l.set_procs(gone);
        assert_eq!(l.selected, None);
        assert!(!l.details);
    }

    #[test]
    fn a_reused_pid_is_not_the_selected_process() {
        let mut l = List::default();
        l.set_procs(sample());
        l.select(Some(Key { pid: 300, created: 300 }));
        let mut next = sample();
        next[5].key.created = 999;
        l.set_procs(next);
        assert_eq!(l.selected, None);
    }

    #[test]
    fn search_shows_matching_members_in_open_groups() {
        let mut l = List::default();
        l.set_procs(sample());
        l.set_query("102");
        assert_eq!(kinds(&l), ["A1", "g100", "m102"]);
        l.set_query("audio");
        assert_eq!(kinds(&l), ["B1", "g300"]);
        l.set_query("nothing-like-this");
        assert!(l.lines.is_empty());
    }

    #[test]
    fn targets_are_the_group_or_the_process() {
        let mut l = List::default();
        l.set_procs(sample());
        l.select(Some(Key { pid: 100, created: 100 }));
        let t = l.target().unwrap();
        assert!(t.group.is_some());
        assert_eq!(l.target_procs(t).len(), 3);
        l.select(Some(Key { pid: 102, created: 102 }));
        let t = l.target().unwrap();
        assert_eq!(t.group, None);
        assert_eq!(l.target_procs(t), [3]);
    }

    #[test]
    fn a_tree_ends_children_first() {
        let mut l = List::default();
        l.set_procs(sample());
        let tree: Vec<u32> = l.tree(0).into_iter().map(|i| l.procs[i].key.pid).collect();
        assert_eq!(tree.last(), Some(&10));
        assert_eq!(tree.len(), 6);
        let pos = |pid| tree.iter().position(|&p| p == pid).unwrap();
        assert!(pos(101) < pos(100));
    }

    #[test]
    fn hung_and_suspended_groups() {
        let mut v = sample();
        v[3].windows.push(Window { hwnd: 5, title: "x".into(), hung: true });
        v[5].suspended = true;
        let mut l = List::default();
        l.set_procs(v);
        assert_eq!(l.group_status(1), Status::NotResponding);
        assert_eq!(l.group_status(3), Status::Suspended);
        assert_eq!(l.group_status(2), Status::Running);
    }

    #[test]
    fn only_lines_in_view_are_drawn() {
        let lines = vec![Line::Heading { apps: true, count: 1 }; 100];
        let (tops, height) = tops(&lines, 30.0, 90.0);
        assert_eq!(height, 3000.0);
        assert_eq!(visible(&tops, height, 0.0, 300.0), 0..10);
        assert_eq!(visible(&tops, height, 45.0, 100.0), 1..4);
        assert_eq!(visible(&tops, height, 5000.0, 5300.0), 100..100);
        assert_eq!(visible(&[], 0.0, 0.0, 300.0), 0..0);
        let mixed = [Line::Details { proc: 0 }, Line::Heading { apps: true, count: 1 }];
        let (tops, h) = super::tops(&mixed, 30.0, 90.0);
        assert_eq!((tops.as_slice(), h), ([0.0, 90.0].as_slice(), 120.0));
    }
}
