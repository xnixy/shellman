//! The calendar behind the Today widget: CalDAV (built for Fastmail, any CalDAV server works),
//! iCalendar with its time zones and recurrences, and what the widget lists.
//!
//! The pieces are pure and tested on recorded answers (`ical`, `rrule`, `zone`, `expand`,
//! `caldav`, `select`); `client` does the requests and `system` asks Windows about zones.
//! [`fetch`] puts them together for the shelf's worker; [`Snapshot`]s are cached to disk (events
//! only, never the password, which lives in Credential Manager under [`CREDENTIAL`]).

pub mod caldav;
pub mod client;
pub mod expand;
pub mod feed;
pub mod ical;
pub mod month;
pub mod rrule;
pub mod select;
pub mod span;
pub mod system;
pub mod time;
pub mod zone;

pub use caldav::CalendarRef;
pub use client::{Account, CalError};
pub use expand::Instance;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The Credential Manager name of the calendar's app password.
pub const CREDENTIAL: &str = "caldav";
/// How far ahead events are read.
pub const AHEAD_DAYS: i64 = 14;

/// What was last read from the calendar.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    /// When, UTC seconds.
    pub fetched: i64,
    /// The account it was read for (a cache from another account is not shown).
    pub account: String,
    pub calendars: Vec<CalendarRef>,
    pub instances: Vec<Instance>,
}

impl Snapshot {
    pub fn calendar(&self, href: &str) -> Option<&CalendarRef> {
        self.calendars.iter().find(|c| c.href == href)
    }
}

/// The account saved in the settings, with its password from Credential Manager; None when
/// there is no user name or no password.
pub fn account(server: &str, username: &str) -> Option<Account> {
    let username = username.trim();
    if username.is_empty() || server.trim().is_empty() {
        return None;
    }
    let secret = crate::credentials::read(CREDENTIAL)?;
    (!secret.secret.is_empty()).then(|| Account { server: server.trim().to_owned(), username: username.to_owned(), password: secret.secret })
}

/// A short name for an account, for the cache: `user@server`.
pub fn account_key(server: &str, username: &str) -> String {
    format!("{}@{}", username.trim(), server.trim().trim_end_matches('/'))
}

/// Read the account's calendars (all, or those `wanted` keeps) and their events from a day ago
/// to [`AHEAD_DAYS`] ahead of `now`. One calendar that fails to answer leaves the rest.
pub fn fetch(account: &Account, wanted: &dyn Fn(&CalendarRef) -> bool, now: i64) -> Result<Snapshot, CalError> {
    let (from, to) = span::regular(now);
    let (calendars, instances) = fetch_range(account, wanted, from, to)?;
    Ok(Snapshot { fetched: now, account: account_key(&account.server, &account.username), calendars, instances })
}

/// The account's calendars and the events of `[from, to)` (UTC seconds), recurrences expanded
/// for that range, sorted by start.
pub fn fetch_range(
    account: &Account,
    wanted: &dyn Fn(&CalendarRef) -> bool,
    from: i64,
    to: i64,
) -> Result<(Vec<CalendarRef>, Vec<Instance>), CalError> {
    let calendars = account.discover()?;
    let clock = system::SystemClock::new();
    let mut instances = Vec::new();
    let mut failures = 0;
    let shown: Vec<&CalendarRef> = calendars.iter().filter(|c| wanted(c)).collect();
    for cal in &shown {
        match account.events(&cal.href, from, to) {
            Ok(texts) => {
                let objects: Vec<Vec<ical::Component>> = texts.iter().map(|t| ical::parse(t)).collect();
                let mut zones = zone::Zones::default();
                for o in &objects {
                    zones.add(o);
                }
                let r = zone::Resolver { zones: &zones, clock: &clock };
                instances.extend(expand::instances(&objects, &cal.href, from, to, &r));
            }
            Err(CalError::SignIn) => return Err(CalError::SignIn),
            Err(e) => {
                failures += 1;
                if failures == shown.len() {
                    return Err(e);
                }
            }
        }
    }
    instances.sort_by(|a, b| a.start.cmp(&b.start));
    Ok((calendars, instances))
}

/// A calendar's colour: its own (`#3A87AD`), else one of a few by its place in the list. None
/// for a calendar not in the list.
pub fn color_of(calendars: &[CalendarRef], href: &str) -> Option<[u8; 3]> {
    let i = calendars.iter().position(|c| c.href == href)?;
    Some(calendars[i].color.as_deref().and_then(hex).unwrap_or(FALLBACK[i % FALLBACK.len()]))
}

/// `#3A87AD` (or `#3A87ADFF`) as red, green and blue.
pub fn hex(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().strip_prefix('#')?;
    let n = u32::from_str_radix(h.get(..6)?, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

/// A calendar without a colour of its own gets one of these, by its place in the list.
const FALLBACK: [[u8; 3]; 5] = [[91, 140, 255], [244, 63, 142], [31, 209, 165], [251, 146, 60], [176, 107, 255]];

fn cache_file() -> Option<PathBuf> {
    crate::config::cache_dir().map(|d| d.join("calendar.json"))
}

pub fn load_cache() -> Option<Snapshot> {
    let s: Snapshot = crate::config::load_from(&cache_file()?);
    (s.fetched > 0).then_some(s)
}

pub fn save_cache(s: &Snapshot) {
    if let Some(f) = cache_file() {
        crate::config::save_to(&f, s);
    }
}

pub fn clear_cache() {
    if let Some(f) = cache_file() {
        let _ = std::fs::remove_file(f);
    }
}
