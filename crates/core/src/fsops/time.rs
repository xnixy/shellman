//! Local-time conversion and formatting for file timestamps.

use std::collections::HashMap;
use std::time::SystemTime;
use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::Time::{
    FileTimeToSystemTime, SystemTimeToFileTime, SystemTimeToTzSpecificLocalTime,
};

/// Seconds between 1601 (FILETIME's epoch) and 1970.
const FILETIME_EPOCH_SECS: i64 = 11_644_473_600;

/// UTC-to-local offsets, looked up once per distinct hour.
///
/// Each date gets the daylight-saving rule that applied on it, exactly as Explorer shows it, yet
/// a folder of 100 000 files costs one Win32 call per distinct hour they were written in, not two
/// per file.
#[derive(Default)]
pub(crate) struct LocalClock {
    by_hour: HashMap<i64, i64>,
}

impl LocalClock {
    pub(crate) fn offset_for(&mut self, t: SystemTime) -> i64 {
        let Ok(d) = t.duration_since(SystemTime::UNIX_EPOCH) else {
            return 0;
        };
        let hour = d.as_secs() as i64 / 3600;
        *self
            .by_hour
            .entry(hour)
            .or_insert_with(|| tz_offset_at(hour * 3600))
    }
}

/// Seconds to add to a UTC time (seconds since 1970) to get local time at that moment.
fn tz_offset_at(utc_secs: i64) -> i64 {
    let ticks = ((utc_secs + FILETIME_EPOCH_SECS) * 10_000_000) as u64;
    let ft = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let (mut utc, mut local) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    let mut local_ft = FILETIME::default();
    let ok = unsafe {
        FileTimeToSystemTime(&ft, &mut utc).is_ok()
            && SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_ok()
            && SystemTimeToFileTime(&local, &mut local_ft).is_ok()
    };
    if !ok {
        return 0;
    }
    let local_ticks = (local_ft.dwHighDateTime as u64) << 32 | local_ft.dwLowDateTime as u64;
    (local_ticks / 10_000_000) as i64 - FILETIME_EPOCH_SECS - utc_secs
}

/// Local date and time like "2026-11-11 14:05", from pure arithmetic so a 100k-file listing makes
/// no per-entry system calls.
pub(crate) fn format_time_at(t: SystemTime, offset_secs: i64) -> String {
    let Ok(d) = t.duration_since(SystemTime::UNIX_EPOCH) else {
        return String::new();
    };
    let secs = d.as_secs() as i64 + offset_secs;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil-from-days (Howard Hinnant), valid for the whole proleptic Gregorian calendar.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + (month <= 2) as i64;
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_offsets_are_whole_quarter_hours() {
        let mut clock = LocalClock::default();
        let now = SystemTime::now();
        let winter = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_704_067_200);
        let summer = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_719_792_000);
        for t in [now, winter, summer] {
            let off = clock.offset_for(t);
            assert_eq!(off % 900, 0, "offset {off}");
            assert!(off.abs() <= 14 * 3600, "offset {off}");
        }
        // The same hour is answered from the cache.
        assert_eq!(clock.offset_for(now), clock.offset_for(now));
        assert!(clock.by_hour.len() <= 3);
    }

    #[test]
    fn time_format() {
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        assert_eq!(format_time_at(t, 0), "2023-11-14 22:13");
        assert_eq!(format_time_at(t, 3600), "2023-11-14 23:13");
        let leap = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_709_164_800);
        assert_eq!(format_time_at(leap, 0), "2024-02-29 00:00");
    }
}
