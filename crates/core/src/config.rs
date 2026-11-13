//! Per-app settings as JSON files under `%APPDATA%\shellman`, shared by every shell app.

use serde::{de::DeserializeOwned, Serialize};
use std::path::{Path, PathBuf};

/// Setting this environment variable to a folder makes every app use it instead of
/// `%APPDATA%\shellman`: for trying things out, and for tests, without touching the real files.
pub const DIR_OVERRIDE: &str = "SHELLMAN_CONFIG_DIR";

/// `%APPDATA%\shellman`, where each app keeps its `<name>.json` (or the folder named by
/// `SHELLMAN_CONFIG_DIR`).
pub fn config_dir() -> Option<PathBuf> {
    pick_dir(std::env::var_os(DIR_OVERRIDE), dirs::config_dir())
}

/// Where apps keep what they can fetch again (the widgets' last calendar and weather):
/// `%LOCALAPPDATA%\shellman\cache`, or `cache` in the `SHELLMAN_CONFIG_DIR` folder.
pub fn cache_dir() -> Option<PathBuf> {
    match std::env::var_os(DIR_OVERRIDE).filter(|o| !o.is_empty()) {
        Some(dir) => Some(PathBuf::from(dir).join("cache")),
        None => dirs::data_local_dir().map(|d| d.join("shellman").join("cache")),
    }
}

/// The override when it is set and not empty, else the standard folder's `shellman`.
fn pick_dir(over: Option<std::ffi::OsString>, base: Option<PathBuf>) -> Option<PathBuf> {
    match over.filter(|o| !o.is_empty()) {
        Some(dir) => Some(PathBuf::from(dir)),
        None => base.map(|d| d.join("shellman")),
    }
}

/// The settings named `name`, or their default when missing or unreadable.
pub fn load<T: DeserializeOwned + Default>(name: &str) -> T {
    match config_dir() {
        Some(dir) => load_from(&dir.join(format!("{name}.json"))),
        None => T::default(),
    }
}

/// Save the settings named `name`; failures are ignored, since settings are never worth a crash.
pub fn save<T: Serialize>(name: &str, value: &T) {
    if let Some(dir) = config_dir() {
        save_to(&dir.join(format!("{name}.json")), value);
    }
}

/// `load` from an explicit file; the seam that keeps tests off the real config directory.
pub fn load_from<T: DeserializeOwned + Default>(path: &Path) -> T {
    let Ok(bytes) = std::fs::read(path) else {
        return T::default();
    };
    // Editors on Windows may save with a byte-order mark, which JSON parsers reject.
    let json = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    serde_json::from_slice(json).unwrap_or_default()
}

/// `save` to an explicit file. Written to a `.tmp` file and renamed, so a crash never leaves
/// half a file behind.
pub fn save_to<T: Serialize>(path: &Path, value: &T) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_vec_pretty(value) {
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Serialize, Deserialize, Default, PartialEq, Debug)]
    struct Sample {
        name: String,
        n: u32,
    }

    #[test]
    fn override_wins_when_set() {
        let base = Some(PathBuf::from("C:\\Users\\a\\AppData\\Roaming"));
        assert_eq!(
            pick_dir(Some("D:\\scratch".into()), base.clone()),
            Some(PathBuf::from("D:\\scratch"))
        );
        let standard = base.clone().map(|d| d.join("shellman"));
        assert_eq!(pick_dir(None, base.clone()), standard);
        assert_eq!(pick_dir(Some("".into()), base), standard);
    }

    #[test]
    fn round_trip_and_bom() {
        let dir = std::env::temp_dir().join(format!("shellman-config-{}", std::process::id()));
        let path = dir.join("nested").join("sample.json");
        let value = Sample {
            name: "a".into(),
            n: 7,
        };
        save_to(&path, &value);
        assert_eq!(load_from::<Sample>(&path), value);
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend(serde_json::to_vec(&value).unwrap());
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(load_from::<Sample>(&path), value);
        assert_eq!(
            load_from::<Sample>(&dir.join("missing.json")),
            Sample::default()
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
