//! What each widget shows, worked out from the data and the clock, apart from the painting:
//! the date, the weather's line, the rows (`calendar::select`) or the note that stands in for
//! them, and the media session's lines. Pure, so the states are tested.

use super::feeds::{CalStatus, State};
use super::place::TodayShape;
use egui::Color32;
use shellman_core::calendar::select::{self, Group};
use shellman_core::calendar::time::{civil_from_days, weekday, DAY};
use shellman_core::system::media::NowPlaying;
use shellman_core::weather::{degrees, Sky};

#[derive(Clone, Debug, PartialEq)]
pub struct WeatherLine {
    pub sky: Sky,
    /// "6°".
    pub temp: String,
    /// "London · light rain".
    pub line: String,
    /// "H 8° · L 3°".
    pub range: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub time: String,
    pub title: String,
    pub detail: String,
    pub color: Option<Color32>,
    pub chip: Option<String>,
    pub next: bool,
}

/// A line standing in for rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub title: String,
    pub detail: Option<String>,
    /// Drawn in the text colour and heavier (a problem to fix), not muted.
    pub strong: bool,
    /// A click opens Settings › Desktop.
    pub settings: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TodayModel {
    pub weekday: String,
    pub day: String,
    pub month: String,
    pub weather: Option<WeatherLine>,
    /// "Tomorrow" over tomorrow's rows.
    pub heading: Option<String>,
    pub rows: Vec<Row>,
    pub note: Option<Note>,
}

impl TodayModel {
    pub fn shape(&self) -> TodayShape {
        let note = self.note.as_ref().map_or(0, |n| if n.detail.is_some() { 2 } else { 1 });
        TodayShape { rows: self.rows.len(), heading: self.heading.is_some(), note }
    }
}

const WEEKDAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// `#3A87AD` as a colour.
pub fn hex(s: &str) -> Option<Color32> {
    let h = s.trim().strip_prefix('#')?;
    let n = u32::from_str_radix(h.get(..6)?, 16).ok()?;
    Some(Color32::from_rgb((n >> 16) as u8, (n >> 8) as u8, n as u8))
}

/// A calendar without a colour of its own gets one of these, by its place in the list.
const FALLBACK: [Color32; 5] = [
    Color32::from_rgb(91, 140, 255),
    Color32::from_rgb(244, 63, 142),
    Color32::from_rgb(31, 209, 165),
    Color32::from_rgb(251, 146, 60),
    Color32::from_rgb(176, 107, 255),
];

fn note(title: &str, detail: Option<&str>, strong: bool, settings: bool) -> Option<Note> {
    Some(Note { title: title.into(), detail: detail.map(str::to_owned), strong, settings })
}

/// Today at `now` (UTC), with the viewer's offset from `offset`.
pub fn today(state: &State, calendar_set_up: bool, now: i64, offset: &dyn Fn(i64) -> i64, h24: bool) -> TodayModel {
    let local_day = (now + offset(now)).div_euclid(DAY);
    let (_, month, day) = civil_from_days(local_day);
    let weather = state.weather.as_ref().map(|w| {
        let (sky, words) = w.condition();
        WeatherLine {
            sky,
            temp: degrees(w.temp),
            line: if w.place.is_empty() { words.to_owned() } else { format!("{} · {words}", w.place) },
            range: format!("H {} · L {}", degrees(w.high), degrees(w.low)),
        }
    });
    let mut m = TodayModel {
        weekday: WEEKDAYS[weekday(local_day) as usize].into(),
        day: day.to_string(),
        month: MONTHS[(month - 1) as usize].into(),
        weather,
        heading: None,
        rows: Vec::new(),
        note: None,
    };
    if !calendar_set_up {
        m.note = note("Calendar not connected", Some("Add an account in Settings › Desktop"), false, true);
        return m;
    }
    let snapshot = match (&state.calendar, &state.snapshot) {
        (CalStatus::SignInFailed, _) => {
            m.note = note("Sign-in failed", Some("Check the app password in Settings › Desktop"), true, true);
            return m;
        }
        (CalStatus::NotConnected, _) => {
            m.note = note("Calendar not connected", Some("Save the app password in Settings › Desktop"), false, true);
            return m;
        }
        (CalStatus::Connecting, None) => {
            m.note = note("Connecting to your calendar…", None, false, false);
            return m;
        }
        (CalStatus::Unreachable(_), None) => {
            m.note = note("Can't reach the calendar", Some("Trying again in a few minutes"), true, false);
            return m;
        }
        (_, None) => {
            m.note = note("Connecting to your calendar…", None, false, false);
            return m;
        }
        (_, Some(s)) => s,
    };
    let name_of = |href: &str| snapshot.calendar(href).map(|c| c.name.clone()).unwrap_or_default();
    let shown = select::select(&snapshot.instances, now, offset, &name_of, h24, super::place::MAX_ROWS);
    match shown {
        None => m.note = note("Nothing on today or tomorrow", None, false, false),
        Some(s) => {
            if s.group == Group::Tomorrow {
                m.heading = Some("Nothing else today · Tomorrow".into());
            }
            m.rows = s
                .rows
                .into_iter()
                .map(|r| {
                    let color = snapshot
                        .calendars
                        .iter()
                        .position(|c| c.href == r.calendar)
                        .map(|i| snapshot.calendars[i].color.as_deref().and_then(hex).unwrap_or(FALLBACK[i % FALLBACK.len()]));
                    Row { time: r.time, title: r.title, detail: r.detail, color, chip: r.chip, next: r.next }
                })
                .collect();
        }
    }
    m
}

#[derive(Clone, Debug, PartialEq)]
pub struct NowModel {
    pub title: String,
    /// "M83 · 1:52 / 4:03", or the artist alone when the player gives no times.
    pub line: String,
    pub playing: bool,
    pub position: f64,
    pub duration: f64,
    pub can_previous: bool,
    pub can_next: bool,
    pub can_toggle: bool,
}

/// Now playing at `now` (UTC seconds).
pub fn now_playing(n: &NowPlaying, now: f64) -> NowModel {
    let position = n.position_at(now);
    let who = if n.artist.is_empty() { n.album.clone() } else { n.artist.clone() };
    let times = (n.duration > 0.0).then(|| format!("{} / {}", super::paint::clock(position), super::paint::clock(n.duration)));
    let line = match (who.is_empty(), times) {
        (false, Some(t)) => format!("{who} · {t}"),
        (true, Some(t)) => t,
        (_, None) => who,
    };
    NowModel {
        title: n.title.clone(),
        line,
        playing: n.playing,
        position,
        duration: n.duration,
        can_previous: n.can_previous,
        can_next: n.can_next,
        can_toggle: n.can_toggle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellman_core::calendar::{CalendarRef, Instance, Snapshot};
    use shellman_core::weather::Weather;

    /// Thursday 12 Nov 2026, 12:41 UTC.
    const NOW: i64 = 1_794_487_260;

    fn ready(instances: Vec<Instance>) -> State {
        State {
            calendar: CalStatus::Ready,
            snapshot: Some(Snapshot {
                fetched: NOW,
                account: "a".into(),
                calendars: vec![
                    CalendarRef { href: "work".into(), name: "Work".into(), color: Some("#3A87AD".into()) },
                    CalendarRef { href: "home".into(), name: "Home".into(), color: None },
                ],
                instances,
            }),
            weather: None,
            spans: Vec::new(),
        }
    }

    fn ev(title: &str, cal: &str, start: i64) -> Instance {
        Instance { uid: title.into(), title: title.into(), location: String::new(), calendar: cal.into(), start, end: start + 3600, days: None }
    }

    #[test]
    fn the_date_and_the_weather() {
        let mut s = ready(Vec::new());
        s.weather = Some(Weather { place: "London".into(), temp: 6.4, code: 61, day: true, high: 8.2, low: 2.6, fetched: NOW, ..Default::default() });
        let m = today(&s, true, NOW, &|_| 0, true);
        assert_eq!((m.weekday.as_str(), m.day.as_str(), m.month.as_str()), ("Thursday", "12", "November"));
        let w = m.weather.unwrap();
        assert_eq!((w.temp.as_str(), w.line.as_str(), w.range.as_str()), ("6°", "London · light rain", "H 8° · L 3°"));
        assert_eq!(m.note.unwrap().title, "Nothing on today or tomorrow");
        // Late in the evening west of UTC it is still Thursday; east of it, Friday.
        assert_eq!(today(&s, true, NOW + 11 * 3600 + 30 * 60, &|_| 3600, true).weekday, "Friday");
    }

    #[test]
    fn events_with_their_calendars_colours() {
        let m = today(&ready(vec![ev("Review", "work", NOW + 3600), ev("Dinner", "home", NOW + 5 * 3600)]), true, NOW, &|_| 0, true);
        assert!(m.note.is_none() && m.heading.is_none());
        assert_eq!(m.rows.len(), 2);
        assert_eq!(m.rows[0].color, Some(Color32::from_rgb(0x3A, 0x87, 0xAD)));
        assert_eq!(m.rows[0].detail, "Work");
        assert_eq!(m.rows[0].chip.as_deref(), Some("in 1 h"));
        assert_eq!(m.rows[1].color, Some(FALLBACK[1]));
        assert_eq!(m.shape(), TodayShape { rows: 2, heading: false, note: 0 });
    }

    #[test]
    fn tomorrow_gets_a_heading() {
        let m = today(&ready(vec![ev("Standup", "work", NOW + 20 * 3600)]), true, NOW, &|_| 0, true);
        assert!(m.heading.unwrap().contains("Tomorrow"));
        assert_eq!(m.rows.len(), 1);
    }

    #[test]
    fn every_problem_says_so_never_a_blank() {
        let m = today(&State::default(), false, NOW, &|_| 0, true);
        let n = m.note.unwrap();
        assert_eq!(n.title, "Calendar not connected");
        assert!(n.settings);
        let failed = State { calendar: CalStatus::SignInFailed, ..Default::default() };
        let n = today(&failed, true, NOW, &|_| 0, true).note.unwrap();
        assert_eq!(n.title, "Sign-in failed");
        assert!(n.strong && n.settings);
        let away = State { calendar: CalStatus::Unreachable("x".into()), ..Default::default() };
        assert_eq!(today(&away, true, NOW, &|_| 0, true).note.unwrap().title, "Can't reach the calendar");
        let connecting = State { calendar: CalStatus::Connecting, ..Default::default() };
        assert!(today(&connecting, true, NOW, &|_| 0, true).note.unwrap().title.starts_with("Connecting"));
        // Unreachable with cached events: the events show.
        let mut cached = ready(vec![ev("Review", "work", NOW + 3600)]);
        cached.calendar = CalStatus::Unreachable("offline".into());
        let m = today(&cached, true, NOW, &|_| 0, true);
        assert_eq!(m.rows.len(), 1);
        assert!(m.note.is_none());
    }

    #[test]
    fn media_lines() {
        let n = NowPlaying { title: "Midnight City".into(), artist: "M83".into(), playing: true, position: 100.0, at: 1000.0, duration: 243.0, ..Default::default() };
        let m = now_playing(&n, 1012.4);
        assert_eq!(m.line, "M83 · 1:52 / 4:03");
        let paused = NowPlaying { playing: false, ..n.clone() };
        assert_eq!(now_playing(&paused, 5000.0).line, "M83 · 1:40 / 4:03");
        let no_times = NowPlaying { duration: 0.0, ..n };
        assert_eq!(now_playing(&no_times, 1012.0).line, "M83");
        assert_eq!(hex("#3a87adff"), Some(Color32::from_rgb(0x3A, 0x87, 0xAD)));
        assert_eq!(hex("blue"), None);
    }
}
