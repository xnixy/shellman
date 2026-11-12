//! The launcher's data, saved in `%APPDATA%\shellman\launcher.json`: for now how often each app
//! was launched from it, which feeds its "most used" row. More launcher options go in here.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const NAME: &str = "launcher";

#[derive(Clone, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct LauncherSettings {
    /// Launch count by app id.
    pub counts: HashMap<String, u32>,
}

impl LauncherSettings {
    pub fn load() -> Self {
        crate::config::load(NAME)
    }

    pub fn save(&self) {
        crate::config::save(NAME, self);
    }

    pub fn count(&self, id: &str) -> u32 {
        self.counts.get(id).copied().unwrap_or(0)
    }

    /// Forget every launch.
    pub fn clear_history(&mut self) {
        self.counts.clear();
    }

    /// How many apps have been launched at least once.
    pub fn apps_used(&self) -> usize {
        self.counts.values().filter(|&&n| n > 0).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_and_round_trip() {
        let dir = std::env::temp_dir().join(format!("shellman-launcher-{}", std::process::id()));
        let path = dir.join("launcher.json");
        let mut s = LauncherSettings::default();
        s.counts.insert("a".into(), 3);
        s.counts.insert("b".into(), 0);
        assert_eq!((s.count("a"), s.count("zzz"), s.apps_used()), (3, 0, 1));
        crate::config::save_to(&path, &s);
        assert_eq!(crate::config::load_from::<LauncherSettings>(&path), s);
        s.clear_history();
        crate::config::save_to(&path, &s);
        assert_eq!(crate::config::load_from::<LauncherSettings>(&path), LauncherSettings::default());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_fields_default() {
        assert_eq!(serde_json::from_str::<LauncherSettings>("{}").unwrap(), LauncherSettings::default());
    }
}
