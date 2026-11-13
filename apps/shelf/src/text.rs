//! The words the shelf shows: how long ago, how big, how many. Pure: times come in as seconds
//! since 1970 in local time, so the day boundaries are the user's.

/// Seconds in a day.
const DAY: i64 = 86_400;

/// How long ago `then` was, as of `now` (both local): "just now", "12 min ago", "2 h ago",
/// "this morning", "yesterday", "Monday", "3 Nov", "3 Nov 2025".
pub fn age(then: i64, now: i64) -> String {
    let ago = now - then;
    if ago < 60 {
        return "just now".into();
    }
    if ago < 3600 {
        return format!("{} min ago", ago / 60);
    }
    let (day, today) = (then.div_euclid(DAY), now.div_euclid(DAY));
    if ago < 3 * 3600 || (day == today && ago < 6 * 3600) {
        return format!("{} h ago", ago / 3600);
    }
    if day == today {
        let hour = then.rem_euclid(DAY) / 3600;
        return match hour {
            0..=11 => "this morning",
            12..=17 => "this afternoon",
            _ => "this evening",
        }
        .into();
    }
    if day + 1 == today {
        return "yesterday".into();
    }
    if day < today && today - day < 7 {
        return weekday(day).into();
    }
    date(day, today)
}

/// Which day `then` was, for a stack's line: "today", "yesterday", or as `age` has it.
pub fn day(then: i64, now: i64) -> String {
    let (day, today) = (then.div_euclid(DAY), now.div_euclid(DAY));
    if day >= today {
        "today".into()
    } else if day + 1 == today {
        "yesterday".into()
    } else if today - day < 7 {
        weekday(day).into()
    } else {
        date(day, today)
    }
}

/// The weekday of a day counted from 1970-01-01, which was a Thursday.
fn weekday(day: i64) -> &'static str {
    ["Thursday", "Friday", "Saturday", "Sunday", "Monday", "Tuesday", "Wednesday"][day.rem_euclid(7) as usize]
}

/// "3 Nov", with the year when it is not this year's.
fn date(day: i64, today: i64) -> String {
    let (y, m, d) = civil(day);
    let month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][(m - 1) as usize];
    if civil(today).0 == y {
        format!("{d} {month}")
    } else {
        format!("{d} {month} {y}")
    }
}

/// Year, month and day of a day counted from 1970-01-01 (Howard Hinnant's civil-from-days).
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// "12 items", "1 item", "Empty".
pub fn count(n: usize) -> String {
    match n {
        0 => "Empty".into(),
        1 => "1 item".into(),
        n => format!("{n} items"),
    }
}

/// A fan item's second line: "3.1 MB · 2 min ago", or "Folder · yesterday".
pub fn item_line(size: Option<u64>, then: i64, now: i64) -> String {
    let what = size.map_or_else(|| "Folder".to_owned(), shellman_core::fsops::format_size);
    format!("{what} · {}", age(then, now))
}

/// The last pill's words: "Open Downloads in Files", and "14 more" when the fan left some out.
pub fn open_in_files(folder: &str, more: usize) -> (String, Option<String>) {
    (format!("Open {folder} in Files"), (more > 0).then(|| format!("{more} more")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Thursday 12 November 2026, 12:41 local, as seconds since 1970.
    const NOW: i64 = 1_794_487_260;

    #[test]
    fn the_clock_is_where_the_tests_think() {
        assert_eq!(civil(NOW.div_euclid(DAY)), (2026, 11, 12));
        assert_eq!(weekday(NOW.div_euclid(DAY)), "Thursday");
        assert_eq!(NOW.rem_euclid(DAY), 12 * 3600 + 41 * 60);
    }

    #[test]
    fn recent_times_count_minutes_then_hours() {
        assert_eq!(age(NOW - 5, NOW), "just now");
        assert_eq!(age(NOW - 2 * 60, NOW), "2 min ago");
        assert_eq!(age(NOW - 38 * 60, NOW), "38 min ago");
        assert_eq!(age(NOW - 3600, NOW), "1 h ago");
        assert_eq!(age(NOW - 2 * 3600 - 59, NOW), "2 h ago");
        // A clock that is a little ahead of the file's.
        assert_eq!(age(NOW + 30, NOW), "just now");
    }

    #[test]
    fn earlier_today_then_days() {
        // 07:10 today, five and a half hours ago.
        assert_eq!(age(NOW - 5 * 3600 - 31 * 60, NOW), "5 h ago");
        // 05:00 today.
        let five = NOW.div_euclid(DAY) * DAY + 5 * 3600;
        assert_eq!(age(five, NOW), "this morning");
        assert_eq!(age(NOW - DAY, NOW), "yesterday");
        assert_eq!(age(NOW - 3 * DAY, NOW), "Monday");
        assert_eq!(age(NOW - 9 * DAY, NOW), "3 Nov");
        assert_eq!(age(NOW - 400 * DAY, NOW), "8 Oct 2025");
    }

    #[test]
    fn the_evening_before_midnight_is_yesterday() {
        let late = NOW.div_euclid(DAY) * DAY - 60;
        let early = NOW.div_euclid(DAY) * DAY + 2 * 3600 + 60;
        // Two hours apart across midnight: hours win.
        assert_eq!(age(late, early), "2 h ago");
        assert_eq!(age(late, NOW), "yesterday");
        assert_eq!(day(late, NOW), "yesterday");
    }

    #[test]
    fn days_for_a_stack() {
        assert_eq!(day(NOW - 60, NOW), "today");
        assert_eq!(day(NOW - DAY, NOW), "yesterday");
        assert_eq!(day(NOW - 2 * DAY, NOW), "Tuesday");
        assert_eq!(day(NOW - 30 * DAY, NOW), "13 Oct");
    }

    #[test]
    fn counts_and_lines() {
        assert_eq!(count(0), "Empty");
        assert_eq!(count(1), "1 item");
        assert_eq!(count(20), "20 items");
        assert_eq!(item_line(Some(3_250_000), NOW - 120, NOW), "3.1 MB · 2 min ago");
        assert_eq!(item_line(None, NOW - DAY, NOW), "Folder · yesterday");
        assert_eq!(open_in_files("Downloads", 14), ("Open Downloads in Files".into(), Some("14 more".into())));
        assert_eq!(open_in_files("Recent", 0).1, None);
    }
}
