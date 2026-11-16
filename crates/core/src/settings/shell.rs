//! Shell-wide settings, saved in `%APPDATA%\shellman\shell.json`.

use serde::{Deserialize, Serialize};

pub const NAME: &str = "shell";

/// When Windows' host for modern apps (Settings, Calculator, the Store, `ms-settings:` links)
/// runs while shellman is the shell. See `app_host`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum WindowsApps {
    /// The first time one of them is opened, for the rest of that sign-in (the default).
    WhenNeeded,
    /// From every sign-in on.
    Always,
    /// Not at all: those apps and links do not open.
    Never,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ShellSettings {
    /// Run Windows' own host for modern apps (ShellAppRuntime) while shellman is the shell, so
    /// Settings, Calculator, the Store and `ms-settings:` links open. See `app_host`. Off unless
    /// the user asks for it: it is Microsoft's process, and it runs startup apps and Windows' own
    /// Start menu host.
    pub modern_apps: bool,
    /// The choice that replaced `modern_apps` (0.1.3). Unset in older files: `modern_apps` on
    /// means `Always`, off means `WhenNeeded`. Use [`ShellSettings::windows_apps`].
    pub windows_apps: Option<WindowsApps>,
    /// Install new shellman releases by themselves (`shellman-update --auto`, started by the
    /// session about 5 minutes after sign-in and then daily). On unless the user turns it off.
    pub auto_update: bool,
}

impl Default for ShellSettings {
    fn default() -> Self {
        Self { modern_apps: false, windows_apps: None, auto_update: true }
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

    /// When the host runs, from the new choice or, in an older file, the old switch.
    pub fn windows_apps(&self) -> WindowsApps {
        self.windows_apps.unwrap_or(if self.modern_apps { WindowsApps::Always } else { WindowsApps::WhenNeeded })
    }

    /// Choose; the old switch follows (on only for `Always`), so an older shellman reads it too.
    pub fn set_windows_apps(&mut self, choice: WindowsApps) {
        self.windows_apps = Some(choice);
        self.modern_apps = choice == WindowsApps::Always;
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

    #[test]
    fn the_old_switch_reads_as_the_new_choice() {
        let read = |j: &str| serde_json::from_str::<ShellSettings>(j).unwrap().windows_apps();
        assert_eq!(read("{}"), WindowsApps::WhenNeeded);
        assert_eq!(read(r#"{ "modern_apps": true }"#), WindowsApps::Always);
        assert_eq!(read(r#"{ "modern_apps": false }"#), WindowsApps::WhenNeeded);
        assert_eq!(read(r#"{ "modern_apps": true, "windows_apps": "Never" }"#), WindowsApps::Never);
        let mut s = ShellSettings::default();
        s.set_windows_apps(WindowsApps::Always);
        assert!(s.modern_apps);
        s.set_windows_apps(WindowsApps::Never);
        assert!(!s.modern_apps && s.windows_apps() == WindowsApps::Never);
    }

    #[test]
    fn auto_update_default_on() {
        assert!(ShellSettings::default().auto_update);
        // A file saved before the setting existed keeps updating on.
        assert!(serde_json::from_str::<ShellSettings>(r#"{ "modern_apps": true }"#).unwrap().auto_update);
        assert!(!serde_json::from_str::<ShellSettings>(r#"{ "auto_update": false }"#).unwrap().auto_update);
    }
}
