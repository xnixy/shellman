//! Time zones for calendar times: the `VTIMEZONE` definitions a calendar carries (Fastmail and
//! most servers send one per zone they use), with the system's zone database behind them for a
//! TZID a calendar names but does not define, and the viewer's own zone for floating times and
//! all-day events. The rules here are pure; the system's lookups come in through [`Clock`].

use super::ical::{self, Component};
use super::rrule::{self, RRule};
use super::time::{wall, DAY};
use std::collections::HashMap;

/// What the system knows about zones. Offsets are seconds east of UTC.
pub trait Clock {
    /// The viewer's own offset at the instant `utc`.
    fn local_offset(&self, utc: i64) -> i64;
    /// A named zone's offset at `utc` from the system's zone database (IANA or Windows names),
    /// None when it does not know the name.
    fn zone_offset(&self, tzid: &str, utc: i64) -> Option<i64>;
}

/// One STANDARD or DAYLIGHT part of a VTIMEZONE.
#[derive(Clone, Debug)]
struct Observance {
    /// Its first onset, as a wall time in the offset before it.
    start: i64,
    from: i64,
    to: i64,
    rule: Option<RRule>,
    rdates: Vec<i64>,
}

/// `+0100`, `-0530`, `+023000`.
fn offset(s: &str) -> Option<i64> {
    let s = s.trim();
    let sign = match s.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let n = |a: usize, b: usize| s.get(a..b).and_then(|t| t.parse::<i64>().ok());
    let secs = n(1, 3)? * 3600 + n(3, 5)? * 60 + n(5, 7).unwrap_or(0);
    Some(sign * secs)
}

impl Observance {
    fn read(c: &Component) -> Option<Self> {
        let start = match ical::date_value(c.prop("DTSTART")?)? {
            ical::DateValue::Floating(w) | ical::DateValue::Zoned(w, _) | ical::DateValue::Utc(w) => w,
            ical::DateValue::Date(d) => d * DAY,
        };
        let from = offset(&c.prop("TZOFFSETFROM")?.value)?;
        let to = offset(&c.prop("TZOFFSETTO")?.value)?;
        let rule = c.prop("RRULE").and_then(|p| rrule::parse(&p.value));
        let rdates = c
            .props_named("RDATE")
            .flat_map(ical::date_values)
            .filter_map(|d| match d {
                ical::DateValue::Floating(w) | ical::DateValue::Zoned(w, _) => Some(w),
                ical::DateValue::Utc(u) => Some(u + from),
                ical::DateValue::Date(_) => None,
            })
            .collect();
        Some(Self { start, from, to, rule, rdates })
    }

    /// The latest onset (UTC) at or before `utc`, if any.
    fn last_onset(&self, utc: i64) -> Option<i64> {
        let as_utc = |w: i64| w - self.from;
        let mut best: Option<i64> = None;
        let mut take = |u: i64| {
            if u <= utc && best.is_none_or(|b| u > b) {
                best = Some(u);
            }
        };
        take(as_utc(self.start));
        for w in &self.rdates {
            take(as_utc(*w));
        }
        if let Some(rule) = &self.rule {
            let until = rule.until.clone();
            let from = self.from;
            let past = move |w: i64| match &until {
                Some(rrule::Until::Utc(u)) => w - from > *u,
                Some(rrule::Until::Wall(u)) => w > *u,
                Some(rrule::Until::Day(d)) => w.div_euclid(DAY) > *d,
                None => false,
            };
            // Onsets up to the instant (a year of slack covers any offset).
            for w in rrule::expand(rule, self.start, self.start, utc + self.from + DAY, &past, 4000) {
                take(as_utc(w));
            }
        }
        best
    }
}

/// The VTIMEZONEs of one or more calendars, by TZID.
#[derive(Clone, Debug, Default)]
pub struct Zones {
    map: HashMap<String, Vec<Observance>>,
}

impl Zones {
    /// Take every VTIMEZONE under `components`.
    pub fn add(&mut self, components: &[Component]) {
        for top in components {
            let mut found = Vec::new();
            if top.name == "VTIMEZONE" {
                found.push(top);
            }
            top.find_all("VTIMEZONE", &mut found);
            for tz in found {
                let Some(id) = tz.prop("TZID").map(|p| p.value.trim().trim_start_matches('/').to_owned()) else { continue };
                let obs: Vec<Observance> = tz
                    .children
                    .iter()
                    .filter(|c| c.name == "STANDARD" || c.name == "DAYLIGHT")
                    .filter_map(Observance::read)
                    .collect();
                if !obs.is_empty() {
                    self.map.entry(id).or_insert(obs);
                }
            }
        }
    }

    /// The offset of zone `tzid` at `utc` from its VTIMEZONE, if the calendar defined it.
    pub fn offset_at(&self, tzid: &str, utc: i64) -> Option<i64> {
        let obs = self.map.get(tzid)?;
        let latest = obs.iter().filter_map(|o| o.last_onset(utc).map(|t| (t, o.to))).max_by_key(|(t, _)| *t);
        Some(match latest {
            Some((_, to)) => to,
            // Before every onset: the earliest observance's offset before it.
            None => obs.iter().min_by_key(|o| o.start).map_or(0, |o| o.from),
        })
    }
}

/// How the instants of calendar times are found: the calendar's own VTIMEZONEs first, then the
/// system's zone database, then (an unknown name) the viewer's own zone.
pub struct Resolver<'a> {
    pub zones: &'a Zones,
    pub clock: &'a dyn Clock,
}

impl Resolver<'_> {
    /// The offset of `tzid` (None: the viewer's zone) at `utc`.
    pub fn offset(&self, tzid: Option<&str>, utc: i64) -> i64 {
        match tzid {
            None => self.clock.local_offset(utc),
            Some(id) => self
                .zones
                .offset_at(id, utc)
                .or_else(|| self.clock.zone_offset(id, utc))
                .unwrap_or_else(|| self.clock.local_offset(utc)),
        }
    }

    /// The instant a wall time in `tzid` names. A time that happens twice (the hour the clocks
    /// go back) is its first; one that never happens (the hour they skip) is read with the
    /// offset from before the change, so 02:30 becomes 03:30 summer time (RFC 5545 §3.3.5).
    pub fn to_utc(&self, tzid: Option<&str>, w: i64) -> i64 {
        let before = self.offset(tzid, w - DAY);
        let after = self.offset(tzid, w + DAY);
        for o in [before, after] {
            if self.offset(tzid, w - o) == o {
                return w - o;
            }
        }
        w - before
    }

    /// The instant of a calendar time; a DATE is its day's start in the viewer's zone.
    pub fn instant(&self, v: &ical::DateValue) -> i64 {
        match v {
            ical::DateValue::Utc(t) => *t,
            ical::DateValue::Floating(w) => self.to_utc(None, *w),
            ical::DateValue::Zoned(w, id) => self.to_utc(Some(id), *w),
            ical::DateValue::Date(d) => self.to_utc(None, d * DAY),
        }
    }
}

/// A fixed offset, for tests and as a last resort.
pub struct Fixed(pub i64);

impl Clock for Fixed {
    fn local_offset(&self, _utc: i64) -> i64 {
        self.0
    }
    fn zone_offset(&self, _tzid: &str, _utc: i64) -> Option<i64> {
        None
    }
}

/// A clock whose own zone is a VTIMEZONE (tests: "the viewer is in London").
pub struct ZoneClock {
    pub zones: Zones,
    pub local: String,
}

impl Clock for ZoneClock {
    fn local_offset(&self, utc: i64) -> i64 {
        self.zones.offset_at(&self.local, utc).unwrap_or(0)
    }
    fn zone_offset(&self, tzid: &str, utc: i64) -> Option<i64> {
        self.zones.offset_at(tzid, utc)
    }
}

/// Europe/London and America/New_York as calendars send them, for tests.
pub const SAMPLE_ZONES: &str = "BEGIN:VCALENDAR\r
BEGIN:VTIMEZONE\r
TZID:Europe/London\r
BEGIN:DAYLIGHT\r
TZOFFSETFROM:+0000\r
TZOFFSETTO:+0100\r
TZNAME:BST\r
DTSTART:19810329T010000\r
RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU\r
END:DAYLIGHT\r
BEGIN:STANDARD\r
TZOFFSETFROM:+0100\r
TZOFFSETTO:+0000\r
TZNAME:GMT\r
DTSTART:19961027T020000\r
RRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU\r
END:STANDARD\r
END:VTIMEZONE\r
BEGIN:VTIMEZONE\r
TZID:America/New_York\r
BEGIN:DAYLIGHT\r
TZOFFSETFROM:-0500\r
TZOFFSETTO:-0400\r
DTSTART:20070311T020000\r
RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=2SU\r
END:DAYLIGHT\r
BEGIN:STANDARD\r
TZOFFSETFROM:-0400\r
TZOFFSETTO:-0500\r
DTSTART:20071104T020000\r
RRULE:FREQ=YEARLY;BYMONTH=11;BYDAY=1SU\r
END:STANDARD\r
END:VTIMEZONE\r
END:VCALENDAR\r
";

/// The sample zones, with London as the viewer's.
pub fn london() -> ZoneClock {
    let mut zones = Zones::default();
    zones.add(&ical::parse(SAMPLE_ZONES));
    ZoneClock { zones, local: "Europe/London".into() }
}

/// A wall time's parts, for tests and logs.
pub fn wall_at(y: i64, m: u32, d: u32, hh: u32, mm: u32) -> i64 {
    wall(y, m, d, hh, mm, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_follow_the_rules() {
        let clock = london();
        let r = Resolver { zones: &clock.zones, clock: &clock };
        // 29 Mar 2026 01:00 UTC London springs forward.
        assert_eq!(r.offset(Some("Europe/London"), wall_at(2026, 3, 29, 0, 59)), 0);
        assert_eq!(r.offset(Some("Europe/London"), wall_at(2026, 3, 29, 1, 0)), 3600);
        // 25 Oct 2026 01:00 UTC it falls back.
        assert_eq!(r.offset(Some("Europe/London"), wall_at(2026, 10, 25, 0, 59)), 3600);
        assert_eq!(r.offset(Some("Europe/London"), wall_at(2026, 10, 25, 1, 0)), 0);
        assert_eq!(r.offset(Some("America/New_York"), wall_at(2026, 11, 12, 12, 0)), -5 * 3600);
        assert_eq!(r.offset(Some("America/New_York"), wall_at(2026, 7, 1, 12, 0)), -4 * 3600);
        // A name nobody defined: the viewer's zone.
        assert_eq!(r.offset(Some("Mars/Olympus"), wall_at(2026, 7, 1, 12, 0)), 3600);
    }

    #[test]
    fn wall_times_through_the_changes() {
        let clock = london();
        let r = Resolver { zones: &clock.zones, clock: &clock };
        let lon = Some("Europe/London");
        assert_eq!(r.to_utc(lon, wall_at(2026, 7, 1, 9, 0)), wall_at(2026, 7, 1, 8, 0));
        assert_eq!(r.to_utc(lon, wall_at(2026, 12, 1, 9, 0)), wall_at(2026, 12, 1, 9, 0));
        // 01:30 on the spring day does not exist: read as GMT, 02:30 BST.
        assert_eq!(r.to_utc(lon, wall_at(2026, 3, 29, 1, 30)), wall_at(2026, 3, 29, 1, 30));
        // 01:30 on the autumn day happens twice: the first, still BST.
        assert_eq!(r.to_utc(lon, wall_at(2026, 10, 25, 1, 30)), wall_at(2026, 10, 25, 0, 30));
        assert_eq!(offset("+0530"), Some(19_800));
        assert_eq!(offset("-023000"), Some(-9000));
        assert_eq!(offset("0100"), None);
    }
}
