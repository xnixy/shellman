//! Words for items, worked out without a window: a long name cut in the middle so its extension
//! stays, a date as people say it, and the detail a card shows under its name.

use super::tab::SortKey;
use shellman_core::fsops::{self, Entry};

/// `name` cut in the middle with an ellipsis until `fits` accepts it, keeping its extension and
/// the end of its stem ("A very long fi…final.pdf"), so files that differ only at the end still
/// tell apart. Names that fit come back whole.
pub(crate) fn middle_ellipsis(name: &str, fits: impl Fn(&str) -> bool) -> String {
    if fits(name) {
        return name.to_owned();
    }
    let chars: Vec<char> = name.chars().collect();
    // The extension, if it is a short one after a real stem.
    let dot = name
        .rfind('.')
        .filter(|&i| i > 0 && name.len() - i <= 8)
        .map(|i| name[..i].chars().count());
    let stem_len = dot.unwrap_or(chars.len());
    let tail_keep = (stem_len / 4).min(6);
    let tail: String = chars[stem_len - tail_keep..].iter().collect();
    let make = |head: usize| -> String {
        let mut s: String = chars[..head].iter().collect();
        s.push('…');
        s.push_str(&tail);
        s
    };
    // The longest head that fits; with none, just the ellipsis and the tail.
    let (mut lo, mut hi) = (0, stem_len - tail_keep);
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if fits(&make(mid)) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    make(lo)
}

/// A listing's "YYYY-MM-DD HH:MM" as people say it: "Today, 12:24", "Yesterday, 08:01", or the
/// date alone. `today` and `yesterday` are "YYYY-MM-DD".
pub(crate) fn friendly_date(text: &str, today: &str, yesterday: &str) -> String {
    let (date, time) = text.split_once(' ').unwrap_or((text, ""));
    if date == today {
        format!("Today, {time}")
    } else if date == yesterday {
        format!("Yesterday, {time}")
    } else {
        date.to_owned()
    }
}

/// Today's and yesterday's local dates as "YYYY-MM-DD".
pub(crate) fn today() -> (String, String) {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let t = unsafe { GetLocalTime() };
    let today = format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay);
    // Yesterday from the civil date, by days since an epoch.
    let days = days_from_civil(i64::from(t.wYear), i64::from(t.wMonth), i64::from(t.wDay)) - 1;
    let (y, m, d) = civil_from_days(days);
    (today, format!("{y:04}-{m:02}-{d:02}"))
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// The muted line under a card's name, following the sort: the date when sorted by date, the
/// type by type, else the size (a folder's item count, when known, or just "Folder").
pub(crate) fn card_detail(e: &Entry, sort: SortKey, items: Option<usize>, days: &(String, String)) -> String {
    match sort {
        SortKey::Modified => friendly_date(&e.modified_text, &days.0, &days.1),
        SortKey::Kind => e.kind.clone(),
        SortKey::Name | SortKey::Size if e.is_dir => match items {
            Some(n) => count(n, "item"),
            None => "Folder".into(),
        },
        SortKey::Name | SortKey::Size => fsops::format_size(e.size),
    }
}

/// What a screen reader says for an item: its name, what it is, when it changed and its size
/// (a folder's item count when known).
pub(crate) fn spoken_item(e: &Entry, date: &str, items: Option<usize>) -> String {
    let mut parts = vec![e.name.clone()];
    parts.push(if e.is_dir { "Folder".to_owned() } else { e.kind.clone() });
    if !date.is_empty() {
        parts.push(format!("modified {date}"));
    }
    if e.is_dir {
        if let Some(n) = items {
            parts.push(count(n, "item"));
        }
    } else {
        parts.push(fsops::format_size(e.size));
    }
    parts.retain(|p| !p.is_empty());
    parts.join(", ")
}

/// "1 item", "3 items".
pub(crate) fn count(n: usize, what: &str) -> String {
    format!("{n} {what}{}", if n == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fits when it has at most `n` characters.
    fn under(n: usize) -> impl Fn(&str) -> bool {
        move |s: &str| s.chars().count() <= n
    }

    #[test]
    fn a_long_name_keeps_its_extension_and_its_end() {
        let name = "A very long file name that keeps going final.pdf";
        let cut = middle_ellipsis(name, under(24));
        assert!(cut.chars().count() <= 24, "{cut}");
        assert!(cut.ends_with("final.pdf"), "{cut}");
        assert!(cut.starts_with("A very"), "{cut}");
        assert!(cut.contains('…'));
        // One that fits is left alone.
        assert_eq!(middle_ellipsis("short.txt", under(24)), "short.txt");
    }

    #[test]
    fn names_without_an_extension_or_with_a_long_one_cut_too() {
        let cut = middle_ellipsis("README-for-the-whole-project", under(12));
        assert!(cut.chars().count() <= 12 && cut.contains('…'), "{cut}");
        // A dotfile's "extension" is its name: the dot at 0 is not one.
        let cut = middle_ellipsis(".gitignore-backup-copy", under(10));
        assert!(cut.chars().count() <= 10, "{cut}");
        // Room for almost nothing: still the ellipsis and the end.
        let cut = middle_ellipsis("abcdefghijklmnop.txt", under(3));
        assert!(cut.starts_with('…'), "{cut}");
    }

    #[test]
    fn dates_read_as_people_say_them() {
        assert_eq!(friendly_date("2026-11-12 12:24", "2026-11-12", "2026-11-11"), "Today, 12:24");
        assert_eq!(
            friendly_date("2026-11-11 08:01", "2026-11-12", "2026-11-11"),
            "Yesterday, 08:01"
        );
        assert_eq!(friendly_date("2025-01-02 10:00", "2026-11-12", "2026-11-11"), "2025-01-02");
        assert_eq!(friendly_date("", "2026-11-12", "2026-11-11"), "");
    }

    #[test]
    fn yesterday_crosses_months_and_years() {
        for (y, m, d) in [(2026, 3, 1), (2024, 3, 1), (2026, 1, 1)] {
            let (py, pm, pd) = civil_from_days(days_from_civil(y, m, d) - 1);
            let back = days_from_civil(py, pm, pd);
            assert_eq!(back + 1, days_from_civil(y, m, d));
        }
        assert_eq!(civil_from_days(days_from_civil(2024, 3, 1) - 1), (2024, 2, 29));
        assert_eq!(civil_from_days(days_from_civil(2026, 1, 1) - 1), (2025, 12, 31));
    }

    #[test]
    fn counts_have_their_plural() {
        assert_eq!(count(1, "item"), "1 item");
        assert_eq!(count(0, "item"), "0 items");
    }
}
