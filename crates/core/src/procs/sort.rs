//! Sorting the list by a column, with a stable order: ties fall back to the name and then the
//! PID, so equal rows never swap between refreshes, and while the pointer is over the list the
//! order is held (see [`hold`]) so a row never moves away from under it.

use super::Status;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::hash::Hash;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Column {
    Name,
    Cpu,
    Memory,
    Disk,
    Gpu,
    Pid,
    Status,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SortBy {
    pub column: Column,
    pub descending: bool,
}

impl Default for SortBy {
    fn default() -> Self {
        Self { column: Column::Name, descending: false }
    }
}

impl SortBy {
    /// A click on `column`'s header: the same column flips, another starts the way it reads best
    /// (names A to Z, numbers biggest first).
    pub fn clicked(self, column: Column) -> Self {
        if self.column == column {
            Self { column, descending: !self.descending }
        } else {
            Self { column, descending: !matches!(column, Column::Name | Column::Pid) }
        }
    }
}

/// What a row is sorted on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Values {
    pub name: String,
    pub cpu: f32,
    pub memory: u64,
    pub disk: f64,
    pub gpu: f32,
    pub pid: u32,
    pub status: Status,
}

fn status_rank(s: Status) -> u8 {
    match s {
        Status::Running => 0,
        Status::Suspended => 1,
        Status::NotResponding => 2,
    }
}

/// The order of two rows. Shares are compared to a tenth of a percent, the precision shown, so
/// values that look the same are a tie, and the tie goes to the name and the PID.
pub fn compare(a: &Values, b: &Values, by: SortBy) -> Ordering {
    let tenth = |v: f32| (v * 10.0).round() as i64;
    let by_column = match by.column {
        Column::Name => Ordering::Equal,
        Column::Cpu => tenth(a.cpu).cmp(&tenth(b.cpu)),
        Column::Memory => (a.memory / 100_000).cmp(&(b.memory / 100_000)),
        Column::Disk => ((a.disk / 50_000.0) as u64).cmp(&((b.disk / 50_000.0) as u64)),
        Column::Gpu => tenth(a.gpu).cmp(&tenth(b.gpu)),
        Column::Pid => a.pid.cmp(&b.pid),
        Column::Status => status_rank(a.status).cmp(&status_rank(b.status)),
    };
    let names = || a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.pid.cmp(&b.pid));
    if by.column == Column::Name {
        return if by.descending { names().reverse() } else { names() };
    }
    // Only the column turns over; ties still read A to Z.
    let column = if by.descending { by_column.reverse() } else { by_column };
    column.then_with(names)
}

/// The order to show while it is held: rows already shown stay where they were, rows that are
/// gone drop out, and new rows go at the end in their sorted order.
pub fn hold<K: Copy + Eq + Hash>(shown: &[K], sorted: &[K]) -> Vec<K> {
    let now: HashSet<K> = sorted.iter().copied().collect();
    let mut out: Vec<K> = shown.iter().copied().filter(|k| now.contains(k)).collect();
    let kept: HashSet<K> = out.iter().copied().collect();
    out.extend(sorted.iter().copied().filter(|k| !kept.contains(k)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(name: &str, pid: u32, cpu: f32) -> Values {
        Values { name: name.into(), pid, cpu, ..Default::default() }
    }

    fn sorted(mut rows: Vec<Values>, by: SortBy) -> Vec<u32> {
        rows.sort_by(|a, b| compare(a, b, by));
        rows.iter().map(|r| r.pid).collect()
    }

    #[test]
    fn by_name_ignores_case_and_ties_go_to_the_pid() {
        let rows = vec![v("b", 3, 0.0), v("A", 9, 0.0), v("a", 2, 0.0)];
        assert_eq!(sorted(rows, SortBy::default()), [2, 9, 3]);
    }

    #[test]
    fn descending_numbers_keep_names_in_order_for_ties() {
        let by = SortBy { column: Column::Cpu, descending: true };
        let rows = vec![v("z", 1, 0.0), v("m", 2, 5.0), v("a", 3, 0.0), v("b", 4, 5.04)];
        // 5.0 and 5.04 show the same and so tie; the tie reads A to Z.
        assert_eq!(sorted(rows, by), [4, 2, 3, 1]);
    }

    #[test]
    fn small_changes_do_not_reorder() {
        let by = SortBy { column: Column::Memory, descending: true };
        let mut a = v("a", 1, 0.0);
        let mut b = v("b", 2, 0.0);
        a.memory = 50_000_010;
        b.memory = 50_000_020;
        assert_eq!(compare(&a, &b, by), Ordering::Less, "the same 0.1 MB: by name");
    }

    #[test]
    fn header_clicks() {
        let s = SortBy::default().clicked(Column::Cpu);
        assert_eq!(s, SortBy { column: Column::Cpu, descending: true });
        assert!(!s.clicked(Column::Cpu).descending);
        assert_eq!(s.clicked(Column::Name), SortBy { column: Column::Name, descending: false });
        assert!(!s.clicked(Column::Pid).descending);
    }

    #[test]
    fn holding_keeps_rows_in_place() {
        assert_eq!(hold(&[3, 1, 2], &[1, 2, 3]), [3, 1, 2]);
        assert_eq!(hold(&[3, 1, 2], &[2, 4, 3]), [3, 2, 4], "1 went, 4 came at the end");
        assert_eq!(hold::<u32>(&[], &[5, 6]), [5, 6]);
    }

    #[test]
    fn statuses_sort_running_first() {
        let by = SortBy { column: Column::Status, descending: false };
        let mut hung = v("a", 1, 0.0);
        hung.status = Status::NotResponding;
        let mut susp = v("b", 2, 0.0);
        susp.status = Status::Suspended;
        assert_eq!(sorted(vec![hung, susp, v("c", 3, 0.0)], by), [3, 2, 1]);
    }
}
