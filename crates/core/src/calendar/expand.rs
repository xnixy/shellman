//! Calendar objects to the instances in a window of time: recurring events expanded (RRULE and
//! RDATE), EXDATEs taken out, overridden instances (RECURRENCE-ID) put in their place, cancelled
//! ones dropped. Pure, with the zones coming in through a [`Resolver`].

use super::ical::{self, Component, DateValue};
use super::rrule::{self, Until};
use super::time::DAY;
use super::zone::Resolver;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One occurrence of an event, as the widget shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instance {
    pub uid: String,
    pub title: String,
    pub location: String,
    /// The calendar it is from (its URL).
    pub calendar: String,
    /// Start and end, UTC seconds. For an all-day event, its days' starts in the viewer's zone.
    pub start: i64,
    pub end: i64,
    /// For an all-day event, the days it covers: `[first, last + 1)`, days since 1970.
    pub days: Option<(i64, i64)>,
}

/// An event's fields that matter here.
#[derive(Clone, Debug)]
struct Event {
    uid: String,
    recurrence_id: Option<DateValue>,
    title: String,
    location: String,
    start: DateValue,
    /// DTEND, or DTSTART + DURATION; None: a point (timed) or one day (all-day).
    end: Option<DateValue>,
    duration: Option<i64>,
    rrule: Option<String>,
    rdates: Vec<DateValue>,
    exdates: Vec<DateValue>,
    cancelled: bool,
}

impl Event {
    fn read(c: &Component) -> Option<Self> {
        let start = ical::date_value(c.prop("DTSTART")?)?;
        Some(Self {
            uid: c.text("UID").unwrap_or_default(),
            recurrence_id: c.prop("RECURRENCE-ID").and_then(ical::date_value),
            title: c.text("SUMMARY").map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).unwrap_or_else(|| "(No title)".into()),
            location: c.text("LOCATION").map(|s| s.lines().next().unwrap_or("").trim().to_owned()).unwrap_or_default(),
            start,
            end: c.prop("DTEND").and_then(ical::date_value),
            duration: c.prop("DURATION").and_then(|p| ical::duration(&p.value)),
            rrule: c.prop("RRULE").map(|p| p.value.clone()),
            rdates: c.props_named("RDATE").flat_map(ical::date_values).collect(),
            exdates: c.props_named("EXDATE").flat_map(ical::date_values).collect(),
            cancelled: c.text("STATUS").is_some_and(|s| s.eq_ignore_ascii_case("CANCELLED")),
        })
    }

    /// How long each occurrence lasts: seconds for a timed event, days for an all-day one.
    fn length(&self, r: &Resolver) -> i64 {
        match (&self.start, &self.end) {
            (DateValue::Date(a), Some(DateValue::Date(b))) => (b - a).max(1),
            (DateValue::Date(_), _) => self.duration.map_or(1, |d| (d / DAY).max(1)),
            (s, Some(e)) => (r.instant(e) - r.instant(s)).max(0),
            (_, None) => self.duration.unwrap_or(0).max(0),
        }
    }
}

/// The zone a value's wall time is in (None: the viewer's).
fn tz_of(v: &DateValue) -> Option<&str> {
    match v {
        DateValue::Zoned(_, id) => Some(id),
        _ => None,
    }
}

/// The wall time of a value on its own clock.
fn wall_of(v: &DateValue) -> i64 {
    match v {
        DateValue::Date(d) => d * DAY,
        DateValue::Floating(w) | DateValue::Zoned(w, _) | DateValue::Utc(w) => *w,
    }
}

/// A value of the same kind as `like` at another wall time.
fn same_kind(like: &DateValue, w: i64) -> DateValue {
    match like {
        DateValue::Date(_) => DateValue::Date(w.div_euclid(DAY)),
        DateValue::Floating(_) => DateValue::Floating(w),
        DateValue::Utc(_) => DateValue::Utc(w),
        DateValue::Zoned(_, id) => DateValue::Zoned(w, id.clone()),
    }
}

/// What identifies one occurrence: its day (all-day) or its instant.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Key {
    Day(i64),
    At(i64),
}

fn key(v: &DateValue, r: &Resolver) -> Key {
    match v {
        DateValue::Date(d) => Key::Day(*d),
        other => Key::At(r.instant(other)),
    }
}

fn instance(e: &Event, start: &DateValue, calendar: &str, r: &Resolver) -> Instance {
    let len = e.length(r);
    let (s, end, days) = match start {
        DateValue::Date(d) => {
            let (a, b) = (*d, d + len);
            (r.to_utc(None, a * DAY), r.to_utc(None, b * DAY), Some((a, b)))
        }
        other => {
            let s = r.instant(other);
            (s, s + len, None)
        }
    };
    Instance {
        uid: e.uid.clone(),
        title: e.title.clone(),
        location: e.location.clone(),
        calendar: calendar.to_owned(),
        start: s,
        end,
        days,
    }
}

/// Does an instance touch `[from, to)`? A zero-length one counts at its start.
fn overlaps(i: &Instance, from: i64, to: i64) -> bool {
    i.start < to && (i.end > from || (i.end == i.start && i.start >= from))
}

/// The start values of the occurrences of a master event that can touch `[from, to)`: those
/// before are passed over (they still count toward COUNT), so a rule with thousands of earlier
/// occurrences still reaches the window.
fn occurrences(e: &Event, from: i64, to: i64, r: &Resolver) -> Vec<DateValue> {
    let tz = tz_of(&e.start).map(str::to_owned);
    let mut out = vec![e.start.clone()];
    if let Some(rule) = e.rrule.as_deref().and_then(rrule::parse) {
        let start_wall = wall_of(&e.start);
        // The window on the event's own clock, with slack for the offset; an occurrence that
        // starts before the window but lasts into it counts too.
        let end_wall = to + 2 * DAY;
        let len = if e.start.is_date() { e.length(r).saturating_mul(DAY) } else { e.length(r) };
        let from_wall = from.saturating_sub(len).saturating_sub(2 * DAY);
        let until = rule.until.clone();
        let is_utc = matches!(e.start, DateValue::Utc(_));
        let past = |w: i64| match &until {
            Some(Until::Day(d)) => w.div_euclid(DAY) > *d,
            Some(Until::Utc(u)) => {
                let at = if is_utc { w } else if e.start.is_date() { r.to_utc(None, w) } else { r.to_utc(tz.as_deref(), w) };
                at > *u
            }
            Some(Until::Wall(u)) => w > *u,
            None => false,
        };
        out = rrule::expand(&rule, start_wall, from_wall, end_wall, &past, 2000).into_iter().map(|w| same_kind(&e.start, w)).collect();
        if out.is_empty() {
            out.push(e.start.clone());
        }
    }
    for rd in &e.rdates {
        // An RDATE of another kind (a DATE on a timed event) takes the event's time of day.
        let v = if rd.is_date() == e.start.is_date() { rd.clone() } else { same_kind(&e.start, wall_of(rd) + wall_of(&e.start).rem_euclid(DAY)) };
        if !out.contains(&v) {
            out.push(v);
        }
    }
    out
}

/// Every instance of the events in `objects` (each an iCalendar text's components) from
/// `calendar` that touches `[from, to)`, sorted by start.
pub fn instances(objects: &[Vec<Component>], calendar: &str, from: i64, to: i64, r: &Resolver) -> Vec<Instance> {
    let mut by_uid: HashMap<String, (Option<Event>, Vec<Event>)> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for comps in objects {
        let mut events = Vec::new();
        for top in comps {
            if top.name == "VEVENT" {
                events.push(top);
            }
            top.find_all("VEVENT", &mut events);
        }
        for (n, c) in events.into_iter().enumerate() {
            let Some(mut e) = Event::read(c) else { continue };
            if e.uid.is_empty() {
                // No UID: stands on its own.
                e.uid = format!("no-uid-{}-{n}-{}", order.len(), wall_of(&e.start));
            }
            if !by_uid.contains_key(&e.uid) {
                order.push(e.uid.clone());
            }
            let slot = by_uid.entry(e.uid.clone()).or_default();
            if e.recurrence_id.is_some() {
                slot.1.push(e);
            } else if slot.0.is_none() {
                slot.0 = Some(e);
            }
        }
    }
    let mut out = Vec::new();
    for uid in order {
        let Some((master, overrides)) = by_uid.remove(&uid) else { continue };
        let mut used = vec![false; overrides.len()];
        if let Some(m) = &master {
            if !m.cancelled {
                let ex: Vec<Key> = m.exdates.iter().map(|x| key(x, r)).collect();
                for occ in occurrences(m, from, to, r) {
                    let k = key(&occ, r);
                    if ex.contains(&k) {
                        continue;
                    }
                    let replaced = overrides.iter().position(|o| o.recurrence_id.as_ref().is_some_and(|rid| key(rid, r) == k));
                    let i = match replaced {
                        Some(n) => {
                            used[n] = true;
                            if overrides[n].cancelled {
                                continue;
                            }
                            instance(&overrides[n], &overrides[n].start, calendar, r)
                        }
                        None => instance(m, &occ, calendar, r),
                    };
                    if overlaps(&i, from, to) {
                        out.push(i);
                    }
                }
            }
        }
        // Overrides of occurrences the master does not give (or with no master sent at all).
        for (n, o) in overrides.iter().enumerate() {
            if used[n] || o.cancelled || master.as_ref().is_some_and(|m| m.cancelled) {
                continue;
            }
            let i = instance(o, &o.start, calendar, r);
            if overlaps(&i, from, to) {
                out.push(i);
            }
        }
    }
    out.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.title.cmp(&b.title)));
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::ical::parse;
    use crate::calendar::time::stamp;
    use crate::calendar::zone::{london, wall_at, Zones};

    fn run(ics: &str, from: i64, to: i64) -> Vec<Instance> {
        let clock = london();
        let comps = parse(ics);
        let mut zones = Zones::default();
        zones.add(&comps);
        let r = Resolver { zones: &zones, clock: &clock };
        instances(&[comps], "cal", from, to, &r)
    }

    fn cal(body: &str) -> String {
        format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{}{body}END:VCALENDAR\r\n", zone_text())
    }

    fn zone_text() -> String {
        let z = crate::calendar::zone::SAMPLE_ZONES;
        z.trim_start_matches("BEGIN:VCALENDAR\r\n").trim_end_matches("END:VCALENDAR\r\n").to_owned()
    }

    fn starts(list: &[Instance]) -> Vec<String> {
        list.iter().map(|i| stamp(i.start)).collect()
    }

    #[test]
    fn a_weekly_meeting_keeps_its_wall_time_across_the_dst_change() {
        // Thursdays 09:00 London; the clocks go back on Sunday 25 Oct 2026.
        let ics = cal("BEGIN:VEVENT\r\nUID:standup\r\nSUMMARY:Standup\r\nDTSTART;TZID=Europe/London:20261015T090000\r\nDTEND;TZID=Europe/London:20261015T091500\r\nRRULE:FREQ=WEEKLY\r\nEND:VEVENT\r\n");
        let got = run(&ics, wall_at(2026, 10, 14, 0, 0), wall_at(2026, 11, 1, 0, 0));
        // 08:00 UTC in summer time, 09:00 UTC after.
        assert_eq!(starts(&got), ["20261015T080000", "20261022T080000", "20261029T090000"]);
        assert!(got.iter().all(|i| i.end - i.start == 900));
    }

    #[test]
    fn new_york_events_move_for_their_own_change_not_londons() {
        // 10:00 New York daily, across 1 Nov 2026 (New York falls back) and 25 Oct (London does).
        let ics = cal("BEGIN:VEVENT\r\nUID:ny\r\nSUMMARY:NY call\r\nDTSTART;TZID=America/New_York:20261030T100000\r\nDURATION:PT30M\r\nRRULE:FREQ=DAILY;COUNT=4\r\nEND:VEVENT\r\n");
        let got = run(&ics, wall_at(2026, 10, 29, 0, 0), wall_at(2026, 11, 5, 0, 0));
        assert_eq!(starts(&got), ["20261030T140000", "20261031T140000", "20261101T150000", "20261102T150000"]);
    }

    #[test]
    fn exdates_and_overrides() {
        let ics = cal(concat!(
            "BEGIN:VEVENT\r\nUID:gym\r\nSUMMARY:Gym\r\nDTSTART;TZID=Europe/London:20261109T180000\r\nDTEND;TZID=Europe/London:20261109T190000\r\n",
            "RRULE:FREQ=DAILY;COUNT=6\r\nEXDATE;TZID=Europe/London:20261111T180000\r\nEND:VEVENT\r\n",
            // The 12th moved to 20:00 and renamed; the 13th cancelled.
            "BEGIN:VEVENT\r\nUID:gym\r\nRECURRENCE-ID;TZID=Europe/London:20261112T180000\r\nSUMMARY:Gym (late)\r\nDTSTART;TZID=Europe/London:20261112T200000\r\nDTEND;TZID=Europe/London:20261112T210000\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:gym\r\nRECURRENCE-ID;TZID=Europe/London:20261113T180000\r\nSTATUS:CANCELLED\r\nDTSTART;TZID=Europe/London:20261113T180000\r\nEND:VEVENT\r\n",
        ));
        let got = run(&ics, wall_at(2026, 11, 1, 0, 0), wall_at(2026, 12, 1, 0, 0));
        assert_eq!(starts(&got), ["20261109T180000", "20261110T180000", "20261112T200000", "20261114T180000"]);
        assert_eq!(got[2].title, "Gym (late)");
    }

    #[test]
    fn an_all_day_event_across_midnight_covers_both_days() {
        // A two-day all-day event, 12 and 13 Nov, in a UTC-stamped calendar.
        let ics = cal("BEGIN:VEVENT\r\nUID:trip\r\nSUMMARY:Lisbon\r\nDTSTART;VALUE=DATE:20261112\r\nDTEND;VALUE=DATE:20261114\r\nEND:VEVENT\r\n");
        let got = run(&ics, wall_at(2026, 11, 13, 12, 0), wall_at(2026, 11, 20, 0, 0));
        assert_eq!(got.len(), 1, "still under way on the 13th");
        let days = got[0].days.unwrap();
        assert_eq!(days.1 - days.0, 2);
        assert_eq!(stamp(got[0].start), "20261112T000000");
        assert_eq!(stamp(got[0].end), "20261114T000000");
        // A one-day all-day event with no DTEND, in summer: London midnight is 23:00 UTC.
        let ics = cal("BEGIN:VEVENT\r\nUID:bday\r\nSUMMARY:Birthday\r\nDTSTART;VALUE=DATE:20260701\r\nRRULE:FREQ=YEARLY\r\nEND:VEVENT\r\n");
        let got = run(&ics, wall_at(2027, 6, 1, 0, 0), wall_at(2027, 8, 1, 0, 0));
        assert_eq!(starts(&got), ["20270630T230000"]);
        assert_eq!(got[0].end - got[0].start, DAY);
    }

    #[test]
    fn a_timed_event_across_midnight_and_utc_and_floating_times() {
        let ics = cal(concat!(
            "BEGIN:VEVENT\r\nUID:late\r\nSUMMARY:Night shift\r\nDTSTART:20261112T220000Z\r\nDTEND:20261113T060000Z\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:float\r\nSUMMARY:Floating lunch\r\nDTSTART:20260701T120000\r\nDTEND:20260701T130000\r\nEND:VEVENT\r\n",
        ));
        let got = run(&ics, wall_at(2026, 11, 13, 1, 0), wall_at(2026, 11, 14, 0, 0));
        assert_eq!(starts(&got), ["20261112T220000"]);
        let got = run(&ics, wall_at(2026, 7, 1, 0, 0), wall_at(2026, 7, 2, 0, 0));
        // Floating: the viewer's zone (London, summer).
        assert_eq!(starts(&got), ["20260701T110000"]);
    }

    #[test]
    fn todays_occurrence_of_a_long_running_rule_shows() {
        let ics = cal(concat!(
            "BEGIN:VEVENT\r\nUID:pill\r\nSUMMARY:Pill\r\nDTSTART;TZID=Europe/London:20150101T080000\r\nDURATION:PT5M\r\nRRULE:FREQ=DAILY;COUNT=5000\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:standup\r\nSUMMARY:Standup\r\nDTSTART;TZID=Europe/London:20150101T093000\r\nDURATION:PT15M\r\nRRULE:FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR\r\nEND:VEVENT\r\n",
        ));
        let got = run(&ics, wall_at(2026, 11, 12, 0, 0), wall_at(2026, 11, 13, 0, 0));
        assert_eq!(starts(&got), ["20261112T080000", "20261112T093000"]);
    }

    #[test]
    fn an_override_without_its_master_still_shows() {
        let ics = cal("BEGIN:VEVENT\r\nUID:x\r\nRECURRENCE-ID:20261112T100000Z\r\nSUMMARY:Moved\r\nDTSTART:20261112T150000Z\r\nDTEND:20261112T160000Z\r\nEND:VEVENT\r\n");
        let got = run(&ics, wall_at(2026, 11, 12, 0, 0), wall_at(2026, 11, 13, 0, 0));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].title, "Moved");
    }
}
