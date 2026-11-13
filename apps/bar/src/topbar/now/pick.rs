//! What the "now" pill shows, and how it changes from one thing to another. Pure: the calendar,
//! the media session and the time come in as arguments, so every rule is tested.
//!
//! Priority: an event starting within 15 minutes or under way (only with a calendar connected
//! and events allowed), else what is playing (or was paused less than 10 minutes ago, so a click
//! that paused it can resume it), else nothing: the pill is hidden. The app in front is not
//! repeated here: the left island already names it and its window.

use shellman_core::calendar::{color_of, select::clock, CalendarRef, Instance};
use shellman_core::settings::bar::NowSettings;
use shellman_core::system::media::NowPlaying;
use std::time::{Duration, Instant};

/// An event shows this long before it starts.
pub const EVENT_AHEAD: i64 = 15 * 60;
/// A paused track stays this long.
pub const PAUSED_FOR: i64 = 10 * 60;
/// The cross-fade between two things.
pub const FADE: Duration = Duration::from_millis(220);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Event,
    Media,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub kind: Kind,
    /// Which thing it is: a change of key cross-fades, a change of text alone does not.
    pub key: String,
    pub title: String,
    /// "in 12 min", "now", or the artist.
    pub detail: String,
    /// The event's calendar colour.
    pub color: Option<[u8; 3]>,
    pub playing: bool,
    /// The viewer's day of the event (the calendar opens on it).
    pub day: i64,
}

/// What may go in the pill.
pub struct Inputs<'a> {
    pub settings: NowSettings,
    /// The calendar's events and calendars, when one is connected.
    pub events: Option<(&'a [Instance], &'a [CalendarRef])>,
    pub media: Option<&'a NowPlaying>,
    /// When the session was first seen paused (UTC seconds).
    pub paused_since: Option<i64>,
    pub now: i64,
    pub h24: bool,
}

/// "in 5 min" (never "in 0 min").
fn until(secs: i64) -> String {
    format!("in {} min", ((secs + 59) / 60).max(1))
}

fn event(inp: &Inputs, offset: &dyn Fn(i64) -> i64) -> Option<Shown> {
    let (events, calendars) = inp.events?;
    let now = inp.now;
    let timed = |i: &&Instance| i.days.is_none() && i.end > now && i.start - now <= EVENT_AHEAD;
    // One about to start beats one under way: it is what needs doing next.
    let soon = events.iter().filter(timed).filter(|i| i.start > now).min_by_key(|i| (i.start, i.title.clone()));
    let running = events.iter().filter(timed).filter(|i| i.start <= now).max_by_key(|i| (i.start, std::cmp::Reverse(i.title.clone())));
    let i = soon.or(running)?;
    let detail = if i.start > now { until(i.start - now) } else { format!("now · until {}", clock(i.end + offset(i.end), inp.h24)) };
    Some(Shown {
        kind: Kind::Event,
        key: format!("event:{}:{}", i.uid, i.start),
        title: i.title.clone(),
        detail,
        color: color_of(calendars, &i.calendar),
        playing: false,
        day: (i.start.max(now) + offset(i.start.max(now))).div_euclid(86_400),
    })
}

fn media(inp: &Inputs) -> Option<Shown> {
    let m = inp.media?;
    if m.title.is_empty() {
        return None;
    }
    if !m.playing && inp.paused_since.is_some_and(|since| inp.now - since >= PAUSED_FOR) {
        return None;
    }
    let who = if m.artist.is_empty() { m.album.clone() } else { m.artist.clone() };
    Some(Shown {
        kind: Kind::Media,
        key: format!("media:{}\u{1f}{}", m.title, who),
        title: m.title.clone(),
        detail: who,
        color: None,
        playing: m.playing,
        day: 0,
    })
}

/// What the pill shows now, if anything.
pub fn pick(inp: &Inputs, offset: &dyn Fn(i64) -> i64) -> Option<Shown> {
    if !inp.settings.on {
        return None;
    }
    let e = if inp.settings.events { event(inp, offset) } else { None };
    e.or_else(|| if inp.settings.media { media(inp) } else { None })
}

/// The pill between two things: the old one fading out while the new one fades in.
#[derive(Clone, Debug, Default)]
pub struct Fade {
    pub from: Option<Shown>,
    pub to: Option<Shown>,
    pub started: Option<Instant>,
}

impl Fade {
    /// Take what should show now. A different thing starts a cross-fade; the same thing with new
    /// words ("in 11 min") just takes them. True when it changed at all.
    pub fn update(&mut self, next: Option<Shown>, now: Instant) -> bool {
        let same = match (&self.to, &next) {
            (Some(a), Some(b)) => a.key == b.key,
            (None, None) => true,
            _ => false,
        };
        if same {
            let changed = self.to != next;
            self.to = next;
            return changed;
        }
        // Mid-fade, what was fading in is what fades out now.
        self.from = self.to.take();
        self.to = next;
        self.started = Some(now);
        true
    }

    /// How far the fade is, 0 to 1 (1 when nothing fades).
    pub fn progress(&self, now: Instant) -> f32 {
        match self.started {
            Some(at) => (now.saturating_duration_since(at).as_secs_f32() / FADE.as_secs_f32()).min(1.0),
            None => 1.0,
        }
    }

    /// Whether frames are still needed for the fade.
    pub fn moving(&self, now: Instant) -> bool {
        self.started.is_some() && self.progress(now) < 1.0
    }

    /// Done fading: forget what faded out.
    pub fn settle(&mut self, now: Instant) {
        if self.started.is_some() && self.progress(now) >= 1.0 {
            self.started = None;
            self.from = None;
        }
    }

    /// Whether the pill's island is there: while something shows or fades out.
    pub fn visible(&self) -> bool {
        self.to.is_some() || self.from.is_some()
    }
}

/// The island's width: the new thing's at once (the glass snaps to it, only the content fades),
/// or the old one's while it fades out to nothing. None when there is no island.
pub fn island_width(from_w: Option<f32>, to_w: Option<f32>) -> Option<f32> {
    to_w.or(from_w)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellman_core::calendar::zone::wall_at;

    /// Thursday 12 Nov 2026, 12:41 UTC.
    const NOW: i64 = 1_794_487_260;

    fn ev(title: &str, start: i64, mins: i64) -> Instance {
        Instance { uid: title.into(), title: title.into(), location: String::new(), calendar: "work".into(), start, end: start + mins * 60, days: None }
    }

    fn cals() -> Vec<CalendarRef> {
        vec![CalendarRef { href: "work".into(), name: "Work".into(), color: Some("#3A87AD".into()) }]
    }

    fn song(playing: bool) -> NowPlaying {
        NowPlaying { title: "Midnight City".into(), artist: "M83".into(), playing, ..Default::default() }
    }

    fn run(events: &[Instance], media: Option<&NowPlaying>, paused_since: Option<i64>, now: i64) -> Option<Shown> {
        let c = cals();
        let inp = Inputs { settings: NowSettings::default(), events: Some((events, &c)), media, paused_since, now, h24: true };
        pick(&inp, &|_| 0)
    }

    #[test]
    fn an_event_within_fifteen_minutes_comes_first() {
        let review = ev("Design review", NOW + 12 * 60, 60);
        let s = run(&[review.clone()], Some(&song(true)), None, NOW).unwrap();
        assert_eq!((s.kind, s.title.as_str(), s.detail.as_str()), (Kind::Event, "Design review", "in 12 min"));
        assert_eq!(s.color, Some([0x3A, 0x87, 0xAD]));
        // Sixteen minutes ahead: not yet, the song shows.
        let s = run(&[ev("Later", NOW + 16 * 60, 30)], Some(&song(true)), None, NOW).unwrap();
        assert_eq!(s.kind, Kind::Media);
        // Under way: "now", with its end.
        let s = run(&[ev("Call", NOW - 10 * 60, 30)], None, None, NOW).unwrap();
        assert_eq!(s.detail, "now · until 13:01");
        // Over, or all-day: nothing.
        let mut all_day = ev("Holiday", NOW - 3600, 600);
        all_day.days = Some((20_769, 20_770));
        assert_eq!(run(&[ev("Done", NOW - 3600, 30), all_day], None, None, NOW), None);
    }

    #[test]
    fn about_to_start_beats_under_way() {
        let s = run(&[ev("Long", NOW - 3600, 180), ev("Standup", NOW + 5 * 60, 15)], None, None, NOW).unwrap();
        assert_eq!(s.title, "Standup");
        assert_eq!(s.day, 20_769);
        // Seconds before it starts: "in 1 min", never "in 0 min".
        assert_eq!(run(&[ev("X", NOW + 20, 15)], None, None, NOW).unwrap().detail, "in 1 min");
    }

    #[test]
    fn media_while_playing_and_a_while_after_pausing() {
        let s = run(&[], Some(&song(true)), None, NOW).unwrap();
        assert_eq!((s.title.as_str(), s.detail.as_str(), s.playing), ("Midnight City", "M83", true));
        let paused = song(false);
        assert!(run(&[], Some(&paused), Some(NOW - 60), NOW).is_some_and(|s| !s.playing));
        assert_eq!(run(&[], Some(&paused), Some(NOW - PAUSED_FOR), NOW), None);
        let untitled = NowPlaying { title: String::new(), ..song(true) };
        assert_eq!(run(&[], Some(&untitled), None, NOW), None);
    }

    #[test]
    fn settings_limit_what_shows() {
        let c = cals();
        let events = [ev("Review", NOW + 60, 30)];
        let m = song(true);
        let mut inp = Inputs { settings: NowSettings::default(), events: Some((&events, &c)), media: Some(&m), paused_since: None, now: NOW, h24: true };
        inp.settings.events = false;
        assert_eq!(pick(&inp, &|_| 0).unwrap().kind, Kind::Media);
        inp.settings.media = false;
        assert_eq!(pick(&inp, &|_| 0), None);
        inp.settings = NowSettings { on: false, ..Default::default() };
        assert_eq!(pick(&inp, &|_| 0), None);
        // No calendar connected: no event, whatever the settings.
        let inp = Inputs { events: None, media: None, ..inp };
        assert_eq!(pick(&Inputs { settings: NowSettings::default(), ..inp }, &|_| 0), None);
    }

    #[test]
    fn the_viewers_zone_is_used_for_the_end_time_and_the_day() {
        // 23:50 UTC is 00:50 the next day in UTC+1.
        let late = wall_at(2026, 11, 12, 23, 50);
        let c = cals();
        let events = [ev("Night call", late, 30)];
        let inp = Inputs { settings: NowSettings::default(), events: Some((&events, &c)), media: None, paused_since: None, now: late - 60, h24: true };
        let s = pick(&inp, &|_| 3600).unwrap();
        assert_eq!(s.day, 20_770);
        let s = pick(&Inputs { now: late + 60, ..inp }, &|_| 3600).unwrap();
        assert_eq!(s.detail, "now · until 01:20");
    }

    #[test]
    fn a_new_thing_cross_fades_new_words_do_not() {
        let t0 = Instant::now();
        let a = run(&[ev("Review", NOW + 12 * 60, 30)], None, None, NOW).unwrap();
        let mut f = Fade::default();
        assert!(f.update(Some(a.clone()), t0));
        assert!(f.moving(t0) && f.from.is_none() && f.visible());
        f.settle(t0 + FADE);
        assert!(!f.moving(t0 + FADE));
        // A minute later: the same event, new words, no fade.
        let a2 = run(&[ev("Review", NOW + 12 * 60, 30)], None, None, NOW + 60).unwrap();
        assert!(f.update(Some(a2.clone()), t0 + FADE * 3));
        assert!(f.started.is_none() && f.to.as_ref().unwrap().detail == "in 11 min");
        assert!(!f.update(Some(a2.clone()), t0 + FADE * 3), "nothing new");
        // The song takes over: the event fades out as the song fades in.
        let b = run(&[], Some(&song(true)), None, NOW).unwrap();
        let t1 = t0 + FADE * 4;
        assert!(f.update(Some(b), t1));
        assert_eq!(f.from.as_ref().map(|s| s.kind), Some(Kind::Event));
        assert_eq!(f.progress(t1 + FADE / 2), 0.5);
        // Then nothing: the island stays while the song fades out, then goes.
        let t2 = t1 + FADE * 2;
        f.settle(t2);
        assert!(f.update(None, t2));
        assert!(f.visible());
        f.settle(t2 + FADE);
        assert!(!f.visible());
    }

    #[test]
    fn the_glass_snaps_to_the_new_width() {
        assert_eq!(island_width(Some(200.0), Some(150.0)), Some(150.0));
        assert_eq!(island_width(Some(200.0), None), Some(200.0), "fading out at its own width");
        assert_eq!(island_width(None, None), None);
    }
}
