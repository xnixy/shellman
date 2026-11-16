//! What the "now" pill shows, and how it changes from one thing to another. Pure: the calendar,
//! the media session, what apps tell the taskbar and the time come in as arguments, so every rule
//! is tested.
//!
//! Priority, first that applies:
//! 1. an app asking for attention (its window flashes), for its first 10 minutes: something is
//!    waiting on you now (the newest call, "+2 more" when several);
//! 2. an event starting within 15 minutes (only with a calendar connected): it needs doing next;
//! 3. progress an app shows on its taskbar button: a copy in shellman Files ("Copying · 64%"),
//!    any app's ("Edge · 40%"); a stopped or paused one before one that runs, else the one that
//!    started first, so the pill does not hop between them;
//! 4. an event under way ("… · now · until 15:00");
//! 5. a call for attention older than 10 minutes: still not answered, but no longer news (the
//!    dock keeps its steady amber dot);
//! 6. what is playing (or was paused less than 10 minutes ago, so a click that paused it can
//!    resume it);
//! 7. else nothing: the pill is hidden.
//!
//! A call for attention lasts until its window comes forward (`dock::tasks`). The app in front is
//! not repeated here as such: the left island already names it and its window.

use shellman_core::calendar::{color_of, select::clock, CalendarRef, Instance};
use shellman_core::settings::bar::NowSettings;
use shellman_core::system::media::NowPlaying;
use shellman_core::taskbar_list::Progress;
use std::time::{Duration, Instant};

/// An event shows this long before it starts.
pub const EVENT_AHEAD: i64 = 15 * 60;
/// A paused track stays this long.
pub const PAUSED_FOR: i64 = 10 * 60;
/// A call for attention outranks everything this long, then steps down below the calendar and
/// progress (it stays until answered).
pub const ATTENTION_FRESH: Duration = Duration::from_secs(10 * 60);
/// The cross-fade between two things.
pub const FADE: Duration = Duration::from_millis(220);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Event,
    Media,
    /// Work an app shows progress for: a click brings its window (the copy dialog) forward.
    Progress,
    /// An app asking for attention: a click brings its window forward, which answers it.
    Attention,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub kind: Kind,
    /// Which thing it is: a change of key cross-fades, a change of text alone does not.
    pub key: String,
    pub title: String,
    /// "in 12 min", "now", the artist, "64%", "asks for attention".
    pub detail: String,
    /// The event's calendar colour.
    pub color: Option<[u8; 3]>,
    pub playing: bool,
    /// The viewer's day of the event (the calendar opens on it).
    pub day: i64,
    /// The window a click brings forward (progress, attention).
    pub hwnd: isize,
    /// The progress shown, and the fraction done.
    pub progress: Progress,
    pub done: f32,
}

impl Shown {
    fn new(kind: Kind, key: String, title: String, detail: String) -> Self {
        Self { kind, key, title, detail, color: None, playing: false, day: 0, hwnd: 0, progress: Progress::None, done: 0.0 }
    }

    /// The detail at its widest, so the glass keeps one width while the percentage climbs.
    pub fn widest_detail(&self) -> String {
        if self.kind != Kind::Progress {
            return self.detail.clone();
        }
        match self.detail.find('%') {
            Some(end) => {
                let start = self.detail[..end].rfind(|c: char| !c.is_ascii_digit()).map_or(0, |i| i + 1);
                format!("{}100{}", &self.detail[..start], &self.detail[end..])
            }
            None => self.detail.clone(),
        }
    }

    /// What a screen reader says for the pill, and what a click does.
    pub fn spoken(&self) -> String {
        let what = if self.detail.is_empty() { self.title.clone() } else { format!("{}, {}", self.title, self.detail) };
        let does = match self.kind {
            Kind::Event => "Open the calendar",
            Kind::Media if self.playing => "Pause",
            Kind::Media => "Play",
            Kind::Progress => "Show the progress",
            Kind::Attention => "Bring it forward",
        };
        format!("{what}. {does}")
    }
}

/// What one app window tells the taskbar, as the pill weighs it.
#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub hwnd: isize,
    /// The app's name, as the dock and the left island say it.
    pub name: String,
    /// "Copying", "Moving", "Deleting": file work a shellman app named (`fsops::busy`).
    pub verb: Option<&'static str>,
    pub progress: Progress,
    /// The fraction done, 0..=1.
    pub done: f32,
    /// How long it has shown progress.
    pub progress_for: Duration,
    /// How long it has asked for attention, if it does.
    pub attention_for: Option<Duration>,
}

/// What may go in the pill.
pub struct Inputs<'a> {
    pub settings: NowSettings,
    /// The calendar's events and calendars, when one is connected.
    pub events: Option<(&'a [Instance], &'a [CalendarRef])>,
    pub media: Option<&'a NowPlaying>,
    /// When the session was first seen paused (UTC seconds).
    pub paused_since: Option<i64>,
    /// What the app windows tell the taskbar.
    pub tasks: &'a [Task],
    pub now: i64,
    pub h24: bool,
}

/// "in 5 min" (never "in 0 min").
fn until(secs: i64) -> String {
    format!("in {} min", ((secs + 59) / 60).max(1))
}

/// " · +2 more" when `n` others wait behind the one shown.
fn more(n: usize) -> String {
    if n == 0 {
        String::new()
    } else {
        format!(" · +{n} more")
    }
}

/// The event to show, and whether it is still to start (else under way).
fn event(inp: &Inputs, offset: &dyn Fn(i64) -> i64) -> Option<(Shown, bool)> {
    let (events, calendars) = inp.events?;
    let now = inp.now;
    let timed = |i: &&Instance| i.days.is_none() && i.end > now && i.start - now <= EVENT_AHEAD;
    // One about to start beats one under way: it is what needs doing next.
    let soon = events.iter().filter(timed).filter(|i| i.start > now).min_by_key(|i| (i.start, i.title.clone()));
    let running = events.iter().filter(timed).filter(|i| i.start <= now).max_by_key(|i| (i.start, std::cmp::Reverse(i.title.clone())));
    let i = soon.or(running)?;
    let ahead = i.start > now;
    let detail = if ahead { until(i.start - now) } else { format!("now · until {}", clock(i.end + offset(i.end), inp.h24)) };
    let mut s = Shown::new(Kind::Event, format!("event:{}:{}", i.uid, i.start), i.title.clone(), detail);
    s.color = color_of(calendars, &i.calendar);
    s.day = (i.start.max(now) + offset(i.start.max(now))).div_euclid(86_400);
    Some((s, ahead))
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
    let mut s = Shown::new(Kind::Media, format!("media:{}\u{1f}{}", m.title, who), m.title.clone(), who);
    s.playing = m.playing;
    Some(s)
}

/// The whole percent done, never 100 before it is.
fn percent(done: f32) -> u32 {
    (done.clamp(0.0, 1.0) * 100.0).floor() as u32
}

/// The progress to show: a stopped one, then a paused one, then the one that started first.
fn progress(inp: &Inputs) -> Option<Shown> {
    let rank = |p: Progress| match p {
        Progress::Error => 0,
        Progress::Paused => 1,
        _ => 2,
    };
    let mut busy: Vec<&Task> = inp.tasks.iter().filter(|t| t.progress != Progress::None).collect();
    busy.sort_by_key(|t| (rank(t.progress), std::cmp::Reverse(t.progress_for), t.hwnd));
    let t = *busy.first()?;
    let pct = format!("{}%", percent(t.done));
    let detail = match t.progress {
        Progress::Indeterminate => "in progress".to_owned(),
        Progress::Paused => format!("{pct} · paused"),
        Progress::Error => format!("{pct} · stopped"),
        _ => pct,
    } + &more(busy.len() - 1);
    let title = t.verb.map_or_else(|| t.name.clone(), str::to_owned);
    let mut s = Shown::new(Kind::Progress, format!("progress:{}", t.hwnd), title, detail);
    s.hwnd = t.hwnd;
    s.progress = t.progress;
    s.done = t.done;
    Some(s)
}

/// The newest call for attention, and whether it is fresh (under [`ATTENTION_FRESH`]).
fn attention(inp: &Inputs) -> Option<(Shown, bool)> {
    let asking: Vec<(&Task, Duration)> = inp.tasks.iter().filter_map(|t| Some((t, t.attention_for?))).collect();
    let (t, age) = asking.iter().min_by_key(|(t, age)| (*age, t.hwnd)).copied()?;
    let detail = format!("asks for attention{}", more(asking.len() - 1));
    let mut s = Shown::new(Kind::Attention, format!("attention:{}", t.hwnd), t.name.clone(), detail);
    s.hwnd = t.hwnd;
    Some((s, age < ATTENTION_FRESH))
}

/// Split something that ranks differently by a flag into (when true, when false).
fn split(x: Option<(Shown, bool)>) -> (Option<Shown>, Option<Shown>) {
    match x {
        Some((s, true)) => (Some(s), None),
        Some((s, false)) => (None, Some(s)),
        None => (None, None),
    }
}

/// What the pill shows now, if anything (the order is in the module's comment).
pub fn pick(inp: &Inputs, offset: &dyn Fn(i64) -> i64) -> Option<Shown> {
    let set = inp.settings;
    if !set.on {
        return None;
    }
    let (fresh, stale) = split(if set.attention { attention(inp) } else { None });
    let (soon, running) = split(if set.events { event(inp, offset) } else { None });
    fresh
        .or(soon)
        .or_else(|| if set.progress { progress(inp) } else { None })
        .or(running)
        .or(stale)
        .or_else(|| if set.media { media(inp) } else { None })
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
        let inp = Inputs { settings: NowSettings::default(), events: Some((events, &c)), media, paused_since, tasks: &[], now, h24: true };
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
        let mut inp = Inputs { settings: NowSettings::default(), events: Some((&events, &c)), media: Some(&m), paused_since: None, tasks: &[], now: NOW, h24: true };
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
        let inp = Inputs { settings: NowSettings::default(), events: Some((&events, &c)), media: None, paused_since: None, tasks: &[], now: late - 60, h24: true };
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

    fn task(hwnd: isize, name: &str) -> Task {
        Task { hwnd, name: name.into(), verb: None, progress: Progress::None, done: 0.0, progress_for: Duration::ZERO, attention_for: None }
    }

    fn copying(hwnd: isize, done: f32, secs: u64) -> Task {
        Task { verb: Some("Copying"), progress: Progress::Normal, done, progress_for: Duration::from_secs(secs), ..task(hwnd, "Files") }
    }

    fn asking(hwnd: isize, name: &str, secs: u64) -> Task {
        Task { attention_for: Some(Duration::from_secs(secs)), ..task(hwnd, name) }
    }

    fn with_tasks(events: &[Instance], media: Option<&NowPlaying>, tasks: &[Task]) -> Option<Shown> {
        let c = cals();
        let inp = Inputs { settings: NowSettings::default(), events: Some((events, &c)), media, paused_since: None, tasks, now: NOW, h24: true };
        pick(&inp, &|_| 0)
    }

    #[test]
    fn a_copy_shows_its_verb_and_whole_percent() {
        let s = with_tasks(&[], None, &[copying(5, 0.647, 3)]).unwrap();
        assert_eq!((s.kind, s.title.as_str(), s.detail.as_str(), s.hwnd), (Kind::Progress, "Copying", "64%", 5));
        // Never 100 % before it is done.
        assert_eq!(with_tasks(&[], None, &[copying(5, 0.999, 3)]).unwrap().detail, "99%");
        // The same copy further on: the same thing (no cross-fade), and room kept for "100%".
        let later = with_tasks(&[], None, &[copying(5, 0.9, 30)]).unwrap();
        assert_eq!(later.key, s.key);
        assert_eq!(s.widest_detail(), "100%");
        assert_eq!(Shown { detail: "7% · paused".into(), ..s.clone() }.widest_detail(), "100% · paused");
        assert_eq!(s.spoken(), "Copying, 64%. Show the progress");
    }

    #[test]
    fn other_apps_progress_is_named_by_the_app() {
        let edge = Task { progress: Progress::Normal, done: 0.4, ..task(7, "Edge") };
        let s = with_tasks(&[], None, &[edge.clone()]).unwrap();
        assert_eq!((s.title.as_str(), s.detail.as_str()), ("Edge", "40%"));
        let detail = |p| with_tasks(&[], None, &[Task { progress: p, ..edge.clone() }]).unwrap().detail;
        assert_eq!(detail(Progress::Indeterminate), "in progress");
        assert_eq!(detail(Progress::Paused), "40% · paused");
        assert_eq!(detail(Progress::Error), "40% · stopped");
    }

    #[test]
    fn a_stopped_copy_comes_first_then_the_one_started_first() {
        let stopped = Task { progress: Progress::Error, ..copying(9, 0.2, 1) };
        let s = with_tasks(&[], None, &[copying(3, 0.5, 60), stopped, copying(4, 0.1, 5)]).unwrap();
        assert_eq!((s.hwnd, s.detail.as_str()), (9, "20% · stopped · +2 more"));
        let paused = Task { progress: Progress::Paused, ..copying(8, 0.2, 1) };
        assert_eq!(with_tasks(&[], None, &[copying(3, 0.5, 60), paused]).unwrap().hwnd, 8);
        // Two running: the older stays, however the newer moves.
        let s = with_tasks(&[], None, &[copying(4, 0.9, 5), copying(3, 0.1, 60)]).unwrap();
        assert_eq!((s.hwnd, s.detail.as_str()), (3, "10% · +1 more"));
    }

    #[test]
    fn the_newest_call_for_attention_shows_until_answered() {
        let s = with_tasks(&[], None, &[asking(1, "Teams", 300), asking(2, "Discord", 20)]).unwrap();
        assert_eq!((s.kind, s.title.as_str(), s.detail.as_str(), s.hwnd), (Kind::Attention, "Discord", "asks for attention · +1 more", 2));
        assert_eq!(s.spoken(), "Discord, asks for attention · +1 more. Bring it forward");
        // Hours later it is still there (answering it is what ends it).
        assert_eq!(with_tasks(&[], None, &[asking(1, "Teams", 5 * 3600)]).unwrap().kind, Kind::Attention);
    }

    #[test]
    fn the_priority_runs_attention_soon_progress_under_way_old_attention_media() {
        let soon = ev("Standup", NOW + 5 * 60, 15);
        let under_way = ev("Call", NOW - 10 * 60, 30);
        let fresh = asking(1, "Teams", 60);
        let old = asking(2, "Mail", ATTENTION_FRESH.as_secs());
        let copy = copying(3, 0.5, 10);
        let m = song(true);
        let kind = |events: &[Instance], tasks: &[Task]| with_tasks(events, Some(&m), tasks).map(|s| (s.kind, s.title));
        // 1 over 2: an app waiting on you beats a meeting in five minutes.
        assert_eq!(kind(&[soon.clone()], &[fresh.clone(), copy.clone()]), Some((Kind::Attention, "Teams".into())));
        // 2 over 3: the meeting about to start beats a copy.
        assert_eq!(kind(&[soon.clone()], &[copy.clone()]), Some((Kind::Event, "Standup".into())));
        // 3 over 4: a copy beats a meeting already under way.
        assert_eq!(kind(&[under_way.clone()], &[copy.clone()]), Some((Kind::Progress, "Copying".into())));
        // 4 over 5: the meeting under way beats an old call for attention.
        assert_eq!(kind(&[under_way.clone()], &[old.clone()]), Some((Kind::Event, "Call".into())));
        // 5 over 6: an old call for attention still beats the music.
        assert_eq!(kind(&[], &[old.clone()]), Some((Kind::Attention, "Mail".into())));
        // An old call steps below a copy, a fresh one above.
        assert_eq!(kind(&[], &[old, copy.clone()]).unwrap().0, Kind::Progress);
        assert_eq!(kind(&[], &[fresh, copy]).unwrap().0, Kind::Attention);
        // 6: then the music.
        assert_eq!(kind(&[], &[task(4, "Idle")]).unwrap().0, Kind::Media);
    }

    #[test]
    fn progress_and_attention_can_be_turned_off() {
        let c = cals();
        let tasks = [copying(3, 0.5, 10), asking(1, "Teams", 60)];
        let mut inp = Inputs { settings: NowSettings::default(), events: Some((&[], &c)), media: None, paused_since: None, tasks: &tasks, now: NOW, h24: true };
        inp.settings.attention = false;
        assert_eq!(pick(&inp, &|_| 0).unwrap().kind, Kind::Progress);
        inp.settings.progress = false;
        assert_eq!(pick(&inp, &|_| 0), None);
    }
}
