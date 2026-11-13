//! What the Services list does with what was read: the labels, the sort, the search. No Win32.

use super::{Service, State, Startup};
use std::cmp::Ordering;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Column {
    Display,
    Name,
    Status,
    Startup,
    Pid,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SortBy {
    pub column: Column,
    pub descending: bool,
}

impl Default for SortBy {
    fn default() -> Self {
        Self { column: Column::Display, descending: false }
    }
}

impl SortBy {
    /// A click on `column`'s header: the same column flips, another starts A to Z (the PID
    /// smallest first; the status with what runs first).
    pub fn clicked(self, column: Column) -> Self {
        if self.column == column {
            Self { column, descending: !self.descending }
        } else {
            Self { column, descending: false }
        }
    }
}

impl State {
    /// From the SCM's `dwCurrentState`: 1 stopped, 2 start pending, 3 stop pending, 4 running,
    /// 5 continue pending, 6 pause pending, 7 paused.
    pub fn from_raw(raw: u32) -> State {
        match raw {
            4 => State::Running,
            2 | 5 => State::Starting,
            3 => State::Stopping,
            6 | 7 => State::Paused,
            _ => State::Stopped,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            State::Running => "Running",
            State::Stopped => "Stopped",
            State::Starting => "Starting",
            State::Stopping => "Stopping",
            State::Paused => "Paused",
        }
    }

    fn rank(self) -> u8 {
        match self {
            State::Running => 0,
            State::Starting => 1,
            State::Paused => 2,
            State::Stopping => 3,
            State::Stopped => 4,
        }
    }
}

impl Startup {
    /// From the config's `dwStartType` (0 boot, 1 system, 2 automatic, 3 manual, 4 disabled) and
    /// whether an automatic one is delayed.
    pub fn from_raw(raw: u32, delayed: bool) -> Option<Startup> {
        Some(match raw {
            0 => Startup::Boot,
            1 => Startup::System,
            2 if delayed => Startup::AutomaticDelayed,
            2 => Startup::Automatic,
            3 => Startup::Manual,
            4 => Startup::Disabled,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Startup::Boot => "Boot",
            Startup::System => "System",
            Startup::Automatic => "Automatic",
            Startup::AutomaticDelayed => "Automatic (delayed)",
            Startup::Manual => "Manual",
            Startup::Disabled => "Disabled",
        }
    }

    fn rank(self) -> u8 {
        match self {
            Startup::Boot => 0,
            Startup::System => 1,
            Startup::Automatic => 2,
            Startup::AutomaticDelayed => 3,
            Startup::Manual => 4,
            Startup::Disabled => 5,
        }
    }
}

impl Service {
    /// The startup type as shown; a dash when it could not be read.
    pub fn startup_label(&self) -> &'static str {
        self.startup.map_or("—", Startup::label)
    }

    /// The PID as shown; empty when it has none (it is not running).
    pub fn pid_label(&self) -> String {
        if self.pid == 0 {
            String::new()
        } else {
            self.pid.to_string()
        }
    }
}

/// The order of two services. Ties go to the display name, then the service name, so equal rows
/// never swap between refreshes. A missing PID or startup type sorts after the rest either way.
pub fn compare(a: &Service, b: &Service, by: SortBy) -> Ordering {
    let names = || {
        a.display
            .to_lowercase()
            .cmp(&b.display.to_lowercase())
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    };
    let flip = |o: Ordering| if by.descending { o.reverse() } else { o };
    // Rows without a value stay last whichever way the column turns.
    let optional = |x: Option<u32>, y: Option<u32>| match (x, y) {
        (Some(x), Some(y)) => flip(x.cmp(&y)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    };
    let by_column = match by.column {
        Column::Display => return flip(names()),
        Column::Name => flip(a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        Column::Status => flip(a.state.rank().cmp(&b.state.rank())),
        Column::Startup => optional(a.startup.map(|s| s.rank() as u32), b.startup.map(|s| s.rank() as u32)),
        Column::Pid => optional((a.pid != 0).then_some(a.pid), (b.pid != 0).then_some(b.pid)),
    };
    by_column.then_with(names)
}

/// True when every word of `query` is found (ignoring case) in the service's display name,
/// service name or description, or a word is its PID. An empty query matches everything.
pub fn matches(s: &Service, query: &str) -> bool {
    let pid = s.pid_label();
    query.split_whitespace().all(|word| {
        let word = word.to_lowercase();
        (s.pid != 0 && word == pid)
            || s.name.to_lowercase().contains(&word)
            || s.display.to_lowercase().contains(&word)
            || s.description.as_deref().is_some_and(|d| d.to_lowercase().contains(&word))
    })
}

/// The indexes of the services to show, in order: those matching `query`, sorted by `by`.
pub fn shown(all: &[Service], query: &str, by: SortBy) -> Vec<usize> {
    let mut rows: Vec<usize> = (0..all.len()).filter(|&i| matches(&all[i], query)).collect();
    rows.sort_by(|&a, &b| compare(&all[a], &all[b], by));
    rows
}

/// How many of `all` run.
pub fn running(all: &[Service]) -> usize {
    all.iter().filter(|s| s.state == State::Running).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc(name: &str, display: &str, state: State, startup: Option<Startup>, pid: u32) -> Service {
        Service { name: name.into(), display: display.into(), state, startup, pid, description: None }
    }

    fn order(all: &[Service], query: &str, by: SortBy) -> Vec<String> {
        shown(all, query, by).into_iter().map(|i| all[i].name.clone()).collect()
    }

    fn sample() -> Vec<Service> {
        vec![
            svc("Spooler", "Print Spooler", State::Running, Some(Startup::Automatic), 2200),
            svc("wuauserv", "Windows Update", State::Stopped, Some(Startup::Manual), 0),
            svc("BITS", "Background Intelligent Transfer Service", State::Running, Some(Startup::AutomaticDelayed), 1500),
            svc("Fax", "fax", State::Stopped, Some(Startup::Disabled), 0),
            svc("Mystery", "Mystery", State::Starting, None, 0),
        ]
    }

    #[test]
    fn states_and_startups_from_the_scm() {
        assert_eq!(State::from_raw(4), State::Running);
        assert_eq!(State::from_raw(1), State::Stopped);
        assert_eq!(State::from_raw(2), State::Starting);
        assert_eq!(State::from_raw(5), State::Starting);
        assert_eq!(State::from_raw(3), State::Stopping);
        assert_eq!(State::from_raw(7), State::Paused);
        assert_eq!(State::from_raw(6), State::Paused);
        assert_eq!(State::from_raw(99), State::Stopped);
        assert_eq!(Startup::from_raw(2, false), Some(Startup::Automatic));
        assert_eq!(Startup::from_raw(2, true), Some(Startup::AutomaticDelayed));
        assert_eq!(Startup::from_raw(3, true), Some(Startup::Manual), "delayed only means something for automatic");
        assert_eq!(Startup::from_raw(4, false), Some(Startup::Disabled));
        assert_eq!(Startup::from_raw(9, false), None);
    }

    #[test]
    fn labels() {
        assert_eq!(Startup::AutomaticDelayed.label(), "Automatic (delayed)");
        assert_eq!(State::Stopping.label(), "Stopping");
        let s = svc("x", "x", State::Running, None, 0);
        assert_eq!((s.startup_label(), s.pid_label().as_str()), ("—", ""));
        assert_eq!(svc("x", "x", State::Running, Some(Startup::Manual), 42).pid_label(), "42");
    }

    #[test]
    fn by_display_name_ignores_case() {
        assert_eq!(order(&sample(), "", SortBy::default()), ["BITS", "Fax", "Mystery", "Spooler", "wuauserv"]);
        let desc = SortBy { column: Column::Display, descending: true };
        assert_eq!(order(&sample(), "", desc), ["wuauserv", "Spooler", "Mystery", "Fax", "BITS"]);
    }

    #[test]
    fn by_status_running_first_ties_by_name() {
        let by = SortBy { column: Column::Status, descending: false };
        assert_eq!(order(&sample(), "", by), ["BITS", "Spooler", "Mystery", "Fax", "wuauserv"]);
    }

    #[test]
    fn by_startup_unknown_last_either_way() {
        let asc = SortBy { column: Column::Startup, descending: false };
        assert_eq!(order(&sample(), "", asc), ["Spooler", "BITS", "wuauserv", "Fax", "Mystery"]);
        let desc = SortBy { column: Column::Startup, descending: true };
        assert_eq!(order(&sample(), "", desc), ["Fax", "wuauserv", "BITS", "Spooler", "Mystery"]);
    }

    #[test]
    fn by_pid_those_without_one_stay_last() {
        let asc = SortBy { column: Column::Pid, descending: false };
        assert_eq!(order(&sample(), "", asc), ["BITS", "Spooler", "Fax", "Mystery", "wuauserv"]);
        let desc = SortBy { column: Column::Pid, descending: true };
        assert_eq!(order(&sample(), "", desc), ["Spooler", "BITS", "Fax", "Mystery", "wuauserv"]);
    }

    #[test]
    fn by_service_name() {
        let by = SortBy { column: Column::Name, descending: false };
        assert_eq!(order(&sample(), "", by), ["BITS", "Fax", "Mystery", "Spooler", "wuauserv"]);
    }

    #[test]
    fn header_clicks() {
        let s = SortBy::default().clicked(Column::Pid);
        assert_eq!(s, SortBy { column: Column::Pid, descending: false });
        assert!(s.clicked(Column::Pid).descending);
        assert_eq!(s.clicked(Column::Pid).clicked(Column::Name), SortBy { column: Column::Name, descending: false });
    }

    #[test]
    fn search_by_name_display_description_or_pid() {
        let mut all = sample();
        all[3].description = Some("Lets you send and receive faxes".into());
        assert_eq!(order(&all, "SPOOL", SortBy::default()), ["Spooler"]);
        assert_eq!(order(&all, "windows update", SortBy::default()), ["wuauserv"]);
        assert_eq!(order(&all, "transfer service", SortBy::default()), ["BITS"]);
        assert_eq!(order(&all, "receive", SortBy::default()), ["Fax"], "the description counts");
        assert_eq!(order(&all, "2200", SortBy::default()), ["Spooler"]);
        assert!(order(&all, "220", SortBy::default()).is_empty(), "a PID matches whole");
        assert!(order(&all, "0", SortBy::default()).is_empty(), "no PID is not PID 0");
        assert!(order(&all, "spooler zzz", SortBy::default()).is_empty(), "every word");
        assert_eq!(order(&all, "   ", SortBy::default()).len(), 5);
    }

    #[test]
    fn counts_the_running() {
        assert_eq!(running(&sample()), 2);
    }
}
