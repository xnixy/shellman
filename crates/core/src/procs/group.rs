//! Processes into groups: an app and its helpers (a browser and its content processes) become
//! one row that opens to show them, and groups with a window are the Apps, the rest Background.
//!
//! A process joins its parent's group when the parent is still the process that started it (it
//! started earlier: a PID can be reused by a newer process, which must not adopt the old one's
//! children), runs in the same session, and is the same program, or a helper every app may have
//! (its console host, its web view).

use super::Proc;
use std::collections::HashMap;

/// Helpers that belong to whoever started them.
const HELPERS: [&str; 3] = ["conhost.exe", "openconsole.exe", "msedgewebview2.exe"];

/// One row of the list: a process and the ones that joined it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// Index of the topmost process of the group.
    pub root: usize,
    /// Indexes of the rest, oldest first.
    pub members: Vec<usize>,
    /// Some process of it has a window.
    pub app: bool,
}

impl Group {
    /// The root and then the members.
    pub fn all(&self) -> impl Iterator<Item = usize> + '_ {
        std::iter::once(self.root).chain(self.members.iter().copied())
    }
}

/// The index of the process that `procs[i]` joins, if it joins one.
fn joins(procs: &[Proc], by_pid: &HashMap<u32, usize>, i: usize) -> Option<usize> {
    let child = &procs[i];
    let &p = by_pid.get(&child.parent)?;
    let parent = &procs[p];
    if p == i || parent.key.pid <= 4 || parent.key.created > child.key.created || parent.session != child.session {
        return None;
    }
    let same = parent.name.eq_ignore_ascii_case(&child.name);
    let helper = HELPERS.iter().any(|h| child.name.eq_ignore_ascii_case(h));
    (same || helper).then_some(p)
}

/// Every process in exactly one group. Groups come in the order of their roots in `procs`.
pub fn group(procs: &[Proc]) -> Vec<Group> {
    let by_pid: HashMap<u32, usize> = procs.iter().enumerate().map(|(i, p)| (p.key.pid, i)).collect();
    let up: Vec<Option<usize>> = (0..procs.len()).map(|i| joins(procs, &by_pid, i)).collect();
    let root_of = |mut i: usize| {
        // Start times only go back up the chain, so it cannot loop; the bound is for safety.
        for _ in 0..procs.len() {
            match up[i] {
                Some(p) => i = p,
                None => break,
            }
        }
        i
    };
    let mut slot: HashMap<usize, usize> = HashMap::new();
    let mut groups: Vec<Group> = Vec::new();
    for (i, proc) in procs.iter().enumerate() {
        let root = root_of(i);
        let at = *slot.entry(root).or_insert_with(|| {
            groups.push(Group { root, members: Vec::new(), app: false });
            groups.len() - 1
        });
        if i != root {
            groups[at].members.push(i);
        }
        groups[at].app |= !proc.windows.is_empty();
    }
    // The root may come after its members in `procs`; put groups in the roots' order.
    groups.sort_by_key(|g| g.root);
    for g in &mut groups {
        g.members.sort_by_key(|&m| (procs[m].key.created, procs[m].key.pid));
    }
    groups
}

/// A group's numbers as its row shows them: everything added up, the GPU's busiest engine.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Totals {
    pub cpu: f32,
    pub memory: u64,
    pub disk: f64,
    pub gpu: Option<f32>,
}

pub fn totals(procs: &[Proc], group: &Group) -> Totals {
    group.all().fold(Totals::default(), |t, i| {
        let p = &procs[i];
        Totals {
            cpu: (t.cpu + p.cpu).min(100.0),
            memory: t.memory + p.memory,
            disk: t.disk + p.disk,
            gpu: match (t.gpu, p.gpu) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (a, b) => a.or(b),
            },
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procs::{Key, Window};

    fn p(pid: u32, parent: u32, created: i64, name: &str) -> Proc {
        Proc { key: Key { pid, created }, parent, name: name.into(), session: 1, ..Default::default() }
    }

    fn windowed(mut p: Proc) -> Proc {
        p.windows.push(Window { hwnd: p.key.pid as isize, title: "w".into(), hung: false });
        p
    }

    #[test]
    fn a_browser_takes_its_helpers() {
        let procs = vec![
            p(100, 50, 10, "launcher.exe"),
            windowed(p(200, 100, 20, "firefox.exe")),
            p(201, 200, 30, "firefox.exe"),
            p(202, 200, 25, "Firefox.exe"),
            p(203, 202, 40, "firefox.exe"),
            p(300, 100, 30, "notepad.exe"),
        ];
        let g = group(&procs);
        assert_eq!(g.len(), 3);
        assert_eq!(g[1], Group { root: 1, members: vec![3, 2, 4], app: true }, "nested helpers too, oldest first");
        assert!(!g[0].app && g[0].members.is_empty(), "a different program is its own row");
        assert!(!g[2].app);
    }

    #[test]
    fn a_reused_pid_adopts_nobody() {
        // PID 200 now belongs to a process that started after the child it would adopt.
        let procs = vec![p(200, 1, 500, "firefox.exe"), p(201, 200, 100, "firefox.exe")];
        let g = group(&procs);
        assert_eq!(g.len(), 2);
    }

    #[test]
    fn console_hosts_follow_their_app_but_other_sessions_do_not() {
        let mut other = p(31, 30, 6, "cmd.exe");
        other.session = 0;
        let procs = vec![p(30, 1, 5, "cmd.exe"), p(40, 30, 6, "conhost.exe"), other];
        let g = group(&procs);
        assert_eq!(g[0].members, vec![1]);
        assert_eq!(g.len(), 2);
    }

    #[test]
    fn a_root_listed_after_its_members_still_heads_its_group() {
        let procs = vec![p(201, 200, 30, "code.exe"), windowed(p(200, 1, 20, "code.exe"))];
        let g = group(&procs);
        assert_eq!(g, vec![Group { root: 1, members: vec![0], app: true }]);
    }

    #[test]
    fn system_parents_never_adopt() {
        let procs = vec![p(4, 0, 1, "System"), p(90, 4, 2, "System")];
        assert_eq!(group(&procs).len(), 2);
    }

    #[test]
    fn totals_add_up() {
        let mut a = p(10, 1, 1, "a.exe");
        a.cpu = 60.0;
        a.memory = 10;
        a.disk = 1.0;
        a.gpu = Some(5.0);
        let mut b = p(20, 10, 2, "a.exe");
        b.cpu = 50.0;
        b.memory = 5;
        b.gpu = Some(9.0);
        let procs = vec![a, b];
        let g = &group(&procs)[0];
        let t = totals(&procs, g);
        assert_eq!(t, Totals { cpu: 100.0, memory: 15, disk: 1.0, gpu: Some(9.0) });
    }
}
