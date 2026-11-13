//! What the Today widget lists: what is left of today (all-day events first, then by time), or,
//! when today has nothing left, tomorrow's events under a "Tomorrow" heading. The next event to
//! start gets a chip ("in 1 h 19 min", or "now" while it runs). Pure: the viewer's offset comes
//! in as a function.

use super::expand::Instance;
use super::time::DAY;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Today,
    Tomorrow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// "14:00", "2:00 PM", or "All day".
    pub time: String,
    pub title: String,
    /// The location, else the calendar's name.
    pub detail: String,
    /// The calendar's URL, for its colour.
    pub calendar: String,
    /// "in 25 min", "in 1 h 19 min", "now".
    pub chip: Option<String>,
    /// The next one: drawn in the heavier weight.
    pub next: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub group: Group,
    pub rows: Vec<Row>,
    /// How many more there are in the group than rows.
    pub more: usize,
}

/// A chip is shown for the next event starting within this long.
const CHIP_AHEAD: i64 = 3 * 3600;

/// `14:05` or `2:05 PM`, for a local wall time.
pub fn clock(local: i64, h24: bool) -> String {
    let t = local.rem_euclid(DAY);
    let (h, m) = (t / 3600, t / 60 % 60);
    if h24 {
        format!("{h:02}:{m:02}")
    } else {
        let hh = if h % 12 == 0 { 12 } else { h % 12 };
        format!("{hh}:{m:02} {}", if h < 12 { "AM" } else { "PM" })
    }
}

/// "in 5 min", "in 1 h 19 min", "in 2 h".
fn until(secs: i64) -> String {
    let mins = (secs + 59) / 60;
    if mins < 60 {
        format!("in {} min", mins.max(1))
    } else if mins % 60 == 0 {
        format!("in {} h", mins / 60)
    } else {
        format!("in {} h {} min", mins / 60, mins % 60)
    }
}

/// The rows for `now` (UTC) out of `instances`. `offset` gives the viewer's offset at an instant;
/// `name_of` a calendar's name by URL; at most `max` rows.
pub fn select(
    instances: &[Instance],
    now: i64,
    offset: &dyn Fn(i64) -> i64,
    name_of: &dyn Fn(&str) -> String,
    h24: bool,
    max: usize,
) -> Option<Shown> {
    let today = (now + offset(now)).div_euclid(DAY);
    // The instants local midnights fall on (found by walking the offset at that day).
    let midnight = |day: i64| {
        let guess = day * DAY;
        guess - offset(guess - offset(guess))
    };
    let (t1, t2) = (midnight(today + 1), midnight(today + 2));
    let covers = |i: &Instance, day: i64| i.days.is_some_and(|(a, b)| a <= day && day < b);
    let today_list: Vec<&Instance> = instances
        .iter()
        .filter(|i| match i.days {
            Some(_) => covers(i, today),
            None => i.start < t1 && (i.end > now || (i.end == i.start && i.start >= now)),
        })
        .collect();
    let (group, mut list) = if !today_list.is_empty() {
        (Group::Today, today_list)
    } else {
        let tomorrow = today + 1;
        let l: Vec<&Instance> = instances
            .iter()
            .filter(|i| match i.days {
                Some(_) => covers(i, tomorrow),
                None => i.start >= t1 && i.start < t2,
            })
            .collect();
        if l.is_empty() {
            return None;
        }
        (Group::Tomorrow, l)
    };
    list.sort_by(|a, b| b.days.is_some().cmp(&a.days.is_some()).then(a.start.cmp(&b.start)).then(a.title.cmp(&b.title)));
    // The next timed event: running now, or the first still to start.
    let next = list.iter().position(|i| i.days.is_none() && i.end > now);
    let more = list.len().saturating_sub(max);
    let rows = list
        .iter()
        .take(max)
        .enumerate()
        .map(|(n, i)| {
            let is_next = Some(n) == next && group == Group::Today;
            let chip = if !is_next {
                None
            } else if i.start <= now {
                Some("now".to_owned())
            } else if i.start - now <= CHIP_AHEAD {
                Some(until(i.start - now))
            } else {
                None
            };
            Row {
                time: if i.days.is_some() { "All day".into() } else { clock(i.start + offset(i.start), h24) },
                title: i.title.clone(),
                detail: if i.location.is_empty() { name_of(&i.calendar) } else { i.location.clone() },
                calendar: i.calendar.clone(),
                chip,
                next: is_next,
            }
        })
        .collect();
    Some(Shown { group, rows, more })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::zone::wall_at;

    /// Thursday 12 Nov 2026, 12:41 in London (GMT, so UTC too).
    const NOW: i64 = 1_794_487_260;

    fn timed(title: &str, start: i64, mins: i64) -> Instance {
        Instance { uid: title.into(), title: title.into(), location: String::new(), calendar: "c".into(), start, end: start + mins * 60, days: None }
    }

    fn all_day(title: &str, first: i64, days: i64) -> Instance {
        Instance { uid: title.into(), title: title.into(), location: "".into(), calendar: "c".into(), start: first * DAY, end: (first + days) * DAY, days: Some((first, first + days)) }
    }

    fn pick(list: &[Instance], now: i64, offset: i64) -> Option<Shown> {
        select(list, now, &|_| offset, &|_| "Personal".into(), true, 3)
    }

    #[test]
    fn todays_events_left_with_the_next_ones_chip() {
        assert_eq!(NOW, wall_at(2026, 11, 12, 12, 41));
        let list = vec![
            timed("Breakfast", wall_at(2026, 11, 12, 8, 0), 30),
            timed("Design review", wall_at(2026, 11, 12, 14, 0), 60),
            timed("Dentist", wall_at(2026, 11, 12, 16, 30), 45),
            all_day("Lisbon trip", 20_769, 2),
            timed("Tomorrow thing", wall_at(2026, 11, 13, 9, 0), 60),
        ];
        let s = pick(&list, NOW, 0).unwrap();
        assert_eq!(s.group, Group::Today);
        let titles: Vec<_> = s.rows.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, ["Lisbon trip", "Design review", "Dentist"]);
        assert_eq!(s.rows[0].time, "All day");
        assert_eq!(s.rows[1].time, "14:00");
        assert_eq!(s.rows[1].chip.as_deref(), Some("in 1 h 19 min"));
        assert!(s.rows[1].next && !s.rows[2].next);
        assert_eq!(s.rows[2].detail, "Personal");
        assert_eq!(s.more, 0);
    }

    #[test]
    fn a_running_event_says_now_and_twelve_hour_clocks() {
        let list = vec![timed("Call", NOW - 600, 30)];
        let s = select(&list, NOW, &|_| 0, &|_| String::new(), false, 3).unwrap();
        assert_eq!(s.rows[0].chip.as_deref(), Some("now"));
        assert_eq!(s.rows[0].time, "12:31 PM");
        assert_eq!(clock(0, false), "12:00 AM");
        assert_eq!(clock(13 * 3600 + 5 * 60, false), "1:05 PM");
    }

    #[test]
    fn tomorrow_when_today_is_done() {
        let list = vec![timed("Done", NOW - 7200, 30), timed("Standup", wall_at(2026, 11, 13, 9, 0), 15), all_day("Holiday", 20_770, 1), timed("Later", wall_at(2026, 11, 14, 9, 0), 15)];
        let s = pick(&list, NOW, 0).unwrap();
        assert_eq!(s.group, Group::Tomorrow);
        let titles: Vec<_> = s.rows.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, ["Holiday", "Standup"]);
        assert!(s.rows.iter().all(|r| r.chip.is_none()));
        assert_eq!(pick(&[timed("Done", NOW - 7200, 30)], NOW, 0), None);
    }

    #[test]
    fn days_are_the_viewers() {
        // 23:30 UTC on the 12th is 00:30 on the 13th in UTC+1: today is the 13th there.
        let late = wall_at(2026, 11, 12, 23, 30);
        let list = vec![all_day("Thirteenth", 20_770, 1), timed("Early", wall_at(2026, 11, 13, 7, 0), 30)];
        let s = pick(&list, late, 3600).unwrap();
        assert_eq!(s.group, Group::Today);
        assert_eq!(s.rows.len(), 2);
        assert_eq!(s.rows[1].time, "08:00");
        // In UTC it is still the 12th, so these are tomorrow's.
        assert_eq!(pick(&list, late, 0).unwrap().group, Group::Tomorrow);
    }

    #[test]
    fn more_than_fit() {
        let list: Vec<_> = (0..5).map(|i| timed(&format!("E{i}"), NOW + 3600 * (i + 1), 10)).collect();
        let s = pick(&list, NOW, 0).unwrap();
        assert_eq!(s.rows.len(), 3);
        assert_eq!(s.more, 2);
    }
}
