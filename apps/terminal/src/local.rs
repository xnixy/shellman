//! Paths on this computer's own drives, told apart from network ones without touching the
//! network: what a link or a directory a program printed may name before the terminal acts on it.
//!
//! Output is anyone's (a file shown with `cat`, a remote host over ssh), so a path in it is only
//! used when it is a drive-letter path (`C:\...`) on a fixed, removable or RAM drive. UNC paths
//! (`\\host\share`), device paths (`\\?\`, `\\.\`) and mapped network drives are refused: opening
//! or even probing one would wait for the network and hand the user's credentials to that host.

use std::path::{Path, PathBuf};

/// `s` as a drive-letter path (`C:\...`, absolute), when it is one and holds nothing a path
/// cannot: no control characters, none of `"<>|?*`, and no colon past the drive's (no
/// alternate data streams). Syntax only: nothing is looked up.
pub fn drive_path(s: &str) -> Option<PathBuf> {
    let b = s.as_bytes();
    if b.len() < 3 || !b[0].is_ascii_alphabetic() || b[1] != b':' || b[2] != b'\\' {
        return None;
    }
    if s.len() > 32_000 || s[2..].chars().any(|c| c.is_control() || matches!(c, '"' | '<' | '>' | '|' | '?' | '*' | ':')) {
        return None;
    }
    Some(PathBuf::from(s))
}

/// Whether `p`'s drive is one of this computer's own (fixed, removable or a RAM disk), asked of
/// the drive letter only (`GetDriveTypeW` on `C:\`): a mapped network drive, a CD and a letter
/// that is not there all say no, and nothing on the drive is read.
pub fn on_local_drive(p: &Path) -> bool {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetDriveTypeW;
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;
    const DRIVE_RAMDISK: u32 = 6;
    let Some(s) = p.to_str() else { return false };
    if drive_path(s).is_none() {
        return false;
    }
    let root = HSTRING::from(&s[..3]);
    matches!(unsafe { GetDriveTypeW(&root) }, DRIVE_REMOVABLE | DRIVE_FIXED | DRIVE_RAMDISK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_drive_letter_paths() {
        assert_eq!(drive_path(r"C:\Users\me"), Some(PathBuf::from(r"C:\Users\me")));
        assert_eq!(drive_path(r"d:\"), Some(PathBuf::from(r"d:\")));
        for bad in [
            r"\\attacker\share",
            r"\\?\C:\x",
            r"\\.\pipe\x",
            r"C:relative",
            "C:",
            "/home/me",
            r"C:\x\file.exe:stream",
            "C:\\a\"b",
            "C:\\a\nb",
            "C:\\a\u{9b}b",
            r"C:\a?b",
            "",
        ] {
            assert_eq!(drive_path(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_system_drive_is_local_and_no_unc_is() {
        let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        assert!(on_local_drive(Path::new(&format!(r"{system}\Windows"))));
        // Syntactic refusals: these are never asked about (no network is touched).
        assert!(!on_local_drive(Path::new(r"\\attacker.invalid\share")));
    }
}
