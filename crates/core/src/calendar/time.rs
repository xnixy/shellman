//! Calendar arithmetic without a time zone: days counted from 1970-01-01, and "wall" times
//! (seconds since 1970-01-01 00:00 on a clock of no particular zone). Pure.

/// Seconds in a day.
pub const DAY: i64 = 86_400;

/// Days since 1970-01-01 of a calendar date (Howard Hinnant's days-from-civil).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Year, month and day of a day counted from 1970-01-01.
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// The weekday of a day: 0 Monday to 6 Sunday (1970-01-01 was a Thursday).
pub fn weekday(days: i64) -> u32 {
    (days + 3).rem_euclid(7) as u32
}

pub fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

pub fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if is_leap(y) => 29,
        _ => 28,
    }
}

/// A wall time from its parts.
pub fn wall(y: i64, m: u32, d: u32, hh: u32, mm: u32, ss: u32) -> i64 {
    days_from_civil(y, m, d) * DAY + (hh * 3600 + mm * 60 + ss) as i64
}

/// The day a wall time falls on.
pub fn day_of(wall: i64) -> i64 {
    wall.div_euclid(DAY)
}

/// Seconds into its day.
pub fn time_of(wall: i64) -> i64 {
    wall.rem_euclid(DAY)
}

/// `20261112T140000` (no zone mark), as iCalendar and CalDAV write a UTC time without its `Z`.
pub fn stamp(wall: i64) -> String {
    let (y, m, d) = civil_from_days(day_of(wall));
    let t = time_of(wall);
    format!("{y:04}{m:02}{d:02}T{:02}{:02}{:02}", t / 3600, t / 60 % 60, t % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_round_trips() {
        for days in [-800_000, -1, 0, 1, 59, 60, 10_957, 20_769, 2_932_896] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days, "{y}-{m}-{d}");
        }
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(days_from_civil(2026, 11, 12), 20_769);
        assert_eq!(weekday(20_769), 3, "12 Nov 2026 is a Thursday");
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2100, 2), 28);
        assert_eq!(stamp(wall(2026, 3, 29, 1, 5, 9)), "20260329T010509");
    }
}
