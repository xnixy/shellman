//! Putting one file in place, safely. Everything here is confined to the install folder by
//! `paths::Root`; a copy goes to a temporary name first and is verified, so a failed copy never
//! leaves a half-written exe; an existing file (possibly a running exe) is renamed aside to
//! `name.old<HHmmss>.ext` first, and that rename is rolled back if the new file cannot be put in
//! its place.

use crate::manifest::aside_of;
use crate::paths::Root;
use crate::sha256;
use std::io;
use std::path::{Path, PathBuf};

/// The operations that can fail half-way (injectable, so the rollback is tested).
pub trait Fs {
    fn copy(&self, from: &Path, to: &Path) -> io::Result<u64>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_file(&self, p: &Path) -> io::Result<()>;
}

pub struct RealFs;

impl Fs for RealFs {
    fn copy(&self, from: &Path, to: &Path) -> io::Result<u64> {
        std::fs::copy(from, to)
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }
    fn remove_file(&self, p: &Path) -> io::Result<()> {
        std::fs::remove_file(p)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Placed {
    Added,
    /// Replaced. `aside` is the old file's new name when it could not be deleted (it is running).
    Replaced { aside: Option<PathBuf> },
}

/// `name.ext` -> `name.old<stamp><n>.ext` (`n` empty for the first try).
pub fn aside_name(file_name: &str, stamp: &str, n: u32) -> String {
    let suffix = if n == 0 { String::new() } else { n.to_string() };
    match file_name.rfind('.') {
        Some(i) if i > 0 => format!("{}.old{stamp}{suffix}{}", &file_name[..i], &file_name[i..]),
        _ => format!("{file_name}.old{stamp}{suffix}"),
    }
}

fn temp_name(dest: &Path) -> PathBuf {
    let name = dest.file_name().unwrap_or_default().to_string_lossy();
    dest.with_file_name(format!("{name}.new{}.tmp", std::process::id()))
}

/// Put `src` (whose SHA-256 is `hash`) at `rel` below the root.
pub fn place(fs: &dyn Fs, root: &Root, rel: &str, src: &Path, hash: &str, stamp: &str) -> Result<Placed, String> {
    let dest = root.join(rel)?;
    root.guard(&dest)?;
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        root.guard(&dest)?;
    }
    let tmp = temp_name(&dest);
    root.guard(&tmp)?;
    let _ = fs.remove_file(&tmp);
    if let Err(e) = fs.copy(src, &tmp) {
        let _ = fs.remove_file(&tmp);
        return Err(format!("copy to {} failed: {e}", tmp.display()));
    }
    match sha256::of_file(&tmp) {
        Ok(h) if h == hash => {}
        other => {
            let _ = fs.remove_file(&tmp);
            return Err(format!("the copy of {rel} does not match its source ({})", other.map_or_else(|e| e.to_string(), |_| "hash differs".into())));
        }
    }
    if !dest.exists() {
        return match fs.rename(&tmp, &dest) {
            Ok(()) => Ok(Placed::Added),
            Err(e) => {
                let _ = fs.remove_file(&tmp);
                Err(format!("cannot move the new {rel} into place: {e}"))
            }
        };
    }
    // Rename the old one aside (works for a running exe), then put the new one in its place.
    let file_name = dest.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let aside = (0..100)
        .map(|n| dest.with_file_name(aside_name(&file_name, stamp, n)))
        .find(|p| !p.exists())
        .ok_or("no free name to rename the old file aside to")?;
    root.guard(&aside)?;
    if let Err(e) = fs.rename(&dest, &aside) {
        let _ = fs.remove_file(&tmp);
        return Err(format!("cannot rename {rel} aside: {e}"));
    }
    if let Err(e) = fs.rename(&tmp, &dest) {
        let back = fs.rename(&aside, &dest);
        let _ = fs.remove_file(&tmp);
        return Err(match back {
            Ok(()) => format!("cannot put the new {rel} in place ({e}); the old one was put back"),
            Err(b) => format!("cannot put the new {rel} in place ({e}) and could not put the old one back ({b}): it is at {}", aside.display()),
        });
    }
    // A running exe cannot be deleted; anything else goes now.
    Ok(Placed::Replaced { aside: fs.remove_file(&aside).err().map(|_| aside) })
}

/// Delete the leftover asides of `rels` (those not in use stay: they fail to delete). Returns how
/// many went.
pub fn sweep_asides(root: &Root, rels: &[String]) -> usize {
    let mut dirs = std::collections::BTreeSet::new();
    let mut wanted = std::collections::HashSet::new();
    for rel in rels {
        if let Ok(p) = root.join(rel) {
            if let Some(d) = p.parent() {
                dirs.insert(d.to_path_buf());
            }
            wanted.insert(p.to_string_lossy().to_ascii_lowercase());
        }
    }
    let mut n = 0;
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(orig) = aside_of(&name) else { continue };
            let orig_path = dir.join(orig).to_string_lossy().to_ascii_lowercase();
            if wanted.contains(&orig_path) && e.file_type().is_ok_and(|t| t.is_file()) && root.guard(&e.path()).is_ok() && std::fs::remove_file(e.path()).is_ok() {
                n += 1;
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shellman-setup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn file(p: &Path, content: &[u8]) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    #[test]
    fn aside_names() {
        assert_eq!(aside_name("shellman-bar.exe", "141815", 0), "shellman-bar.old141815.exe");
        assert_eq!(aside_name("shellman-bar.exe", "141815", 2), "shellman-bar.old1418152.exe");
        assert_eq!(aside_name("LICENSE", "1", 0), "LICENSE.old1");
        assert_eq!(aside_of(&aside_name("a.b.dll", "9", 1)).as_deref(), Some("a.b.dll"));
    }

    #[test]
    fn adds_a_new_file_and_leaves_no_temp() {
        let d = temp("add");
        let root = Root::new(d.join("inst"));
        std::fs::create_dir_all(root.path()).unwrap();
        let src = d.join("src.exe");
        file(&src, b"new build");
        let h = sha256::of_bytes(b"new build");
        assert_eq!(place(&RealFs, &root, "sub\\a.exe", &src, &h, "100000").unwrap(), Placed::Added);
        assert_eq!(std::fs::read(root.path().join("sub").join("a.exe")).unwrap(), b"new build");
        let names: Vec<_> = std::fs::read_dir(root.path().join("sub")).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names.len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn replaces_and_removes_the_old_file_when_it_is_not_in_use() {
        let d = temp("replace");
        let root = Root::new(d.join("inst"));
        file(&root.path().join("a.exe"), b"old");
        let src = d.join("src.exe");
        file(&src, b"new");
        let r = place(&RealFs, &root, "a.exe", &src, &sha256::of_bytes(b"new"), "100000").unwrap();
        assert_eq!(r, Placed::Replaced { aside: None });
        assert_eq!(std::fs::read(root.path().join("a.exe")).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_wrong_hash_changes_nothing() {
        let d = temp("hash");
        let root = Root::new(d.join("inst"));
        file(&root.path().join("a.exe"), b"old");
        let src = d.join("src.exe");
        file(&src, b"new");
        assert!(place(&RealFs, &root, "a.exe", &src, "deadbeef", "100000").is_err());
        assert_eq!(std::fs::read(root.path().join("a.exe")).unwrap(), b"old");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1, "no temp file left");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Fails the n-th rename.
    struct Flaky {
        fail_rename: usize,
        count: RefCell<usize>,
        fail_copy: bool,
    }
    impl Fs for Flaky {
        fn copy(&self, from: &Path, to: &Path) -> io::Result<u64> {
            if self.fail_copy {
                // Half a file, then the failure.
                std::fs::write(to, b"ha")?;
                return Err(io::Error::other("disk full"));
            }
            std::fs::copy(from, to)
        }
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            *self.count.borrow_mut() += 1;
            if *self.count.borrow() == self.fail_rename {
                return Err(io::Error::other("denied"));
            }
            std::fs::rename(from, to)
        }
        fn remove_file(&self, p: &Path) -> io::Result<()> {
            std::fs::remove_file(p)
        }
    }

    #[test]
    fn a_failed_swap_rolls_the_rename_back() {
        let d = temp("rollback");
        let root = Root::new(d.join("inst"));
        file(&root.path().join("a.exe"), b"old");
        let src = d.join("src.exe");
        file(&src, b"new");
        // rename #1 = old aside (ok), #2 = new into place (fails), #3 = the rollback (ok)
        let fs = Flaky { fail_rename: 2, count: RefCell::new(0), fail_copy: false };
        let err = place(&fs, &root, "a.exe", &src, &sha256::of_bytes(b"new"), "100000").unwrap_err();
        assert!(err.contains("put back"), "{err}");
        assert_eq!(std::fs::read(root.path().join("a.exe")).unwrap(), b"old");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1, "no aside, no temp left");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_failed_aside_rename_leaves_everything_as_it_was() {
        let d = temp("noaside");
        let root = Root::new(d.join("inst"));
        file(&root.path().join("a.exe"), b"old");
        let src = d.join("src.exe");
        file(&src, b"new");
        let fs = Flaky { fail_rename: 1, count: RefCell::new(0), fail_copy: false };
        assert!(place(&fs, &root, "a.exe", &src, &sha256::of_bytes(b"new"), "100000").is_err());
        assert_eq!(std::fs::read(root.path().join("a.exe")).unwrap(), b"old");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_failed_copy_leaves_no_half_written_file() {
        let d = temp("copyfail");
        let root = Root::new(d.join("inst"));
        file(&root.path().join("a.exe"), b"old");
        let src = d.join("src.exe");
        file(&src, b"new");
        let fs = Flaky { fail_rename: 0, count: RefCell::new(0), fail_copy: true };
        assert!(place(&fs, &root, "a.exe", &src, &sha256::of_bytes(b"new"), "100000").is_err());
        assert_eq!(std::fs::read(root.path().join("a.exe")).unwrap(), b"old");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn refuses_to_write_outside_the_root() {
        let d = temp("outside");
        let root = Root::new(d.join("inst"));
        std::fs::create_dir_all(root.path()).unwrap();
        let src = d.join("src.exe");
        file(&src, b"new");
        let h = sha256::of_bytes(b"new");
        for bad in ["..\\evil.exe", "../evil.exe", "a\\..\\..\\evil.exe", "C:\\evil.exe"] {
            assert!(place(&RealFs, &root, bad, &src, &h, "1").is_err(), "{bad}");
        }
        assert!(!d.join("evil.exe").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn sweeps_only_asides_of_listed_files() {
        let d = temp("sweep");
        let root = Root::new(d.join("inst"));
        for f in ["shellman-bar.old1.exe", "shellman-bar.exe", "mine.old2.dll", "other.old3.exe", "notes.old4.txt"] {
            file(&root.path().join(f), b"x");
        }
        file(&root.path().join("sub").join("shellman-bar.old5.exe"), b"x");
        let n = sweep_asides(&root, &["shellman-bar.exe".into(), "mine.dll".into()]);
        assert_eq!(n, 2);
        assert!(root.path().join("other.old3.exe").exists());
        assert!(root.path().join("notes.old4.txt").exists());
        assert!(root.path().join("sub").join("shellman-bar.old5.exe").exists());
        assert!(root.path().join("shellman-bar.exe").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
