//! The top bar's month grid: which days it shows, how its weeks are numbered under the Windows
//! region's rules, and which days have events. Pure: days are counted from 1970-01-01 (see
//! `time`), weekdays from Monday (0) as Windows' `iFirstDayOfWeek` counts, and the viewer's
//! offset comes in as a function, so daylight saving days are handled by the caller's clock.

use super::expand::Instance;
use super::time::{civil_from_days, days_from_civil, weekday, DAY};
use std::collections::BTreeSet;

/// The grid always has six weeks, so it keeps one height from month to month.
pub const WEEKS: usize = 6;

/// How the first week of a year is chosen: Windows' `iFirstWeekOfYear`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WeekRule {
    /// The week holding 1 January (0; the United States).
    #[default]
    FirstDay,
    /// The first week wholly in the year (1).
    FirstFullWeek,
    /// The first week with at least four days in the year (2; ISO 8601, most of Europe).
    FirstFourDays,
}

impl WeekRule {
    pub fn from_windows(n: u32) -> Self {
        match n {
            1 => WeekRule::FirstFullWeek,
            2 => WeekRule::FirstFourDays,
            _ => WeekRule::FirstDay,
        }
    }
}

/// The first day of the week holding `day`, with weeks starting on `first` (0 Monday).
pub fn week_start(day: i64, first: u32) -> i64 {
    day - ((weekday(day) + 7 - first.min(6)) % 7) as i64
}

/// Where week 1 of `year` starts.
fn week_one(year: i64, first: u32, rule: WeekRule) -> i64 {
    let jan1 = days_from_civil(year, 1, 1);
    let start = week_start(jan1, first);
    let in_year = 7 - (jan1 - start);
    let take = match rule {
        WeekRule::FirstDay => true,
        WeekRule::FirstFullWeek => start == jan1,
        WeekRule::FirstFourDays => in_year >= 4,
    };
    if take { start } else { start + 7 }
}

/// The number of the week holding `day`.
pub fn week_number(day: i64, first: u32, rule: WeekRule) -> u32 {
    let (y, _, _) = civil_from_days(day);
    let start = week_start(day, first);
    let year = if start >= week_one(y + 1, first, rule) {
        y + 1
    } else if start < week_one(y, first, rule) {
        y - 1
    } else {
        y
    };
    ((start - week_one(year, first, rule)) / 7 + 1) as u32
}

/// One month's grid: six weeks from the week holding the 1st.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grid {
    pub year: i64,
    pub month: u32,
    /// The first cell's day.
    pub first_cell: i64,
    /// Each week's number, top to bottom.
    pub weeks: [u32; WEEKS],
}

impl Grid {
    pub fn new(year: i64, month: u32, first: u32, rule: WeekRule) -> Self {
        let first_cell = week_start(days_from_civil(year, month, 1), first);
        let weeks = std::array::from_fn(|w| week_number(first_cell + 7 * w as i64, first, rule));
        Self { year, month, first_cell, weeks }
    }

    /// The day in week `w` (0 to 5), column `col` (0 to 6).
    pub fn day(&self, w: usize, col: usize) -> i64 {
        self.first_cell + (w * 7 + col) as i64
    }

    /// The last cell's day, plus one.
    pub fn end(&self) -> i64 {
        self.first_cell + (WEEKS * 7) as i64
    }

    /// Whether `day` is in the grid's own month (not a neighbour's day shown in the corners).
    pub fn in_month(&self, day: i64) -> bool {
        let (y, m, _) = civil_from_days(day);
        y == self.year && m == self.month
    }
}

/// The month after (`+1`) or before (`-1`) `year`/`month`.
pub fn step(year: i64, month: u32, by: i32) -> (i64, u32) {
    let n = year * 12 + month as i64 - 1 + by as i64;
    (n.div_euclid(12), (n.rem_euclid(12) + 1) as u32)
}

/// The UTC instants a grid's days run between, a day wider on each side so no zone is cut short.
pub fn utc_range(grid: &Grid) -> (i64, i64) {
    ((grid.first_cell - 1) * DAY, (grid.end() + 1) * DAY)
}

/// The viewer's day of an instant.
pub fn local_day(utc: i64, offset: &dyn Fn(i64) -> i64) -> i64 {
    (utc + offset(utc)).div_euclid(DAY)
}

/// The viewer's days an instance falls on: an all-day event its days; a timed one every day
/// from its start to just before its end (an event ending at midnight is not on the next day).
pub fn days_of(i: &Instance, offset: &dyn Fn(i64) -> i64) -> (i64, i64) {
    match i.days {
        Some((a, b)) => (a, b.max(a + 1)),
        None => {
            let first = local_day(i.start, offset);
            let last = local_day((i.end - 1).max(i.start), offset);
            (first, last + 1)
        }
    }
}

/// The days in `[from, to)` that have an event.
pub fn busy_days(instances: &[&Instance], from: i64, to: i64, offset: &dyn Fn(i64) -> i64) -> BTreeSet<i64> {
    let mut out = BTreeSet::new();
    for i in instances {
        let (a, b) = days_of(i, offset);
        for d in a.max(from)..b.min(to) {
            out.insert(d);
        }
    }
    out
}

/// The events on `day`: all-day first, then by start time.
pub fn on_day<'a>(instances: &[&'a Instance], day: i64, offset: &dyn Fn(i64) -> i64) -> Vec<&'a Instance> {
    let mut out: Vec<&Instance> = instances
        .iter()
        .copied()
        .filter(|i| {
            let (a, b) = days_of(i, offset);
            a <= day && day < b
        })
        .collect();
    out.sort_by(|a, b| b.days.is_some().cmp(&a.days.is_some()).then(a.start.cmp(&b.start)).then(a.title.cmp(&b.title)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::time::is_leap;
    use crate::calendar::zone::{london, wall_at, Clock};

    const MON: u32 = 0;
    const SUN: u32 = 6;

    fn d(y: i64, m: u32, day: u32) -> i64 {
        days_from_civil(y, m, day)
    }

    #[test]
    fn the_grid_starts_on_the_regions_first_day() {
        // November 2026 begins on a Sunday.
        let g = Grid::new(2026, 11, MON, WeekRule::FirstFourDays);
        assert_eq!(civil_from_days(g.first_cell), (2026, 10, 26), "Monday before the 1st");
        let g = Grid::new(2026, 11, SUN, WeekRule::FirstDay);
        assert_eq!(g.first_cell, d(2026, 11, 1), "the 1st is a Sunday: no days before it");
        // Saturday first (some Middle East regions).
        let g = Grid::new(2026, 11, 5, WeekRule::FirstDay);
        assert_eq!(civil_from_days(g.first_cell), (2026, 10, 31));
        assert_eq!(g.end() - g.first_cell, 42);
        assert!(g.in_month(d(2026, 11, 30)) && !g.in_month(d(2026, 12, 1)));
    }

    #[test]
    fn iso_weeks() {
        // ISO: 29 Dec 2025 is week 1 of 2026; 1 Jan 2021 was week 53 of 2020.
        assert_eq!(week_number(d(2025, 12, 29), MON, WeekRule::FirstFourDays), 1);
        assert_eq!(week_number(d(2021, 1, 1), MON, WeekRule::FirstFourDays), 53);
        assert_eq!(week_number(d(2026, 11, 12), MON, WeekRule::FirstFourDays), 46);
        assert_eq!(week_number(d(2027, 1, 4), MON, WeekRule::FirstFourDays), 1);
        let g = Grid::new(2026, 11, MON, WeekRule::FirstFourDays);
        assert_eq!(g.weeks, [44, 45, 46, 47, 48, 49]);
    }

    #[test]
    fn us_weeks_start_with_the_first_of_january() {
        // Sunday first, week 1 holds 1 January: the row with 28 Dec 2025 to 3 Jan 2026 is week 1.
        assert_eq!(week_number(d(2025, 12, 28), SUN, WeekRule::FirstDay), 1);
        assert_eq!(week_number(d(2026, 1, 3), SUN, WeekRule::FirstDay), 1);
        assert_eq!(week_number(d(2026, 1, 4), SUN, WeekRule::FirstDay), 2);
        assert_eq!(week_number(d(2026, 11, 12), SUN, WeekRule::FirstDay), 46);
    }

    #[test]
    fn first_full_week() {
        // 2026 starts on a Thursday: with Monday weeks, week 1 starts on Monday 5 January.
        assert_eq!(week_number(d(2026, 1, 5), MON, WeekRule::FirstFullWeek), 1);
        assert_eq!(week_number(d(2026, 1, 4), MON, WeekRule::FirstFullWeek), 52, "the last of 2025");
        // 2024 started on a Monday: week 1 is that week.
        assert_eq!(week_number(d(2024, 1, 1), MON, WeekRule::FirstFullWeek), 1);
    }

    #[test]
    fn leap_years() {
        assert!(is_leap(2028) && !is_leap(2100) && is_leap(2000));
        let feb = Grid::new(2028, 2, MON, WeekRule::FirstFourDays);
        assert!(feb.in_month(d(2028, 2, 29)));
        assert_eq!(civil_from_days(d(2028, 2, 29) + 1), (2028, 3, 1));
        let feb = Grid::new(2100, 2, MON, WeekRule::FirstFourDays);
        assert!(!feb.in_month(d(2100, 2, 28) + 1));
        // A leap year that starts on a Thursday has 53 ISO weeks (2026 does not, 2032 does).
        assert_eq!(week_number(d(2032, 12, 31), MON, WeekRule::FirstFourDays), 53);
    }

    #[test]
    fn stepping_months() {
        assert_eq!(step(2026, 12, 1), (2027, 1));
        assert_eq!(step(2026, 1, -1), (2025, 12));
        assert_eq!(step(2026, 11, -13), (2025, 10));
    }

    #[test]
    fn events_land_on_the_viewers_days_across_daylight_saving() {
        let clock = london();
        let offset = |t: i64| clock.local_offset(t);
        // 29 March 2026: London springs forward at 01:00 UTC. 22:30-23:30 UTC that evening is
        // 23:30-00:30 BST: the 29th and the 30th.
        let late = Instance {
            uid: "late".into(),
            title: "late".into(),
            location: String::new(),
            calendar: "c".into(),
            start: wall_at(2026, 3, 29, 22, 30),
            end: wall_at(2026, 3, 29, 23, 30),
            days: None,
        };
        assert_eq!(days_of(&late, &offset), (d(2026, 3, 29), d(2026, 3, 31)), "it ends at 00:30 BST on the 30th");
        // The same UTC times in winter are on the 29th of their month only.
        let winter = Instance { start: wall_at(2026, 1, 29, 22, 30), end: wall_at(2026, 1, 29, 23, 30), ..late.clone() };
        assert_eq!(days_of(&winter, &offset), (d(2026, 1, 29), d(2026, 1, 30)));
        // Ending at local midnight: not on the next day.
        let to_midnight = Instance { start: wall_at(2026, 7, 1, 21, 0), end: wall_at(2026, 7, 1, 23, 0), ..late.clone() };
        assert_eq!(days_of(&to_midnight, &offset), (d(2026, 7, 1), d(2026, 7, 2)));
        // All-day events keep their days; a two-day one marks both.
        let trip = Instance { days: Some((d(2026, 3, 28), d(2026, 3, 30))), ..late.clone() };
        let all = [&late, &trip];
        let busy = busy_days(&all, d(2026, 3, 1), d(2026, 4, 1), &offset);
        assert_eq!(busy.into_iter().collect::<Vec<_>>(), [d(2026, 3, 28), d(2026, 3, 29), d(2026, 3, 30)]);
        let on = on_day(&all, d(2026, 3, 30), &offset);
        assert_eq!(on.len(), 1);
        assert_eq!(on[0].uid, "late");
        let on = on_day(&all, d(2026, 3, 29), &offset);
        assert_eq!(on.iter().map(|i| i.days.is_some()).collect::<Vec<_>>(), [true, false], "all-day first");
    }

    #[test]
    fn a_long_event_marks_every_day_it_spans() {
        let offset = |_: i64| 0;
        let conf = Instance {
            uid: "conf".into(),
            title: "conf".into(),
            location: String::new(),
            calendar: "c".into(),
            start: wall_at(2026, 11, 10, 9, 0),
            end: wall_at(2026, 11, 12, 17, 0),
            days: None,
        };
        let busy = busy_days(&[&conf], d(2026, 11, 11), d(2026, 12, 1), &offset);
        assert_eq!(busy.into_iter().collect::<Vec<_>>(), [d(2026, 11, 11), d(2026, 11, 12)], "cut to the range");
        let g = Grid::new(2026, 11, MON, WeekRule::FirstFourDays);
        let (a, b) = utc_range(&g);
        assert!(a < wall_at(2026, 10, 26, 0, 0) && b > wall_at(2026, 12, 6, 23, 0));
    }
}
