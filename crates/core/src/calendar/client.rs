//! Talking to a CalDAV server: finding the account's calendars (RFC 6764's `/.well-known/caldav`,
//! then the principal, then the calendar home) and fetching the events of a time range. Blocking;
//! called from a worker thread. The password goes into the Authorization header and nowhere else.

use super::caldav::{self, CalendarRef};
use super::time::stamp;
use crate::net::{self, NetError};

/// Fastmail's CalDAV server, the default.
pub const FASTMAIL: &str = "https://caldav.fastmail.com/";

/// How many redirects a discovery step follows.
const MAX_REDIRECTS: usize = 5;

pub struct Account {
    pub server: String,
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account").field("server", &self.server).field("username", &self.username).finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CalError {
    /// The server refused the user name and password (401 or 403).
    SignIn,
    /// No connection to the server.
    Network(String),
    /// The server answered, but not as a CalDAV server would.
    Server(String),
}

impl CalError {
    /// What Settings and the widget say.
    pub fn line(&self) -> String {
        match self {
            CalError::SignIn => "Sign-in failed".into(),
            CalError::Network(why) => format!("Can't reach the calendar: {why}"),
            CalError::Server(why) => format!("The calendar server answered oddly: {why}"),
        }
    }
}

impl From<NetError> for CalError {
    fn from(e: NetError) -> Self {
        match e {
            NetError::BadUrl => CalError::Server("the server address is not valid".into()),
            NetError::Network(why) => CalError::Network(why),
        }
    }
}

fn host_of(url: &str) -> Option<(bool, String, u16)> {
    net::parse_url(url).map(|u| (u.https, u.host.to_ascii_lowercase(), u.port))
}

impl Account {
    fn auth(&self) -> String {
        format!("Basic {}", net::base64(format!("{}:{}", self.username, self.password).as_bytes()))
    }

    /// One request, following redirects; credentials are only sent to the server's own host.
    fn dav(&self, method: &str, url: &str, depth: &str, body: &str) -> Result<(String, net::Response), CalError> {
        let home = host_of(&self.server);
        let mut url = url.to_owned();
        for _ in 0..=MAX_REDIRECTS {
            let auth = self.auth();
            let mut headers = vec![("Content-Type", "application/xml; charset=utf-8"), ("Depth", depth)];
            // Never send the password to another host, nor over plain HTTP to a remote one.
            let target = host_of(&url);
            let local = target.as_ref().is_some_and(|(_, h, _)| h == "127.0.0.1" || h == "localhost" || h == "[::1]");
            if target.is_some() && target == home && (target.as_ref().is_some_and(|t| t.0) || local) {
                headers.push(("Authorization", &auth));
            }
            let r = net::send(method, &url, &headers, Some(body.as_bytes()))?;
            match r.status {
                301 | 302 | 303 | 307 | 308 => {
                    let Some(loc) = r.location.clone() else { return Err(CalError::Server(format!("redirect {} without a place", r.status))) };
                    url = caldav::absolute(&url, &loc);
                }
                401 | 403 => return Err(CalError::SignIn),
                _ => return Ok((url, r)),
            }
        }
        Err(CalError::Server("too many redirects".into()))
    }

    fn propfind(&self, url: &str, depth: &str, body: &str) -> Result<(String, String), CalError> {
        let (at, r) = self.dav("PROPFIND", url, depth, body)?;
        if r.status != 207 {
            return Err(CalError::Server(format!("PROPFIND answered {}", r.status)));
        }
        Ok((at, r.text()))
    }

    /// The account's calendars that hold events.
    pub fn discover(&self) -> Result<Vec<CalendarRef>, CalError> {
        let server = self.server.trim();
        let base = if server.ends_with('/') { server.to_owned() } else { format!("{server}/") };
        if net::parse_url(&base).is_none() {
            return Err(CalError::Server("the server address is not valid".into()));
        }
        // The principal: from the well-known address, else the address as given.
        let well_known = caldav::absolute(&base, "/.well-known/caldav");
        let mut principal = None;
        let mut last_err = None;
        for start in [well_known.as_str(), base.as_str()] {
            match self.propfind(start, "0", caldav::PRINCIPAL_BODY) {
                Ok((at, xml)) => match caldav::principal(&xml, &at) {
                    Ok(Some(p)) => {
                        principal = Some(p);
                        break;
                    }
                    Ok(None) => {}
                    Err(e) => last_err = Some(CalError::Server(e)),
                },
                Err(CalError::SignIn) => return Err(CalError::SignIn),
                Err(e @ CalError::Network(_)) => return Err(e),
                Err(e) => last_err = Some(e),
            }
        }
        let Some(principal) = principal else {
            return Err(last_err.unwrap_or_else(|| CalError::Server("no calendar account found there".into())));
        };
        let (at, xml) = self.propfind(&principal, "0", caldav::HOME_BODY)?;
        let home = caldav::home(&xml, &at).map_err(CalError::Server)?.ok_or_else(|| CalError::Server("no calendar home".into()))?;
        let (at, xml) = self.propfind(&home, "1", caldav::LIST_BODY)?;
        caldav::calendars(&xml, &at).map_err(CalError::Server)
    }

    /// The iCalendar texts of the events of `calendar` touching `[from, to)` (UTC seconds).
    pub fn events(&self, calendar: &str, from: i64, to: i64) -> Result<Vec<String>, CalError> {
        let body = caldav::query_body(&format!("{}Z", stamp(from)), &format!("{}Z", stamp(to)));
        let (_, r) = self.dav("REPORT", calendar, "1", &body)?;
        if r.status != 207 {
            return Err(CalError::Server(format!("REPORT answered {}", r.status)));
        }
        caldav::calendar_data(&r.text()).map_err(CalError::Server)
    }
}
