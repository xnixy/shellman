//! Shell-wide settings, saved in `%APPDATA%\shellman\shell.json`.

use serde::{Deserialize, Serialize};

pub const NAME: &str = "shell";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ShellSettings {
    /// Run Windows' own host for modern apps (ShellAppRuntime) while shellman is the shell, so
    /// Settings, Calculator, the Store and `ms-settings:` links open. See `app_host`.
    pub modern_apps: bool,
}

impl Default for ShellSettings {
    fn default() -> Self {
        Self { modern_apps: true }
    }
}

impl ShellSettings {
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
    fn modern_apps_default_on() {
        assert!(ShellSettings::default().modern_apps);
        assert!(serde_json::from_str::<ShellSettings>("{}").unwrap().modern_apps);
        assert!(!serde_json::from_str::<ShellSettings>(r#"{ "modern_apps": false }"#).unwrap().modern_apps);
    }
}
