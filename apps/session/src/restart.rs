//! "Restart app X": a request from `shellman-setup update --restart` (or anyone) that an app the
//! session supervises be ended and started again from its (replaced) exe.
//!
//! Transport: `WM_COPYDATA` to the session's window (found by class `ShellmanSession` and title,
//! see `window::title`), `dwData` = [`MAGIC`], data = the UTF-8 text `restart <name>` where
//! `<name>` is the app's short name (`bar`, `shelf`, ...). The window procedure answers at once
//! with one of [`Reply`] and queues the request; the supervisor handles it on its next pass.
//!
//! Why this cannot trip the crash back-off or the bar's safety net: the supervisor remembers
//! that the exit of that process was asked for, and treats it as a restart that is not a crash
//! (`policy::Crashes::on_requested_exit`: no entry in the crash list, no pause), whatever the
//! exit code is. The bar's slot goes from running straight to "start scheduled" (never "lost"),
//! and the safety net's 20 s tray deadline counts from the new copy's own start. A new copy that
//! then crashes by itself is an ordinary crash again, so a broken update still ends in Explorer.

use std::sync::Mutex;

/// `dwData` of the COPYDATASTRUCT: "SHMR".
pub const MAGIC: usize = 0x5348_4D52;

/// How the window answers (`SendMessage` result).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reply {
    /// Not an app this session supervises.
    Unknown = 0,
    /// Queued: the app will be asked to close and started again.
    Accepted = 1,
    /// The session is ending, or has handed over to Explorer: nothing is restarted.
    Refused = 2,
}

/// The request text for an app (what `shellman-setup` sends; here to test the format).
#[cfg(test)]
pub fn encode(name: &str) -> Vec<u8> {
    format!("restart {name}").into_bytes()
}

/// The app name in a request, or `None` when it is not a well-formed request.
pub fn decode(data: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(data).ok()?;
    let name = text.strip_prefix("restart ")?;
    let ok = !name.is_empty() && name.len() <= 32 && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    ok.then(|| name.to_string())
}

/// What to answer for `name`, given the supervised apps and whether the session is quiet.
pub fn reply_for(known: &[String], name: &str, quiet: bool) -> Reply {
    if quiet {
        Reply::Refused
    } else if known.iter().any(|k| k == name) {
        Reply::Accepted
    } else {
        Reply::Unknown
    }
}

static KNOWN: Mutex<Vec<String>> = Mutex::new(Vec::new());
static QUEUE: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The supervisor says which apps it runs (once, at its start).
pub fn publish_names(names: Vec<String>) {
    if let Ok(mut k) = KNOWN.lock() {
        *k = names;
    }
}

/// Called from the window procedure with the raw request.
pub fn on_request(data: &[u8], quiet: bool) -> Reply {
    let Some(name) = decode(data) else { return Reply::Unknown };
    let known = KNOWN.lock().map(|k| k.clone()).unwrap_or_default();
    let reply = reply_for(&known, &name, quiet);
    if reply == Reply::Accepted {
        if let Ok(mut q) = QUEUE.lock() {
            if !q.contains(&name) {
                q.push(name);
            }
        }
    }
    reply
}

/// The queued requests, for the supervisor.
pub fn take() -> Vec<String> {
    QUEUE.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip() {
        assert_eq!(decode(&encode("bar")).as_deref(), Some("bar"));
        assert_eq!(decode(b"restart shelf-2").as_deref(), Some("shelf-2"));
    }

    #[test]
    fn malformed_requests_are_ignored() {
        for bad in [&b""[..], b"restart", b"restart ", b"restart Bar", b"restart ..\\x", b"stop bar", b"restart a b", &[0xff, 0xfe]] {
            assert_eq!(decode(bad), None, "{bad:?}");
        }
        assert_eq!(decode(format!("restart {}", "a".repeat(33)).as_bytes()), None);
    }

    #[test]
    fn answers_by_what_is_supervised() {
        let known = vec!["bar".to_string(), "shelf".to_string()];
        assert_eq!(reply_for(&known, "bar", false), Reply::Accepted);
        assert_eq!(reply_for(&known, "files", false), Reply::Unknown);
        assert_eq!(reply_for(&known, "bar", true), Reply::Refused, "ending or parked: nothing restarts");
    }

    #[test]
    fn accepted_requests_are_queued_once() {
        publish_names(vec!["qa".to_string(), "qb".to_string()]);
        assert_eq!(on_request(&encode("qa"), false), Reply::Accepted);
        assert_eq!(on_request(&encode("qa"), false), Reply::Accepted);
        assert_eq!(on_request(&encode("zz"), false), Reply::Unknown);
        assert_eq!(on_request(&encode("qb"), true), Reply::Refused);
        assert_eq!(take(), ["qa"]);
        assert!(take().is_empty());
    }
}
