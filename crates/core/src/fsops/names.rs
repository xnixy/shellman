//! Name rules: natural sort order, rename validation, and unused child names.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

/// Compares names the way Explorer does: digit runs compare as numbers, so "file2" comes before
/// "file10". Works in place on the bytes (UTF-8 byte order equals code point order), because it
/// runs O(n log n) times per sort and must not allocate. Callers pass lowercased keys.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let (ei, ej) = (run_end(a, i), run_end(b, j));
            // Leading zeros don't count; then a longer run is a bigger number.
            let (ra, rb) = (&a[skip_zeros(a, i, ei)..ei], &b[skip_zeros(b, j, ej)..ej]);
            let ord = ra.len().cmp(&rb.len()).then_with(|| ra.cmp(rb));
            if ord != Ordering::Equal {
                return ord;
            }
            (i, j) = (ei, ej);
        } else if a[i] != b[j] {
            return a[i].cmp(&b[j]);
        } else {
            i += 1;
            j += 1;
        }
    }
    (a.len() - i).cmp(&(b.len() - j))
}

fn run_end(s: &[u8], mut i: usize) -> usize {
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    i
}

fn skip_zeros(s: &[u8], mut i: usize, end: usize) -> usize {
    while i < end && s[i] == b'0' {
        i += 1;
    }
    i
}

/// "New folder", or "New folder (2)" and up when that is taken.
pub fn unique_child(dir: &Path, base: &str) -> PathBuf {
    let first = dir.join(base);
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{base} ({n})")))
        .find(|p| !p.exists())
        .unwrap()
}

/// "Pasted image.png", or "Pasted image (2).png" and up when that is taken: the number goes
/// before the extension.
pub fn unique_file(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => name.split_at(i),
        _ => (name, ""),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .unwrap()
}

/// The names `unique_file` tries for `name`, in order: "a.png", "a (2).png", "a (3).png", …
fn file_names(name: &str) -> impl Iterator<Item = String> + '_ {
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => name.split_at(i),
        _ => (name, ""),
    };
    std::iter::once(name.to_owned()).chain((2..).map(move |n| format!("{stem} ({n}){ext}")))
}

/// How many numbered names are tried before giving up.
const MAX_TRIES: usize = 10_000;

/// Write `bytes` to a new file in `dir` named after `name` (numbered when taken, as
/// `unique_file`). The file is made only if no item has that name, in the same step: something
/// made under that name between a look and the write is never written over.
pub fn create_new_file(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    use std::io::{ErrorKind, Write};
    for candidate in file_names(name).take(MAX_TRIES) {
        let path = dir.join(candidate);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                f.write_all(bytes)?;
                return Ok(path);
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            // A folder by that name: Windows says "access denied" rather than "exists".
            Err(e) if e.kind() == ErrorKind::PermissionDenied && std::fs::symlink_metadata(&path).is_ok() => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(ErrorKind::AlreadyExists, "every name is taken"))
}

/// A new folder in `dir` named `base`, or "base (2)" and up when taken; made in the same step as
/// the name is found free, so it never fails on, or takes, a folder made meanwhile.
pub fn create_new_dir(dir: &Path, base: &str) -> std::io::Result<PathBuf> {
    use std::io::ErrorKind;
    let names = std::iter::once(base.to_owned()).chain((2..).map(|n| format!("{base} ({n})")));
    for candidate in names.take(MAX_TRIES) {
        let path = dir.join(candidate);
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(ErrorKind::AlreadyExists, "every name is taken"))
}

/// Rename `from` to `to`, never replacing an item already at `to` (`std::fs::rename` does replace
/// on Windows). A change of case only works, as the same item; in a case-sensitive folder where
/// another item has that name, it fails. AlreadyExists when the name is taken.
pub fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVE_FILE_FLAGS};
    let (a, b) = (super::shell::wide_path(from), super::shell::wide_path(to));
    unsafe { MoveFileExW(PCWSTR(a.as_ptr()), PCWSTR(b.as_ptr()), MOVE_FILE_FLAGS(0)) }
        .map_err(|e| std::io::Error::from_raw_os_error(e.code().0 & 0xFFFF))
}

/// Checks a new name before renaming, with the reason Windows would refuse it.
pub fn check_name(name: &str) -> Result<(), &'static str> {
    let name = name.trim();
    if name.is_empty() {
        return Err("A name can't be empty.");
    }
    if name
        .chars()
        .any(|c| r#"\/:*?"<>|"#.contains(c) || (c as u32) < 32)
    {
        return Err(r#"A name can't contain \ / : * ? " < > |"#);
    }
    if name.ends_with('.') {
        return Err("A name can't end with a dot.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut v = vec!["file10", "file2", "File1", "a", "file02b"];
        v.sort_by(|a, b| natural_cmp(&a.to_lowercase(), &b.to_lowercase()));
        assert_eq!(v, ["a", "File1", "file2", "file02b", "file10"]);
    }

    #[test]
    fn natural_leading_zeros_and_prefixes() {
        assert_eq!(natural_cmp("a007", "a7"), Ordering::Equal);
        assert_eq!(natural_cmp("a007", "a8"), Ordering::Less);
        assert_eq!(natural_cmp("a0010", "a9"), Ordering::Greater);
        assert_eq!(natural_cmp("a1b", "a01c"), Ordering::Less);
        assert_eq!(natural_cmp("v1.10", "v1.9"), Ordering::Greater);
        assert_eq!(natural_cmp("abc", "abcd"), Ordering::Less);
        assert_eq!(natural_cmp("", "a"), Ordering::Less);
        assert_eq!(natural_cmp("x00", "x0"), Ordering::Equal);
    }

    #[test]
    fn natural_non_ascii() {
        assert_eq!(natural_cmp("é2", "é10"), Ordering::Less);
        assert_eq!(natural_cmp("z", "é"), Ordering::Less);
        assert_eq!(natural_cmp("日本1", "日本01"), Ordering::Equal);
        assert_eq!(natural_cmp("日本", "日本語"), Ordering::Less);
        assert_eq!(natural_cmp("ä", "ö"), Ordering::Less);
    }

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shellman-names-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn new_files_never_write_over_an_item() {
        let dir = temp("new");
        std::fs::write(dir.join("a.txt"), b"keep").unwrap();
        // Taken meanwhile (made after any look): the next number, the old file untouched.
        std::fs::write(dir.join("a (2).txt"), b"keep2").unwrap();
        let made = create_new_file(&dir, "a.txt", b"new").unwrap();
        assert_eq!(made, dir.join("a (3).txt"));
        assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"keep");
        assert_eq!(std::fs::read(dir.join("a (2).txt")).unwrap(), b"keep2");
        assert_eq!(std::fs::read(&made).unwrap(), b"new");
        // A folder by that name is taken too.
        std::fs::create_dir(dir.join("b")).unwrap();
        assert_eq!(create_new_file(&dir, "b", b"").unwrap(), dir.join("b (2)"));
        assert_eq!(create_new_dir(&dir, "b").unwrap(), dir.join("b (3)"));
        assert_eq!(create_new_dir(&dir, "c").unwrap(), dir.join("c"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_rename_never_replaces() {
        let dir = temp("rename");
        let (a, b) = (dir.join("a.txt"), dir.join("b.txt"));
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let err = rename_no_replace(&a, &b).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists, "{err}");
        assert_eq!(std::fs::read(&b).unwrap(), b"b");
        assert_eq!(std::fs::read(&a).unwrap(), b"a");
        // A change of case only: the same item, renamed.
        rename_no_replace(&a, &dir.join("A.txt")).unwrap();
        let names: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert!(names.contains(&"A.txt".to_owned()), "{names:?}");
        rename_no_replace(&dir.join("A.txt"), &dir.join("c.txt")).unwrap();
        assert_eq!(std::fs::read(dir.join("c.txt")).unwrap(), b"a");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn names() {
        assert!(check_name("ok.txt").is_ok());
        assert!(check_name("a:b").is_err());
        assert!(check_name("  ").is_err());
        assert!(check_name("trail.").is_err());
    }
}
