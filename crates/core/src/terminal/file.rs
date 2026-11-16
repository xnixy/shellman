//! `terminal.json` as Settings edits it: read whole, the fields it shows changed, written back
//! whole, so fields it does not know (newer ones, or ones only the terminal reads) are kept. The
//! terminal reads the same file (its `Config`) and follows changes while it runs.

use serde_json::{Map, Value};
use std::path::Path;

/// The defaults the terminal uses for a missing field (its `Config::default`).
pub const DEFAULT_PROFILE: &str = "Windows PowerShell";
pub const DEFAULT_FONT_PT: f32 = 12.0;
pub const DEFAULT_SCROLLBACK: u64 = 10_000;
/// The font sizes the terminal takes, in points.
pub const MIN_PT: f32 = 5.0;
pub const MAX_PT: f32 = 72.0;
pub const MAX_SCROLLBACK: u64 = 1_000_000;

#[derive(Clone, Default, PartialEq, Debug)]
pub struct File {
    pub raw: Map<String, Value>,
}

impl File {
    pub fn load() -> Self {
        Self { raw: crate::config::load(super::NAME) }
    }

    pub fn load_from(path: &Path) -> Self {
        Self { raw: crate::config::load_from(path) }
    }

    pub fn save(&self) {
        crate::config::save(super::NAME, &self.raw);
    }

    pub fn save_to(&self, path: &Path) {
        crate::config::save_to(path, &self.raw);
    }

    pub fn text(&self, key: &str, default: &str) -> String {
        self.raw.get(key).and_then(Value::as_str).unwrap_or(default).to_owned()
    }

    pub fn number(&self, key: &str, default: f64) -> f64 {
        self.raw.get(key).and_then(Value::as_f64).unwrap_or(default)
    }

    pub fn flag(&self, key: &str, default: bool) -> bool {
        self.raw.get(key).and_then(Value::as_bool).unwrap_or(default)
    }

    pub fn set(&mut self, key: &str, value: impl Into<Value>) {
        self.raw.insert(key.to_owned(), value.into());
    }

    pub fn default_profile(&self) -> String {
        self.text("default_profile", DEFAULT_PROFILE)
    }

    /// Empty for the terminal's own choice (Cascadia Mono, then Cascadia Code, then Consolas).
    pub fn font_family(&self) -> String {
        self.text("font_family", "")
    }

    pub fn font_size(&self) -> f32 {
        (self.number("font_size", DEFAULT_FONT_PT as f64) as f32).clamp(MIN_PT, MAX_PT)
    }

    /// The 16 colours' scheme (`terminal.json`'s `color_scheme`, one of [`COLOR_SCHEMES`]).
    pub fn color_scheme(&self) -> String {
        let name = self.text("color_scheme", COLOR_SCHEMES[0].0);
        COLOR_SCHEMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(name.trim())).map_or(COLOR_SCHEMES[0].0, |(n, _)| n).to_owned()
    }

    pub fn scrollback(&self) -> u64 {
        (self.number("scrollback", DEFAULT_SCROLLBACK as f64).max(0.0) as u64).min(MAX_SCROLLBACK)
    }

    pub fn copy_on_select(&self) -> bool {
        self.flag("copy_on_select", false)
    }

    /// `"window"` or `"tab"`.
    pub fn new_instance(&self) -> String {
        self.text("new_instance", "window")
    }

    /// The terminal "Open Terminal here" uses, and the custom command line.
    pub fn choice(&self) -> super::Choice {
        serde_json::from_value(Value::Object(self.raw.clone())).unwrap_or_default()
    }

    pub fn set_choice(&mut self, choice: &super::Choice) {
        if let Ok(Value::Object(m)) = serde_json::to_value(choice) {
            for (k, v) in m {
                self.raw.insert(k, v);
            }
        }
    }
}

/// The terminal's colour schemes: the name `terminal.json` keeps, and what Settings calls it.
/// The terminal's `palette::Scheme` has the same names (tested there).
pub const COLOR_SCHEMES: [(&str, &str); 4] = [
    ("shellman", "shellman (made for the glass)"),
    ("campbell", "Campbell"),
    ("one-half", "One Half"),
    ("tango", "Tango"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::{Choice, Which};

    #[test]
    fn unknown_fields_survive_an_edit() {
        let dir = std::env::temp_dir().join(format!("shellman-termfile-{}", std::process::id()));
        let path = dir.join("terminal.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, r#"{"font_size": 14, "some_future_field": {"a": [1, 2]}, "default_terminal": "custom", "custom_terminal": "wezterm"}"#).unwrap();
        let mut f = File::load_from(&path);
        assert_eq!(f.font_size(), 14.0);
        assert_eq!(f.choice(), Choice { default_terminal: Which::Custom, custom_terminal: "wezterm".into() });
        f.set("copy_on_select", true);
        f.set_choice(&Choice { default_terminal: Which::WindowsTerminal, custom_terminal: "wezterm".into() });
        f.save_to(&path);
        let v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(v["some_future_field"]["a"][1], 2);
        assert_eq!(v["copy_on_select"], true);
        assert_eq!(v["default_terminal"], "windows-terminal");
        assert_eq!(v["font_size"], 14);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_or_odd_values_read_as_the_terminal_s_defaults() {
        let f = File::default();
        assert_eq!((f.default_profile().as_str(), f.font_size(), f.scrollback(), f.copy_on_select()), (DEFAULT_PROFILE, 12.0, 10_000, false));
        assert_eq!(f.new_instance(), "window");
        let mut f = File::default();
        f.set("font_size", 400);
        f.set("scrollback", -5);
        assert_eq!((f.font_size(), f.scrollback()), (MAX_PT, 0));
    }
}
