//! `icons.json`: which icon pack to use, and the per-app exceptions.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const NAME: &str = "icons";

/// The pack used when none was ever chosen: shellman's own, Islands.
///
/// How a saved file reads: no `icons.json`, or one without a `pack` key, is the default (a fresh
/// setup, or one from before packs existed, which never chose anything); `"pack": null` is the
/// user's choice of every app's own icon ("Windows icons"), kept as it is; a name is that pack.
/// Settings always writes the key, so a choice once made is never taken for "none chosen".
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct IconSettings {
    /// The pack: `Islands` (built in), or a folder name under `icon-packs`; none for every app's
    /// own icon.
    pub pack: Option<String>,
    /// An app (its id, or its program's path) to an image file that is its icon, pack or not.
    pub overrides: BTreeMap<String, String>,
    /// Apps that keep their own icon even when the pack has one.
    pub skip: Vec<String>,
}

impl Default for IconSettings {
    fn default() -> Self {
        Self { pack: Some(super::builtin::NAME.to_string()), overrides: BTreeMap::new(), skip: Vec::new() }
    }
}

/// How an app id or path is compared: lower case, one kind of slash.
pub fn key(text: &str) -> String {
    text.trim().replace('/', "\\").to_lowercase()
}

impl IconSettings {
    pub fn load() -> Self {
        crate::config::load(NAME)
    }

    pub fn save(&self) {
        crate::config::save(NAME, self);
    }

    /// The image set for the app, going by any of the keys it is known by.
    pub fn override_for(&self, keys: &[String]) -> Option<&str> {
        keys.iter().find_map(|k| self.overrides.iter().find(|(o, _)| key(o) == *k).map(|(_, v)| v.as_str()))
    }

    pub fn is_skipped(&self, keys: &[String]) -> bool {
        keys.iter().any(|k| self.skip.iter().any(|s| key(s) == *k))
    }

    /// Give the app this image, which also ends any "use original".
    pub fn set_override(&mut self, app: &str, image: &str) {
        self.use_pack(app);
        self.overrides.retain(|o, _| key(o) != key(app));
        self.overrides.insert(app.to_string(), image.to_string());
    }

    /// Show the app's own icon again: no override, and the pack does not apply to it.
    pub fn use_original(&mut self, app: &str) {
        self.overrides.retain(|o, _| key(o) != key(app));
        if !self.skip.iter().any(|s| key(s) == key(app)) {
            self.skip.push(app.to_string());
        }
    }

    /// Back to what the pack says for the app: no override, not skipped.
    pub fn use_pack(&mut self, app: &str) {
        self.overrides.retain(|o, _| key(o) != key(app));
        self.skip.retain(|s| key(s) != key(app));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_format_is_the_documented_one() {
        let s: IconSettings = serde_json::from_str(r#"{ "pack": "Papirus", "overrides": { "C:\\a\\b.exe": "D:\\i.png" }, "skip": ["x"] }"#).unwrap();
        assert_eq!(s.pack.as_deref(), Some("Papirus"));
        assert_eq!(s.override_for(&[key(r"c:\A\B.exe")]), Some(r"D:\i.png"));
        assert!(s.is_skipped(&["x".to_string()]));
        assert_eq!(serde_json::from_str::<IconSettings>("{}").unwrap(), IconSettings::default());
        let none: IconSettings = serde_json::from_str(r#"{ "pack": null }"#).unwrap();
        assert_eq!(none.pack, None);
    }

    #[test]
    fn islands_unless_something_else_was_chosen() {
        let read = |json: &str| serde_json::from_str::<IconSettings>(json).unwrap().pack;
        // Nothing chosen yet: a fresh setup, or a file with only overrides in it.
        assert_eq!(IconSettings::default().pack.as_deref(), Some("Islands"));
        assert_eq!(read("{}").as_deref(), Some("Islands"));
        assert_eq!(read(r#"{ "skip": ["x"] }"#).as_deref(), Some("Islands"));
        // A choice stays: every app's own icon, or another pack.
        assert_eq!(read(r#"{ "pack": null }"#), None);
        assert_eq!(read(r#"{ "pack": "Papirus" }"#).as_deref(), Some("Papirus"));
        // And is written so that it stays.
        let none = IconSettings { pack: None, ..Default::default() };
        assert_eq!(serde_json::from_str::<IconSettings>(&serde_json::to_string(&none).unwrap()).unwrap(), none);
        // A missing or unreadable file is the default too.
        let missing = std::env::temp_dir().join("shellman-no-such-icons.json");
        assert_eq!(crate::config::load_from::<IconSettings>(&missing).pack.as_deref(), Some("Islands"));
    }

    #[test]
    fn overrides_and_skips_replace_each_other() {
        let mut s = IconSettings::default();
        s.set_override(r"C:\a.exe", "x.png");
        assert_eq!(s.override_for(&[key(r"c:\a.exe")]), Some("x.png"));
        s.use_original(r"c:\A.exe");
        assert_eq!(s.override_for(&[key(r"c:\a.exe")]), None);
        assert!(s.is_skipped(&[key(r"c:\a.exe")]));
        s.use_original(r"c:\a.exe");
        assert_eq!(s.skip.len(), 1, "once");
        s.set_override(r"c:\a.exe", "y.png");
        assert!(!s.is_skipped(&[key(r"c:\a.exe")]), "a chosen image is no longer skipped");
        s.use_pack(r"C:\A.EXE");
        assert_eq!(s, IconSettings::default());
    }

    #[test]
    fn round_trips_through_a_file() {
        let path = std::env::temp_dir().join(format!("shellman-icons-{}.json", std::process::id()));
        let mut s = IconSettings { pack: Some("Tela".into()), ..Default::default() };
        s.use_original("a");
        crate::config::save_to(&path, &s);
        assert_eq!(crate::config::load_from::<IconSettings>(&path), s);
        let _ = std::fs::remove_file(path);
    }
}
