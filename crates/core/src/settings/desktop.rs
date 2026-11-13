//! The desktop's settings, saved in `%APPDATA%\shellman\desktop.json`: which style the desktop
//! has (the shelf, and later the widgets, or the wallpaper alone), and how the shelf is set up.
//! The shelf (`apps/shelf`) follows the file live; Settings › Desktop edits it.

use crate::calendar::CalendarRef;
use crate::weather::Units;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const NAME: &str = "desktop";

/// What sits on the wallpaper.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Style {
    /// The shelf on one edge and, later, a few widgets top right.
    #[default]
    ShelfWidgets,
    /// The wallpaper alone; everything starts from the right-click menu.
    Clean,
}

impl Style {
    pub const ALL: [Style; 2] = [Style::ShelfWidgets, Style::Clean];

    pub fn label(self) -> &'static str {
        match self {
            Style::ShelfWidgets => "Shelf + widgets",
            Style::Clean => "Clean",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Style::ShelfWidgets => "Files on the left edge, glanceable info top right",
            Style::Clean => "Wallpaper only. Everything starts from right-click",
        }
    }
}

/// Which edge of its monitor the shelf stands on.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Edge {
    #[default]
    Left,
    Right,
}

impl Edge {
    pub const ALL: [Edge; 2] = [Edge::Left, Edge::Right];

    pub fn label(self) -> &'static str {
        match self {
            Edge::Left => "Left",
            Edge::Right => "Right",
        }
    }
}

/// Which monitors have a shelf.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ShelfOn {
    /// The primary monitor only.
    Primary,
    /// Every monitor, each with its own shelf showing the same stacks.
    #[default]
    All,
}

impl ShelfOn {
    pub const ALL: [ShelfOn; 2] = [ShelfOn::Primary, ShelfOn::All];

    pub fn label(self) -> &'static str {
        match self {
            ShelfOn::Primary => "Primary display",
            ShelfOn::All => "All displays",
        }
    }
}

/// One stack on the shelf.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Stack {
    /// The user's Desktop folder (and the common Desktop when it has anything).
    Desktop,
    Downloads,
    /// Windows' recent items.
    Recent,
}

impl Stack {
    /// Every stack, in the order the shelf shows them.
    pub const ALL: [Stack; 3] = [Stack::Desktop, Stack::Downloads, Stack::Recent];

    pub fn label(self) -> &'static str {
        match self {
            Stack::Desktop => "Desktop",
            Stack::Downloads => "Downloads",
            Stack::Recent => "Recent",
        }
    }
}

/// Where files dropped on the shelf are moved to.
#[derive(Clone, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum DropTo {
    #[default]
    Desktop,
    Downloads,
    Documents,
    /// A folder of the user's choosing.
    Folder(PathBuf),
}

impl DropTo {
    /// The fixed choices; `Folder` is chosen with the picker.
    pub const KNOWN: [DropTo; 3] = [DropTo::Desktop, DropTo::Downloads, DropTo::Documents];

    pub fn label(&self) -> String {
        match self {
            DropTo::Desktop => "Desktop".into(),
            DropTo::Downloads => "Downloads".into(),
            DropTo::Documents => "Documents".into(),
            DropTo::Folder(p) => p
                .file_name()
                .map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned()),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ShelfSettings {
    /// Off hides the shelf in either style.
    pub show: bool,
    /// The primary monitor only, or every monitor.
    pub on: ShelfOn,
    pub edge: Edge,
    /// The stacks shown, each once. The shelf shows them in `Stack::ALL`'s order.
    pub stacks: Vec<Stack>,
    pub drop_to: DropTo,
}

impl Default for ShelfSettings {
    fn default() -> Self {
        Self {
            show: true,
            on: ShelfOn::All,
            edge: Edge::Left,
            stacks: Stack::ALL.to_vec(),
            drop_to: DropTo::Desktop,
        }
    }
}

/// The calendar account the Today widget reads. The password is not here: it lives in Windows
/// Credential Manager (`crate::calendar::CREDENTIAL`).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CalendarSettings {
    /// The CalDAV server; Fastmail's unless changed.
    pub server: String,
    pub username: String,
    /// The account's calendars as last found (by Settings' "Test connection" or the shelf).
    pub calendars: Vec<CalendarRef>,
    /// The URLs of calendars the user turned off; new calendars show.
    pub hidden: Vec<String>,
    /// When the password was last saved (UTC seconds), so the shelf tries a new one at once.
    pub saved: i64,
}

impl Default for CalendarSettings {
    fn default() -> Self {
        Self { server: crate::calendar::client::FASTMAIL.into(), username: String::new(), calendars: Vec::new(), hidden: Vec::new(), saved: 0 }
    }
}

impl CalendarSettings {
    pub fn shows(&self, href: &str) -> bool {
        !self.hidden.iter().any(|h| h == href)
    }
}

/// The weather in the Today widget: a place typed in Settings, found once by name and kept as
/// name and coordinates. No place, no weather.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WeatherSettings {
    pub place: String,
    pub lat: f64,
    pub lon: f64,
    pub units: Units,
}

impl WeatherSettings {
    pub fn is_set(&self) -> bool {
        !self.place.trim().is_empty() && self.lat.is_finite() && self.lon.is_finite() && (self.lat != 0.0 || self.lon != 0.0)
    }
}

/// The widgets of the Shelf + widgets style, top right of the desktop.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct WidgetSettings {
    pub show: bool,
    /// Date, weather and the next events.
    pub today: bool,
    /// Media from any app.
    pub now_playing: bool,
    /// The primary monitor only (the default), or every monitor.
    pub on: ShelfOn,
    pub calendar: CalendarSettings,
    pub weather: WeatherSettings,
}

impl Default for WidgetSettings {
    fn default() -> Self {
        Self {
            show: true,
            today: true,
            now_playing: true,
            on: ShelfOn::Primary,
            calendar: CalendarSettings::default(),
            weather: WeatherSettings::default(),
        }
    }
}

#[derive(Clone, Default, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DesktopSettings {
    pub style: Style,
    pub shelf: ShelfSettings,
    pub widgets: WidgetSettings,
}

impl DesktopSettings {
    pub fn load() -> Self {
        crate::config::load(NAME)
    }

    pub fn save(&self) {
        crate::config::save(NAME, self);
    }

    /// Whether the shelf is on screen at all: the style has one and it is not turned off.
    pub fn shelf_shown(&self) -> bool {
        self.style == Style::ShelfWidgets && self.shelf.show
    }

    /// Whether the widgets are on screen at all: the style has them, they are on, and at least
    /// one of them is.
    pub fn widgets_shown(&self) -> bool {
        self.style == Style::ShelfWidgets && self.widgets.show && (self.widgets.today || self.widgets.now_playing)
    }

    /// The stacks to show, in the shelf's order, each once.
    pub fn stacks(&self) -> Vec<Stack> {
        Stack::ALL.into_iter().filter(|s| self.shelf.stacks.contains(s)).collect()
    }

    /// Show `stack` if hidden, hide it if shown.
    pub fn toggle_stack(&mut self, stack: Stack) {
        if self.shelf.stacks.contains(&stack) {
            self.shelf.stacks.retain(|s| *s != stack);
        } else {
            self.shelf.stacks.push(stack);
        }
        self.shelf.stacks = self.stacks();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_show_the_shelf_on_the_left_with_every_stack() {
        let s = DesktopSettings::default();
        assert_eq!(s.style, Style::ShelfWidgets);
        assert!(s.shelf_shown());
        assert_eq!(s.shelf.edge, Edge::Left);
        assert_eq!(s.shelf.on, ShelfOn::All);
        assert_eq!(s.stacks(), Stack::ALL);
        assert_eq!(s.shelf.drop_to, DropTo::Desktop);
        assert_eq!(serde_json::from_str::<DesktopSettings>("{}").unwrap(), s);
        let missing = crate::config::load_from::<DesktopSettings>(std::path::Path::new("Z:\\no\\desktop.json"));
        assert_eq!(missing, s);
    }

    #[test]
    fn clean_or_off_hides_the_shelf() {
        let mut s = DesktopSettings { style: Style::Clean, ..Default::default() };
        assert!(!s.shelf_shown());
        s.style = Style::ShelfWidgets;
        s.shelf.show = false;
        assert!(!s.shelf_shown());
    }

    #[test]
    fn partial_files_keep_the_other_defaults() {
        let s: DesktopSettings = serde_json::from_str(r#"{ "style": "Clean", "shelf": { "edge": "Right" } }"#).unwrap();
        assert_eq!(s.style, Style::Clean);
        assert_eq!(s.shelf.edge, Edge::Right);
        assert!(s.shelf.show);
        assert_eq!(s.stacks(), Stack::ALL);
        assert!(s.widgets.today);
        // A file from before the setting existed gets a shelf on every monitor.
        assert_eq!(s.shelf.on, ShelfOn::All);
        let primary: DesktopSettings = serde_json::from_str(r#"{ "shelf": { "on": "Primary" } }"#).unwrap();
        assert_eq!(primary.shelf.on, ShelfOn::Primary);
        let folder: DesktopSettings =
            serde_json::from_str(r#"{ "shelf": { "drop_to": { "Folder": "D:\\Inbox" } } }"#).unwrap();
        assert_eq!(folder.shelf.drop_to, DropTo::Folder(PathBuf::from("D:\\Inbox")));
        assert_eq!(folder.shelf.drop_to.label(), "Inbox");
    }

    #[test]
    fn stacks_keep_the_shelf_order_and_appear_once() {
        let mut s = DesktopSettings::default();
        s.shelf.stacks = vec![Stack::Recent, Stack::Desktop, Stack::Recent];
        assert_eq!(s.stacks(), [Stack::Desktop, Stack::Recent]);
        s.toggle_stack(Stack::Downloads);
        assert_eq!(s.stacks(), Stack::ALL);
        assert_eq!(s.shelf.stacks, Stack::ALL);
        s.toggle_stack(Stack::Desktop);
        assert_eq!(s.stacks(), [Stack::Downloads, Stack::Recent]);
    }

    #[test]
    fn widgets_default_to_the_primary_with_fastmail_and_old_files_keep_working() {
        let s = DesktopSettings::default();
        assert!(s.widgets_shown());
        assert_eq!(s.widgets.on, ShelfOn::Primary);
        assert_eq!(s.widgets.calendar.server, "https://caldav.fastmail.com/");
        assert!(!s.widgets.weather.is_set());
        // A file from stage 1 (only the three switches).
        let old: DesktopSettings = serde_json::from_str(r#"{ "widgets": { "show": true, "today": false, "now_playing": true } }"#).unwrap();
        assert!(!old.widgets.today && old.widgets.now_playing);
        assert_eq!(old.widgets.on, ShelfOn::Primary);
        assert_eq!(old.widgets.calendar, CalendarSettings::default());
        let mut off = DesktopSettings { style: Style::Clean, ..Default::default() };
        assert!(!off.widgets_shown());
        off.style = Style::ShelfWidgets;
        off.widgets.today = false;
        off.widgets.now_playing = false;
        assert!(!off.widgets_shown());
    }

    #[test]
    fn widget_settings_round_trip_without_the_password() {
        let dir = std::env::temp_dir().join(format!("shellman-widgets-{}", std::process::id()));
        let path = dir.join("desktop.json");
        let mut s = DesktopSettings::default();
        s.widgets.on = ShelfOn::All;
        s.widgets.calendar.username = "someone@fastmail.com".into();
        s.widgets.calendar.calendars = vec![CalendarRef { href: "https://x/cal/".into(), name: "Personal".into(), color: Some("#3A87AD".into()) }];
        s.widgets.calendar.hidden = vec!["https://x/other/".into()];
        s.widgets.weather = WeatherSettings { place: "London".into(), lat: 51.50853, lon: -0.12574, units: Units::Imperial };
        crate::config::save_to(&path, &s);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.to_ascii_lowercase().contains("password"));
        let back = crate::config::load_from::<DesktopSettings>(&path);
        assert_eq!(back, s);
        assert!(back.widgets.weather.is_set());
        assert!(back.widgets.calendar.shows("https://x/cal/") && !back.widgets.calendar.shows("https://x/other/"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn file_round_trip() {
        let dir = std::env::temp_dir().join(format!("shellman-desktop-{}", std::process::id()));
        let path = dir.join("desktop.json");
        let mut s = DesktopSettings { style: Style::Clean, ..Default::default() };
        s.shelf.drop_to = DropTo::Folder(PathBuf::from("C:\\x"));
        crate::config::save_to(&path, &s);
        assert_eq!(crate::config::load_from::<DesktopSettings>(&path), s);
        let _ = std::fs::remove_dir_all(dir);
    }
}
