//! The one place that decides where setup may write or delete: inside the install folder.
//! Every destructive operation takes a path made by [`Root::join`] (relative names only, no
//! `..`, no drive, no device names) and is checked again by [`Root::guard`] right before it runs
//! (still inside the folder, and not reached through a link that leaves it).

use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Root(PathBuf);

const DEVICES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2",
    "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Whether one path component is acceptable as a name.
fn good_component(c: &str) -> Result<(), String> {
    if c.is_empty() || c == "." || c == ".." {
        return Err(format!("bad name component '{c}'"));
    }
    if c.chars().any(|ch| ch.is_control() || "<>:\"|?*".contains(ch)) {
        return Err(format!("bad character in '{c}'"));
    }
    if c.ends_with('.') || c.ends_with(' ') {
        return Err(format!("'{c}' ends in a dot or space"));
    }
    let stem = c.split('.').next().unwrap_or("").to_ascii_lowercase();
    if DEVICES.contains(&stem.as_str()) {
        return Err(format!("'{c}' is a device name"));
    }
    Ok(())
}

/// A relative path as written in a manifest (`/` or `\`), checked, as its components.
pub fn components(rel: &str) -> Result<Vec<&str>, String> {
    if rel.is_empty() {
        return Err("empty path".into());
    }
    if rel.starts_with(['/', '\\']) {
        return Err(format!("'{rel}' is not relative"));
    }
    let parts: Vec<&str> = rel.split(['/', '\\']).collect();
    for p in &parts {
        good_component(p).map_err(|e| format!("'{rel}': {e}"))?;
    }
    Ok(parts)
}

impl Root {
    pub fn new(path: impl Into<PathBuf>) -> Root {
        Root(path.into())
    }

    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.0
    }

    /// `rel` below the root, or why it is refused.
    pub fn join(&self, rel: &str) -> Result<PathBuf, String> {
        let mut p = self.0.clone();
        for c in components(rel)? {
            p.push(c);
        }
        Ok(p)
    }

    /// Checked again before an operation touches `path`: it must be below the root with plain
    /// components only, and what exists of it must not lead out of the root through a link.
    pub fn guard(&self, path: &Path) -> Result<(), String> {
        let rest = path.strip_prefix(&self.0).map_err(|_| format!("{} is outside {}", path.display(), self.0.display()))?;
        if rest.as_os_str().is_empty() {
            return Err("refusing to operate on the install folder itself".into());
        }
        for c in rest.components() {
            match c {
                Component::Normal(n) => good_component(&n.to_string_lossy()).map_err(|e| format!("{}: {e}", path.display()))?,
                _ => return Err(format!("{} is not a plain path below the install folder", path.display())),
            }
        }
        // Links: the nearest existing parent must really be inside the root.
        if let (Ok(root), Some(parent)) = (std::fs::canonicalize(&self.0), path.parent()) {
            let mut p = parent;
            loop {
                if let Ok(real) = std::fs::canonicalize(p) {
                    if !real.starts_with(&root) {
                        return Err(format!("{} leads outside {}", path.display(), self.0.display()));
                    }
                    break;
                }
                match p.parent() {
                    Some(up) if up.starts_with(&self.0) => p = up,
                    _ => break,
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> Root {
        Root::new(r"C:\Users\u\AppData\Local\Programs\shellman")
    }

    #[test]
    fn joins_plain_relative_names() {
        assert_eq!(root().join("shellman-bar.exe").unwrap(), Path::new(r"C:\Users\u\AppData\Local\Programs\shellman\shellman-bar.exe"));
        assert!(root().join("redist/conpty.dll").unwrap().ends_with(r"redist\conpty.dll"));
        assert!(root().join(r"a\b\c.txt").is_ok());
    }

    #[test]
    fn refuses_traversal_and_absolute_paths() {
        for bad in [
            "..",
            "../x.exe",
            r"..\x.exe",
            "a/../../x",
            "a/..",
            "./x",
            "a//b",
            "/x",
            r"\x",
            r"C:\Windows\x.exe",
            "C:x",
            r"\\server\share\x",
            "x:y",
            "",
            "nul",
            "a/con.txt",
            "bad.",
            "a /b",
            "q?.exe",
            "a\u{0}b",
        ] {
            assert!(root().join(bad).is_err(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn guard_keeps_operations_inside() {
        let r = root();
        assert!(r.guard(&r.join("a.exe").unwrap()).is_ok());
        assert!(r.guard(&r.join("sub/a.exe").unwrap()).is_ok());
        for bad in [
            r"C:\Users\u\AppData\Local\Programs\shellman",
            r"C:\Users\u\AppData\Local\Programs\shellman\..\x.exe",
            r"C:\Users\u\AppData\Local\Programs\shellman\..\..\x.exe",
            r"C:\Users\u\AppData\Local\Programs\shellmanx\a.exe",
            r"C:\Users\u\AppData\Local\Programs\a.exe",
            r"C:\Windows\System32\cmd.exe",
            r"D:\shellman\a.exe",
        ] {
            assert!(r.guard(Path::new(bad)).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn guard_catches_a_link_out_of_the_root() {
        let base = std::env::temp_dir().join(format!("shellman-setup-guard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (inside, outside) = (base.join("in"), base.join("out"));
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        // A directory junction (no privilege needed) from inside to outside.
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(inside.join("link"))
            .arg(&outside)
            .output();
        let r = Root::new(&inside);
        if status.is_ok_and(|o| o.status.success()) {
            assert!(r.guard(&inside.join("link").join("victim.exe")).is_err());
        }
        assert!(r.guard(&inside.join("real").join("a.exe")).is_ok());
        let _ = std::fs::remove_dir(inside.join("link"));
        let _ = std::fs::remove_dir_all(&base);
    }
}
