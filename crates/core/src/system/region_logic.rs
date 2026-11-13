//! The rules of the Date, time & region page, with no Win32: turning a time pattern between 12
//! and 24 hours, the order of the time zone list, and the names of the week's first day. Tested.

/// Whether a time pattern shows the hour on a 24 hour clock: an upper-case `H` and no am/pm
/// marker.
pub fn is_24h(pattern: &str) -> bool {
    let p = unquoted(pattern);
    p.contains('H') && !p.contains('t')
}

/// The pattern without its quoted text, which may hold any letter.
fn unquoted(pattern: &str) -> String {
    let mut quoted = false;
    pattern
        .chars()
        .filter(|c| {
            if *c == '\'' {
                quoted = !quoted;
                return false;
            }
            !quoted
        })
        .collect()
}

/// `h:mm:ss tt` to `HH:mm:ss`: 24 hour, the marker gone. Text in quotes is left alone. A pattern
/// already on 24 hours comes back as it was.
pub fn to_24h(pattern: &str) -> String {
    if is_24h(pattern) {
        return pattern.to_owned();
    }
    let mut out = String::new();
    let mut quoted = false;
    for c in pattern.chars() {
        match c {
            '\'' => {
                quoted = !quoted;
                out.push(c);
            }
            'h' if !quoted => out.push('H'),
            't' if !quoted => {}
            c => out.push(c),
        }
    }
    // The hour of a 24 hour clock is two digits: `H:mm` becomes `HH:mm` as Windows writes it.
    let out = out.trim().to_owned();
    let hours = out.chars().filter(|c| *c == 'H').count();
    if hours == 1 {
        out.replacen('H', "HH", 1)
    } else {
        out
    }
}

/// `HH:mm:ss` to `h:mm:ss tt`: 12 hour with the marker after. A pattern already on 12 hours
/// comes back as it was.
pub fn to_12h(pattern: &str) -> String {
    let p = unquoted(pattern);
    if !p.contains('H') && p.contains('t') {
        return pattern.to_owned();
    }
    let mut out = String::new();
    let mut quoted = false;
    let mut last_h = false;
    for c in pattern.chars() {
        match c {
            '\'' => {
                quoted = !quoted;
                out.push(c);
                last_h = false;
            }
            'H' if !quoted => {
                // HH and H both become a single h: Windows' own 12 hour patterns use `h`.
                if !last_h {
                    out.push('h');
                }
                last_h = true;
            }
            c => {
                out.push(c);
                last_h = false;
            }
        }
    }
    if !unquoted(&out).contains('t') {
        out.push_str(" tt");
    }
    out
}

/// One time zone of the list.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Zone {
    /// The registry key name, `Pacific Standard Time`: what is set.
    pub key: String,
    /// `(UTC-08:00) Pacific Time (US & Canada)`.
    pub display: String,
    /// Minutes west of UTC, as Windows' bias: -60 is UTC+1.
    pub bias: i32,
    pub observes_dst: bool,
}

/// `(UTC+05:30)` from a bias in minutes west of UTC.
pub fn utc_label(bias: i32) -> String {
    let east = -bias;
    let sign = if east < 0 { '-' } else { '+' };
    format!("(UTC{sign}{:02}:{:02})", east.abs() / 60, east.abs() % 60)
}

/// The list as Windows' own picker orders it: by offset, west to east, then by name.
pub fn sort_zones(zones: &mut [Zone]) {
    zones.sort_by(|a, b| b.bias.cmp(&a.bias).then_with(|| a.display.cmp(&b.display)));
}

/// The first day of the week: Windows' `iFirstDayOfWeek` counts from Monday.
pub const DAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

pub fn day_name(n: u32) -> &'static str {
    DAYS[(n as usize).min(6)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twelve_to_twenty_four() {
        assert_eq!(to_24h("h:mm:ss tt"), "HH:mm:ss");
        assert_eq!(to_24h("h:mm tt"), "HH:mm");
        assert_eq!(to_24h("hh:mm tt"), "HH:mm");
        assert_eq!(to_24h("tt h:mm"), "HH:mm");
        assert_eq!(to_24h("HH:mm"), "HH:mm");
    }

    #[test]
    fn twenty_four_to_twelve() {
        assert_eq!(to_12h("HH:mm:ss"), "h:mm:ss tt");
        assert_eq!(to_12h("H:mm"), "h:mm tt");
        assert_eq!(to_12h("h:mm tt"), "h:mm tt");
    }

    #[test]
    fn quoted_text_is_kept() {
        assert_eq!(to_12h("HH 'at'"), "h 'at' tt");
        assert!(is_24h("HH 'at'"));
        assert_eq!(to_24h("h 'hours' mm tt"), "HH 'hours' mm");
        assert_eq!(to_12h("HH 'hrs'"), "h 'hrs' tt");
    }

    #[test]
    fn the_clock_kind_is_read_from_the_pattern() {
        assert!(is_24h("HH:mm"));
        assert!(!is_24h("h:mm tt"));
        assert!(!is_24h("hh:mm"));
    }

    #[test]
    fn the_round_trip_is_stable() {
        for p in ["h:mm:ss tt", "h:mm tt"] {
            assert_eq!(to_12h(&to_24h(p)), p);
        }
    }

    #[test]
    fn zones_go_west_to_east() {
        let z = |key: &str, display: &str, bias| Zone { key: key.into(), display: display.into(), bias, observes_dst: false };
        let mut v = vec![z("c", "(UTC+01:00) Berlin", -60), z("a", "(UTC-08:00) Pacific", 480), z("b", "(UTC-05:00) Eastern", 300), z("d", "(UTC+01:00) Amsterdam", -60)];
        sort_zones(&mut v);
        let order: Vec<&str> = v.iter().map(|z| z.key.as_str()).collect();
        assert_eq!(order, ["a", "b", "d", "c"]);
    }

    #[test]
    fn offset_labels() {
        assert_eq!(utc_label(480), "(UTC-08:00)");
        assert_eq!(utc_label(-330), "(UTC+05:30)");
        assert_eq!(utc_label(0), "(UTC+00:00)");
    }

    #[test]
    fn day_names_start_on_monday() {
        assert_eq!(day_name(0), "Monday");
        assert_eq!(day_name(6), "Sunday");
        assert_eq!(day_name(99), "Sunday");
    }
}
