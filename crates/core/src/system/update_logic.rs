//! Windows Update, the part that decides: what kind an update is, its KB number, what the page's
//! headline says, the pause dates, and what a history entry's result means. Nothing here talks
//! to Windows, so all of it is tested; `updates` reads Windows and hands the facts in.

/// What kind of update it is, as the page's chip says.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Kind {
    Security,
    Quality,
    Driver,
    Feature,
    Definition,
    Other,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Security => "Security",
            Kind::Quality => "Quality",
            Kind::Driver => "Driver",
            Kind::Feature => "Feature",
            Kind::Definition => "Definition",
            Kind::Other => "Other",
        }
    }
}

/// The flags the first contract of the update API gives each update.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Flags {
    pub driver: bool,
    pub feature: bool,
    pub security: bool,
}

/// `WindowsSoftwareUpdateCategory` (second contract): Other, Application, Driver, Firmware,
/// Definition, Feature, Quality, AIComponent.
pub mod category {
    pub const DRIVER: i32 = 2;
    pub const FIRMWARE: i32 = 3;
    pub const DEFINITION: i32 = 4;
    pub const FEATURE: i32 = 5;
    pub const QUALITY: i32 = 6;
}

/// `WindowsUpdateAttentionRequiredReason` values the page acts on.
pub mod reason {
    /// An optional update: Windows waits for someone to ask for it.
    pub const SEEKER: i32 = 1;
    pub const READY_TO_REBOOT: i32 = 2;
}

/// The KB number in an update's title ("2026-11 Security Update (KB5132061) (26300.9974)" gives
/// "KB5132061"), when it has one. The first contract has no field for it.
pub fn kb(title: &str) -> Option<String> {
    let b = title.as_bytes();
    let mut i = 0;
    while i + 2 < b.len() {
        let boundary = i == 0 || !b[i - 1].is_ascii_alphanumeric();
        if boundary && b[i].eq_ignore_ascii_case(&b'K') && b[i + 1].eq_ignore_ascii_case(&b'B') {
            let digits = b[i + 2..].iter().take_while(|c| c.is_ascii_digit()).count();
            let end = i + 2 + digits;
            let clean_end = end == b.len() || !b[end].is_ascii_alphanumeric();
            if (6..=8).contains(&digits) && clean_end {
                return Some(format!("KB{}", &title[i + 2..end]));
            }
        }
        i += 1;
    }
    None
}

/// Whether a title reads as an antivirus definition or security platform update.
fn definition_words(lower: &str) -> bool {
    lower.contains("security intelligence") || lower.contains("defender") || lower.contains("windows security platform")
}

/// A title of the form Windows gives drivers: "Vendor - Class - [date -] version".
fn driver_title(title: &str) -> bool {
    let parts: Vec<&str> = title.split(" - ").collect();
    parts.len() >= 3 && parts.last().is_some_and(|v| v.trim().starts_with(|c: char| c.is_ascii_digit()))
}

/// Whether a title reads as a monthly or cumulative Windows (or .NET) update.
fn quality_words(title: &str, lower: &str) -> bool {
    let dated = title.len() > 8 && title.as_bytes()[..4].iter().all(u8::is_ascii_digit) && title.as_bytes()[4] == b'-' && title.as_bytes()[5..7].iter().all(u8::is_ascii_digit) && title.as_bytes()[7] == b' ';
    dated || ((lower.contains("security update") || lower.contains("cumulative update") || lower.contains(".net") || lower.contains("update for windows")) && kb(title).is_some())
}

/// The kind of a pending update: the second contract's category when Windows gives one, else the
/// first contract's flags, else its title.
pub fn kind_of(category: Option<i32>, flags: Flags, title: &str) -> Kind {
    let lower = title.to_lowercase();
    match category {
        Some(category::DRIVER | category::FIRMWARE) => return Kind::Driver,
        Some(category::DEFINITION) => return Kind::Definition,
        Some(category::FEATURE) => return Kind::Feature,
        Some(category::QUALITY) => {
            return if flags.security || lower.contains("security update") { Kind::Security } else { Kind::Quality };
        }
        _ => {}
    }
    if flags.driver {
        Kind::Driver
    } else if flags.feature {
        Kind::Feature
    } else if definition_words(&lower) {
        Kind::Definition
    } else if flags.security || lower.contains("security update") {
        Kind::Security
    } else if quality_words(title, &lower) {
        Kind::Quality
    } else if driver_title(title) {
        Kind::Driver
    } else {
        Kind::Other
    }
}

/// What Windows is doing with an update (its `CurrentAction`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Action {
    Download,
    Install,
    /// Installed; the restart finishes it ("Commit").
    Commit,
    Other(String),
}

impl Action {
    pub fn parse(s: &str) -> Action {
        match s {
            "Download" => Action::Download,
            "Install" => Action::Install,
            "Commit" => Action::Commit,
            other => Action::Other(other.to_owned()),
        }
    }
}

/// One update's facts, as the page needs them for its line of status.
#[derive(Clone, PartialEq, Debug)]
pub struct Facts {
    pub action: Action,
    pub reason: i32,
    pub seeker: bool,
    /// 0 to 1, from the progress events, while Windows works on it.
    pub progress: Option<f32>,
    /// Why the last try failed, if it did.
    pub failed: Option<String>,
}

impl Facts {
    /// Installed and waiting for the restart. Only the orchestrator knows; WUA's own flag and the
    /// usual registry keys were found to say no while five drivers waited.
    pub fn restart_needed(&self) -> bool {
        self.reason == reason::READY_TO_REBOOT || self.action == Action::Commit
    }

    /// Optional: Windows offers it but installs it only when asked.
    pub fn optional(&self) -> bool {
        self.seeker && self.reason == reason::SEEKER && !self.restart_needed()
    }

    /// The line under the update's title.
    pub fn status(&self) -> String {
        let pct = |p: f32| (p.clamp(0.0, 1.0) * 100.0).round() as u32;
        if self.restart_needed() {
            return "Waiting for a restart".to_owned();
        }
        if self.optional() {
            return "Not downloaded".to_owned();
        }
        match (&self.action, self.progress) {
            (Action::Download, Some(p)) => return format!("Downloading {}%", pct(p)),
            (Action::Install, Some(p)) => return format!("Installing {}%", pct(p)),
            _ => {}
        }
        if let Some(why) = &self.failed {
            return format!("Last try failed: {why}");
        }
        match self.action {
            Action::Download => "Waiting to download".to_owned(),
            Action::Install => "Waiting to install".to_owned(),
            _ => "Pending".to_owned(),
        }
    }
}

/// What the page knows, for the headline.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Overview {
    /// Windows' update service answered.
    pub available: bool,
    pub scanning: bool,
    /// When a pause that is still in force ends (seconds since 1970).
    pub paused_until: Option<i64>,
    /// An update is downloading or installing now (seen in the progress events; Windows'
    /// "working" flag alone was found true with nothing going on).
    pub downloading: bool,
    pub installing: bool,
    pub restart: bool,
    /// Updates Windows will install on its own (optional ones do not count).
    pub pending: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Headline {
    Unavailable,
    Checking,
    Installing,
    Downloading,
    RestartNeeded,
    Paused(i64),
    Available,
    UpToDate,
}

pub fn headline(o: &Overview) -> Headline {
    if !o.available {
        Headline::Unavailable
    } else if o.scanning {
        Headline::Checking
    } else if o.installing {
        Headline::Installing
    } else if o.downloading {
        Headline::Downloading
    } else if o.restart {
        Headline::RestartNeeded
    } else if let Some(t) = o.paused_until {
        Headline::Paused(t)
    } else if o.pending > 0 {
        Headline::Available
    } else {
        Headline::UpToDate
    }
}

impl Headline {
    /// The words; `date` writes a pause's end as the page shows dates.
    pub fn text(self, date: impl Fn(i64) -> String) -> String {
        match self {
            Headline::Unavailable => "Windows Update is not available".to_owned(),
            Headline::Checking => "Checking for updates\u{2026}".to_owned(),
            Headline::Installing => "Installing\u{2026}".to_owned(),
            Headline::Downloading => "Downloading\u{2026}".to_owned(),
            Headline::RestartNeeded => "Restart needed".to_owned(),
            Headline::Paused(t) => format!("Updates paused until {}", date(t)),
            Headline::Available => "Updates available".to_owned(),
            Headline::UpToDate => "You're up to date".to_owned(),
        }
    }

    /// The status island's main button.
    pub fn primary(self) -> Primary {
        match self {
            Headline::Unavailable => Primary::None,
            Headline::Checking => Primary::Checking,
            Headline::RestartNeeded => Primary::Restart,
            Headline::Paused(_) => Primary::Resume,
            _ => Primary::Check,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Primary {
    None,
    Check,
    /// "Check for updates", greyed while a check runs.
    Checking,
    /// "Update and restart", after a confirmation on the page.
    Restart,
    Resume,
}

/// How long Windows lets updates be paused when it does not say (Windows 11: five weeks).
pub const DEFAULT_MAX_PAUSE_DAYS: u32 = 35;

/// The pause lengths to offer, in weeks: one to five, as far as Windows' maximum allows.
pub fn pause_weeks(max_days: u32) -> Vec<u32> {
    (1..=(max_days / 7).clamp(1, 5)).collect()
}

/// When a pause of `weeks` started now ends, never past Windows' maximum.
pub fn pause_end(now: i64, weeks: u32, max_days: u32) -> i64 {
    now + i64::from((weeks * 7).min(max_days.max(1))) * 86_400
}

/// The end of a pause that is still in force, from the stored expiry text.
pub fn paused_until(expiry: Option<&str>, now: i64) -> Option<i64> {
    expiry.and_then(parse_iso).filter(|t| *t > now)
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

/// (year, month, day, hour, minute) of a count of seconds since 1970.
pub fn civil(secs: i64) -> (i64, u32, u32, u32, u32) {
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day, (rem / 3600) as u32, (rem % 3600 / 60) as u32)
}

/// UTC as Windows Update stores a pause: "2026-11-20T02:02:33Z".
pub fn iso(secs: i64) -> String {
    let (y, mo, d, h, mi) = civil(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{:02}Z", secs.rem_euclid(60))
}

/// Read "YYYY-MM-DDTHH:MM:SS", with or without a trailing "Z" or fraction, as UTC.
pub fn parse_iso(s: &str) -> Option<i64> {
    let s = s.trim();
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b' ') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> { s.get(r).filter(|t| t.bytes().all(|c| c.is_ascii_digit()))?.parse().ok() };
    let (y, mo, d, h, mi, se) = (num(0..4)?, num(5..7)?, num(8..10)?, num(11..13)?, num(14..16)?, num(17..19)?);
    let rest = &s[19..];
    let rest_ok = rest.is_empty() || rest == "Z" || (rest.starts_with('.') && rest.trim_end_matches('Z')[1..].bytes().all(|c| c.is_ascii_digit()));
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 || !rest_ok {
        return None;
    }
    Some(days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se)
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// A local time (seconds since 1970, already shifted to local) as "19 Nov 2026".
pub fn date_text(local: i64) -> String {
    let (y, m, d, _, _) = civil(local);
    format!("{d} {} {y}", MONTHS[(m - 1) as usize])
}

/// "Last checked today at 14:32", "yesterday at 09:05" or "on 12 Nov 2026 at 18:00", from two
/// local times.
pub fn checked_text(last: i64, now: i64) -> String {
    let (_, _, _, h, mi) = civil(last);
    let day = |t: i64| t.div_euclid(86_400);
    let when = match day(now) - day(last) {
        0 => "today".to_owned(),
        1 => "yesterday".to_owned(),
        _ => format!("on {}", date_text(last)),
    };
    format!("Last checked {when} at {h:02}:{mi:02}")
}

/// An OLE automation date (days since 1899-12-30, as WUA gives history dates) in seconds since
/// 1970.
pub fn ole_date_to_unix(d: f64) -> i64 {
    ((d - 25_569.0) * 86_400.0).round() as i64
}

/// A WinRT `DateTime` (100 ns ticks since 1601) in seconds since 1970.
pub fn ticks_to_unix(ticks: i64) -> i64 {
    ticks.div_euclid(10_000_000) - 11_644_473_600
}

/// An update id as the second contract spells it, "<guid>:<revision>": checked strictly before
/// it is used to find an update to approve.
pub fn valid_update_id(s: &str) -> bool {
    let Some((guid, rev)) = s.split_once(':') else { return false };
    let shape = guid.len() == 36
        && guid.char_indices().all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { c == '-' } else { c.is_ascii_hexdigit() });
    let rev_ok = !rev.is_empty() && rev.len() <= 10 && rev.bytes().all(|c| c.is_ascii_digit()) && rev.parse::<u32>().is_ok();
    shape && rev_ok
}

/// The groups the history is shown in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum Group {
    Quality,
    Driver,
    Definition,
    Other,
}

impl Group {
    pub const ALL: [Group; 4] = [Group::Quality, Group::Driver, Group::Definition, Group::Other];

    pub fn label(self) -> &'static str {
        match self {
            Group::Quality => "Quality updates",
            Group::Driver => "Driver updates",
            Group::Definition => "Definition updates",
            Group::Other => "Other updates",
        }
    }
}

/// The Microsoft Store's service in WUA's history: app updates, which this page leaves out.
pub const STORE_SERVICE: &str = "855e8a7c-ecb4-4ca3-b045-1dfa50104289";

pub fn is_store(service_id: &str) -> bool {
    service_id.trim().eq_ignore_ascii_case(STORE_SERVICE)
}

/// The group a history entry goes in, from its title, its categories (often empty) and the
/// program that installed it.
pub fn history_group(title: &str, categories: &[String], client: &str) -> Group {
    let lower = title.to_lowercase();
    let cats = categories.iter().map(|c| c.to_lowercase()).collect::<Vec<_>>();
    if definition_words(&lower) || cats.iter().any(|c| c.contains("defender") || c.contains("definition")) {
        Group::Definition
    } else if client.eq_ignore_ascii_case("Device Driver Retrieval Client") || cats.iter().any(|c| c.contains("driver")) || lower.contains("driver") || driver_title(title) {
        Group::Driver
    } else if quality_words(title, &lower) {
        Group::Quality
    } else {
        Group::Other
    }
}

/// How a history entry ended.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Outcome {
    Succeeded,
    /// Installed, with problems Windows noted.
    Partly,
    Failed(String),
    Cancelled,
    InProgress,
}

impl Outcome {
    /// WUA's `OperationResultCode` and the entry's HRESULT.
    pub fn from_wua(result: i32, hresult: i32) -> Outcome {
        match result {
            2 => Outcome::Succeeded,
            3 => Outcome::Partly,
            4 => Outcome::Failed(failure_text(hresult)),
            5 => Outcome::Cancelled,
            _ => Outcome::InProgress,
        }
    }

    pub fn text(&self) -> String {
        match self {
            Outcome::Succeeded => "Installed".to_owned(),
            Outcome::Partly => "Installed, with errors".to_owned(),
            Outcome::Failed(why) => format!("Failed: {why}"),
            Outcome::Cancelled => "Cancelled".to_owned(),
            Outcome::InProgress => "Not finished".to_owned(),
        }
    }
}

/// A Windows Update error in a few plain words; the code itself when it is not a common one.
pub fn failure_text(hresult: i32) -> String {
    let code = hresult as u32;
    let text = match code {
        0x8024_0034 => "the download failed",
        0x8024_402C | 0x8024_401C | 0x8024_4022 | 0x8007_2EE2 | 0x8007_2EE7 | 0x8007_2EFD | 0x8007_2EFE | 0x8007_2F8F => "could not reach Windows Update",
        0x8007_0070 | 0x8007_0027 => "not enough disk space",
        0x8007_0005 => "access was denied",
        0x8007_3712 | 0x800F_081F => "Windows files it needs are missing or damaged",
        0x8024_0017 => "not needed on this PC",
        0x8024_200D => "it needs another download",
        0x8024_0022 => "every update in the batch failed",
        0x8024_000B => "it was cancelled",
        0x8024_2016 => "something went wrong after the restart",
        0x8007_0643 | 0x800F_0922 => "the installer failed",
        0 => "no reason given",
        _ => return format!("error 0x{code:08X}"),
    };
    text.to_owned()
}

/// Why the page cannot use Windows Update at all, from the error its first call returned.
pub fn unavailable_text(hresult: i32) -> String {
    match hresult as u32 {
        0x8004_0154 | 0x8000_4002 => "This version of Windows does not let other apps manage updates.".to_owned(),
        0x8007_0422 => "The Windows Update service is turned off.".to_owned(),
        0x8007_0005 => "Windows refused access to its update service.".to_owned(),
        code => format!("Windows' update service did not answer (error 0x{code:08X})."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kb_numbers_are_found_in_titles() {
        assert_eq!(kb("2026-11 Security Update (KB5132061) (26300.9974)").as_deref(), Some("KB5132061"));
        assert_eq!(kb("Security Intelligence Update - KB2267602 (Version 1.463.246.0)").as_deref(), Some("KB2267602"));
        assert_eq!(kb("Windows Malicious Software Removal Tool x64 - v5.147 (KB890830)").as_deref(), Some("KB890830"));
        assert_eq!(kb("Logitech USB Driver Update (1.1.84.7627)"), None);
        assert_eq!(kb("KB12345"), None, "too short");
        assert_eq!(kb("XKB5132061"), None, "inside a word");
        assert_eq!(kb("kb5132061 lower").as_deref(), Some("KB5132061"));
        assert_eq!(kb("KB5132061x"), None);
        assert_eq!(kb(""), None);
    }

    #[test]
    fn the_category_decides_the_kind_first() {
        let none = Flags::default();
        assert_eq!(kind_of(Some(category::DRIVER), none, "anything"), Kind::Driver);
        assert_eq!(kind_of(Some(category::FIRMWARE), none, "anything"), Kind::Driver);
        assert_eq!(kind_of(Some(category::DEFINITION), none, "x"), Kind::Definition);
        assert_eq!(kind_of(Some(category::FEATURE), none, "x"), Kind::Feature);
        assert_eq!(kind_of(Some(category::QUALITY), none, "2026-11 Cumulative Update (KB5000001)"), Kind::Quality);
        assert_eq!(kind_of(Some(category::QUALITY), none, "2026-11 Security Update (KB5132061)"), Kind::Security);
        assert_eq!(kind_of(Some(category::QUALITY), Flags { security: true, ..none }, "x"), Kind::Security);
    }

    #[test]
    fn without_a_category_flags_then_titles_decide() {
        let none = Flags::default();
        assert_eq!(kind_of(None, Flags { driver: true, ..none }, "ITE - USB - 1.3.7.109"), Kind::Driver);
        assert_eq!(kind_of(Some(0), Flags { feature: true, ..none }, "Windows 11, version 26H2"), Kind::Feature);
        assert_eq!(kind_of(None, none, "Security Intelligence Update for Microsoft Defender Antivirus - KB2267602"), Kind::Definition);
        assert_eq!(kind_of(None, none, "2026-10 .NET Framework Security Update (KB5127694)"), Kind::Security);
        assert_eq!(kind_of(None, none, "2026-10 Cumulative Update Preview (KB5127000)"), Kind::Quality);
        assert_eq!(kind_of(None, none, "Advanced Micro Devices, Inc - System - 8/30/2017 12:00:00 AM - 5.12.0.38"), Kind::Driver);
        assert_eq!(kind_of(None, none, "Windows Malicious Software Removal Tool x64 - v5.147 (KB890830)"), Kind::Other);
    }

    fn facts(action: &str, reason: i32, seeker: bool) -> Facts {
        Facts { action: Action::parse(action), reason, seeker, progress: None, failed: None }
    }

    #[test]
    fn restart_and_optional_come_from_the_orchestrator() {
        assert!(facts("Commit", 0, false).restart_needed());
        assert!(facts("Install", reason::READY_TO_REBOOT, false).restart_needed());
        assert!(!facts("Download", 0, false).restart_needed());
        assert!(facts("Download", reason::SEEKER, true).optional());
        assert!(!facts("Download", 0, true).optional(), "approved: no longer waits to be asked");
        assert!(!facts("Commit", reason::READY_TO_REBOOT, true).optional());
    }

    #[test]
    fn the_status_line_says_what_is_happening() {
        assert_eq!(facts("Commit", 2, false).status(), "Waiting for a restart");
        assert_eq!(facts("Download", 1, true).status(), "Not downloaded");
        assert_eq!(facts("Download", 0, false).status(), "Waiting to download");
        assert_eq!(facts("Install", 0, false).status(), "Waiting to install");
        let mut f = facts("Download", 0, false);
        f.progress = Some(0.456);
        assert_eq!(f.status(), "Downloading 46%");
        f.action = Action::Install;
        f.progress = Some(1.5);
        assert_eq!(f.status(), "Installing 100%");
        f.progress = None;
        f.failed = Some("the download failed".into());
        assert_eq!(f.status(), "Last try failed: the download failed");
        assert_eq!(facts("Deploy", 0, false).status(), "Pending");
    }

    #[test]
    fn the_headline_follows_the_most_pressing_state() {
        let base = Overview { available: true, ..Default::default() };
        assert_eq!(headline(&Overview::default()), Headline::Unavailable);
        assert_eq!(headline(&base), Headline::UpToDate);
        assert_eq!(headline(&Overview { pending: 2, ..base }), Headline::Available);
        assert_eq!(headline(&Overview { pending: 2, paused_until: Some(9), ..base }), Headline::Paused(9));
        assert_eq!(headline(&Overview { pending: 2, restart: true, paused_until: Some(9), ..base }), Headline::RestartNeeded);
        assert_eq!(headline(&Overview { restart: true, downloading: true, ..base }), Headline::Downloading);
        assert_eq!(headline(&Overview { downloading: true, installing: true, ..base }), Headline::Installing);
        assert_eq!(headline(&Overview { scanning: true, installing: true, ..base }), Headline::Checking);
    }

    #[test]
    fn each_headline_has_its_button_and_words() {
        assert_eq!(Headline::RestartNeeded.primary(), Primary::Restart);
        assert_eq!(Headline::Checking.primary(), Primary::Checking);
        assert_eq!(Headline::Paused(0).primary(), Primary::Resume);
        assert_eq!(Headline::UpToDate.primary(), Primary::Check);
        assert_eq!(Headline::Available.primary(), Primary::Check);
        assert_eq!(Headline::Unavailable.primary(), Primary::None);
        assert_eq!(Headline::Paused(5).text(|t| format!("day {t}")), "Updates paused until day 5");
        assert_eq!(Headline::UpToDate.text(|_| String::new()), "You're up to date");
    }

    #[test]
    fn pauses_are_offered_and_capped_by_windows_maximum() {
        assert_eq!(pause_weeks(35), vec![1, 2, 3, 4, 5]);
        assert_eq!(pause_weeks(70), vec![1, 2, 3, 4, 5]);
        assert_eq!(pause_weeks(14), vec![1, 2]);
        assert_eq!(pause_weeks(7), vec![1]);
        assert_eq!(pause_weeks(3), vec![1], "at least one, capped by the end date");
        let now = 1_794_535_000;
        assert_eq!(pause_end(now, 2, 35), now + 14 * 86_400);
        assert_eq!(pause_end(now, 1, 3), now + 3 * 86_400);
        assert_eq!(pause_end(now, 1, 0), now + 86_400);
    }

    #[test]
    fn pause_times_round_trip_as_windows_writes_them() {
        assert_eq!(iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(parse_iso("2026-11-20T02:02:33Z"), Some(1_795_140_153));
        assert_eq!(iso(1_795_140_153), "2026-11-20T02:02:33Z");
        assert_eq!(parse_iso("2024-02-29T00:00:00Z"), Some(1_709_164_800));
        assert_eq!(parse_iso("2024-02-29 00:00:00"), Some(1_709_164_800));
        assert_eq!(parse_iso("2024-02-29T00:00:00.1234567Z"), Some(1_709_164_800));
        for bad in ["", "2024-13-01T00:00:00Z", "2024-02-29T24:00:00Z", "2024/02/29T00:00:00Z", "2024-02-29T00:00:00+01:00", "abcd-ef-ghTij:kl:mn"] {
            assert_eq!(parse_iso(bad), None, "{bad}");
        }
        let now = 1_794_535_000;
        assert_eq!(paused_until(Some("2026-11-20T02:02:33Z"), now), Some(1_795_140_153));
        assert_eq!(paused_until(Some("2020-01-01T00:00:00Z"), now), None, "a pause that ended");
        assert_eq!(paused_until(None, now), None);
        assert_eq!(paused_until(Some("garbage"), now), None);
    }

    #[test]
    fn dates_read_plainly() {
        assert_eq!(date_text(1_795_140_153), "20 Nov 2026");
        let now = 1_795_140_153;
        assert_eq!(checked_text(now - 60, now), "Last checked today at 02:01");
        assert_eq!(checked_text(now - 86_400, now), "Last checked yesterday at 02:02");
        assert_eq!(checked_text(now - 5 * 86_400, now), "Last checked on 15 Nov 2026 at 02:02");
        assert_eq!(ole_date_to_unix(25_569.0), 0);
        assert_eq!(ole_date_to_unix(25_569.5), 43_200);
        assert_eq!(ticks_to_unix(116_444_736_000_000_000), 0);
    }

    #[test]
    fn update_ids_are_checked_strictly() {
        assert!(valid_update_id("702e6ed7-4f31-4f3b-9f49-de333467baf8:1"));
        assert!(valid_update_id("3B038648-DA7B-4521-8241-42A2890FEC78:200"));
        for bad in [
            "",
            "702e6ed7-4f31-4f3b-9f49-de333467baf8",
            "702e6ed7-4f31-4f3b-9f49-de333467baf8:",
            "702e6ed7-4f31-4f3b-9f49-de333467baf8:-1",
            "702e6ed7-4f31-4f3b-9f49-de333467baf8:1 ",
            "702e6ed7-4f31-4f3b-9f49-de333467baf8:99999999999",
            "702e6ed74f314f3b9f49de333467baf8:1",
            "702e6ed7-4f31-4f3b-9f49-de333467baxz:1",
            "{702e6ed7-4f31-4f3b-9f49-de333467baf8}:1",
            "702e6ed7-4f31-4f3b-9f49-de333467baf8:1:2",
        ] {
            assert!(!valid_update_id(bad), "{bad}");
        }
    }

    #[test]
    fn history_entries_fall_into_groups() {
        let none: Vec<String> = Vec::new();
        assert_eq!(history_group("2026-11 Security Update (KB5132061) (26300.9974)", &none, "MoUpdateOrchestrator"), Group::Quality);
        assert_eq!(history_group("2026-10 .NET Framework Security Update (KB5127694)", &none, ""), Group::Quality);
        assert_eq!(history_group("Advanced Micro Devices, Inc. Display Driver Update (32.0.21042.62)", &none, "MoUpdateOrchestrator"), Group::Driver);
        assert_eq!(history_group("Logitech - Image - 8/28/2017 12:00:00 AM - 1.0.85.0", &none, "Device Driver Retrieval Client"), Group::Driver);
        assert_eq!(history_group("ITE - USB - 1.3.7.109", &none, ""), Group::Driver);
        let defender = vec!["Microsoft Defender Antivirus".to_owned()];
        assert_eq!(history_group("Security Intelligence Update for Microsoft Defender Antivirus - KB2267602 (Version 1.463.271.0) - Current Channel (Broad)", &defender, "Windows Defender"), Group::Definition);
        assert_eq!(history_group("Update for Windows Security platform - KB5007651 (Version 10.0.29680.1000)", &none, ""), Group::Definition);
        assert_eq!(history_group("Windows Malicious Software Removal Tool x64 - v5.147 (KB890830)", &none, ""), Group::Other);
        assert!(is_store("855E8A7C-ECB4-4CA3-B045-1DFA50104289"));
        assert!(!is_store("8b24b027-1dee-babb-9a95-3517dfb9c552"));
        assert!(!is_store(""));
    }

    #[test]
    fn results_read_plainly() {
        assert_eq!(Outcome::from_wua(2, 0), Outcome::Succeeded);
        assert_eq!(Outcome::from_wua(3, 0), Outcome::Partly);
        assert_eq!(Outcome::from_wua(5, 0), Outcome::Cancelled);
        assert_eq!(Outcome::from_wua(1, 0), Outcome::InProgress);
        assert_eq!(Outcome::from_wua(4, 0x8024_0034_u32 as i32).text(), "Failed: the download failed");
        assert_eq!(Outcome::from_wua(4, 0x8007_0070_u32 as i32).text(), "Failed: not enough disk space");
        assert_eq!(failure_text(0x8024_1234_u32 as i32), "error 0x80241234");
        assert_eq!(Outcome::Succeeded.text(), "Installed");
        assert!(unavailable_text(0x8007_0422_u32 as i32).contains("turned off"));
        assert!(unavailable_text(0x8004_0154_u32 as i32).contains("does not"));
        assert!(unavailable_text(0x1234).contains("0x00001234"));
    }
}
