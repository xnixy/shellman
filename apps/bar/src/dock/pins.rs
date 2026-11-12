//! The pinned apps, saved in `%APPDATA%\shellman\dock.json`. A pin keeps what is needed to match
//! a running window (its app id) and to start the app when nothing is running (its program, or
//! its entry in the Apps folder for Store apps).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const NAME: &str = "dock";

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

#[derive(Clone, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Pins {
    pub pins: Vec<Pin>,
}

impl Pins {
    pub fn load() -> Self {
        shellman_core::config::load(NAME)
    }

    pub fn save(&self) {
        shellman_core::config::save(NAME, self);
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
        let mut pins = Pins::default();
        pins.add("C:\\Apps\\Foo.exe", Some(Path::new("C:\\Apps\\Foo.exe")));
        pins.add("c:\\apps\\foo.exe", None);
        pins.add("", None);
        assert_eq!(pins.pins.len(), 1);
        assert!(pins.contains("c:\\apps\\FOO.exe"));
        pins.remove("C:\\APPS\\foo.exe");
        assert!(pins.pins.is_empty());
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
        let dir = std::env::temp_dir().join(format!("shellman-pins-{}", std::process::id()));
        let path = dir.join("dock.json");
        let mut pins = Pins::default();
        pins.add("x", Some(Path::new("C:\\x.exe")));
        shellman_core::config::save_to(&path, &pins);
        assert_eq!(shellman_core::config::load_from::<Pins>(&path), pins);
        let _ = std::fs::remove_dir_all(dir);
    }
}
