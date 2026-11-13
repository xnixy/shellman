//! The dock's settings, saved in `%APPDATA%\shellman\dock.json`: the pinned apps and the icon
//! size. A pin keeps what is needed to match a running window (its app id) and to start the app
//! when nothing is running (its program, or its entry in the Apps folder for Store apps).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const NAME: &str = "dock";

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Pin {
    pub app_id: String,
    #[serde(default)]
    pub exe: Option<PathBuf>,
}

impl Pin {
    /// Whether a window with this app id belongs to the pin. Ids are compared without case, since
    /// paths are and AppUserModelIDs are written both ways.
    pub fn matches(&self, app_id: &str) -> bool {
        self.app_id.eq_ignore_ascii_case(app_id)
    }
}

/// How big the dock's icons are.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum IconSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl IconSize {
    pub const ALL: [IconSize; 3] = [IconSize::Small, IconSize::Medium, IconSize::Large];

    pub fn label(self) -> &'static str {
        match self {
            IconSize::Small => "Small",
            IconSize::Medium => "Medium",
            IconSize::Large => "Large",
        }
    }

    /// The icon's side, in points. The dock's spacing and height follow from it (see the bar's
    /// `dock::metrics`).
    pub fn icon(self) -> f32 {
        match self {
            IconSize::Small => 40.0,
            IconSize::Medium => 48.0,
            IconSize::Large => 56.0,
        }
    }
}

#[derive(Clone, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DockSettings {
    pub pins: Vec<Pin>,
    pub icon_size: IconSize,
}

impl DockSettings {
    pub fn load() -> Self {
        crate::config::load(NAME)
    }

    /// The saved settings, or why they could not be read (keep what you have then).
    pub fn try_load() -> Result<Self, crate::config::LoadError> {
        crate::config::try_load(NAME)
    }

    pub fn save(&self) {
        crate::config::save(NAME, self);
    }

    pub fn contains(&self, app_id: &str) -> bool {
        self.pins.iter().any(|p| p.matches(app_id))
    }

    /// Add a pin at the end, unless the app is already pinned.
    pub fn add(&mut self, app_id: &str, exe: Option<&Path>) {
        if !app_id.is_empty() && !self.contains(app_id) {
            self.pins.push(Pin {
                app_id: app_id.to_owned(),
                exe: exe.map(Path::to_path_buf),
            });
        }
    }

    pub fn remove(&mut self, app_id: &str) {
        self.pins.retain(|p| !p.matches(app_id));
    }

    /// Move the pin at `index` one place earlier (`down` false) or later. False at the ends.
    pub fn shift(&mut self, index: usize, down: bool) -> bool {
        let other = if down { index + 1 } else { index.wrapping_sub(1) };
        if index >= self.pins.len() || other >= self.pins.len() {
            return false;
        }
        self.pins.swap(index, other);
        true
    }
}

/// Store apps (package family and app, joined by `!`) can only be started through the Apps folder.
fn is_packaged(app_id: &str) -> bool {
    !app_id.contains('\\') && app_id.contains('!')
}

fn apps_folder(app_id: &str) -> PathBuf {
    PathBuf::from(format!("shell:AppsFolder\\{app_id}"))
}

/// What to open to start the app: its program, or its entry in the Apps folder.
pub fn launch_target(app_id: &str, exe: Option<&Path>) -> Option<PathBuf> {
    if is_packaged(app_id) {
        return Some(apps_folder(app_id));
    }
    icon_source(app_id, exe)
}

/// Where its icon comes from: the program file's own icon, as Explorer's taskbar shows it.
/// Only without one, the app's entry in the Apps folder (whose Store icons come on a plate).
pub fn icon_source(app_id: &str, exe: Option<&Path>) -> Option<PathBuf> {
    let is_path = app_id.contains('\\');
    exe.map(Path::to_path_buf)
        .or_else(|| (!is_path && !app_id.is_empty()).then(|| apps_folder(app_id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_remove_and_match_ignore_case() {
        let mut pins = DockSettings::default();
        pins.add("C:\\Apps\\Foo.exe", Some(Path::new("C:\\Apps\\Foo.exe")));
        pins.add("c:\\apps\\foo.exe", None);
        pins.add("", None);
        assert_eq!(pins.pins.len(), 1);
        assert!(pins.contains("c:\\apps\\FOO.exe"));
        pins.remove("C:\\APPS\\foo.exe");
        assert!(pins.pins.is_empty());
    }

    #[test]
    fn shifting_stops_at_the_ends() {
        let mut s = DockSettings::default();
        for id in ["a", "b", "c"] {
            s.add(id, None);
        }
        let order = |s: &DockSettings| s.pins.iter().map(|p| p.app_id.clone()).collect::<String>();
        assert!(!s.shift(0, false));
        assert!(!s.shift(2, true));
        assert!(!s.shift(7, false));
        assert!(s.shift(0, true));
        assert_eq!(order(&s), "bac");
        assert!(s.shift(2, false));
        assert_eq!(order(&s), "bca");
    }

    #[test]
    fn launch_targets() {
        let exe = Path::new("C:\\a\\b.exe");
        assert_eq!(launch_target("c:\\a\\b.exe", Some(exe)), Some(exe.to_path_buf()));
        assert_eq!(
            launch_target("Pkg_123!App", Some(Path::new("C:\\host.exe"))),
            Some(PathBuf::from("shell:AppsFolder\\Pkg_123!App"))
        );
        assert_eq!(launch_target("Chrome", None), Some(PathBuf::from("shell:AppsFolder\\Chrome")));
        assert_eq!(launch_target("c:\\gone.exe", None), None);
    }

    #[test]
    fn file_round_trip() {
        let dir = std::env::temp_dir().join(format!("shellman-dock-{}", std::process::id()));
        let path = dir.join("dock.json");
        let mut s = DockSettings::default();
        s.add("x", Some(Path::new("C:\\x.exe")));
        s.icon_size = IconSize::Large;
        crate::config::save_to(&path, &s);
        assert_eq!(crate::config::load_from::<DockSettings>(&path), s);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn old_files_without_new_fields_still_load() {
        // What the bar wrote before the icon size existed.
        let old = r#"{ "pins": [ { "app_id": "a", "exe": null }, { "app_id": "b" } ] }"#;
        let s: DockSettings = serde_json::from_str(old).unwrap();
        assert_eq!(s.pins.len(), 2);
        assert_eq!(s.icon_size, IconSize::Medium);
        // And an empty object, or a missing file, is all defaults.
        assert_eq!(serde_json::from_str::<DockSettings>("{}").unwrap(), DockSettings::default());
        let missing = crate::config::load_from::<DockSettings>(Path::new("Z:\\no\\such\\dock.json"));
        assert_eq!(missing, DockSettings::default());
    }

    #[test]
    fn sizes_grow_in_steps() {
        let [s, m, l] = IconSize::ALL.map(IconSize::icon);
        assert_eq!((s, m, l), (40.0, 48.0, 56.0));
    }
}
