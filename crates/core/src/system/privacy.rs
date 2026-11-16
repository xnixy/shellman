//! Privacy & security: app permissions and the privacy switches of Windows' Settings.
//!
//! App permissions live in the capability access consent store,
//! `...\CurrentVersion\CapabilityAccessManager\ConsentStore\<capability>`:
//! - `HKLM\...\<capability>\Value` is the device's switch ("Location services", "Camera
//!   access"): shown, never written (it needs an administrator).
//! - `HKCU\...\<capability>\Value` is "Let apps access your ..." (`Allow` / `Deny`, missing is
//!   allowed).
//! - `HKCU\...\<capability>\<package family>\Value` is one Store app's switch (`Allow`, `Deny`,
//!   or `Prompt` while it has not asked yet).
//! - `HKCU\...\<capability>\NonPackaged\Value` is "Let desktop apps access ..."; its subkeys are
//!   the desktop apps that used it (the path with `#` for `\`), with the time of last use. Windows
//!   lists those without a switch each, and so does this page.
//! An administrator's policy (`HKLM\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy`,
//! `LetAppsAccess*` non-zero) takes the decision away.
//!
//! App permissions are only read. The capability access service (`camsvc`) keeps the store and
//! does not follow writes made to it: measured on 26300, setting the user's or the desktop apps'
//! microphone `Value` to `Deny` left WinRT `AppCapability.CheckAccess("microphone")` saying
//! `Allowed`. Windows' Settings changes them through the service, which has no public API, so the
//! page shows each permission and opens Windows' Settings on its page.
//!
//! The other switches are per-user values Windows' Settings writes and Windows reads where it
//! uses them (the advertising ID, the language list for websites, app-launch tracking, suggested
//! content, search permissions). Values in HKLM or set by policy (diagnostic data, activity
//! history, feedback) are only read; the page links to Windows' Settings for them.
//!
//! Everything is written against [`Registry`], so the decisions are tested with the fake.

use crate::registry::{Hive, Registry, Value};

pub const CONSENT: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore";
const APP_PRIVACY_POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows\AppPrivacy";
const NON_PACKAGED: &str = "NonPackaged";

/// One kind of permission, as Windows' Settings lists it under App permissions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Capability {
    /// The consent store's key name.
    pub key: &'static str,
    pub name: &'static str,
    /// Its page in Windows' Settings.
    pub uri: &'static str,
    /// Whether Windows offers "Let desktop apps access" for it.
    pub desktop: bool,
    /// The `AppPrivacy` policy value that can decide it, `LetAppsAccess` + this.
    pub policy: &'static str,
}

const fn cap(key: &'static str, name: &'static str, uri: &'static str, desktop: bool, policy: &'static str) -> Capability {
    Capability { key, name, uri, desktop, policy }
}

/// The permissions shown, in Windows' order.
pub const CAPABILITIES: &[Capability] = &[
    cap("location", "Location", "ms-settings:privacy-location", true, "Location"),
    cap("webcam", "Camera", "ms-settings:privacy-webcam", true, "Camera"),
    cap("microphone", "Microphone", "ms-settings:privacy-microphone", true, "Microphone"),
    cap("humanPresence", "Presence sensing", "ms-settings:privacy-presence", false, "HumanPresence"),
    cap("userNotificationListener", "Notifications", "ms-settings:privacy-notifications", false, "Notifications"),
    cap("userAccountInformation", "Account info", "ms-settings:privacy-accountinfo", false, "AccountInfo"),
    cap("contacts", "Contacts", "ms-settings:privacy-contacts", false, "Contacts"),
    cap("appointments", "Calendar", "ms-settings:privacy-calendar", false, "Calendar"),
    cap("phoneCall", "Phone calls", "ms-settings:privacy-phonecalls", false, "Phone"),
    cap("phoneCallHistory", "Call history", "ms-settings:privacy-callhistory", false, "CallHistory"),
    cap("email", "Email", "ms-settings:privacy-email", false, "Email"),
    cap("userDataTasks", "Tasks", "ms-settings:privacy-tasks", false, "Tasks"),
    cap("chat", "Messaging", "ms-settings:privacy-messaging", false, "Messaging"),
    cap("radios", "Radios", "ms-settings:privacy-radios", false, "Radios"),
    cap("bluetoothSync", "Other devices", "ms-settings:privacy-customdevices", false, "SyncWithDevices"),
    cap("appDiagnostics", "App diagnostics", "ms-settings:privacy-appdiagnostics", false, "DiagnosticInfo"),
    cap("documentsLibrary", "Documents", "ms-settings:privacy-documents", false, ""),
    cap("downloadsFolder", "Downloads folder", "ms-settings:privacy-downloadsfolder", false, ""),
    cap("musicLibrary", "Music library", "ms-settings:privacy-musiclibrary", false, ""),
    cap("picturesLibrary", "Pictures", "ms-settings:privacy-pictures", false, ""),
    cap("videosLibrary", "Videos", "ms-settings:privacy-videos", false, ""),
    cap("broadFileSystemAccess", "File system", "ms-settings:privacy-broadfilesystemaccess", false, ""),
    cap("graphicsCaptureProgrammatic", "Screenshots and screen recording", "ms-settings:privacy-graphicscaptureprogrammatic", true, "GraphicsCaptureProgrammatic"),
    cap("graphicsCaptureWithoutBorder", "Screenshot borders", "ms-settings:privacy-graphicscapturewithoutborder", true, "GraphicsCaptureWithoutBorder"),
    cap("systemAIModels", "Text and image generation", "ms-settings:privacy-systemaimodels", false, "SystemAIModels"),
];

/// A consent store value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Consent {
    Allow,
    Deny,
    /// The app has not asked yet; it will ask the first time.
    Prompt,
}

impl Consent {
    pub fn parse(text: &str) -> Option<Consent> {
        match text.trim() {
            t if t.eq_ignore_ascii_case("Allow") => Some(Consent::Allow),
            t if t.eq_ignore_ascii_case("Deny") => Some(Consent::Deny),
            t if t.eq_ignore_ascii_case("Prompt") => Some(Consent::Prompt),
            _ => None,
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            Consent::Allow => "Allow",
            Consent::Deny => "Deny",
            Consent::Prompt => "Prompt",
        }
    }

    /// `true` for allowed or ask: what a switch shows (Windows shows a not-yet-asked app on).
    pub fn on(self) -> bool {
        self != Consent::Deny
    }

    pub fn from_switch(on: bool) -> Consent {
        if on {
            Consent::Allow
        } else {
            Consent::Deny
        }
    }
}

/// One Store app's switch for a permission.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AppConsent {
    /// The package family name, the key's name.
    pub family: String,
    pub consent: Consent,
}

/// A desktop app that used a permission: its path and when it last did.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DesktopUse {
    pub path: String,
    /// The last use, as a FILETIME (100 ns since 1601), when Windows kept one.
    pub last_used: Option<u64>,
}

/// What an administrator's policy says about a permission.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Policy {
    /// No policy: the user decides.
    None,
    ForceAllow,
    ForceDeny,
}

/// One permission's state.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Permission {
    pub cap: Capability,
    /// The device's switch; `true` when missing.
    pub device: bool,
    /// "Let apps access"; `true` when missing.
    pub user: bool,
    /// "Let desktop apps access", for the permissions that have it.
    pub desktop: Option<bool>,
    pub apps: Vec<AppConsent>,
    pub desktop_apps: Vec<DesktopUse>,
    pub policy: Policy,
}

impl Permission {
    /// Whether the user's switches can be changed here: a policy decides otherwise, and with the
    /// device's switch off Windows greys them out too.
    pub fn editable(&self) -> bool {
        self.policy == Policy::None && self.device
    }

    /// Whether apps get it at all, before each app's own switch.
    pub fn apps_allowed(&self) -> bool {
        match self.policy {
            Policy::ForceAllow => true,
            Policy::ForceDeny => false,
            Policy::None => self.device && self.user,
        }
    }

    /// Whether one Store app gets it.
    pub fn app_allowed(&self, app: &AppConsent) -> bool {
        self.apps_allowed() && (self.policy == Policy::ForceAllow || app.consent == Consent::Allow)
    }

    /// A short line for the permission's row.
    pub fn summary(&self) -> String {
        match self.policy {
            Policy::ForceAllow => return "Allowed by your organisation".to_owned(),
            Policy::ForceDeny => return "Turned off by your organisation".to_owned(),
            Policy::None => {}
        }
        if !self.device {
            return "Off for this device".to_owned();
        }
        if !self.user {
            return "Off".to_owned();
        }
        let on = self.apps.iter().filter(|a| a.consent == Consent::Allow).count();
        match (self.apps.len(), on) {
            (0, _) => "On".to_owned(),
            (n, k) if n == k => format!("On for {n} app{}", if n == 1 { "" } else { "s" }),
            (n, k) => format!("On for {k} of {n} apps"),
        }
    }
}

fn text(reg: &dyn Registry, hive: Hive, key: &str) -> Option<Consent> {
    reg.get(hive, key, "Value").and_then(|v| v.text().and_then(Consent::parse))
}

/// The policy for `cap`: `LetAppsAccess<policy>` 1 allows, 2 denies, 0 or missing leaves it to
/// the user.
fn policy(reg: &dyn Registry, cap: &Capability) -> Policy {
    if cap.policy.is_empty() {
        return Policy::None;
    }
    match reg.get(Hive::Hklm, APP_PRIVACY_POLICY, &format!("LetAppsAccess{}", cap.policy)) {
        Some(Value::Dword(1)) => Policy::ForceAllow,
        Some(Value::Dword(2)) => Policy::ForceDeny,
        _ => Policy::None,
    }
}

/// A REG_QWORD as the registry trait hands it over (eight bytes).
fn qword(v: Option<Value>) -> Option<u64> {
    match v? {
        Value::Binary(b) if b.len() >= 8 => Some(u64::from_le_bytes(b[..8].try_into().ok()?)).filter(|t| *t != 0),
        Value::Dword(d) => Some(d as u64).filter(|t| *t != 0),
        _ => None,
    }
}

/// A NonPackaged subkey's name as a path: `C:#Program Files#app.exe` is `C:\Program Files\app.exe`.
pub fn desktop_path(key: &str) -> String {
    key.replace('#', "\\")
}

/// Read one permission.
pub fn read_one(reg: &dyn Registry, cap: Capability) -> Permission {
    let base = format!(r"{CONSENT}\{}", cap.key);
    let mut apps = Vec::new();
    let mut desktop_apps = Vec::new();
    for sub in reg.subkeys(Hive::Hkcu, &base) {
        if sub.eq_ignore_ascii_case(NON_PACKAGED) {
            let np = format!(r"{base}\{sub}");
            for exe in reg.subkeys(Hive::Hkcu, &np) {
                let k = format!(r"{np}\{exe}");
                let last_used = qword(reg.get(Hive::Hkcu, &k, "LastUsedTimeStop")).or_else(|| qword(reg.get(Hive::Hkcu, &k, "LastUsedTimeStart")));
                desktop_apps.push(DesktopUse { path: desktop_path(&exe), last_used });
            }
        } else if let Some(consent) = text(reg, Hive::Hkcu, &format!(r"{base}\{sub}")) {
            apps.push(AppConsent { family: sub, consent });
        }
    }
    apps.sort_by_key(|a| family_label(&a.family).to_lowercase());
    // The most recent first, as Windows lists them.
    desktop_apps.sort_by(|a, b| b.last_used.cmp(&a.last_used));
    Permission {
        cap,
        device: text(reg, Hive::Hklm, &base) != Some(Consent::Deny),
        user: text(reg, Hive::Hkcu, &base) != Some(Consent::Deny),
        desktop: cap.desktop.then(|| text(reg, Hive::Hkcu, &format!(r"{base}\{NON_PACKAGED}")) != Some(Consent::Deny)),
        apps,
        desktop_apps,
        policy: policy(reg, &cap),
    }
}

/// Read every permission.
pub fn read_permissions(reg: &dyn Registry) -> Vec<Permission> {
    CAPABILITIES.iter().map(|c| read_one(reg, *c)).collect()
}

/// "Microsoft.WindowsNotepad_8wekyb3d8bbwe" → "Windows Notepad": a name for a package whose
/// display name Windows did not give.
pub fn family_label(family: &str) -> String {
    let name = family.rsplit_once('_').map_or(family, |(n, _)| n);
    let last = name.rsplit('.').next().unwrap_or(name);
    let mut out = String::new();
    let chars: Vec<char> = last.chars().collect();
    for (i, ch) in chars.iter().enumerate() {
        let next_lower = chars.get(i + 1).is_some_and(|c| c.is_lowercase());
        if i > 0 && ch.is_uppercase() && (chars[i - 1].is_lowercase() || next_lower) {
            out.push(' ');
        }
        out.push(*ch);
    }
    if out.is_empty() {
        family.to_owned()
    } else {
        out
    }
}

/// A switch that is one or more DWORD values in HKCU.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Toggle {
    pub id: &'static str,
    pub title: &'static str,
    pub detail: &'static str,
    pub key: &'static str,
    /// The values it writes, all the same.
    pub names: &'static [&'static str],
    /// The value that means on, and the one that means off.
    pub on: u32,
    pub off: u32,
    /// What a missing value means.
    pub default_on: bool,
    /// An HKLM policy value that, when it is `policy.2`, turns the switch off for good.
    pub policy: Option<(&'static str, &'static str, u32)>,
}

/// "General" in Windows' Privacy & security.
pub const GENERAL: &[Toggle] = &[
    Toggle {
        id: "adid",
        title: "Let apps show me personalised ads by using my advertising ID",
        detail: "Turning it off resets the ID apps see.",
        key: r"Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo",
        names: &["Enabled"],
        on: 1,
        off: 0,
        default_on: true,
        policy: Some((r"SOFTWARE\Policies\Microsoft\Windows\AdvertisingInfo", "DisabledByGroupPolicy", 1)),
    },
    Toggle {
        id: "languages",
        title: "Let websites show me locally relevant content by accessing my language list",
        detail: "",
        key: r"Control Panel\International\User Profile",
        names: &["HttpAcceptLanguageOptOut"],
        on: 0,
        off: 1,
        default_on: true,
        policy: None,
    },
    Toggle {
        id: "launches",
        title: "Let Windows improve Start and search results by tracking app launches",
        detail: "",
        key: r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced",
        names: &["Start_TrackProgs"],
        on: 1,
        off: 0,
        default_on: true,
        policy: None,
    },
    Toggle {
        id: "suggested",
        title: "Show me suggested content in the Settings app",
        detail: "",
        key: r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager",
        names: &["SubscribedContent-338393Enabled", "SubscribedContent-353694Enabled", "SubscribedContent-353696Enabled"],
        on: 1,
        off: 0,
        default_on: true,
        policy: None,
    },
    Toggle {
        id: "settings-notifications",
        title: "Show me notifications in the Settings app",
        detail: "",
        key: r"Software\Microsoft\Windows\CurrentVersion\SystemSettings\AccountNotifications",
        names: &["EnableAccountNotifications"],
        on: 1,
        off: 0,
        default_on: true,
        policy: None,
    },
];

const SEARCH: &str = r"Software\Microsoft\Windows\CurrentVersion\SearchSettings";

/// "Search permissions" switches.
pub const SEARCH_TOGGLES: &[Toggle] = &[
    Toggle {
        id: "cloud-msa",
        title: "Microsoft account",
        detail: "Search Outlook, OneDrive, Bing and other services with your Microsoft account.",
        key: SEARCH,
        names: &["IsMSACloudSearchEnabled"],
        on: 1,
        off: 0,
        default_on: true,
        policy: None,
    },
    Toggle {
        id: "cloud-aad",
        title: "Work or school account",
        detail: "Search SharePoint, OneDrive for work and Outlook with your work or school account.",
        key: SEARCH,
        names: &["IsAADCloudSearchEnabled"],
        on: 1,
        off: 0,
        default_on: true,
        policy: None,
    },
    Toggle {
        id: "history",
        title: "Search history on this device",
        detail: "Searches done on this device suggest what to search for next.",
        key: SEARCH,
        names: &["IsDeviceSearchHistoryEnabled"],
        on: 1,
        off: 0,
        default_on: true,
        policy: None,
    },
    Toggle {
        id: "highlights",
        title: "Show search highlights",
        detail: "Notable moments and trending searches in Windows' search box.",
        key: SEARCH,
        names: &["IsDynamicSearchBoxEnabled"],
        on: 1,
        off: 0,
        default_on: true,
        policy: None,
    },
];

fn dword(reg: &dyn Registry, hive: Hive, key: &str, name: &str) -> Option<u32> {
    match reg.get(hive, key, name)? {
        Value::Dword(v) => Some(v),
        Value::Text(t) | Value::Expand(t) => t.trim().parse().ok(),
        Value::Binary(_) => None,
    }
}

/// Whether a policy forces a toggle off.
pub fn toggle_locked(reg: &dyn Registry, t: &Toggle) -> bool {
    t.policy.is_some_and(|(key, name, v)| dword(reg, Hive::Hklm, key, name) == Some(v))
}

/// A toggle's state: on only when every value it writes says on (a missing one counts as its
/// default; an unknown number counts as on when it is non-zero and "on" is non-zero).
pub fn read_toggle(reg: &dyn Registry, t: &Toggle) -> bool {
    if toggle_locked(reg, t) {
        return false;
    }
    t.names.iter().all(|name| match dword(reg, Hive::Hkcu, t.key, name) {
        None => t.default_on,
        Some(v) if v == t.on => true,
        Some(v) if v == t.off => false,
        Some(v) => (v != 0) == (t.on != 0),
    })
}

pub fn write_toggle(reg: &dyn Registry, t: &Toggle, on: bool) -> Result<(), String> {
    if toggle_locked(reg, t) {
        return Err("Set by your organisation.".into());
    }
    for name in t.names {
        reg.set_dword(Hive::Hkcu, t.key, name, if on { t.on } else { t.off })?;
    }
    Ok(())
}

/// SafeSearch for web results in Windows' search.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SafeSearch {
    Strict,
    Moderate,
    Off,
}

impl SafeSearch {
    pub const ALL: [SafeSearch; 3] = [SafeSearch::Strict, SafeSearch::Moderate, SafeSearch::Off];

    pub fn label(self) -> &'static str {
        match self {
            SafeSearch::Strict => "Strict",
            SafeSearch::Moderate => "Moderate",
            SafeSearch::Off => "Off",
        }
    }

    /// `SafeSearchMode`: 0 off, 1 moderate (also when missing), 2 strict.
    fn raw(self) -> u32 {
        match self {
            SafeSearch::Off => 0,
            SafeSearch::Moderate => 1,
            SafeSearch::Strict => 2,
        }
    }
}

pub fn read_safe_search(reg: &dyn Registry) -> SafeSearch {
    match dword(reg, Hive::Hkcu, SEARCH, "SafeSearchMode") {
        Some(0) => SafeSearch::Off,
        Some(2) => SafeSearch::Strict,
        _ => SafeSearch::Moderate,
    }
}

pub fn write_safe_search(reg: &dyn Registry, s: SafeSearch) -> Result<(), String> {
    reg.set_dword(Hive::Hkcu, SEARCH, "SafeSearchMode", s.raw())
}

/// What the page shows read-only, each with where it came from.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Managed {
    /// Diagnostic data: "Required only" / "Optional", and whether a policy set it.
    pub diagnostics: Option<(String, bool)>,
    /// Feedback frequency, and whether a policy set it.
    pub feedback: (String, bool),
    /// Activity history set by policy: `Some(false)` when a policy turns it off.
    pub activity_policy: Option<bool>,
    /// Online speech recognition.
    pub online_speech: bool,
}

const DATA_COLLECTION: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\DataCollection";
const DATA_COLLECTION_POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows\DataCollection";
const SYSTEM_POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows\System";

/// `AllowTelemetry` in words.
pub fn diagnostics_label(v: u32) -> &'static str {
    match v {
        0 => "Security only",
        1 => "Required only",
        _ => "Optional",
    }
}

/// The feedback frequency from the `Siuf\Rules` values Windows' Settings writes.
pub fn feedback_label(count: Option<u32>, period: Option<u64>) -> &'static str {
    match (count, period) {
        (None, None) => "Automatically (recommended)",
        (Some(0), _) => "Never",
        (_, Some(p)) if p <= 100_000_000 => "Always",
        (_, Some(864_000_000_000)) => "Once a day",
        (_, Some(6_048_000_000_000)) => "Once a week",
        _ => "Custom",
    }
}

pub fn read_managed(reg: &dyn Registry) -> Managed {
    let policy_telemetry = dword(reg, Hive::Hklm, DATA_COLLECTION_POLICY, "AllowTelemetry");
    let diagnostics = policy_telemetry
        .map(|v| (diagnostics_label(v).to_owned(), true))
        .or_else(|| dword(reg, Hive::Hklm, DATA_COLLECTION, "AllowTelemetry").map(|v| (diagnostics_label(v).to_owned(), false)));
    let feedback = if dword(reg, Hive::Hklm, DATA_COLLECTION_POLICY, "DoNotShowFeedbackNotifications") == Some(1) {
        ("Never".to_owned(), true)
    } else {
        let rules = r"Software\Microsoft\Siuf\Rules";
        let period = match reg.get(Hive::Hkcu, rules, "PeriodInNanoSeconds") {
            Some(Value::Binary(b)) if b.len() >= 8 => b[..8].try_into().ok().map(u64::from_le_bytes),
            Some(Value::Dword(d)) => Some(d as u64),
            _ => None,
        };
        (feedback_label(dword(reg, Hive::Hkcu, rules, "NumberOfSIUFInPeriod"), period).to_owned(), false)
    };
    let activity_policy = ["PublishUserActivities", "UploadUserActivities", "EnableActivityFeed"]
        .iter()
        .find_map(|n| dword(reg, Hive::Hklm, SYSTEM_POLICY, n).filter(|v| *v == 0))
        .map(|_| false);
    Managed {
        diagnostics,
        feedback,
        activity_policy,
        online_speech: dword(reg, Hive::Hkcu, r"Software\Microsoft\Speech_OneCore\Settings\OnlineSpeechPrivacy", "HasAccepted") == Some(1),
    }
}

/// Everything the page shows.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Snapshot {
    pub permissions: Vec<Permission>,
    /// `(id, on, locked)` for [`GENERAL`] and [`SEARCH_TOGGLES`].
    pub toggles: Vec<(&'static str, bool, bool)>,
    pub safe_search: Option<SafeSearch>,
    pub managed: Managed,
}

impl Snapshot {
    pub fn toggle(&self, id: &str) -> Option<(bool, bool)> {
        self.toggles.iter().find(|t| t.0 == id).map(|t| (t.1, t.2))
    }
}

pub fn read(reg: &dyn Registry) -> Snapshot {
    Snapshot {
        permissions: read_permissions(reg),
        toggles: GENERAL.iter().chain(SEARCH_TOGGLES).map(|t| (t.id, read_toggle(reg, t), toggle_locked(reg, t))).collect(),
        safe_search: Some(read_safe_search(reg)),
        managed: read_managed(reg),
    }
}

/// A toggle by its id.
pub fn find_toggle(id: &str) -> Option<&'static Toggle> {
    GENERAL.iter().chain(SEARCH_TOGGLES).find(|t| t.id == id)
}

/// A date for a FILETIME, "15 Nov 2026", in UTC (close enough for "last used").
pub fn filetime_date(ft: u64) -> Option<String> {
    let secs = (ft / 10_000_000).checked_sub(11_644_473_600)?;
    let days = (secs / 86_400) as i64;
    // Civil date from days since 1970 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    Some(format!("{d} {} {y}", MONTHS[(m - 1) as usize]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::fake::Fake;

    fn put(reg: &Fake, hive: Hive, key: &str, v: &str) {
        reg.put(hive, key, "Value", Value::Text(v.into()));
    }

    fn webcam() -> Capability {
        *CAPABILITIES.iter().find(|c| c.key == "webcam").unwrap()
    }

    #[test]
    fn missing_values_allow() {
        let reg = Fake::default();
        let p = read_one(&reg, webcam());
        assert!(p.device && p.user && p.desktop == Some(true));
        assert!(p.apps_allowed() && p.editable());
        assert_eq!(p.summary(), "On");
    }

    #[test]
    fn device_and_user_and_app_must_all_allow() {
        let reg = Fake::default();
        let base = format!(r"{CONSENT}\webcam");
        put(&reg, Hive::Hkcu, &base, "Allow");
        put(&reg, Hive::Hkcu, &format!(r"{base}\Contoso.App_abc"), "Allow");
        put(&reg, Hive::Hkcu, &format!(r"{base}\Other.App_abc"), "Prompt");
        let p = read_one(&reg, webcam());
        assert_eq!(p.apps.len(), 2);
        let contoso = p.apps.iter().find(|a| a.family.eq_ignore_ascii_case("Contoso.App_abc")).unwrap();
        let other = p.apps.iter().find(|a| a.family.eq_ignore_ascii_case("Other.App_abc")).unwrap();
        assert!(p.app_allowed(contoso));
        assert!(!p.app_allowed(other), "an app that has not asked yet has no access");
        assert!(other.consent.on(), "but its switch shows on, as in Windows");
        assert_eq!(p.summary(), "On for 1 of 2 apps");

        put(&reg, Hive::Hkcu, &base, "Deny");
        let p = read_one(&reg, webcam());
        assert!(p.apps.iter().all(|a| !p.app_allowed(a)));
        assert_eq!(p.summary(), "Off");

        put(&reg, Hive::Hkcu, &base, "Allow");
        put(&reg, Hive::Hklm, &base, "Deny");
        let p = read_one(&reg, webcam());
        assert!(!p.apps_allowed() && !p.editable());
        assert_eq!(p.summary(), "Off for this device");
    }

    #[test]
    fn location_off_for_the_device_as_on_this_machine() {
        // As read on this PC: HKLM location Deny, HKCU only has NonPackaged.
        let reg = Fake::default();
        let base = format!(r"{CONSENT}\location");
        put(&reg, Hive::Hklm, &base, "Deny");
        put(&reg, Hive::Hkcu, &format!(r"{base}\NonPackaged"), "Allow");
        let loc = *CAPABILITIES.iter().find(|c| c.key == "location").unwrap();
        let p = read_one(&reg, loc);
        assert!(!p.device && p.user);
        assert!(p.desktop_apps.is_empty(), "the NonPackaged key is the desktop switch, not an app");
        assert!(!p.editable());
    }

    #[test]
    fn a_policy_decides_and_nothing_is_written() {
        let reg = Fake::default();
        reg.put(Hive::Hklm, APP_PRIVACY_POLICY, "LetAppsAccessCamera", Value::Dword(2));
        let p = read_one(&reg, webcam());
        assert_eq!(p.policy, Policy::ForceDeny);
        assert!(!p.apps_allowed() && !p.editable());
        assert_eq!(p.summary(), "Turned off by your organisation");
        reg.put(Hive::Hklm, APP_PRIVACY_POLICY, "LetAppsAccessCamera", Value::Dword(0));
        assert_eq!(read_one(&reg, webcam()).policy, Policy::None, "0 leaves it to the user");
    }

    #[test]
    fn desktop_apps_are_listed_newest_first_with_their_paths() {
        let reg = Fake::default();
        let np = format!(r"{CONSENT}\microphone\NonPackaged");
        put(&reg, Hive::Hkcu, &np, "Allow");
        reg.put(Hive::Hkcu, &format!(r"{np}\C:#Apps#old.exe"), "LastUsedTimeStop", Value::Binary(100u64.to_le_bytes().to_vec()));
        reg.put(Hive::Hkcu, &format!(r"{np}\C:#Apps#new.exe"), "LastUsedTimeStop", Value::Binary(200u64.to_le_bytes().to_vec()));
        let mic = *CAPABILITIES.iter().find(|c| c.key == "microphone").unwrap();
        let p = read_one(&reg, mic);
        let paths: Vec<_> = p.desktop_apps.iter().map(|d| d.path.to_lowercase()).collect();
        assert_eq!(paths, [r"c:\apps\new.exe", r"c:\apps\old.exe"]);
        assert_eq!(p.desktop_apps[0].last_used, Some(200));
    }

    #[test]
    fn package_names_read_well() {
        assert_eq!(family_label("Microsoft.WindowsNotepad_8wekyb3d8bbwe"), "Windows Notepad");
        assert_eq!(family_label("Claude_pzs8sxrjxfjjc"), "Claude");
        assert_eq!(family_label("DolbyLaboratories.DolbyAccessOEM_rz1tebttyb220"), "Dolby Access OEM");
        assert_eq!(family_label("Microsoft.ScreenSketch_8wekyb3d8bbwe"), "Screen Sketch");
        assert_eq!(family_label("google.com"), "com");
    }

    #[test]
    fn toggles_read_missing_as_default_and_write_every_value() {
        let reg = Fake::default();
        let lang = find_toggle("languages").unwrap();
        assert!(read_toggle(&reg, lang), "missing: on");
        write_toggle(&reg, lang, false).unwrap();
        assert_eq!(reg.get(Hive::Hkcu, lang.key, "HttpAcceptLanguageOptOut"), Some(Value::Dword(1)), "off is opting out");
        assert!(!read_toggle(&reg, lang));

        let sug = find_toggle("suggested").unwrap();
        reg.put(Hive::Hkcu, sug.key, "SubscribedContent-353694Enabled", Value::Dword(0));
        assert!(!read_toggle(&reg, sug), "on only when all three are on");
        write_toggle(&reg, sug, true).unwrap();
        assert!(sug.names.iter().all(|n| reg.get(Hive::Hkcu, sug.key, n) == Some(Value::Dword(1))));
        assert!(read_toggle(&reg, sug));
    }

    #[test]
    fn a_policy_locks_the_advertising_id_off() {
        let reg = Fake::default();
        let ad = find_toggle("adid").unwrap();
        reg.put(Hive::Hkcu, ad.key, "Enabled", Value::Dword(1));
        reg.put(Hive::Hklm, r"SOFTWARE\Policies\Microsoft\Windows\AdvertisingInfo", "DisabledByGroupPolicy", Value::Dword(1));
        assert!(toggle_locked(&reg, ad));
        assert!(!read_toggle(&reg, ad));
        assert!(write_toggle(&reg, ad, true).is_err());
    }

    #[test]
    fn safe_search_defaults_to_moderate() {
        let reg = Fake::default();
        assert_eq!(read_safe_search(&reg), SafeSearch::Moderate);
        for s in SafeSearch::ALL {
            write_safe_search(&reg, s).unwrap();
            assert_eq!(read_safe_search(&reg), s);
        }
    }

    #[test]
    fn managed_values_as_on_this_machine() {
        let reg = Fake::default();
        reg.put(Hive::Hklm, DATA_COLLECTION, "AllowTelemetry", Value::Dword(1));
        reg.put(Hive::Hklm, DATA_COLLECTION_POLICY, "DoNotShowFeedbackNotifications", Value::Dword(1));
        reg.put(Hive::Hklm, SYSTEM_POLICY, "PublishUserActivities", Value::Dword(0));
        let m = read_managed(&reg);
        assert_eq!(m.diagnostics, Some(("Required only".to_owned(), false)));
        assert_eq!(m.feedback, ("Never".to_owned(), true));
        assert_eq!(m.activity_policy, Some(false));
        assert!(!m.online_speech);
        reg.put(Hive::Hklm, DATA_COLLECTION_POLICY, "AllowTelemetry", Value::Dword(3));
        assert_eq!(read_managed(&reg).diagnostics, Some(("Optional".to_owned(), true)), "the policy wins");
    }

    #[test]
    fn feedback_frequencies() {
        assert_eq!(feedback_label(None, None), "Automatically (recommended)");
        assert_eq!(feedback_label(Some(0), None), "Never");
        assert_eq!(feedback_label(Some(1), Some(864_000_000_000)), "Once a day");
        assert_eq!(feedback_label(Some(1), Some(6_048_000_000_000)), "Once a week");
        assert_eq!(feedback_label(None, Some(100_000_000)), "Always");
    }

    /// Writes the real privacy values, reads them back, and puts every raw value back (deleting
    /// what was missing). Run by hand, outside any packaged app's registry view:
    /// `cargo test -p shellman-core privacy -- --ignored`.
    #[test]
    #[ignore]
    fn live_writes_take_and_restore() {
        use crate::registry::Real;
        let reg = Real;
        let mut saved: Vec<(String, String, Option<Value>)> = Vec::new();
        let ids = ["languages", "launches", "suggested", "settings-notifications", "cloud-msa", "cloud-aad", "history", "highlights"];
        for id in ids {
            let t = find_toggle(id).unwrap();
            for n in t.names {
                saved.push((t.key.into(), (*n).into(), reg.get(Hive::Hkcu, t.key, n)));
            }
        }
        saved.push((SEARCH.into(), "SafeSearchMode".into(), reg.get(Hive::Hkcu, SEARCH, "SafeSearchMode")));
        let before = read(&reg);
        for id in ids {
            let t = find_toggle(id).unwrap();
            write_toggle(&reg, t, !read_toggle(&reg, t)).unwrap();
        }
        let old_ss = read_safe_search(&reg);
        let new_ss = if old_ss == SafeSearch::Strict { SafeSearch::Off } else { SafeSearch::Strict };
        write_safe_search(&reg, new_ss).unwrap();
        let flipped = read(&reg);
        for (key, name, v) in &saved {
            match v {
                None => {
                    let _ = reg.delete(Hive::Hkcu, key, name);
                }
                Some(Value::Dword(d)) => reg.set_dword(Hive::Hkcu, key, name, *d).unwrap(),
                Some(Value::Text(t)) => reg.set_text(Hive::Hkcu, key, name, t).unwrap(),
                Some(other) => panic!("unexpected {other:?}"),
            }
        }
        let restored: Vec<Option<Value>> = saved.iter().map(|(k, n, _)| reg.get(Hive::Hkcu, k, n)).collect();
        println!("toggles before {:?}
flipped {:?}", before.toggles, flipped.toggles);
        for p in &before.permissions {
            println!("{}: {} ({} apps, {} desktop apps)", p.cap.name, p.summary(), p.apps.len(), p.desktop_apps.len());
        }
        for id in ids {
            assert_ne!(before.toggle(id).unwrap().0, flipped.toggle(id).unwrap().0, "{id}");
        }
        assert_eq!(flipped.safe_search, Some(new_ss));
        assert_eq!(restored, saved.iter().map(|s| s.2.clone()).collect::<Vec<_>>());
        assert_eq!(read(&reg), before);
    }

    #[test]
    fn filetime_dates() {
        // 2026-11-15 00:00 UTC.
        let ft = (1_794_700_800u64 + 11_644_473_600) * 10_000_000;
        assert_eq!(filetime_date(ft).as_deref(), Some("15 Nov 2026"));
        assert_eq!(filetime_date(0), None);
    }
}

/// The display names Windows has for these package families (missing ones are left out). Asks
/// the package manager; call it from a worker with COM set up.
pub fn package_names(families: &[String]) -> std::collections::HashMap<String, String> {
    use windows::core::HSTRING;
    use windows::Management::Deployment::PackageManager;
    let mut out = std::collections::HashMap::new();
    let Ok(manager) = PackageManager::new() else { return out };
    for family in families {
        let Ok(found) = manager.FindPackagesByUserSecurityIdPackageFamilyName(&HSTRING::new(), &HSTRING::from(family.as_str())) else { continue };
        let name = found.into_iter().find_map(|p| p.DisplayName().ok().map(|n| n.to_string()).filter(|n| !n.trim().is_empty() && !n.starts_with("ms-resource:")));
        if let Some(n) = name {
            out.insert(family.clone(), n);
        }
    }
    out
}
