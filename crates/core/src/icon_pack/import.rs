//! Packs on disk: where they live, which there are, and adding one from a folder.

use super::index::IMAGE_EXTENSIONS;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// `<config dir>\icon-packs`.
pub fn packs_dir() -> Option<PathBuf> {
    crate::config::config_dir().map(|d| d.join("icon-packs"))
}

/// The installed packs: the folders in `icon-packs`, by name.
pub fn list() -> Vec<String> {
    let Some(dir) = packs_dir() else { return Vec::new() };
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        // A copy still being made.
        .filter(|n| !n.ends_with(IMPORTING))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

const IMPORTING: &str = ".importing";

/// What a folder offered for adding holds: one pack, or several themes side by side (the usual
/// shape of a downloaded archive), as (pack name, folder) pairs.
pub fn packs_in(source: &Path) -> Vec<(String, PathBuf)> {
    let name_of = |p: &Path| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    if source.join("index.theme").is_file() {
        return vec![(name_of(source), source.to_path_buf())];
    }
    let mut themes: Vec<(String, PathBuf)> = std::fs::read_dir(source)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("index.theme").is_file())
        .map(|p| (name_of(&p), p))
        .collect();
    if themes.is_empty() {
        // The archive's own folder around the themes (`pack-master/Pack`).
        themes = std::fs::read_dir(source)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .flat_map(|p| std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| e.path()).collect::<Vec<_>>())
            .filter(|p| p.join("index.theme").is_file())
            .map(|p| (name_of(&p), p))
            .collect();
    }
    if !themes.is_empty() {
        themes.sort();
        return themes;
    }
    // A plain folder of images.
    if has_images(source) {
        return vec![(name_of(source), source.to_path_buf())];
    }
    Vec::new()
}

fn has_images(dir: &Path) -> bool {
    std::fs::read_dir(dir).into_iter().flatten().flatten().any(|e| is_image(&e.path()) || (e.path().is_dir() && has_images_shallow(&e.path())))
}

fn has_images_shallow(dir: &Path) -> bool {
    std::fs::read_dir(dir).into_iter().flatten().flatten().any(|e| is_image(&e.path()))
}

fn is_image(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| IMAGE_EXTENSIONS.iter().any(|i| i.eq_ignore_ascii_case(e)))
}

/// A folder name that is safe to make under `icon-packs`.
fn safe_name(name: &str) -> String {
    let cleaned: String = name.chars().map(|c| if r#"\/:*?"<>|"#.contains(c) { '_' } else { c }).collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    if cleaned.is_empty() {
        "pack".to_string()
    } else if super::builtin::is_builtin(&cleaned) {
        // The built-in pack's name means the built-in pack; a copy added from a folder is another.
        format!("{cleaned} (folder)")
    } else {
        cleaned
    }
}

/// Copy what a pack needs (its `index.theme`, and the images; for a theme, only the app icons)
/// into `icon-packs`, replacing a pack of the same name. `count` goes up with each file copied,
/// for showing progress. Returns the packs added.
pub fn add_from_folder(source: &Path, count: &AtomicUsize) -> io::Result<Vec<String>> {
    let dest_root = packs_dir().ok_or_else(|| io::Error::other("no config folder"))?;
    let found = packs_in(source);
    if found.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "no icons in that folder"));
    }
    // Several themes only make sense together: the dark one inherits from the plain one.
    let mut added = Vec::new();
    for (name, folder) in found {
        let name = safe_name(&name);
        let tmp = dest_root.join(format!("{name}{IMPORTING}"));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp)?;
        let theme = folder.join("index.theme").is_file();
        copy_pack(&folder, &tmp, theme, false, 0, count)?;
        let dest = dest_root.join(&name);
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::rename(&tmp, &dest)?;
        added.push(name);
    }
    Ok(added)
}

fn copy_pack(from: &Path, to: &Path, theme: bool, in_apps: bool, depth: usize, count: &AtomicUsize) -> io::Result<()> {
    if depth > 6 {
        return Ok(());
    }
    for entry in std::fs::read_dir(from)?.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let lower = name.to_string_lossy().to_ascii_lowercase();
        // A directory symlink, after an archive was extracted on Windows, is a small file naming
        // the directory it stands for (`Papirus-Dark/64x64` holds `../Papirus/64x64`).
        let path = match linked_dir(&path, &lower, theme, from) {
            Some(target) => target,
            None => path,
        };
        if path.is_dir() {
            // Tiny sizes are never the best a name has, and the biggest part of a big theme.
            let tiny = super::index::dir_size(&lower).is_some_and(|s| s < 32);
            if theme && (lower.contains("symbolic") || tiny) {
                continue;
            }
            let child_apps = in_apps || lower == "apps";
            // Of a theme only the `apps` contexts are wanted, but they may sit above or below
            // the size directory, so every directory is entered and its files decide.
            let sub = to.join(&name);
            copy_pack(&path, &sub, theme, child_apps, depth + 1, count)?;
            // Leave no empty folders behind.
            let _ = std::fs::remove_dir(&sub);
            continue;
        }
        let wanted = if theme { lower == "index.theme" && depth == 0 || (in_apps && is_image(&path)) } else { is_image(&path) };
        if !wanted {
            continue;
        }
        std::fs::create_dir_all(to)?;
        std::fs::copy(&path, to.join(&name))?;
        count.fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}

/// The directory a size-named link file points at, if it is one.
fn linked_dir(path: &Path, lower_name: &str, theme: bool, parent: &Path) -> Option<PathBuf> {
    if !theme || path.is_dir() || super::index::dir_size(lower_name).is_none() {
        return None;
    }
    if path.metadata().ok()?.len() > 200 {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let target = parent.join(text.trim());
    target.is_dir().then_some(target)
}

/// Remove an installed pack.
pub fn remove(name: &str) -> io::Result<()> {
    let Some(dir) = packs_dir() else { return Ok(()) };
    let target = dir.join(safe_name(name));
    if target.parent() == Some(dir.as_path()) && target.is_dir() {
        std::fs::remove_dir_all(target)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shellman-import-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put(dir: &Path, rel: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "x").unwrap();
    }

    #[test]
    fn finds_one_theme_or_several_or_a_plain_folder() {
        let one = temp("one");
        put(&one, "index.theme");
        assert_eq!(packs_in(&one).len(), 1);

        let several = temp("several");
        put(&several, "A/index.theme");
        put(&several, "B/index.theme");
        put(&several, "readme.md");
        assert_eq!(packs_in(&several).iter().map(|p| p.0.as_str()).collect::<Vec<_>>(), ["A", "B"]);

        let wrapped = temp("wrapped");
        put(&wrapped, "pack-master/Pack/index.theme");
        put(&wrapped, "pack-master/Pack-Dark/index.theme");
        assert_eq!(packs_in(&wrapped).len(), 2);

        let plain = temp("plain");
        put(&plain, "firefox.png");
        assert_eq!(packs_in(&plain).len(), 1);

        let nothing = temp("nothing");
        put(&nothing, "notes.txt");
        assert!(packs_in(&nothing).is_empty());
        assert!(packs_in(Path::new(r"Z:\no\such")).is_empty());
        for d in [one, several, wrapped, plain, nothing] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn copying_a_theme_keeps_only_app_icons() {
        let src = temp("src-theme");
        put(&src, "index.theme");
        put(&src, "64x64/apps/a.svg");
        put(&src, "64x64/places/folder.svg");
        put(&src, "apps/32/b.png");
        put(&src, "symbolic/apps/c-symbolic.svg");
        put(&src, "64x64/apps/notes.txt");
        let dst = temp("dst-theme");
        let count = AtomicUsize::new(0);
        copy_pack(&src, &dst, true, false, 0, &count).unwrap();
        assert!(dst.join("index.theme").is_file());
        assert!(dst.join("64x64/apps/a.svg").is_file());
        assert!(dst.join("apps/32/b.png").is_file());
        assert!(!dst.join("64x64/places").exists());
        assert!(!dst.join("symbolic").exists());
        assert!(!dst.join("64x64/apps/notes.txt").exists());
        assert_eq!(count.load(Ordering::Relaxed), 3);
        for d in [src, dst] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn a_directory_link_file_is_followed() {
        let src = temp("src-dirlink");
        put(&src, "Light/index.theme");
        put(&src, "Light/64x64/apps/a.svg");
        put(&src, "Dark/index.theme");
        std::fs::write(src.join("Dark/64x64"), "../Light/64x64").unwrap();
        let dst = temp("dst-dirlink");
        copy_pack(&src.join("Dark"), &dst, true, false, 0, &AtomicUsize::new(0)).unwrap();
        assert!(dst.join("64x64/apps/a.svg").is_file());
        for d in [src, dst] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn adding_makes_a_pack_under_the_config_folder() {
        let src = temp("src-add");
        put(&src, "Mine/index.theme");
        put(&src, "Mine/64x64/apps/a.svg");
        let config = temp("cfg-add");
        // The override is process-wide: other tests in this crate that read the config folder
        // are not run against a changed one, since this is the only test that sets it.
        std::env::set_var(crate::config::DIR_OVERRIDE, &config);
        let added = add_from_folder(&src, &AtomicUsize::new(0)).unwrap();
        assert_eq!(added, ["Mine"]);
        assert_eq!(list(), ["Mine"]);
        assert!(config.join("icon-packs/Mine/64x64/apps/a.svg").is_file());
        // Again: replaced, not merged.
        add_from_folder(&src, &AtomicUsize::new(0)).unwrap();
        assert_eq!(list(), ["Mine"]);
        remove("Mine").unwrap();
        assert!(list().is_empty());
        std::env::remove_var(crate::config::DIR_OVERRIDE);
        for d in [src, config] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn names_are_made_safe() {
        assert_eq!(safe_name("a/b:c"), "a_b_c");
        assert_eq!(safe_name(".."), "pack");
        assert_eq!(safe_name(" Papirus "), "Papirus");
        assert_eq!(safe_name("islands"), "islands (folder)");
    }
}
