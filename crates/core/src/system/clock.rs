//! Locale-aware clock text and the schedule for the bar's single periodic repaint.

use std::time::{Duration, SystemTime, UNIX_EPOCH};
use windows::core::{w, PCWSTR};
use windows::Win32::Globalization::{GetDateFormatEx, GetTimeFormatEx, TIME_NOSECONDS};

/// Extra delay past the boundary so the repaint lands in the new minute, not the old one.
const SLACK: Duration = Duration::from_millis(20);

/// (date, time) for the current local moment in the user's locale, e.g. ("Wed 11 Nov", "14:32").
/// Passing no SYSTEMTIME makes Windows use "now", saving a separate GetLocalTime call.
pub fn now_text() -> (String, String) {
    let mut buf = [0u16; 64];
    // SAFETY: buffers are valid for each call; a null locale name means the user default.
    unsafe {
        let n = GetDateFormatEx(
            PCWSTR::null(),
            Default::default(),
            None,
            w!("ddd d MMM"),
            Some(&mut buf),
            PCWSTR::null(),
        );
        let date = to_string(&buf, n);
        let n = GetTimeFormatEx(PCWSTR::null(), TIME_NOSECONDS, None, PCWSTR::null(), Some(&mut buf));
        let time = to_string(&buf, n);
        (date, time)
    }
}

/// Windows returns the length including the terminating NUL, or 0 on failure.
fn to_string(buf: &[u16], n: i32) -> String {
    let len = (n.max(1) as usize - 1).min(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// Time until the next wall-clock minute boundary, plus slack. Minute boundaries are the same in
/// UTC and in every real time zone, so the Unix clock is enough and avoids a time-zone lookup.
pub fn until_next_minute() -> Duration {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let into_minute = Duration::from_nanos((since.as_nanos() % 60_000_000_000) as u64);
    Duration::from_secs(60) - into_minute + SLACK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_text_is_populated() {
        let (d, t) = now_text();
        assert!(!d.is_empty() && !t.is_empty());
        assert!(t.chars().any(|c| c.is_numeric()), "time: {t}");
    }

    #[test]
    fn next_minute_in_range() {
        let d = until_next_minute();
        assert!(d >= Duration::from_millis(20) && d <= Duration::from_millis(60_100), "{d:?}");
    }
}
