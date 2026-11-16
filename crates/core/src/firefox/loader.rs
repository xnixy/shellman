//! The scripts' loader: Firefox's autoconfig files in the Firefox installation folder
//! (`config.js` next to `firefox.exe`, `defaults\pref\config-prefs.js`). Their text is built into
//! this crate from `integrations/firefox/autoconfig`, so whoever writes them (Settings, or the
//! administrator helper for a folder under Program Files) writes only these fixed bytes and never
//! a file it was handed.
//!
//! A file is ours when it says "shellman Islands" (every version of both does). Files that are
//! not ours are never replaced or removed: another autoconfig setup stays as it is, and the
//! scripts are not installed.

use std::path::Path;

pub const CONFIG_JS: &str = include_str!("../../../../integrations/firefox/autoconfig/config.js");
pub const CONFIG_PREFS_JS: &str = include_str!("../../../../integrations/firefox/autoconfig/config-prefs.js");

/// Each file's place below the Firefox folder, and its text.
pub const FILES: [(&str, &str); 2] = [("config.js", CONFIG_JS), ("defaults\\pref\\config-prefs.js", CONFIG_PREFS_JS)];

const MARK: &[u8] = b"shellman Islands";

/// What the Firefox folder holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Neither file.
    Absent,
    /// Both, exactly as this build writes them.
    Current,
    /// Ours, but not all of it, or an older text: writing them again brings it up to date.
    Old,
    /// A file of another autoconfig setup is in the way.
    Foreign,
}

pub fn is_ours(bytes: &[u8]) -> bool {
    bytes.windows(MARK.len()).any(|w| w == MARK)
}

/// The state, from each file's bytes (None: no file). Pure.
pub fn state_of(files: &[Option<Vec<u8>>; 2]) -> State {
    if files.iter().flatten().any(|b| !is_ours(b)) {
        return State::Foreign;
    }
    if files.iter().all(Option::is_none) {
        return State::Absent;
    }
    let current = files.iter().zip(FILES).all(|(f, (_, text))| f.as_deref() == Some(text.as_bytes()));
    if current {
        State::Current
    } else {
        State::Old
    }
}

fn read(dir: &Path) -> [Option<Vec<u8>>; 2] {
    FILES.map(|(rel, _)| std::fs::read(dir.join(rel)).ok())
}

pub fn state(dir: &Path) -> State {
    state_of(&read(dir))
}

/// Whether `dir` is a Firefox installation folder (it has `firefox.exe` and `defaults\pref`).
pub fn is_firefox_dir(dir: &Path) -> bool {
    dir.join("firefox.exe").is_file() && dir.join("defaults").join("pref").is_dir()
}

/// Write both files into the Firefox folder `dir`. Refuses, writing nothing, when a file that is
/// not ours is in the way or `dir` is not a Firefox folder.
pub fn write(dir: &Path) -> Result<(), String> {
    if !is_firefox_dir(dir) {
        return Err(format!("{} is not a Firefox folder.", dir.display()));
    }
    if state(dir) == State::Foreign {
        return Err("Firefox already has another autoconfig setup (config.js); it was left as it is.".into());
    }
    for (rel, text) in FILES {
        let path = dir.join(rel);
        std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }
    Ok(())
}

/// Remove whichever of the two files is ours. Files of another setup stay.
pub fn remove(dir: &Path) -> Result<(), String> {
    for (rel, _) in FILES {
        let path = dir.join(rel);
        if std::fs::read(&path).is_ok_and(|b| is_ours(&b)) {
            std::fs::remove_file(&path).map_err(|e| format!("cannot remove {}: {e}", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cur() -> [Option<Vec<u8>>; 2] {
        FILES.map(|(_, t)| Some(t.as_bytes().to_vec()))
    }

    #[test]
    fn both_texts_are_marked_ours() {
        assert!(FILES.iter().all(|(_, t)| is_ours(t.as_bytes())));
    }

    #[test]
    fn states_from_bytes() {
        assert_eq!(state_of(&[None, None]), State::Absent);
        assert_eq!(state_of(&cur()), State::Current);
        let mut old = cur();
        old[0] = Some(b"// shellman Islands loader, an older one".to_vec());
        assert_eq!(state_of(&old), State::Old);
        let mut half = cur();
        half[1] = None;
        assert_eq!(state_of(&half), State::Old);
        let mut foreign = cur();
        foreign[0] = Some(b"// someone else's autoconfig".to_vec());
        assert_eq!(state_of(&foreign), State::Foreign);
    }

    #[test]
    fn write_and_remove_in_a_fake_firefox_folder() {
        let d = std::env::temp_dir().join(format!("shellman-ff-loader-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        assert!(write(&d).is_err(), "not a Firefox folder");
        std::fs::create_dir_all(d.join("defaults").join("pref")).unwrap();
        std::fs::write(d.join("firefox.exe"), b"").unwrap();
        write(&d).unwrap();
        assert_eq!(state(&d), State::Current);
        remove(&d).unwrap();
        assert_eq!(state(&d), State::Absent);
        // Another setup's config.js is neither replaced nor removed.
        std::fs::write(d.join("config.js"), b"// mine").unwrap();
        assert!(write(&d).is_err());
        assert!(!d.join("defaults\\pref\\config-prefs.js").exists());
        remove(&d).unwrap();
        assert_eq!(std::fs::read(d.join("config.js")).unwrap(), b"// mine");
        let _ = std::fs::remove_dir_all(&d);
    }
}
