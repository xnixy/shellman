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

/// Why settings could not be read. A missing file is not an error (it reads as the default).
#[derive(Debug)]
pub enum LoadError {
    Read(std::io::Error),
    Parse(serde_json::Error),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Read(e) => write!(f, "could not read it: {e}"),
            LoadError::Parse(e) => write!(f, "it is not valid: {e}"),
        }
    }
}

/// The settings named `name`, or their default when missing or unreadable. An unreadable file
/// is then never saved over by this process (see [`save_to`]); a caller that would save what it
/// read uses [`try_load`] and keeps what it has on an error.
pub fn load<T: DeserializeOwned + Default>(name: &str) -> T {
    match config_dir() {
        Some(dir) => load_from(&dir.join(format!("{name}.json"))),
        None => T::default(),
    }
}

/// The settings named `name`: their default when the file is missing, an error when it cannot
/// be read or parsed (tried twice, a few milliseconds apart, for a file caught mid-save).
pub fn try_load<T: DeserializeOwned + Default>(name: &str) -> Result<T, LoadError> {
    match config_dir() {
        Some(dir) => try_load_from(&dir.join(format!("{name}.json"))),
        None => Ok(T::default()),
    }
}

/// Save the settings named `name`; failures are ignored, since settings are never worth a crash.
pub fn save<T: Serialize + DeserializeOwned>(name: &str, value: &T) {
    if let Some(dir) = config_dir() {
        save_to(&dir.join(format!("{name}.json")), value);
    }
}

/// `load` from an explicit file; the seam that keeps tests off the real config directory.
pub fn load_from<T: DeserializeOwned + Default>(path: &Path) -> T {
    match try_load_from(path) {
        Ok(v) => {
            unreadable().lock().unwrap().remove(path);
            v
        }
        Err(e) => {
            eprintln!("shellman: {}: {e}; using the defaults, and not saving over it", path.display());
            unreadable().lock().unwrap().insert(path.to_path_buf());
            T::default()
        }
    }
}

/// `try_load` from an explicit file.
pub fn try_load_from<T: DeserializeOwned + Default>(path: &Path) -> Result<T, LoadError> {
    match read_once(path) {
        Err(_) => {
            // A save in another process may be renaming its file over this one right now.
            std::thread::sleep(std::time::Duration::from_millis(5));
            read_once(path)
        }
        ok => ok,
    }
}

fn read_once<T: DeserializeOwned + Default>(path: &Path) -> Result<T, LoadError> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Err(e) => return Err(LoadError::Read(e)),
    };
    // Editors on Windows may save with a byte-order mark, which JSON parsers reject.
    let json = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    serde_json::from_slice(json).map_err(LoadError::Parse)
}

/// Files this process could not read (and so used defaults for): not saved over while they
/// still cannot be read, so a default never replaces the user's settings.
fn unreadable() -> &'static std::sync::Mutex<std::collections::HashSet<PathBuf>> {
    static SET: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<PathBuf>>> = std::sync::OnceLock::new();
    SET.get_or_init(Default::default)
}

/// `save` to an explicit file; failures are ignored (see [`try_save_to`]).
pub fn save_to<T: Serialize + DeserializeOwned>(path: &Path, value: &T) {
    if let Err(e) = try_save_to(path, value) {
        eprintln!("shellman: not saved to {}: {e}", path.display());
    }
}

/// `save` to an explicit file: written whole to a temporary file of this save's own (named for
/// the process, the thread and a count, so two saves at once never share one), flushed to disk,
/// then renamed over the file, so a reader sees the old file or the new one, never half of one.
/// Refused when this process could not read the file and it still cannot be read as `T` (the
/// user's settings are not replaced by defaults).
pub fn try_save_to<T: Serialize + DeserializeOwned>(path: &Path, value: &T) -> std::io::Result<()> {
    use std::io::Write;
    if unreadable().lock().unwrap().contains(path) {
        let readable = match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice::<T>(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes)).is_ok(),
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        };
        if !readable {
            return Err(std::io::Error::other("the file could not be read; not saving over it"));
        }
        unreadable().lock().unwrap().remove(path);
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let json = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    let tmp = temp_name(path);
    let written = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(&json)?;
        file.sync_all()
    })();
    let result = written.and_then(|()| {
        // Replaces the file (MoveFileEx with REPLACE_EXISTING); a reader that has it open
        // without delete sharing makes it fail for a moment.
        let mut tries = 0;
        loop {
            match std::fs::rename(&tmp, path) {
                Ok(()) => return Ok(()),
                Err(_) if tries < 3 => {
                    tries += 1;
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(e) => return Err(e),
            }
        }
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// `<file>.<pid>-<thread>-<n>.tmp` beside `path`.
fn temp_name(path: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    std::thread::current().id().hash(&mut h);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!("{name}.{}-{:x}-{}.tmp", std::process::id(), h.finish() & 0xffff_ffff, COUNT.fetch_add(1, Ordering::Relaxed)))
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

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shellman-config-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_unreadable_file_reads_as_an_error_and_is_not_saved_over() {
        let dir = temp_dir("bad");
        let path = dir.join("sample.json");
        std::fs::write(&path, b"{\"name\": \"mine\", \"n\": 3").unwrap();
        assert!(matches!(try_load_from::<Sample>(&path), Err(LoadError::Parse(_))));
        // The forgiving load gives the default, and then the default is not saved over it.
        assert_eq!(load_from::<Sample>(&path), Sample::default());
        assert!(try_save_to(&path, &Sample::default()).is_err());
        save_to(&path, &Sample::default());
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"name\": \"mine\", \"n\": 3");
        // Once it reads again (fixed by hand, or saved whole by another app), saving works.
        std::fs::write(&path, br#"{"name": "mine", "n": 3}"#).unwrap();
        let fixed = Sample { name: "mine".into(), n: 4 };
        save_to(&path, &fixed);
        assert_eq!(load_from::<Sample>(&path), fixed);
        // A missing file is no error.
        assert_eq!(try_load_from::<Sample>(&dir.join("none.json")).unwrap(), Sample::default());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Saves at once from several threads (as several apps save the shared look) never leave a
    /// reader with half a file, nor a temporary file behind.
    #[test]
    fn saves_at_once_never_show_half_a_file() {
        let dir = temp_dir("race");
        let path = dir.join("look.json");
        save_to(&path, &Sample { name: "start".into(), n: 0 });
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let writers: Vec<_> = (0..4)
            .map(|w| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for i in 0..150 {
                        // Long enough that a write takes a moment.
                        let value = Sample { name: format!("writer {w} {}", "x".repeat(4000)), n: i };
                        save_to(&path, &value);
                    }
                })
            })
            .collect();
        let readers: Vec<_> = (0..2)
            .map(|_| {
                let (path, stop) = (path.clone(), stop.clone());
                std::thread::spawn(move || {
                    let mut bad = 0;
                    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                        if try_load_from::<Sample>(&path).is_err() {
                            bad += 1;
                        }
                    }
                    bad
                })
            })
            .collect();
        for w in writers {
            w.join().unwrap();
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let bad: usize = readers.into_iter().map(|r| r.join().unwrap()).sum();
        assert_eq!(bad, 0, "reads that found half a file");
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name()).collect();
        assert_eq!(left, [std::ffi::OsString::from("look.json")]);
        assert_eq!(load_from::<Sample>(&path).n, 149);
        let _ = std::fs::remove_dir_all(dir);
    }
}
