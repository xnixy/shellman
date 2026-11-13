//! Recurrence rules (RFC 5545 RRULE): DAILY, WEEKLY, MONTHLY and YEARLY with INTERVAL, COUNT,
//! UNTIL, BYDAY (with ordinals), BYMONTHDAY, BYMONTH, BYYEARDAY, BYSETPOS and WKST. Expanded in
//! wall time (the event's own clock), so a weekly 09:00 stays 09:00 across a DST change; the
//! caller turns each wall time into an instant. Sub-daily rules (HOURLY and finer) are rare in
//! calendars and give only their first occurrence. Pure.

use super::time::{civil_from_days, day_of, days_from_civil, days_in_month, is_leap, time_of, weekday, DAY};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
    /// HOURLY, MINUTELY, SECONDLY.
    SubDaily,
}

/// When a rule ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Until {
    /// The last day an occurrence may fall on (a DATE UNTIL).
    Day(i64),
    /// The last instant, UTC (a DATE-TIME UNTIL with `Z`).
    Utc(i64),
    /// The last wall time (a floating UNTIL).
    Wall(i64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RRule {
    pub freq: Freq,
    pub interval: u32,
    pub count: Option<u32>,
    pub until: Option<Until>,
    /// (ordinal, weekday 0 Monday): ordinal 0 means every such weekday.
    pub by_day: Vec<(i32, u32)>,
    pub by_month_day: Vec<i32>,
    pub by_month: Vec<u32>,
    pub by_year_day: Vec<i32>,
    pub by_set_pos: Vec<i32>,
    /// The week's first day, 0 Monday.
    pub wkst: u32,
}

fn weekday_code(s: &str) -> Option<u32> {
    ["MO", "TU", "WE", "TH", "FR", "SA", "SU"].iter().position(|d| d.eq_ignore_ascii_case(s)).map(|i| i as u32)
}

fn ints(v: &str) -> Vec<i32> {
    v.split(',').filter_map(|n| n.trim().trim_start_matches('+').parse().ok()).collect()
}

/// `FREQ=WEEKLY;BYDAY=MO,WE;UNTIL=20261231T235959Z`. None when it has no usable FREQ.
pub fn parse(s: &str) -> Option<RRule> {
    let mut rule = RRule {
        freq: Freq::Daily,
        interval: 1,
        count: None,
        until: None,
        by_day: Vec::new(),
        by_month_day: Vec::new(),
        by_month: Vec::new(),
        by_year_day: Vec::new(),
        by_set_pos: Vec::new(),
        wkst: 0,
    };
    let mut freq = None;
    for part in s.trim().split(';') {
        let Some((k, v)) = part.split_once('=') else { continue };
        match k.trim().to_ascii_uppercase().as_str() {
            "FREQ" => {
                freq = Some(match v.trim().to_ascii_uppercase().as_str() {
                    "DAILY" => Freq::Daily,
                    "WEEKLY" => Freq::Weekly,
                    "MONTHLY" => Freq::Monthly,
                    "YEARLY" => Freq::Yearly,
                    "HOURLY" | "MINUTELY" | "SECONDLY" => Freq::SubDaily,
                    _ => return None,
                })
            }
            "INTERVAL" => rule.interval = v.trim().parse().unwrap_or(1).max(1),
            "COUNT" => rule.count = v.trim().parse().ok(),
            "UNTIL" => {
                let p = super::ical::Prop { name: "UNTIL".into(), params: Vec::new(), value: v.to_owned() };
                rule.until = super::ical::date_value(&p).map(|d| match d {
                    super::ical::DateValue::Date(day) => Until::Day(day),
                    super::ical::DateValue::Utc(t) => Until::Utc(t),
                    super::ical::DateValue::Floating(w) | super::ical::DateValue::Zoned(w, _) => Until::Wall(w),
                });
            }
            "BYDAY" => {
                rule.by_day = v
                    .split(',')
                    .filter_map(|d| {
                        let d = d.trim();
                        let split = d.len().checked_sub(2)?;
                        let wd = weekday_code(d.get(split..)?)?;
                        let n = d[..split].trim_start_matches('+');
                        let ord = if n.is_empty() { 0 } else { n.parse().ok()? };
                        Some((ord, wd))
                    })
                    .collect()
            }
            "BYMONTHDAY" => rule.by_month_day = ints(v),
            "BYMONTH" => rule.by_month = ints(v).into_iter().filter(|m| (1..=12).contains(m)).map(|m| m as u32).collect(),
            "BYYEARDAY" => rule.by_year_day = ints(v),
            "BYSETPOS" => rule.by_set_pos = ints(v),
            "WKST" => rule.wkst = weekday_code(v.trim()).unwrap_or(0),
            _ => {}
        }
    }
    rule.freq = freq?;
    Some(rule)
}

/// Days of month `m` of `y` that match `BYDAY` entries (ordinals count within the month).
fn month_weekdays(y: i64, m: u32, by_day: &[(i32, u32)]) -> Vec<i64> {
    let first = days_from_civil(y, m, 1);
    let len = days_in_month(y, m) as i64;
    span_weekdays(first, len, by_day)
}

/// Days of the span `[first, first + len)` that match `BYDAY`, ordinals counted within it.
fn span_weekdays(first: i64, len: i64, by_day: &[(i32, u32)]) -> Vec<i64> {
    let mut out = Vec::new();
    for &(ord, wd) in by_day {
        let all: Vec<i64> = (0..len).map(|i| first + i).filter(|d| weekday(*d) == wd).collect();
        if ord == 0 {
            out.extend(all);
        } else if ord > 0 {
            if let Some(d) = all.get(ord as usize - 1) {
                out.push(*d);
            }
        } else if let Some(i) = all.len().checked_sub((-ord) as usize) {
            out.push(all[i]);
        }
    }
    out
}

/// Days of month `m` named by BYMONTHDAY (negative counts from the end).
fn month_days(y: i64, m: u32, list: &[i32]) -> Vec<i64> {
    let len = days_in_month(y, m) as i32;
    let first = days_from_civil(y, m, 1);
    list.iter()
        .filter_map(|&d| {
            let n = if d > 0 { d } else { len + d + 1 };
            (1..=len).contains(&n).then(|| first + n as i64 - 1)
        })
        .collect()
}

/// The days one period of the rule offers, before BYSETPOS and before the start is applied.
/// `start` is the first occurrence's day; `period` is a day inside the period.
fn period_days(rule: &RRule, start: i64, period: i64) -> Vec<i64> {
    let (_, sm, sd) = civil_from_days(start);
    let (py, pm, _) = civil_from_days(period);
    let month_ok = |d: i64| rule.by_month.is_empty() || rule.by_month.contains(&civil_from_days(d).1);
    let mut days: Vec<i64> = match rule.freq {
        Freq::Daily | Freq::SubDaily => {
            let d = period;
            let md_ok = rule.by_month_day.is_empty() || month_days(py, pm, &rule.by_month_day).contains(&d);
            let wd_ok = rule.by_day.is_empty() || rule.by_day.iter().any(|(_, w)| *w == weekday(d));
            if md_ok && wd_ok && month_ok(d) {
                vec![d]
            } else {
                Vec::new()
            }
        }
        Freq::Weekly => {
            let back = (weekday(period) + 7 - rule.wkst) % 7;
            let week = period - back as i64;
            let wanted: Vec<u32> = if rule.by_day.is_empty() { vec![weekday(start)] } else { rule.by_day.iter().map(|(_, w)| *w).collect() };
            (0..7).map(|i| week + i).filter(|d| wanted.contains(&weekday(*d)) && month_ok(*d)).collect()
        }
        Freq::Monthly => {
            if !rule.by_month.is_empty() && !rule.by_month.contains(&pm) {
                return Vec::new();
            }
            month_set(rule, py, pm, sd)
        }
        Freq::Yearly => {
            let mut out = Vec::new();
            if !rule.by_year_day.is_empty() {
                let len = if is_leap(py) { 366 } else { 365 };
                let jan1 = days_from_civil(py, 1, 1);
                for &n in &rule.by_year_day {
                    let i = if n > 0 { n - 1 } else { len + n };
                    if (0..len).contains(&i) {
                        out.push(jan1 + i as i64);
                    }
                }
                out.retain(|d| month_ok(*d));
            } else if rule.by_month.is_empty() && !rule.by_day.is_empty() && rule.by_month_day.is_empty() {
                // BYDAY over the whole year (20MO: the 20th Monday).
                let jan1 = days_from_civil(py, 1, 1);
                out = span_weekdays(jan1, if is_leap(py) { 366 } else { 365 }, &rule.by_day);
            } else {
                let months: Vec<u32> = if !rule.by_month.is_empty() {
                    rule.by_month.clone()
                } else if rule.by_month_day.is_empty() {
                    vec![sm]
                } else {
                    (1..=12).collect()
                };
                for m in months {
                    out.extend(month_set(rule, py, m, sd));
                }
            }
            out
        }
    };
    days.sort_unstable();
    days.dedup();
    days
}

/// The days of month `m` for a MONTHLY rule, or a YEARLY rule's month: BYMONTHDAY and BYDAY
/// (both narrow), else the start's day of the month (skipped where the month is too short).
fn month_set(rule: &RRule, y: i64, m: u32, start_day_of_month: u32) -> Vec<i64> {
    match (rule.by_month_day.is_empty(), rule.by_day.is_empty()) {
        (true, true) => {
            if start_day_of_month <= days_in_month(y, m) {
                vec![days_from_civil(y, m, start_day_of_month)]
            } else {
                Vec::new()
            }
        }
        (false, true) => month_days(y, m, &rule.by_month_day),
        (true, false) => month_weekdays(y, m, &rule.by_day),
        (false, false) => {
            let wd = month_weekdays(y, m, &rule.by_day);
            month_days(y, m, &rule.by_month_day).into_iter().filter(|d| wd.contains(d)).collect()
        }
    }
}

/// BYSETPOS: keep the n-th (or n-th from last) of a period's sorted set.
fn set_pos(days: Vec<i64>, pos: &[i32]) -> Vec<i64> {
    if pos.is_empty() {
        return days;
    }
    let mut out: Vec<i64> = pos
        .iter()
        .filter_map(|&p| {
            let i = if p > 0 { p as usize - 1 } else { days.len().checked_sub((-p) as usize)? };
            days.get(i).copied()
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The day a period number lands on, counted from the start's period.
fn period_start(rule: &RRule, start: i64, k: i64) -> i64 {
    let step = k * rule.interval as i64;
    match rule.freq {
        Freq::Daily | Freq::SubDaily => start + step,
        Freq::Weekly => start + 7 * step,
        Freq::Monthly => {
            let (y, m, _) = civil_from_days(start);
            let months = (y * 12 + (m as i64 - 1)) + step;
            days_from_civil(months.div_euclid(12), (months.rem_euclid(12) + 1) as u32, 1)
        }
        Freq::Yearly => {
            let (y, _, _) = civil_from_days(start);
            days_from_civil(y + step, 1, 1)
        }
    }
}

/// The wall times of the rule's occurrences from `start` (the first occurrence's wall time,
/// DTSTART) that fall in `[from, end]`. `past_until` says whether a wall time is beyond the
/// rule's UNTIL (it knows the event's zone). Occurrences before `from` still count toward
/// COUNT, but are neither given nor count toward `max`: an old rule with thousands of earlier
/// occurrences still reaches the window. At most `max` are given.
pub fn expand(rule: &RRule, start: i64, from: i64, end: i64, past_until: &dyn Fn(i64) -> bool, max: usize) -> Vec<i64> {
    let mut out = Vec::new();
    let first_day = day_of(start);
    let tod = time_of(start);
    if rule.freq == Freq::SubDaily {
        if start >= from && start <= end && !past_until(start) {
            out.push(start);
        }
        return out;
    }
    let mut emitted = 0u32;
    // Without COUNT, whole periods before `end - 2 years` cannot matter for a daily or weekly
    // rule; skip them so an old daily event is not walked from its first day.
    let mut k: i64 = 0;
    if rule.count.is_none() {
        let len = match rule.freq {
            Freq::Daily => rule.interval as i64,
            Freq::Weekly => 7 * rule.interval as i64,
            _ => 0,
        };
        if len > 0 {
            let lead = (day_of(end) - first_day - 800).max(0);
            k = lead / len;
        }
    }
    // A safety bound: no rule needs more periods than this to reach a fortnight ahead.
    for _ in 0..20_000 {
        let p = period_start(rule, first_day, k);
        k += 1;
        if p > day_of(end) + 1 {
            break;
        }
        let days = set_pos(period_days(rule, first_day, p), &rule.by_set_pos);
        for d in days {
            let w = d * DAY + tod;
            if w < start {
                continue;
            }
            if past_until(w) {
                return out;
            }
            if let Some(c) = rule.count {
                if emitted >= c {
                    return out;
                }
            }
            emitted += 1;
            if w > end {
                return out;
            }
            if w < from {
                continue;
            }
            out.push(w);
            if out.len() >= max {
                return out;
            }
        }
        if let Some(c) = rule.count {
            if emitted >= c {
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::time::{stamp, wall};

    fn run(rule: &str, start: i64, end: i64) -> Vec<String> {
        let r = parse(rule).unwrap();
        let until = r.until.clone();
        let past = move |w: i64| match &until {
            Some(Until::Wall(u)) | Some(Until::Utc(u)) => w > *u,
            Some(Until::Day(d)) => day_of(w) > *d,
            None => false,
        };
        expand(&r, start, start, end, &past, 500).into_iter().map(stamp).collect()
    }

    #[test]
    fn parses_the_common_shapes() {
        let r = parse("FREQ=MONTHLY;INTERVAL=2;BYDAY=-1FR,2MO;COUNT=10;WKST=SU").unwrap();
        assert_eq!(r.freq, Freq::Monthly);
        assert_eq!(r.interval, 2);
        assert_eq!(r.by_day, vec![(-1, 4), (2, 0)]);
        assert_eq!(r.count, Some(10));
        assert_eq!(r.wkst, 6);
        assert_eq!(parse("FREQ=YEARLY;UNTIL=20271231").unwrap().until, Some(Until::Day(crate::calendar::time::days_from_civil(2027, 12, 31))));
        assert_eq!(parse("INTERVAL=2"), None);
        assert_eq!(parse("FREQ=FORTNIGHTLY"), None);
    }

    #[test]
    fn weekly_on_two_days_with_count() {
        let start = wall(2026, 11, 2, 9, 0, 0); // a Monday
        let got = run("FREQ=WEEKLY;BYDAY=MO,WE;COUNT=5", start, wall(2027, 1, 1, 0, 0, 0));
        assert_eq!(got, ["20261102T090000", "20261104T090000", "20261109T090000", "20261111T090000", "20261116T090000"]);
    }

    #[test]
    fn daily_until_and_interval() {
        let start = wall(2026, 11, 10, 8, 30, 0);
        let got = run("FREQ=DAILY;INTERVAL=2;UNTIL=20261116T083000Z", start, wall(2026, 12, 1, 0, 0, 0));
        assert_eq!(got, ["20261110T083000", "20261112T083000", "20261114T083000", "20261116T083000"]);
    }

    #[test]
    fn monthly_last_friday_and_31st() {
        let start = wall(2026, 10, 30, 17, 0, 0);
        let got = run("FREQ=MONTHLY;BYDAY=-1FR", start, wall(2027, 1, 31, 0, 0, 0));
        assert_eq!(got, ["20261030T170000", "20261127T170000", "20261225T170000", "20270129T170000"]);
        // The 31st skips months without one.
        let got = run("FREQ=MONTHLY", wall(2026, 10, 31, 9, 0, 0), wall(2027, 3, 31, 23, 0, 0));
        assert_eq!(got, ["20261031T090000", "20261231T090000", "20270131T090000", "20270331T090000"]);
    }

    #[test]
    fn yearly_birthday_and_dst_rule() {
        let got = run("FREQ=YEARLY", wall(1990, 2, 28, 0, 0, 0), wall(1993, 3, 1, 0, 0, 0));
        assert_eq!(got.len(), 4);
        // Europe's spring change: the last Sunday of March.
        let got = run("FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU", wall(1981, 3, 29, 1, 0, 0), wall(2027, 12, 31, 0, 0, 0));
        assert_eq!(got.last().unwrap(), "20270328T010000");
        assert!(got.contains(&"20260329T010000".to_owned()));
    }

    #[test]
    fn set_pos_picks_the_last_weekday_of_the_month() {
        let start = wall(2026, 11, 30, 12, 0, 0);
        let got = run("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1", start, wall(2027, 2, 28, 0, 0, 0));
        assert_eq!(got, ["20261130T120000", "20261231T120000", "20270129T120000", "20270226T120000"]);
    }

    #[test]
    fn an_old_daily_rule_is_not_walked_from_its_start() {
        let start = wall(2001, 1, 1, 7, 0, 0);
        let end = wall(2026, 11, 26, 0, 0, 0);
        let r = parse("FREQ=DAILY").unwrap();
        let got = expand(&r, start, start, end, &|_| false, 100_000);
        assert_eq!(stamp(*got.last().unwrap()), "20261125T070000");
        assert!(got.len() < 1000);
    }

    #[test]
    fn thousands_of_earlier_occurrences_still_reach_the_window() {
        let (from, end) = (wall(2026, 11, 12, 0, 0, 0), wall(2026, 11, 13, 0, 0, 0));
        // Daily, 5000 times from 2015: the 12 Nov 2026 one is about the 4334th.
        let r = parse("FREQ=DAILY;COUNT=5000").unwrap();
        let got = expand(&r, wall(2015, 1, 1, 9, 0, 0), from, end, &|_| false, 2000);
        assert_eq!(got.into_iter().map(stamp).collect::<Vec<_>>(), ["20261112T090000"]);
        // COUNT still counts the earlier ones: 4000 from 2015 ended in 2025.
        let r = parse("FREQ=DAILY;COUNT=4000").unwrap();
        assert!(expand(&r, wall(2015, 1, 1, 9, 0, 0), from, end, &|_| false, 2000).is_empty());
        // Every weekday, monthly, since 2015: some 3,100 before the window.
        let r = parse("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR").unwrap();
        let got = expand(&r, wall(2015, 1, 1, 8, 0, 0), from, end, &|_| false, 2000);
        assert_eq!(got.into_iter().map(stamp).collect::<Vec<_>>(), ["20261112T080000"]);
    }
}
