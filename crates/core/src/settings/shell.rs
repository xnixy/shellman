//! Shell-wide settings, saved in `%APPDATA%\shellman\shell.json`.

use serde::{Deserialize, Serialize};

pub const NAME: &str = "shell";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ShellSettings {
    /// Run Windows' own host for modern apps (ShellAppRuntime) while shellman is the shell, so
    /// Settings, Calculator, the Store and `ms-settings:` links open. See `app_host`. Off unless
    /// the user asks for it: it is Microsoft's process, and it runs startup apps and Windows' own
    /// Start menu host.
    pub modern_apps: bool,
}

impl Default for ShellSettings {
    fn default() -> Self {
        Self { modern_apps: false }
    }
}

impl ShellSettings {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modern_apps_default_off() {
        assert!(!ShellSettings::default().modern_apps);
        assert!(!serde_json::from_str::<ShellSettings>("{}").unwrap().modern_apps);
        assert!(serde_json::from_str::<ShellSettings>(r#"{ "modern_apps": true }"#).unwrap().modern_apps);
    }
}
