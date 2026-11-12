//! Ending things, step by step, inline in the command pill (no dialogs).
//!
//! End task on an app asks its windows to close, as their close buttons would; if it is still
//! there, "End now" ends it at once. A background process is ended at once, after "End <name>?"
//! in the pill. A protected one asks with a warning; a critical one cannot be ended (the button
//! is off). End process tree always asks first and ends the children before the parent.

use crate::model::{List, Target};
use shellman_core::procs::policy::{self, Verdict};
use shellman_core::procs::{Control, Key};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    /// End task: the process, or its group.
    End,
    /// End process tree: the process and everything it started.
    Tree,
}

/// What the pill is asking or saying.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Ask {
    #[default]
    Nothing,
    /// "End <name>?" with End and Cancel.
    Confirm { act: Act, keys: Vec<Key>, name: String, warning: Option<&'static str> },
    /// The windows were asked to close; "End now" if it does not.
    Closing { keys: Vec<Key>, name: String },
    /// What happened, when it did not go as asked.
    Note(String),
}

#[derive(Default)]
pub struct Actions {
    pub ask: Ask,
}

/// The strictest verdict over the processes: a group with one critical process is critical.
pub fn verdict(list: &List, procs: &[usize], own_pid: u32) -> Verdict {
    let rank = |v: &Verdict| match v {
        Verdict::Allowed => 0,
        Verdict::Protected(_) => 1,
        Verdict::Denied(_) => 2,
        Verdict::Critical(_) => 3,
    };
    procs
        .iter()
        .map(|&i| policy::may_end(&list.procs[i], own_pid))
        .max_by_key(rank)
        .unwrap_or(Verdict::Allowed)
}

/// The processes an act would end, in the order to end them.
pub fn affected(list: &List, t: Target, act: Act) -> Vec<usize> {
    match act {
        Act::End => {
            // Members before the root, as for a tree.
            let mut v = list.target_procs(t);
            v.rotate_left(1);
            v
        }
        Act::Tree => list.tree(t.proc),
    }
}

impl Actions {
    /// The End task or End process tree button.
    pub fn start(&mut self, list: &List, act: Act, control: &dyn Control, own_pid: u32) {
        let Some(t) = list.target() else {
            return;
        };
        let procs = affected(list, t, act);
        let v = verdict(list, &procs, own_pid);
        if !v.can_end() {
            return;
        }
        let keys: Vec<Key> = procs.iter().map(|&i| list.procs[i].key).collect();
        let name = match t.group {
            Some(g) => list.group_label(g).to_owned(),
            None => list.procs[t.proc].description.to_string(),
        };
        let windows = windows_of(list, &procs);
        let warning = v.reason();
        if act == Act::End && !windows.is_empty() && warning.is_none() {
            control.close_windows(&windows);
            self.ask = Ask::Closing { keys, name };
        } else {
            self.ask = Ask::Confirm { act, keys, name, warning };
        }
    }

    /// The confirm's End button.
    pub fn confirm(&mut self, list: &List, control: &dyn Control) {
        let Ask::Confirm { act, keys, name, .. } = std::mem::take(&mut self.ask) else {
            return;
        };
        let procs: Vec<usize> = keys.iter().filter_map(|&k| list.index(k)).collect();
        let windows = windows_of(list, &procs);
        if act == Act::End && !windows.is_empty() {
            control.close_windows(&windows);
            self.ask = Ask::Closing { keys, name };
        } else {
            self.terminate(&keys, &name, control);
        }
    }

    /// "End now", while the windows have not closed.
    pub fn end_now(&mut self, control: &dyn Control) {
        if let Ask::Closing { keys, name } = std::mem::take(&mut self.ask) {
            self.terminate(&keys, &name, control);
        }
    }

    pub fn cancel(&mut self) {
        self.ask = Ask::Nothing;
    }

    fn terminate(&mut self, keys: &[Key], name: &str, control: &dyn Control) {
        let failed: Vec<String> = keys.iter().filter_map(|&k| control.terminate(k).err()).collect();
        self.ask = match failed.first() {
            None => Ask::Nothing,
            Some(why) if failed.len() == keys.len() => Ask::Note(format!("{name} was not ended. {why}")),
            Some(why) => Ask::Note(format!("{} of {} processes of {name} were not ended. {why}", failed.len(), keys.len())),
        };
    }

    /// After a new sample: a question about processes that are all gone is over.
    pub fn follow(&mut self, list: &List) {
        let keys = match &self.ask {
            Ask::Confirm { keys, .. } | Ask::Closing { keys, .. } => keys,
            _ => return,
        };
        if keys.iter().all(|&k| list.index(k).is_none()) {
            self.ask = Ask::Nothing;
        }
    }
}

fn windows_of(list: &List, procs: &[usize]) -> Vec<isize> {
    procs.iter().flat_map(|&i| list.procs[i].windows.iter().map(|w| w.hwnd)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{app, proc, sample};
    use shellman_core::procs::{Access, Details};
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        closed: RefCell<Vec<isize>>,
        ended: RefCell<Vec<u32>>,
        refuse: Option<u32>,
    }

    impl Control for Fake {
        fn close_windows(&self, windows: &[isize]) -> usize {
            self.closed.borrow_mut().extend_from_slice(windows);
            windows.len()
        }
        fn terminate(&self, key: Key) -> Result<(), String> {
            if self.refuse == Some(key.pid) {
                return Err("Access is denied.".into());
            }
            self.ended.borrow_mut().push(key.pid);
            Ok(())
        }
        fn details(&self, _: Key) -> Details {
            Details::default()
        }
    }

    fn list_selecting(pid: u32) -> List {
        let mut l = List::default();
        l.set_procs(sample());
        l.select(Some(Key { pid, created: pid as i64 }));
        l
    }

    #[test]
    fn an_app_is_asked_to_close_then_can_be_ended_now() {
        let l = list_selecting(200);
        let (mut a, f) = (Actions::default(), Fake::default());
        a.start(&l, Act::End, &f, 1);
        assert_eq!(*f.closed.borrow(), [2000]);
        assert!(f.ended.borrow().is_empty());
        assert!(matches!(a.ask, Ask::Closing { .. }));
        a.end_now(&f);
        assert_eq!(*f.ended.borrow(), [200]);
        assert_eq!(a.ask, Ask::Nothing);
    }

    #[test]
    fn a_group_ends_members_before_its_root() {
        let l = list_selecting(100);
        let (mut a, f) = (Actions::default(), Fake::default());
        a.start(&l, Act::End, &f, 1);
        a.end_now(&f);
        assert_eq!(*f.ended.borrow(), [101, 102, 100]);
    }

    #[test]
    fn background_asks_first() {
        let l = list_selecting(300);
        let (mut a, f) = (Actions::default(), Fake::default());
        a.start(&l, Act::End, &f, 1);
        // audiodg is protected: the confirm carries a warning.
        let Ask::Confirm { warning, ref name, .. } = a.ask else { panic!("{:?}", a.ask) };
        assert!(warning.is_some() && name == "audiodg");
        assert!(f.ended.borrow().is_empty());
        a.confirm(&l, &f);
        assert_eq!(*f.ended.borrow(), [300]);
    }

    #[test]
    fn cancel_ends_nothing() {
        let mut l = List::default();
        l.set_procs(vec![proc(50, 1, "worker.exe")]);
        l.select(Some(Key { pid: 50, created: 50 }));
        let (mut a, f) = (Actions::default(), Fake::default());
        a.start(&l, Act::End, &f, 1);
        assert!(matches!(a.ask, Ask::Confirm { warning: None, .. }));
        a.cancel();
        a.confirm(&l, &f);
        assert!(f.ended.borrow().is_empty());
    }

    #[test]
    fn critical_and_denied_do_nothing() {
        let mut l = List::default();
        let mut denied = proc(60, 1, "elevated.exe");
        denied.access = Access::ReadOnly;
        l.set_procs(vec![proc(50, 1, "csrss.exe"), denied]);
        let (mut a, f) = (Actions::default(), Fake::default());
        for pid in [50, 60] {
            l.select(Some(Key { pid, created: pid as i64 }));
            a.start(&l, Act::End, &f, 1);
            a.start(&l, Act::Tree, &f, 1);
            assert_eq!(a.ask, Ask::Nothing);
        }
        assert!(f.ended.borrow().is_empty() && f.closed.borrow().is_empty());
    }

    #[test]
    fn a_tree_asks_and_ends_children_first() {
        let mut l = List::default();
        l.set_procs(vec![proc(70, 1, "cmd.exe"), proc(71, 70, "ping.exe"), app(72, 71, "x.exe", "X")]);
        l.select(Some(Key { pid: 70, created: 70 }));
        let (mut a, f) = (Actions::default(), Fake::default());
        a.start(&l, Act::Tree, &f, 1);
        assert!(matches!(a.ask, Ask::Confirm { act: Act::Tree, .. }));
        a.confirm(&l, &f);
        assert_eq!(*f.ended.borrow(), [72, 71, 70]);
        assert!(f.closed.borrow().is_empty(), "a tree is ended, not asked to close");
    }

    #[test]
    fn a_refusal_is_told() {
        let mut l = List::default();
        l.set_procs(vec![proc(80, 1, "worker.exe")]);
        l.select(Some(Key { pid: 80, created: 80 }));
        let mut a = Actions::default();
        let f = Fake { refuse: Some(80), ..Default::default() };
        a.start(&l, Act::End, &f, 1);
        a.confirm(&l, &f);
        assert!(matches!(&a.ask, Ask::Note(n) if n.contains("not ended") && n.contains("denied")));
    }

    #[test]
    fn a_question_about_ended_processes_goes_away() {
        let mut l = list_selecting(200);
        let (mut a, f) = (Actions::default(), Fake::default());
        a.start(&l, Act::End, &f, 1);
        let mut next = sample();
        next.retain(|p| p.key.pid != 200);
        l.set_procs(next);
        a.follow(&l);
        assert_eq!(a.ask, Ask::Nothing);
    }
}
