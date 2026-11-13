//! The Today widget's data, read on a worker thread of its own so the shelf never waits on the
//! network: the calendar every 15 minutes, the weather every 30, both again when the machine
//! wakes or its network connection changes, and sooner after a failure. The last of each is
//! cached to disk and shown at once at the start. The worker sleeps between readings; the frame
//! takes what it published from a shared slot.

use crate::log;
use shellman_core::calendar::{self, CalError, Snapshot};
use shellman_core::weather::{self, Weather};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CALENDAR_EVERY: Duration = Duration::from_secs(15 * 60);
const WEATHER_EVERY: Duration = Duration::from_secs(30 * 60);
/// After a failure to connect; sooner while nothing was read yet (the network may still be
/// coming up after sign-in).
const RETRY: Duration = Duration::from_secs(2 * 60);
const FIRST_RETRY: Duration = Duration::from_secs(30);
/// A wake or network change reads again only when the last reading is older than this.
const FRESH: Duration = Duration::from_secs(60);

/// The calendar account as the settings have it.
#[derive(Clone, Debug, PartialEq)]
pub struct CalConfig {
    pub server: String,
    pub username: String,
    pub hidden: Vec<String>,
    /// Bumped by Settings when the password is saved, so a new one is tried at once.
    pub saved: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WeatherConfig {
    pub place: String,
    pub lat: f64,
    pub lon: f64,
    pub fahrenheit: bool,
}

/// What to read; None: not wanted (the widget is off, or nothing is set up).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Config {
    pub calendar: Option<CalConfig>,
    pub weather: Option<WeatherConfig>,
}

/// Where the calendar stands.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum CalStatus {
    /// No account set up (no user name, or no password saved).
    #[default]
    NotConnected,
    /// Reading for the first time, nothing cached.
    Connecting,
    Ready,
    SignInFailed,
    /// The server could not be reached or answered oddly (the cached events still show).
    Unreachable(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub calendar: CalStatus,
    /// The events: fresh, or from the cache.
    pub snapshot: Option<Snapshot>,
    pub weather: Option<Weather>,
}

enum Cmd {
    Config(Config),
    /// The machine woke, or the network changed.
    Refresh,
    Stop,
}

pub struct Feeds {
    tx: Sender<Cmd>,
    state: Arc<Mutex<(State, u64)>>,
    seen: u64,
}

impl Feeds {
    pub fn start(wake: impl Fn() + Send + 'static) -> Self {
        let (tx, rx) = channel();
        let state = Arc::new(Mutex::new((State::default(), 0)));
        let shared = state.clone();
        let net_tx = tx.clone();
        let _ = std::thread::Builder::new().name("widget-feeds".into()).spawn(move || {
            let hint = network::watch(net_tx);
            run(rx, shared, Box::new(wake));
            network::unwatch(hint);
        });
        Self { tx, state, seen: 0 }
    }

    pub fn configure(&self, config: Config) {
        let _ = self.tx.send(Cmd::Config(config));
    }

    pub fn refresh(&self) {
        let _ = self.tx.send(Cmd::Refresh);
    }

    /// The newest state, when it changed since the last call.
    pub fn take(&mut self) -> Option<State> {
        let guard = self.state.lock().ok()?;
        if guard.1 == self.seen {
            return None;
        }
        self.seen = guard.1;
        Some(guard.0.clone())
    }
}

impl Drop for Feeds {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}

fn now_utc() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

struct Worker {
    config: Config,
    state: State,
    next_cal: Option<Instant>,
    next_weather: Option<Instant>,
    /// When each was last read successfully.
    cal_ok: Option<Instant>,
    weather_ok: Option<Instant>,
}

impl Worker {
    fn apply(&mut self, config: Config) {
        let now = Instant::now();
        if config.calendar != self.config.calendar {
            self.cal_ok = None;
            match &config.calendar {
                Some(c) => {
                    let key = calendar::account_key(&c.server, &c.username);
                    // Events cached for this account show at once.
                    self.state.snapshot = calendar::load_cache().filter(|s| s.account == key);
                    self.state.calendar = if self.state.snapshot.is_some() { CalStatus::Ready } else { CalStatus::Connecting };
                    self.next_cal = Some(now);
                }
                None => {
                    self.state.snapshot = None;
                    self.state.calendar = CalStatus::NotConnected;
                    self.next_cal = None;
                }
            }
        }
        if config.weather != self.config.weather {
            self.weather_ok = None;
            match &config.weather {
                Some(w) => {
                    self.state.weather = weather::load_cache().filter(|c| c.is_for(w.lat, w.lon, w.fahrenheit));
                    self.next_weather = Some(now);
                }
                None => {
                    self.state.weather = None;
                    self.next_weather = None;
                }
            }
        }
        self.config = config;
    }

    fn refresh(&mut self) {
        let now = Instant::now();
        let stale = |ok: Option<Instant>| ok.is_none_or(|t| now.duration_since(t) > FRESH);
        if self.config.calendar.is_some() && stale(self.cal_ok) {
            self.next_cal = Some(now);
        }
        if self.config.weather.is_some() && stale(self.weather_ok) {
            self.next_weather = Some(now);
        }
    }

    fn read_calendar(&mut self) {
        let Some(c) = self.config.calendar.clone() else { return };
        let now = Instant::now();
        let Some(account) = calendar::account(&c.server, &c.username) else {
            self.state.calendar = CalStatus::NotConnected;
            self.state.snapshot = None;
            self.next_cal = Some(now + CALENDAR_EVERY);
            log::line("calendar: no password saved");
            return;
        };
        let hidden = c.hidden.clone();
        match calendar::fetch(&account, &|cal| !hidden.contains(&cal.href), now_utc()) {
            Ok(s) => {
                log::line(format!("calendar: {} calendars, {} events", s.calendars.len(), s.instances.len()));
                calendar::save_cache(&s);
                self.state.snapshot = Some(s);
                self.state.calendar = CalStatus::Ready;
                self.cal_ok = Some(now);
                self.next_cal = Some(now + CALENDAR_EVERY);
            }
            Err(CalError::SignIn) => {
                log::line("calendar: sign-in failed");
                self.state.calendar = CalStatus::SignInFailed;
                self.state.snapshot = None;
                calendar::clear_cache();
                self.next_cal = Some(now + CALENDAR_EVERY);
            }
            Err(e) => {
                log::line(format!("calendar: {}", e.line()));
                self.state.calendar = CalStatus::Unreachable(e.line());
                self.next_cal = Some(now + if self.cal_ok.is_none() { FIRST_RETRY } else { RETRY });
            }
        }
    }

    fn read_weather(&mut self) {
        let Some(w) = self.config.weather.clone() else { return };
        let now = Instant::now();
        match weather::fetch(&w.place, w.lat, w.lon, w.fahrenheit, now_utc()) {
            Ok(reading) => {
                weather::save_cache(&reading);
                self.state.weather = Some(reading);
                self.weather_ok = Some(now);
                self.next_weather = Some(now + WEATHER_EVERY);
            }
            Err(e) => {
                log::line(format!("weather: {e}"));
                // The cached reading keeps showing for a few hours, then goes.
                if self.state.weather.as_ref().is_some_and(|r| now_utc() - r.fetched > 6 * 3600) {
                    self.state.weather = None;
                }
                self.next_weather = Some(now + if self.weather_ok.is_none() { FIRST_RETRY } else { RETRY });
            }
        }
    }
}

fn run(rx: Receiver<Cmd>, shared: Arc<Mutex<(State, u64)>>, wake: Box<dyn Fn() + Send>) {
    let mut w = Worker { config: Config::default(), state: State::default(), next_cal: None, next_weather: None, cal_ok: None, weather_ok: None };
    let publish = |state: &State| {
        if let Ok(mut g) = shared.lock() {
            if g.0 != *state {
                g.0 = state.clone();
                g.1 += 1;
                drop(g);
                wake();
            }
        }
    };
    loop {
        let next = [w.next_cal, w.next_weather].into_iter().flatten().min();
        let cmd = match next {
            Some(at) => rx.recv_timeout(at.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match cmd {
            Ok(Cmd::Config(c)) => w.apply(c),
            Ok(Cmd::Refresh) => w.refresh(),
            Ok(Cmd::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
        // What is cached shows before the network is asked.
        publish(&w.state);
        let now = Instant::now();
        if w.next_cal.is_some_and(|t| t <= now) {
            w.read_calendar();
            publish(&w.state);
        }
        if w.next_weather.is_some_and(|t| t <= now) {
            w.read_weather();
            publish(&w.state);
        }
    }
}

/// Windows' network connectivity hints: a change sends a refresh.
mod network {
    use super::Cmd;
    use std::sync::mpsc::Sender;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::NetworkManagement::IpHelper::{CancelMibChangeNotify2, NotifyNetworkConnectivityHintChange};
    use windows::Win32::Networking::WinSock::NL_NETWORK_CONNECTIVITY_HINT;

    pub struct Hint {
        handle: HANDLE,
        context: *mut Sender<Cmd>,
    }

    unsafe extern "system" fn changed(context: *const core::ffi::c_void, _hint: NL_NETWORK_CONNECTIVITY_HINT) {
        if let Some(tx) = (context as *const Sender<Cmd>).as_ref() {
            let _ = tx.send(Cmd::Refresh);
        }
    }

    pub fn watch(tx: Sender<Cmd>) -> Option<Hint> {
        let context = Box::into_raw(Box::new(tx));
        let mut handle = HANDLE::default();
        let r = unsafe { NotifyNetworkConnectivityHintChange(Some(changed), Some(context as *const _), false, &mut handle) };
        if r.is_ok() {
            Some(Hint { handle, context })
        } else {
            drop(unsafe { Box::from_raw(context) });
            None
        }
    }

    pub fn unwatch(hint: Option<Hint>) {
        if let Some(h) = hint {
            unsafe {
                let _ = CancelMibChangeNotify2(h.handle);
                drop(Box::from_raw(h.context));
            }
        }
    }
}
