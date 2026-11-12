//! How sizes and dates read in the list. Pure.

/// "820 KB", "12.4 MB", "130 MB", "1.5 GB" from a size in kilobytes.
pub fn size(kb: u64) -> String {
    let one = |v: f64, unit: &str| if v < 10.0 { format!("{v:.1} {unit}") } else { format!("{v:.0} {unit}") };
    if kb < 1024 {
        format!("{kb} KB")
    } else if kb < 1024 * 1024 {
        one(kb as f64 / 1024.0, "MB")
    } else {
        one(kb as f64 / (1024.0 * 1024.0), "GB")
    }
}

/// A date held as `yyyymmdd`, shown as "2024-03-15".
pub fn date(d: u32) -> String {
    format!("{:04}-{:02}-{:02}", d / 10000, d / 100 % 100, d % 100)
}

/// The `yyyymmdd` of an InstallDate value: eight digits, with a real month and day.
pub fn parse_install_date(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.len() != 8 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let d: u32 = s.parse().ok()?;
    valid(d).then_some(d)
}

pub fn valid(d: u32) -> bool {
    (1601..=9999).contains(&(d / 10000)) && (1..=12).contains(&(d / 100 % 100)) && (1..=31).contains(&(d % 100))
}

/// The `yyyymmdd` of a Windows time: 100 ns ticks since 1601-01-01 (UTC).
pub fn date_from_ticks(ticks: i64) -> Option<u32> {
    if ticks <= 0 {
        return None;
    }
    // Days since 1970-01-01 shifted to the calendar's March-based era, then the civil-from-days
    // arithmetic.
    let z = ticks / 864_000_000_000 - 134_774 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let d = (year * 10000 + month * 100 + day) as u32;
    valid(d).then_some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 864_000_000_000;
    /// Days from 1601-01-01 to 1970-01-01.
    const EPOCH: i64 = 134_774;

    #[test]
    fn sizes() {
        assert_eq!(size(820), "820 KB");
        assert_eq!(size(5_000), "4.9 MB");
        assert_eq!(size(12_700), "12 MB");
        assert_eq!(size(133_000), "130 MB");
        assert_eq!(size(1_572_864), "1.5 GB");
    }

    #[test]
    fn dates() {
        assert_eq!(date(20240305), "2024-03-05");
        assert_eq!(parse_install_date("20240315"), Some(20240315));
        for bad in ["", "2024-03-15", "20241315", "20240300", "2024031"] {
            assert_eq!(parse_install_date(bad), None, "{bad}");
        }
    }

    #[test]
    fn windows_time_to_a_date() {
        // 2024-03-15 is day 19797 since 1970; 2000-02-29 (a leap day) is day 11016.
        let day = (19_797 + EPOCH) * DAY;
        assert_eq!(date_from_ticks(day), Some(20240315));
        assert_eq!(date_from_ticks(day + DAY - 1), Some(20240315));
        assert_eq!(date_from_ticks(day + DAY), Some(20240316));
        assert_eq!(date_from_ticks((11_016 + EPOCH) * DAY), Some(20000229));
        assert_eq!(date_from_ticks(0), None);
    }
}
