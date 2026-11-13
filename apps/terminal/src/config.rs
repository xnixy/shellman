//! The terminal's own settings, `terminal.json` in shellman's config folder (`%APPDATA%\shellman`,
//! or `SHELLMAN_CONFIG_DIR`). Written with its defaults the first time, so there is a file to
//! edit; a Settings page comes later. `default_terminal` / `custom_terminal` are read by every
//! app (`shellman_core::terminal`), not just this one.

use serde::{Deserialize, Serialize};

const NAME: &str = "terminal";

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// The profile a new tab runs, by name ("Windows PowerShell", "PowerShell", "Debian"...).
    pub default_profile: String,
    /// The grid's font family; empty for Cascadia Mono (then Cascadia Code, then Consolas).
    pub font_family: String,
    /// The grid's font size in points.
    pub font_size: f32,
    /// Lines of history kept above the screen.
    pub scrollback: usize,
    /// Copy a selection as soon as it is made.
    pub copy_on_select: bool,
    /// What starting the terminal again does while a window is open: a new window (as Windows
    /// Terminal does), or a new tab in the open one.
    pub new_instance: NewInstance,
    /// Which terminal shellman's "Open Terminal here" opens (read by `shellman_core::terminal`;
    /// kept here so saving this file keeps it).
    #[serde(flatten)]
    pub open_with: shellman_core::terminal::Choice,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NewInstance {
    #[default]
    Window,
    Tab,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_profile: "Windows PowerShell".into(),
            font_family: String::new(),
            font_size: 12.0,
            scrollback: 10_000,
            copy_on_select: false,
            new_instance: NewInstance::Window,
            open_with: Default::default(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = shellman_core::config::config_dir().map(|d| d.join(format!("{NAME}.json")));
        match path {
            Some(p) if p.exists() => shellman_core::config::load::<Config>(NAME).sane(),
            _ => {
                let c = Config::default();
                shellman_core::config::save(NAME, &c);
                c
            }
        }
    }

    /// The settings without writing the file when it is missing (a window COM started for a
    /// handed-over console writes nothing).
    pub fn read() -> Self {
        shellman_core::config::load::<Config>(NAME).sane()
    }

    /// Values kept within what works.
    fn sane(mut self) -> Self {
        if !self.font_size.is_finite() {
            self.font_size = 12.0;
        }
        self.font_size = self.font_size.clamp(MIN_PT, MAX_PT);
        self.scrollback = self.scrollback.min(1_000_000);
        self
    }
}

/// The font sizes zoom goes between, in points.
pub const MIN_PT: f32 = 5.0;
pub const MAX_PT: f32 = 72.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_take_defaults_and_odd_values_are_held() {
        let c: Config = serde_json::from_str(r#"{"font_size": 400, "copy_on_select": true}"#).unwrap();
        let c = c.sane();
        assert_eq!(c.font_size, MAX_PT);
        assert!(c.copy_on_select);
        assert_eq!(c.scrollback, 10_000);
        assert_eq!(c.default_profile, "Windows PowerShell");
        assert_eq!(c.new_instance, NewInstance::Window);
    }

    #[test]
    fn the_terminal_choice_sits_beside_the_rest() {
        let c: Config = serde_json::from_str(r#"{"new_instance": "tab", "default_terminal": "windows-terminal"}"#).unwrap();
        assert_eq!(c.new_instance, NewInstance::Tab);
        assert_eq!(c.open_with.default_terminal, shellman_core::terminal::Which::WindowsTerminal);
        let v = serde_json::to_value(Config::default()).unwrap();
        assert_eq!((v["default_terminal"].as_str(), v["new_instance"].as_str()), (Some("shellman"), Some("window")));
    }
}
