//! The live data the top bar shows besides windows: the calendar (the same worker and cache as
//! the desktop's Today widget, `core::calendar::feed`) and what is playing (`core::system::media`,
//! the session the media keys control). Both run on threads of their own and only ask the host
//! for a frame when they have something new; the host's frame takes it and wakes the top bars
//! only when what the "now" pill shows may have changed, so a player that reports its position
//! every second does not repaint the bars.

use shellman_core::calendar::feed::{CalConfig, CalStatus, Config, Feeds, State};
use shellman_core::calendar::system::SystemClock;
use shellman_core::calendar::zone::Clock;
use shellman_core::settings::bar::BarSettings;
use shellman_core::settings::desktop::DesktopSettings;
use shellman_core::system::media::{Action, MediaWatch, NowPlaying};
use shellman_core::system::region::{self, Intl};
use std::sync::Arc;

fn now_utc() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn log(_line: &str) {
    // The bar keeps no log of its own; what the worker says is not about the user anyway.
}

pub struct Live {
    feeds: Option<Feeds>,
    config: Option<CalConfig>,
    pub cal: State,
    media: Option<MediaWatch>,
    pub playing: Option<NowPlaying>,
    /// When the session was first seen paused (UTC seconds), so a paused track stays in the
    /// pill for a while and then goes.
    pub paused_since: Option<i64>,
    clock: SystemClock,
    /// The region's formats, read again each minute.
    pub intl: Intl,
    minute: i64,
    /// The last range asked of the worker.
    asked: Option<(i64, i64)>,
}

impl Default for Live {
    fn default() -> Self {
        Self {
            feeds: None,
            config: None,
            cal: State::default(),
            media: None,
            playing: None,
            paused_since: None,
            clock: SystemClock::new(),
            intl: region::read_intl(),
            minute: now_utc() / 60,
            asked: None,
        }
    }
}

impl Live {
    /// Follow the settings: the calendar account (Settings › Desktop › Calendar) and whether the
    /// pill may show media. `wake` runs on the workers' threads.
    pub fn configure(&mut self, bar: &BarSettings, desktop: &DesktopSettings, wake: &Arc<dyn Fn() + Send + Sync>) {
        let cal = &desktop.widgets.calendar;
        let config = (!cal.username.trim().is_empty()).then(|| CalConfig {
            server: cal.server.clone(),
            username: cal.username.clone(),
            hidden: cal.hidden.clone(),
            saved: cal.saved,
        });
        if config != self.config || (config.is_some() && self.feeds.is_none()) {
            match &config {
                Some(c) => {
                    let feeds = self.feeds.get_or_insert_with(|| {
                        let wake = wake.clone();
                        Feeds::start(move || wake(), log)
                    });
                    feeds.configure(Config { calendar: Some(c.clone()), weather: None });
                    self.asked = None;
                }
                None => {
                    self.feeds = None;
                    self.cal = State::default();
                }
            }
            self.config = config;
        }
        let media = bar.now.on && bar.now.media;
        if media && self.media.is_none() {
            let wake = wake.clone();
            self.media = Some(MediaWatch::start(Arc::new(move || wake())));
        } else if !media && self.media.take().is_some() {
            self.playing = None;
            self.paused_since = None;
        }
    }

    /// Take what the workers sent. Returns whether the calendar changed, and whether what is
    /// playing changed in a way the pill shows (title, artist, playing or not).
    pub fn poll(&mut self) -> (bool, bool) {
        let minute = now_utc() / 60;
        if minute != self.minute {
            self.minute = minute;
            self.clock = SystemClock::new();
            self.intl = region::read_intl();
        }
        let mut cal = false;
        if let Some(state) = self.feeds.as_mut().and_then(Feeds::take) {
            cal = state != self.cal;
            self.cal = state;
        }
        let mut media = false;
        if let Some(now) = self.media.as_ref().and_then(MediaWatch::take) {
            let key = |n: &Option<NowPlaying>| n.as_ref().map(|n| (n.title.clone(), n.artist.clone(), n.playing));
            media = key(&now) != key(&self.playing);
            self.paused_since = match &now {
                Some(n) if !n.playing => self.paused_since.or(Some(now_utc())),
                _ => None,
            };
            self.playing = now;
        }
        (cal, media)
    }

    /// The machine woke or the network came back: read the calendar again.
    pub fn refresh(&mut self) {
        self.clock = SystemClock::new();
        self.intl = region::read_intl();
        if let Some(f) = &self.feeds {
            f.refresh();
        }
    }

    /// The viewer's offset from UTC at `utc`, in seconds.
    pub fn offset(&self, utc: i64) -> i64 {
        self.clock.local_offset(utc)
    }

    pub fn h24(&self) -> bool {
        self.intl.is_24h()
    }

    /// A calendar account is set up (it may still be connecting or failing).
    pub fn calendar_set_up(&self) -> bool {
        self.config.is_some()
    }

    pub fn calendar_ready(&self) -> bool {
        matches!(self.cal.calendar, CalStatus::Ready | CalStatus::Unreachable(_)) && self.cal.snapshot.is_some()
    }

    /// Ask for the events of `[from, to)` (UTC seconds), once per range.
    pub fn want(&mut self, from: i64, to: i64) {
        if self.asked != Some((from, to)) {
            self.asked = Some((from, to));
            if let Some(f) = &self.feeds {
                f.want(from, to);
            }
        }
    }

    /// Let the next `want` reach the worker even for the same range (the panel opened again: a
    /// month read long ago, or a failed read, is then looked at again).
    pub fn ask_again(&mut self) {
        self.asked = None;
    }

    /// Play/pause, previous or next, on the current media session.
    pub fn act(&self, action: Action) {
        if let Some(m) = &self.media {
            m.act(action);
        }
    }
}
