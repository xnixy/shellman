//! The bar's settings, saved in `%APPDATA%\shellman\bar.json`: the top bar's clock, which
//! monitors get a dock, and whether notifications pop up.

use serde::{Deserialize, Serialize};

pub const NAME: &str = "bar";

/// 12 or 24 hour time, or whatever Windows is set to.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum HourCycle {
    #[default]
    System,
    H12,
    H24,
}

impl HourCycle {
    pub const ALL: [HourCycle; 3] = [HourCycle::System, HourCycle::H12, HourCycle::H24];

    pub fn label(self) -> &'static str {
        match self {
            HourCycle::System => "Windows setting",
            HourCycle::H12 => "12 hour",
            HourCycle::H24 => "24 hour",
        }
    }

    /// The Windows time pattern for it, or none to use the user's own.
    pub fn pattern(self) -> Option<&'static str> {
        match self {
            HourCycle::System => None,
            HourCycle::H12 => Some("h:mm tt"),
            HourCycle::H24 => Some("HH:mm"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ClockSettings {
    pub hour_cycle: HourCycle,
    pub show_date: bool,
    /// The weekday before the date ("Thu 12 Nov").
    pub show_weekday: bool,
    /// Seconds in the time (the bar then repaints every second).
    pub show_seconds: bool,
}

impl Default for ClockSettings {
    fn default() -> Self {
        Self {
            hour_cycle: HourCycle::default(),
            show_date: true,
            show_weekday: true,
            show_seconds: false,
        }
    }
}

impl HourCycle {
    /// The time pattern with seconds, or none for the user's own long time format.
    pub fn pattern_with_seconds(self) -> Option<&'static str> {
        match self {
            HourCycle::System => None,
            HourCycle::H12 => Some("h:mm:ss tt"),
            HourCycle::H24 => Some("HH:mm:ss"),
        }
    }
}

/// The top bar's "now" pill: whether it shows, and what it may show.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct NowSettings {
    pub on: bool,
    /// An event starting within 15 minutes or under way (needs a calendar).
    pub events: bool,
    /// What is playing.
    pub media: bool,
}

impl Default for NowSettings {
    fn default() -> Self {
        Self { on: true, events: true, media: true }
    }
}

/// Which display notifications pop up on.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToastScreen {
    /// The display with the window in front.
    #[default]
    Focused,
    /// The main display.
    Primary,
}

impl ToastScreen {
    pub const ALL: [ToastScreen; 2] = [ToastScreen::Focused, ToastScreen::Primary];

    pub fn label(self) -> &'static str {
        match self {
            ToastScreen::Focused => "Display with the window in front",
            ToastScreen::Primary => "Main display",
        }
    }
}

/// Week numbers in the calendar.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WeekNumbers {
    /// Shown where the region counts weeks the ISO way (first week with four days).
    #[default]
    Auto,
    On,
    Off,
}

impl WeekNumbers {
    pub const ALL: [WeekNumbers; 3] = [WeekNumbers::Auto, WeekNumbers::On, WeekNumbers::Off];

    pub fn label(self) -> &'static str {
        match self {
            WeekNumbers::Auto => "As the region does",
            WeekNumbers::On => "Show",
            WeekNumbers::Off => "Hide",
        }
    }

    /// Whether they show, for a region whose `iFirstWeekOfYear` is `first_week`.
    pub fn shown(self, first_week: u32) -> bool {
        match self {
            WeekNumbers::Auto => first_week == 2,
            WeekNumbers::On => true,
            WeekNumbers::Off => false,
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct BarSettings {
    pub clock: ClockSettings,
    pub docks: Docks,
    pub tray: TrayMonitors,
    /// Do not disturb: notifications are counted and listed, but do not pop up.
    pub do_not_disturb: bool,
    /// Do not disturb ends by itself at this time (UTC seconds), when set.
    pub dnd_until: Option<i64>,
    pub now: NowSettings,
    pub toasts_on: ToastScreen,
    pub week_numbers: WeekNumbers,
}

impl BarSettings {
    /// Whether do-not-disturb is on at `now` (UTC seconds).
    pub fn dnd_at(&self, now: i64) -> bool {
        self.do_not_disturb && self.dnd_until.is_none_or(|until| now < until)
    }
}

/// Which monitors get a dock. Every monitor always gets a top bar.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Docks {
    /// A dock on every monitor, each showing all apps.
    #[default]
    All,
    /// One dock, on the primary monitor.
    Primary,
}

impl Docks {
    pub const ALL: [Docks; 2] = [Docks::All, Docks::Primary];

    pub fn label(self) -> &'static str {
        match self {
            Docks::All => "Every monitor",
            Docks::Primary => "Main monitor only",
        }
    }
}

/// Which monitors' top bars show the tray.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrayMonitors {
    /// The tray on every monitor's top bar.
    #[default]
    All,
    /// The tray on the primary monitor's top bar only.
    Primary,
}

impl TrayMonitors {
    pub const ALL: [TrayMonitors; 2] = [TrayMonitors::All, TrayMonitors::Primary];

    pub fn label(self) -> &'static str {
        match self {
            TrayMonitors::All => "Every monitor",
            TrayMonitors::Primary => "Main monitor only",
        }
    }
}

impl BarSettings {
    pub fn load() -> Self {
        crate::config::load(NAME)
    }

    pub fn save(&self) {
        crate::config::save(NAME, self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_old_clock() {
        let s = BarSettings::default();
        assert_eq!(s.clock.hour_cycle, HourCycle::System);
        assert!(s.clock.show_date);
    }

    #[test]
    fn round_trip_and_partial_files() {
        let dir = std::env::temp_dir().join(format!("shellman-bar-{}", std::process::id()));
        let path = dir.join("bar.json");
        let mut s = BarSettings::default();
        s.clock.hour_cycle = HourCycle::H12;
        s.clock.show_date = false;
        crate::config::save_to(&path, &s);
        assert_eq!(crate::config::load_from::<BarSettings>(&path), s);
        let _ = std::fs::remove_dir_all(dir);

        // A file with only some fields keeps the defaults for the rest.
        let partial: BarSettings = serde_json::from_str(r#"{ "clock": { "hour_cycle": "H24" } }"#).unwrap();
        assert_eq!(partial.clock.hour_cycle, HourCycle::H24);
        assert!(partial.clock.show_date);
        assert_eq!(serde_json::from_str::<BarSettings>("{}").unwrap(), BarSettings::default());
    }

    #[test]
    fn tray_defaults_to_every_monitor() {
        let read = |json: &str| serde_json::from_str::<BarSettings>(json).unwrap().tray;
        assert_eq!(read("{}"), TrayMonitors::All);
        assert_eq!(read(r#"{ "tray": "primary" }"#), TrayMonitors::Primary);
        assert_eq!(read(r#"{ "tray": "all" }"#), TrayMonitors::All);
    }

    #[test]
    fn do_not_disturb_is_off_unless_saved_on() {
        let read = |json: &str| serde_json::from_str::<BarSettings>(json).unwrap().do_not_disturb;
        assert!(!read("{}"));
        assert!(read(r#"{ "do_not_disturb": true }"#));
    }

    #[test]
    fn new_fields_default_sensibly() {
        let s: BarSettings = serde_json::from_str("{}").unwrap();
        assert!(s.now.on && s.now.events && s.now.media);
        assert_eq!(s.toasts_on, ToastScreen::Focused);
        assert_eq!(s.week_numbers, WeekNumbers::Auto);
        assert!(s.clock.show_weekday && !s.clock.show_seconds);
        let s: BarSettings = serde_json::from_str(r#"{ "toasts_on": "primary", "now": { "media": false } }"#).unwrap();
        assert_eq!(s.toasts_on, ToastScreen::Primary);
        assert!(s.now.on && !s.now.media);
    }

    #[test]
    fn do_not_disturb_until_a_time() {
        let mut s = BarSettings { do_not_disturb: true, ..Default::default() };
        assert!(s.dnd_at(100));
        s.dnd_until = Some(200);
        assert!(s.dnd_at(199) && !s.dnd_at(200));
        s.do_not_disturb = false;
        assert!(!s.dnd_at(0));
    }

    #[test]
    fn week_numbers_follow_the_region_unless_set() {
        assert!(WeekNumbers::Auto.shown(2) && !WeekNumbers::Auto.shown(0));
        assert!(WeekNumbers::On.shown(0) && !WeekNumbers::Off.shown(2));
    }

    #[test]
    fn docks_default_to_every_monitor() {
        let read = |json: &str| serde_json::from_str::<BarSettings>(json).unwrap().docks;
        assert_eq!(read("{}"), Docks::All);
        assert_eq!(read(r#"{ "docks": "primary" }"#), Docks::Primary);
        assert_eq!(read(r#"{ "docks": "all" }"#), Docks::All);
    }
}
