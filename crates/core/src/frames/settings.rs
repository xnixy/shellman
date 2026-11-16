//! `frames.json`: whether other apps' frames follow shellman's look, and which apps to leave be.

use serde::{Deserialize, Serialize};

pub const NAME: &str = "frames";

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct FramesSettings {
    /// Style other apps' windows at all.
    pub enabled: bool,
    /// Give the window in front an accent border (the others get a dim one). Off unless asked for.
    pub accent_border: bool,
    /// Programs to leave alone, by exe name (`notepad.exe` or just `notepad`, any case).
    pub exclude: Vec<String>,
}

impl Default for FramesSettings {
    fn default() -> Self {
        Self { enabled: true, accent_border: false, exclude: Vec::new() }
    }
}

impl FramesSettings {
    pub fn load() -> Self {
        crate::config::load(NAME)
    }

    pub fn save(&self) {
        crate::config::save(NAME, self);
    }
}

/// The name a program is matched by: file name without folders or `.exe`, lower case.
fn key(name: &str) -> String {
    let file = name.rsplit(['\\', '/']).next().unwrap_or(name).trim().to_lowercase();
    file.strip_suffix(".exe").unwrap_or(&file).to_string()
}

/// Is the program at `exe` (a path or a bare name) on the exclusion list?
pub fn is_excluded(exclude: &[String], exe: &str) -> bool {
    let want = key(exe);
    !want.is_empty() && exclude.iter().any(|e| key(e) == want)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_style_frames_without_an_accent_border() {
        let s = FramesSettings::default();
        assert!(s.enabled && !s.accent_border && s.exclude.is_empty());
        assert_eq!(serde_json::from_str::<FramesSettings>("{}").unwrap(), s);
    }

    #[test]
    fn round_trips_through_a_file() {
        let path = std::env::temp_dir().join(format!("shellman-frames-{}.json", std::process::id()));
        let s = FramesSettings { enabled: false, accent_border: false, exclude: vec!["Foo.exe".into()] };
        crate::config::save_to(&path, &s);
        assert_eq!(crate::config::load_from::<FramesSettings>(&path), s);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn exclusion_ignores_case_folders_and_extension() {
        let list = vec!["Notepad.EXE".to_string(), "mspaint".to_string()];
        assert!(is_excluded(&list, r"C:\Windows\notepad.exe"));
        assert!(is_excluded(&list, "MSPAINT.exe"));
        assert!(!is_excluded(&list, r"C:\x\notepad2.exe"));
        assert!(!is_excluded(&[], "a.exe"));
        assert!(!is_excluded(&list, ""));
    }
}
