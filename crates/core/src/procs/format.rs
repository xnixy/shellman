//! Numbers as Task Manager writes them: sizes, rates, shares and durations.

const KB: f64 = 1024.0;
const MB: f64 = KB * 1024.0;
const GB: f64 = MB * 1024.0;
const TB: f64 = GB * 1024.0;

/// A size: `0 B`, `12 KB`, `3.4 MB`, `1.2 GB`. One decimal below 100, none above.
pub fn bytes(n: u64) -> String {
    let n = n as f64;
    let (value, unit) = if n >= TB {
        (n / TB, "TB")
    } else if n >= GB {
        (n / GB, "GB")
    } else if n >= MB {
        (n / MB, "MB")
    } else if n >= KB {
        (n / KB, "KB")
    } else {
        return format!("{} B", n as u64);
    };
    if value >= 100.0 {
        format!("{value:.0} {unit}")
    } else {
        format!("{value:.1} {unit}")
    }
}

/// A process's memory as the column shows it: always in MB, one decimal (`0.4 MB`, `1,204.6 MB`).
pub fn memory_mb(n: u64) -> String {
    let mb = n as f64 / MB;
    let tenths = (mb * 10.0).round() as u64;
    format!("{}.{} MB", thousands(tenths / 10), tenths % 10)
}

/// `1234567` as `1,234,567`.
pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// A process's rate as the Disk column shows it: in MB/s with one decimal, `0 MB/s` when less
/// than a twentieth of one moves.
pub fn rate(bytes_per_sec: f64) -> String {
    if !bytes_per_sec.is_finite() || bytes_per_sec < 0.05 * MB {
        return "0 MB/s".into();
    }
    format!("{:.1} MB/s", bytes_per_sec / MB)
}

/// A machine's rate, in whatever unit fits: `0 B/s`, `640 KB/s`, `12.5 MB/s`.
pub fn rate_any(bytes_per_sec: f64) -> String {
    let b = if bytes_per_sec.is_finite() { bytes_per_sec.max(0.0) } else { 0.0 };
    format!("{}/s", bytes(b as u64))
}

/// A network rate in bits per second, as adapters are rated: `0 Kbps`, `840 Kbps`, `12.3 Mbps`.
pub fn bits(bytes_per_sec: f64) -> String {
    let b = if bytes_per_sec.is_finite() { bytes_per_sec.max(0.0) * 8.0 } else { 0.0 };
    if b >= 1e9 {
        format!("{:.1} Gbps", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} Mbps", b / 1e6)
    } else {
        format!("{:.0} Kbps", b / 1e3)
    }
}

/// A share: `0%`, `0.4%`, `12%`. Below 10 it keeps one decimal, as small values matter there.
pub fn percent(p: f32) -> String {
    let p = if p.is_finite() { p.clamp(0.0, 100.0) } else { 0.0 };
    // Below what one decimal shows, it is nothing.
    if p < 0.05 {
        "0%".into()
    } else if p < 10.0 {
        format!("{p:.1}%")
    } else {
        format!("{p:.0}%")
    }
}

/// An uptime: `0:05:03` under a day, `3:04:05:06` (days first) over one.
pub fn duration(secs: u64) -> String {
    let (d, h, m, s) = (secs / 86_400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if d > 0 {
        format!("{d}:{h:02}:{m:02}:{s:02}")
    } else {
        format!("{h}:{m:02}:{s:02}")
    }
}

/// A clock speed: `3.42 GHz`, `800 MHz`.
pub fn mhz(mhz: f32) -> String {
    if mhz >= 1000.0 {
        format!("{:.2} GHz", mhz / 1000.0)
    } else {
        format!("{mhz:.0} MHz")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1023), "1023 B");
        assert_eq!(bytes(1536), "1.5 KB");
        assert_eq!(bytes(250 * 1024 * 1024), "250 MB");
        assert_eq!(bytes(3 * 1024 * 1024 * 1024 / 2), "1.5 GB");
        assert_eq!(bytes(2 * 1024u64.pow(4)), "2.0 TB");
    }

    #[test]
    fn process_memory_is_in_mb() {
        assert_eq!(memory_mb(0), "0.0 MB");
        assert_eq!(memory_mb(400 * 1024), "0.4 MB");
        assert_eq!(memory_mb(1_263_000_000), "1,204.5 MB");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(999), "999");
    }

    #[test]
    fn rates() {
        assert_eq!(rate(0.0), "0 MB/s");
        assert_eq!(rate(f64::NAN), "0 MB/s");
        assert_eq!(rate(2048.0), "0 MB/s");
        assert_eq!(rate(3.0 * MB), "3.0 MB/s");
        assert_eq!(rate(0.12 * MB), "0.1 MB/s");
        assert_eq!(rate_any(0.0), "0 B/s");
        assert_eq!(rate_any(640.0 * KB), "640 KB/s");
        assert_eq!(rate_any(-5.0), "0 B/s");
        assert_eq!(bits(0.0), "0 Kbps");
        assert_eq!(bits(105_000.0), "840 Kbps");
        assert_eq!(bits(1_537_500.0), "12.3 Mbps");
        assert_eq!(bits(250_000_000.0), "2.0 Gbps");
    }

    #[test]
    fn percents() {
        assert_eq!(percent(0.0), "0%");
        assert_eq!(percent(0.42), "0.4%");
        assert_eq!(percent(12.4), "12%");
        assert_eq!(percent(140.0), "100%");
        assert_eq!(percent(f32::NAN), "0%");
    }

    #[test]
    fn durations_and_speeds() {
        assert_eq!(duration(303), "0:05:03");
        assert_eq!(duration(3 * 86_400 + 4 * 3600 + 5 * 60 + 6), "3:04:05:06");
        assert_eq!(mhz(3420.0), "3.42 GHz");
        assert_eq!(mhz(800.0), "800 MHz");
    }
}
