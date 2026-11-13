//! Locale-aware clock text and the schedule for the bar's single periodic repaint.

use crate::settings::bar::ClockSettings;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Globalization::{GetDateFormatEx, GetTimeFormatEx, TIME_NOSECONDS};

/// Extra delay past the boundary so the repaint lands in the new minute, not the old one.
const SLACK: Duration = Duration::from_millis(20);

/// The text the bar shows, e.g. "Wed 11 Nov   14:32": the date only when the settings ask for it,
/// and the time as 12 or 24 hour if they say so.
pub fn text(settings: &ClockSettings) -> String {
    let (date, time) = parts(settings);
    if settings.show_date {
        format!("{date}   {time}")
    } else {
        time
    }
}

/// (date, time) as the settings ask: the weekday or not, seconds or not.
pub fn parts(settings: &ClockSettings) -> (String, String) {
    let date = if settings.show_weekday { "ddd d MMM" } else { "d MMM" };
    let time = if settings.show_seconds { settings.hour_cycle.pattern_with_seconds() } else { settings.hour_cycle.pattern() };
    format_now(date, time, settings.show_seconds)
}

/// (date, time) for the current local moment in the user's locale, e.g. ("Wed 11 Nov", "14:32").
/// `time_pattern` overrides the locale's own time format. Passing no SYSTEMTIME makes Windows use
/// "now", saving a separate GetLocalTime call.
pub fn now_text(time_pattern: Option<&str>) -> (String, String) {
    format_now("ddd d MMM", time_pattern, false)
}

fn format_now(date_pattern: &str, time_pattern: Option<&str>, seconds: bool) -> (String, String) {
    let mut buf = [0u16; 64];
    let date_pattern = HSTRING::from(date_pattern);
    // SAFETY: buffers are valid for each call; a null locale name means the user default.
    unsafe {
        let n = GetDateFormatEx(PCWSTR::null(), Default::default(), None, &date_pattern, Some(&mut buf), PCWSTR::null());
        let date = to_string(&buf, n);
        let pattern = time_pattern.map(HSTRING::from);
        let pattern_ptr = pattern.as_ref().map_or(PCWSTR::null(), |p| PCWSTR(p.as_ptr()));
        // The no-seconds flag may not be combined with an explicit pattern, which has none anyway.
        let flags = if pattern.is_some() || seconds { Default::default() } else { TIME_NOSECONDS };
        let n = GetTimeFormatEx(PCWSTR::null(), flags, None, pattern_ptr, Some(&mut buf));
        let time = to_string(&buf, n);
        (date, time)
    }
}

/// Time until the next second, plus slack, for a clock that shows seconds.
pub fn until_next_second() -> Duration {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    Duration::from_nanos(1_000_000_000 - (since.as_nanos() % 1_000_000_000) as u64) + SLACK
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
        let (d, t) = now_text(None);
        assert!(!d.is_empty() && !t.is_empty());
        assert!(t.chars().any(|c| c.is_numeric()), "time: {t}");
    }

    #[test]
    fn explicit_patterns_and_hidden_date() {
        let (_, h24) = now_text(Some("HH:mm"));
        assert_eq!(h24.len(), 5, "{h24}");
        assert_eq!(h24.as_bytes()[2], b':');
        let (_, h12) = now_text(Some("h:mm tt"));
        assert!(h12.contains(':') && h12.len() >= 7, "{h12}");

        let mut s = ClockSettings::default();
        assert!(text(&s).contains("   "));
        s.show_date = false;
        assert!(!text(&s).contains("   "));
        // Seconds: one more colon; the weekday left out: a shorter date.
        let s = ClockSettings { hour_cycle: crate::settings::bar::HourCycle::H24, show_seconds: true, ..Default::default() };
        assert_eq!(parts(&s).1.len(), 8, "{:?}", parts(&s));
        let with = parts(&ClockSettings::default()).0;
        let without = parts(&ClockSettings { show_weekday: false, ..Default::default() }).0;
        assert!(without.len() < with.len(), "{with:?} {without:?}");
        assert!(until_next_second() <= Duration::from_millis(1_020));
    }

    #[test]
    fn next_minute_in_range() {
        let d = until_next_minute();
        assert!(d >= Duration::from_millis(20) && d <= Duration::from_millis(60_100), "{d:?}");
    }
}
