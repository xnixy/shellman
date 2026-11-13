//! Updating a running app renames its exe aside to `name.oldHHMMSS.exe` and copies the new one
//! in. Nothing else removes the old ones, so the session does at its start.

use std::path::Path;

/// Exactly `shellman-<name>.old<digits>.exe` (any case): `name` has no dots, there is at least one
/// digit. Nothing else may match, since this decides what gets deleted.
pub fn is_leftover(file_name: &str) -> bool {
    let n = file_name.to_ascii_lowercase();
    let Some(rest) = n.strip_prefix("shellman-").and_then(|r| r.strip_suffix(".exe")) else { return false };
    let Some((name, digits)) = rest.split_once(".old") else { return false };
    !name.is_empty()
        && !name.contains(['.', '\\', '/'])
        && !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
}

/// Delete the leftovers directly in `dir`; the ones still running fail to delete and stay.
/// Returns how many went.
pub fn sweep(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    entries
        .flatten()
        .filter(|e| e.file_name().to_str().is_some_and(is_leftover) && e.file_type().is_ok_and(|t| t.is_file()))
        .filter(|e| std::fs::remove_file(e.path()).is_ok())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_only_renamed_aside_exes() {
        assert!(is_leftover("shellman-bar.old123456.exe"));
        assert!(is_leftover("Shellman-Bar.OLD1.EXE"));
        for no in [
            "shellman-bar.exe",
            "shellman-bar.old.exe",
            "shellman-bar.old12a.exe",
            "shellman-bar.old12.exe.exe",
            "shellman-.old12.exe",
            "shellman-a.b.old12.exe",
            "shellman-bar.old12.dll",
            "other-bar.old12.exe",
            "shellman-bar.old12",
            "bar.old12.exe",
        ] {
            assert!(!is_leftover(no), "{no}");
        }
    }

    #[test]
    fn sweeps_only_matching_files_in_the_folder() {
        let dir = std::env::temp_dir().join(format!("shellman-leftovers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        for f in ["shellman-bar.old010101.exe", "shellman-shelf.old2.exe", "shellman-bar.exe", "notes.old1.exe"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        std::fs::write(dir.join("sub").join("shellman-bar.old3.exe"), b"x").unwrap();
        assert_eq!(sweep(&dir), 2);
        assert!(dir.join("shellman-bar.exe").exists());
        assert!(dir.join("notes.old1.exe").exists());
        assert!(dir.join("sub").join("shellman-bar.old3.exe").exists());
        assert!(!dir.join("shellman-bar.old010101.exe").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
