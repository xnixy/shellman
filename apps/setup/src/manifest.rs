//! What gets installed: the file list, built from a source folder.
//!
//! 1. In the folder itself: every `shellman-*.exe` (not the `.old<digits>.exe` leftovers of an
//!    earlier update), `shellman_pickhook.dll`, and licence files (`LICENSE*`, `NOTICE*`,
//!    `THIRD-PARTY*`).
//! 2. Everything below an `extras` folder in it (and below each `--extras <dir>`), keeping its
//!    sub-path, installed below the install folder. Redistributables go here.
//! 3. The lines of an `install-manifest` file: the one in the folder, else the first found in
//!    the folder's parents (up to 3 levels: a repo's `install-manifest` serves `target\release`).
//!    Format: `source [-> destination]`, `#` comments, a leading `?` means "optional, say
//!    nothing when it is missing". Sources are relative to the manifest's own folder;
//!    destinations are relative names in the install folder (default: the file's name).
//!
//! A file listed twice is installed once (the first wins). Every destination is a checked
//! relative name (`paths::components`), so a manifest cannot write outside the install folder.

use crate::paths;
use std::path::{Path, PathBuf};

pub const MANIFEST_FILE: &str = "install-manifest";
pub const EXTRAS_DIR: &str = "extras";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Destination below the install folder, `\`-separated.
    pub rel: String,
    pub src: PathBuf,
}

#[derive(Default, Debug)]
pub struct Built {
    pub entries: Vec<Entry>,
    pub warnings: Vec<String>,
}

/// `shellman-bar.old123456.exe` and friends: what an update leaves behind. Never installed.
pub fn is_aside(name: &str) -> bool {
    aside_of(name).is_some()
}

/// For `stem.old<mid>.ext`: the name it was renamed from, `stem.ext` (`mid` has no dots).
pub fn aside_of(name: &str) -> Option<String> {
    let dot = name.rfind('.');
    let (base, ext) = match dot {
        Some(i) => (&name[..i], &name[i..]),
        None => (name, ""),
    };
    let i = base.to_ascii_lowercase().rfind(".old")?;
    let mid = &base[i + 4..];
    (i > 0 && !mid.is_empty() && !mid.contains(['.', '\\', '/'])).then(|| format!("{}{ext}", &base[..i]))
}

fn is_core_file(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    if is_aside(&n) {
        return false;
    }
    (n.starts_with("shellman-") && n.ends_with(".exe"))
        || n == "shellman_pickhook.dll"
        || n.starts_with("license")
        || n.starts_with("notice")
        || n.starts_with("third-party")
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<Entry>, depth: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut items: Vec<_> = rd.flatten().collect();
    items.sort_by_key(|e| e.file_name());
    for e in items {
        let name = e.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}\\{name}") };
        match e.file_type() {
            Ok(t) if t.is_file() => out.push(Entry { rel, src: e.path() }),
            Ok(t) if t.is_dir() && depth < 6 => walk(&e.path(), &rel, out, depth + 1),
            _ => {}
        }
    }
}

/// Parse a manifest's text. Pure: `(source, destination, optional)` per line, or an error text.
pub fn parse_manifest(text: &str) -> Vec<Result<(String, Option<String>, bool), String>> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|line| {
            let (optional, line) = match line.strip_prefix('?') {
                Some(r) => (true, r.trim()),
                None => (false, line),
            };
            let (src, dest) = match line.split_once("->") {
                Some((s, d)) => (s.trim(), Some(d.trim().to_string())),
                None => (line, None),
            };
            if src.is_empty() || dest.as_deref() == Some("") {
                return Err(format!("bad line '{line}'"));
            }
            Ok((src.to_string(), dest, optional))
        })
        .collect()
}

/// The manifest file that applies to `source`, if any.
pub fn find_manifest(source: &Path) -> Option<PathBuf> {
    source.ancestors().take(4).map(|d| d.join(MANIFEST_FILE)).find(|p| p.is_file())
}

/// Build the list for `source`, with extra folders given on the command line.
pub fn build(source: &Path, extra_dirs: &[PathBuf]) -> Built {
    let mut out = Built::default();
    let mut all: Vec<Entry> = Vec::new();

    if let Ok(rd) = std::fs::read_dir(source) {
        let mut files: Vec<_> = rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_file())).collect();
        files.sort_by_key(|e| e.file_name());
        for e in files {
            let name = e.file_name().to_string_lossy().into_owned();
            if is_core_file(&name) {
                all.push(Entry { rel: name, src: e.path() });
            }
        }
    }
    for dir in std::iter::once(source.join(EXTRAS_DIR)).chain(extra_dirs.iter().cloned()) {
        if dir.is_dir() {
            walk(&dir, "", &mut all, 0);
        } else if extra_dirs.contains(&dir) {
            out.warnings.push(format!("extras folder {} not found", dir.display()));
        }
    }
    if let Some(mf) = find_manifest(source) {
        let base = mf.parent().unwrap_or(source).to_path_buf();
        match std::fs::read_to_string(&mf) {
            Ok(text) => {
                for line in parse_manifest(&text) {
                    match line {
                        Err(e) => out.warnings.push(format!("{}: {e}", mf.display())),
                        Ok((src, dest, optional)) => {
                            let path = base.join(&src);
                            if !path.is_file() {
                                if !optional {
                                    out.warnings.push(format!("listed in {} but not found: {}", mf.display(), path.display()));
                                }
                                continue;
                            }
                            let rel = dest.unwrap_or_else(|| path.file_name().unwrap_or_default().to_string_lossy().into_owned());
                            all.push(Entry { rel, src: path });
                        }
                    }
                }
            }
            Err(e) => out.warnings.push(format!("cannot read {}: {e}", mf.display())),
        }
    }

    let mut seen = std::collections::HashSet::new();
    for e in all {
        match paths::components(&e.rel) {
            Err(err) => out.warnings.push(format!("skipped: {err}")),
            Ok(parts) => {
                let rel = parts.join("\\");
                if seen.insert(rel.to_ascii_lowercase()) {
                    out.entries.push(Entry { rel, src: e.src });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shellman-setup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    fn rels(b: &Built) -> Vec<&str> {
        b.entries.iter().map(|e| e.rel.as_str()).collect()
    }

    #[test]
    fn asides_are_recognised() {
        assert_eq!(aside_of("shellman-bar.old123456.exe").as_deref(), Some("shellman-bar.exe"));
        assert_eq!(aside_of("shellman_pickhook.old1.dll").as_deref(), Some("shellman_pickhook.dll"));
        assert_eq!(aside_of("README.OLD9.md").as_deref(), Some("README.md"));
        for no in ["shellman-bar.exe", "shellman-bar.old.exe", "shellman-bar.old1.2.exe", ".old1.exe", "old1.exe"] {
            assert!(!is_aside(no), "{no}");
        }
    }

    #[test]
    fn core_files_are_the_exes_the_pickhook_and_licences() {
        let d = temp("core");
        for f in [
            "shellman-bar.exe",
            "shellman-session.exe",
            "shellman-bar.old123456.exe",
            "shellman_pickhook.dll",
            "shellman_glass.dll",
            "shellman-bar.pdb",
            "LICENSE-MIT",
            "random.exe",
            "libshellman_core.rlib",
        ] {
            touch(&d.join(f));
        }
        let b = build(&d, &[]);
        assert_eq!(rels(&b), ["LICENSE-MIT", "shellman-bar.exe", "shellman-session.exe", "shellman_pickhook.dll"]);
        assert!(b.warnings.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn extras_keep_their_sub_paths() {
        let d = temp("extras");
        touch(&d.join("shellman-bar.exe"));
        touch(&d.join("extras").join("conpty.dll"));
        touch(&d.join("extras").join("sub").join("OpenConsole.exe"));
        let other = temp("extras-other");
        touch(&other.join("font.ttf"));
        let b = build(&d, &[other.clone()]);
        assert_eq!(rels(&b), ["shellman-bar.exe", "conpty.dll", "sub\\OpenConsole.exe", "font.ttf"]);
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn manifest_lines_are_parsed() {
        let lines = parse_manifest("# c\n\n..\\redist\\conpty.dll\n? opt.dll -> sub\\o.dll\nbad -> \n");
        assert_eq!(lines[0], Ok(("..\\redist\\conpty.dll".into(), None, false)));
        assert_eq!(lines[1], Ok(("opt.dll".into(), Some("sub\\o.dll".into()), true)));
        assert!(lines[2].is_err());
    }

    #[test]
    fn a_manifest_in_a_parent_applies_and_cannot_escape_the_install_folder() {
        let repo = temp("repo");
        let rel = repo.join("target").join("release");
        touch(&rel.join("shellman-bar.exe"));
        touch(&repo.join("redist").join("conpty.dll"));
        touch(&repo.join("redist").join("OpenConsole.exe"));
        std::fs::write(
            repo.join(MANIFEST_FILE),
            "redist/conpty.dll\n?redist/missing.dll\nredist/OpenConsole.exe -> ..\\evil.exe\nredist/gone.dll\n",
        )
        .unwrap();
        let b = build(&rel, &[]);
        assert_eq!(rels(&b), ["shellman-bar.exe", "conpty.dll"]);
        // The evil destination is skipped with a reason; the missing required file is reported,
        // the missing optional one is not.
        assert_eq!(b.warnings.len(), 2, "{:?}", b.warnings);
        assert!(b.warnings.iter().any(|w| w.contains("..")));
        assert!(b.warnings.iter().any(|w| w.contains("gone.dll")));
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn duplicates_install_once() {
        let d = temp("dups");
        touch(&d.join("shellman-bar.exe"));
        touch(&d.join("extras").join("Shellman-Bar.exe"));
        assert_eq!(build(&d, &[]).entries.len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }
}
