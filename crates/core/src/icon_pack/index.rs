//! A pack read once into memory: name to best image file. Lookups never touch the disk.
//!
//! Two kinds of folder are read:
//! - a freedesktop icon theme (`index.theme`, with `apps` directories at several sizes, either
//!   `64x64/apps` or `apps/64`), where the bigger sizes and SVG come first and an app's icon may
//!   be an alias for another file (a symlink, or, after an archive was extracted on Windows, a
//!   small text file holding the target's name);
//! - any folder of `.png`, `.svg` and `.ico` files named by app.

use super::names::{alias_targets, squash};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A link file is tiny: just a file name.
const LINK_MAX_BYTES: u64 = 300;
/// Folders deeper than this under a pack are not searched.
const MAX_DEPTH: usize = 5;
/// How many themes a pack may inherit from, one after the other.
const MAX_INHERIT: usize = 3;

pub const IMAGE_EXTENSIONS: [&str; 3] = ["svg", "png", "ico"];

#[derive(Default, Debug)]
pub struct PackIndex {
    /// Lower-case name to its best file and how good that is (higher wins).
    exact: HashMap<String, Entry>,
    /// Letters and digits only, to the exact name it came from.
    squashed: HashMap<String, String>,
    /// The last word of a reverse-DNS name (`org.mozilla.firefox` is `firefox`), to its exact name.
    last: HashMap<String, String>,
}

/// What the pack has under a name.
#[derive(Debug)]
struct Entry {
    /// How good it is (higher wins): earlier directories, SVG over PNG, the theme over what it
    /// inherits.
    rank: u32,
    path: PathBuf,
    /// Small enough to be a link to another icon, which an archive extracted on Windows leaves in
    /// place of a symlink. Only read when the name is looked up, so reading the pack stays fast.
    maybe_link: bool,
}

impl PackIndex {
    pub fn len(&self) -> usize {
        self.exact.len()
    }

    pub fn is_empty(&self) -> bool {
        self.exact.is_empty()
    }

    /// Read the pack in `dir`. `siblings` is the folder holding the other packs, where an
    /// inherited theme is looked for.
    pub fn build(dir: &Path, siblings: Option<&Path>) -> Self {
        let mut index = PackIndex::default();
        let mut seen: Vec<PathBuf> = Vec::new();
        index.add_theme(dir, siblings, 0, &mut seen);
        index.finish();
        index
    }

    /// An index of files known by name already (the built-in pack, which is not on disk).
    pub fn from_names(files: impl IntoIterator<Item = (String, PathBuf)>) -> Self {
        let mut index = PackIndex::default();
        for (name, path) in files {
            if let Some(name) = pack_name(&name) {
                index.insert(name, u32::MAX / 2, path, false);
            }
        }
        index.finish();
        index
    }

    fn add_theme(&mut self, dir: &Path, siblings: Option<&Path>, depth: usize, seen: &mut Vec<PathBuf>) {
        if seen.iter().any(|s| s == dir) {
            return;
        }
        seen.push(dir.to_path_buf());
        // Lower than anything the theme itself has, and lower the further it is inherited.
        let base = u32::MAX / 2 - (depth as u32) * (u32::MAX / 8);
        let theme = dir.join("index.theme");
        let found = theme.is_file() && self.add_freedesktop(dir, base);
        if !found {
            self.add_simple(dir, base);
        }
        if depth < MAX_INHERIT {
            for parent in inherits(&theme) {
                if let Some(sib) = siblings {
                    let path = sib.join(&parent);
                    if path.is_dir() {
                        self.add_theme(&path, siblings, depth + 1, seen);
                    }
                }
            }
        }
    }

    /// Read the `apps` directories of a theme. False when it has none.
    fn add_freedesktop(&mut self, root: &Path, base: u32) -> bool {
        let mut dirs: Vec<(Option<u32>, PathBuf)> = Vec::new();
        collect_app_dirs(root, 0, false, None, &mut dirs);
        if dirs.is_empty() {
            return false;
        }
        dirs.sort_by_key(|(size, path)| (dir_order(*size), path.clone()));
        for (order, (_, path)) in dirs.iter().enumerate() {
            // Earlier directories win: the rank falls with each one.
            let rank = base.saturating_sub(order as u32);
            self.add_dir(path, rank);
        }
        true
    }

    fn add_dir(&mut self, dir: &Path, rank: u32) {
        let Ok(read) = std::fs::read_dir(dir) else { return };
        for entry in read.flatten() {
            let path = entry.path();
            let Some(ext) = image_ext(&path) else { continue };
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
            let Some(name) = pack_name(stem) else { continue };
            // A name an earlier (better) directory has needs no look at this file.
            if self.exact.get(&name).is_some_and(|e| e.rank >= rank) {
                continue;
            }
            let Ok(kind) = entry.file_type() else { continue };
            // A symlink that was kept is read through by whoever opens it.
            if !(kind.is_file() || kind.is_symlink()) {
                continue;
            }
            let maybe_link = kind.is_file() && matches!(ext, "svg" | "png") && entry.metadata().is_ok_and(|m| m.len() <= LINK_MAX_BYTES);
            self.insert(name, rank, path, maybe_link);
        }
    }

    /// A folder of images named by app.
    fn add_simple(&mut self, dir: &Path, base: u32) {
        let mut stack = vec![(dir.to_path_buf(), 0usize)];
        while let Some((d, depth)) = stack.pop() {
            let Ok(read) = std::fs::read_dir(&d) else { continue };
            for entry in read.flatten() {
                let path = entry.path();
                let Ok(kind) = entry.file_type() else { continue };
                if kind.is_dir() {
                    if depth < 2 {
                        stack.push((path, depth + 1));
                    }
                    continue;
                }
                let Some(ext) = image_ext(&path) else { continue };
                let Some(name) = path.file_stem().and_then(|s| s.to_str()).and_then(pack_name) else { continue };
                // SVG, then PNG, then ICO.
                let rank = base + match ext {
                    "svg" => 2,
                    "png" => 1,
                    _ => 0,
                };
                self.insert(name, rank, path, false);
            }
        }
    }

    fn insert(&mut self, name: String, rank: u32, path: PathBuf, maybe_link: bool) {
        match self.exact.get(&name) {
            Some(e) if e.rank >= rank => {}
            _ => {
                self.exact.insert(name, Entry { rank, path, maybe_link });
            }
        }
    }

    /// Fill the looser lookups once the exact names are all in.
    fn finish(&mut self) {
        let mut names: Vec<&String> = self.exact.keys().collect();
        names.sort();
        let mut squashed = HashMap::with_capacity(names.len());
        let mut last = HashMap::new();
        for name in names {
            squashed.entry(squash(name)).or_insert_with(|| name.clone());
            if let Some((_, tail)) = name.rsplit_once('.') {
                if name.matches('.').count() >= 2 && !tail.is_empty() {
                    last.entry(tail.to_string()).or_insert_with(|| name.clone());
                }
            }
        }
        self.squashed = squashed;
        self.last = last;
    }

    /// The file an entry stands for: its own, or, for a link, the target's (links may chain).
    /// None for a link to nothing.
    fn file(&self, name: &str) -> Option<PathBuf> {
        let mut entry = self.exact.get(name)?;
        for _ in 0..5 {
            if !entry.maybe_link {
                return Some(entry.path.clone());
            }
            match read_link_text(&entry.path) {
                Some(target) => entry = self.exact.get(&target)?,
                // Not a link after all: a very small image.
                None => return Some(entry.path.clone()),
            }
        }
        None
    }

    /// The file for a pack name: exactly, then ignoring separators, then as the last word of a
    /// reverse-DNS name.
    pub fn get(&self, name: &str) -> Option<PathBuf> {
        if let Some(p) = self.file(name) {
            return Some(p);
        }
        let by = self.squashed.get(&squash(name)).or_else(|| self.last.get(name))?;
        self.file(by)
    }

    /// The exact name only (no loose matching), for the alias targets.
    pub fn get_exact(&self, name: &str) -> Option<PathBuf> {
        self.file(name)
    }

    /// The best file for an app's candidate names (best candidate first): the built-in alias
    /// targets of any of them, then the names themselves, then the looser matches.
    pub fn lookup(&self, candidates: &[String]) -> Option<PathBuf> {
        // What the built-in aliases know comes first, for every name the app has; a vague name
        // (`settings`) must not shadow a precise one further down.
        for c in candidates {
            for target in alias_targets(c) {
                if let Some(p) = self.get_exact(target) {
                    return Some(p);
                }
            }
        }
        for c in candidates {
            if let Some(p) = self.get(c) {
                return Some(p);
            }
        }
        // Loosest last: the alias targets by squashed name or reverse-DNS tail.
        for c in candidates {
            for target in alias_targets(c) {
                if let Some(p) = self.get(target) {
                    return Some(p);
                }
            }
        }
        None
    }

    /// A few names the pack has, for a quick look at it: those of `showcase` it can show.
    pub fn sample<'a>(&self, showcase: &[&'a str]) -> Vec<(&'a str, PathBuf)> {
        let mut out = Vec::new();
        for name in showcase {
            let c = [super::names::normalise(name)];
            if let Some(p) = self.lookup(&c) {
                out.push((*name, p));
            }
        }
        out
    }
}

/// `png`, `svg` or `ico`, lower case, for an image file's path; not symbolic icons.
fn image_ext(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let ext = IMAGE_EXTENSIONS.into_iter().find(|e| *e == ext)?;
    let stem = path.file_stem()?.to_str()?;
    (!stem.ends_with("-symbolic") && !stem.ends_with(".symbolic")).then_some(ext)
}

/// The name an image file stands for: lower case, without Wine-style `09BE_` prefix and `.0`
/// suffix (Papirus names Windows programs that way), without a trailing size.
pub fn pack_name(stem: &str) -> Option<String> {
    let mut name = stem.to_lowercase();
    if name.len() > 5 && name.as_bytes()[4] == b'_' && name[..4].chars().all(|c| c.is_ascii_hexdigit()) {
        name = name[5..].to_string();
    }
    if let Some((head, tail)) = name.rsplit_once('.') {
        if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) && !head.is_empty() {
            name = head.to_string();
        }
    }
    let name = name.replace(' ', "-");
    (!name.is_empty()).then_some(name)
}

/// The name a link's target has: `../x/firefox.svg` is `firefox`.
fn link_name(target: &Path) -> Option<String> {
    let file = target.file_name()?.to_str()?;
    pack_name(file.rsplit_once('.').map_or(file, |(s, _)| s))
}

/// A tiny file that only names another icon: what a symlink becomes when an archive is extracted
/// on Windows.
fn read_link_text(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > LINK_MAX_BYTES as usize || bytes.starts_with(b"<") || bytes.starts_with(&[0x89, b'P']) {
        return None;
    }
    let text = std::str::from_utf8(&bytes).ok()?.trim();
    if text.is_empty() || text.contains('\n') || !(text.ends_with(".svg") || text.ends_with(".png")) {
        return None;
    }
    link_name(Path::new(text))
}

/// The themes named on the `Inherits=` line of an `index.theme`.
fn inherits(theme: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(theme) else { return Vec::new() };
    text.lines()
        .find_map(|l| l.trim().strip_prefix("Inherits="))
        .map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}

/// A directory name that gives a size: `64x64`, `64`, `scalable` (the biggest).
pub(super) fn dir_size(name: &str) -> Option<u32> {
    let lower = name.to_ascii_lowercase();
    if lower == "scalable" {
        return Some(u32::MAX);
    }
    let first = lower.split('x').next()?;
    (!first.is_empty() && first.len() <= 4 && first.chars().all(|c| c.is_ascii_digit())).then(|| first.parse().ok())?
}

/// Which directory is read first: scalable, then the biggest size up to 256 px, then bigger ones,
/// then those with no size.
fn dir_order(size: Option<u32>) -> (u8, u32) {
    match size {
        Some(u32::MAX) => (0, 0),
        Some(s) if s <= 256 => (1, u32::MAX - s),
        Some(s) => (2, s),
        None => (3, 0),
    }
}

/// Every directory under `apps` (in either layout, `64x64/apps` or `apps/64`) with its size.
fn collect_app_dirs(dir: &Path, depth: usize, in_apps: bool, size: Option<u32>, out: &mut Vec<(Option<u32>, PathBuf)>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut has_files = false;
    let mut subdirs = Vec::new();
    for entry in read.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        // A symlinked directory counts as one.
        let is_dir = kind.is_dir() || (kind.is_symlink() && entry.path().is_dir());
        if is_dir {
            subdirs.push(entry);
        } else {
            has_files = true;
        }
    }
    if in_apps && has_files {
        out.push((size, dir.to_path_buf()));
    }
    for entry in subdirs {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if name.contains("symbolic") {
            continue;
        }
        let child_apps = in_apps || name == "apps";
        let child_size = size.or_else(|| dir_size(&name));
        collect_app_dirs(&entry.path(), depth + 1, child_apps, child_size, out);
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use std::path::{Path, PathBuf};

    /// A fresh empty folder, different on every call: tests share a process and run in parallel, so
    /// two calls with the same tag must not share (and delete) one folder.
    pub fn temp(tag: &str) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("shellman-iconpack-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub fn touch(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Real image files are bigger than a link; pad so they are not taken for one.
        let mut text = content.to_string();
        while text.len() < 400 {
            text.push(' ');
        }
        std::fs::write(path, text).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::{temp, touch};
    use super::*;
    use crate::icon_pack::names::{candidates, AppRef};

    #[test]
    fn pack_names_drop_wine_decorations() {
        assert_eq!(pack_name("09BE_EpicGamesLauncher.0").as_deref(), Some("epicgameslauncher"));
        assert_eq!(pack_name("Firefox").as_deref(), Some("firefox"));
        assert_eq!(pack_name("org.mozilla.firefox").as_deref(), Some("org.mozilla.firefox"));
        assert_eq!(pack_name("wxyz_x").as_deref(), Some("wxyz_x"));
        assert_eq!(pack_name(""), None);
    }

    #[test]
    fn sizes_come_from_directory_names() {
        assert_eq!(dir_size("64x64"), Some(64));
        assert_eq!(dir_size("48"), Some(48));
        assert_eq!(dir_size("scalable"), Some(u32::MAX));
        assert_eq!(dir_size("apps"), None);
        assert!(dir_order(Some(u32::MAX)) < dir_order(Some(128)));
        assert!(dir_order(Some(128)) < dir_order(Some(64)));
        assert!(dir_order(Some(64)) < dir_order(Some(512)));
        assert!(dir_order(Some(512)) < dir_order(None));
    }

    #[test]
    fn a_freedesktop_theme_prefers_scalable_then_big() {
        let dir = temp("fd");
        touch(&dir, "index.theme", "[Icon Theme]\nName=T\n");
        touch(&dir, "16x16/apps/firefox.svg", "<svg 16/>");
        touch(&dir, "64x64/apps/firefox.svg", "<svg 64/>");
        touch(&dir, "64x64/apps/gimp.png", "png");
        touch(&dir, "16x16/apps/only-small.png", "png");
        touch(&dir, "64x64/places/folder.svg", "<svg/>");
        touch(&dir, "symbolic/apps/firefox-symbolic.svg", "<svg/>");
        let index = PackIndex::build(&dir, None);
        assert!(index.get("firefox").unwrap().to_string_lossy().contains("64x64"));
        assert!(index.get("only-small").is_some(), "a name only a small size has");
        assert!(index.get("folder").is_none(), "only app icons");
        assert!(index.get("firefox-symbolic").is_none());
        assert_eq!(index.len(), 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_other_layout_with_the_size_below_apps() {
        let dir = temp("fd2");
        touch(&dir, "index.theme", "[Icon Theme]\n");
        touch(&dir, "apps/32/vlc.svg", "<svg/>");
        touch(&dir, "apps/scalable/vlc.svg", "<svg/>");
        touch(&dir, "status/32/other.svg", "<svg/>");
        let index = PackIndex::build(&dir, None);
        assert!(index.get("vlc").unwrap().to_string_lossy().contains("scalable"));
        assert!(index.get("other").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn text_link_files_follow_their_target() {
        let dir = temp("links");
        touch(&dir, "index.theme", "[Icon Theme]\n");
        touch(&dir, "64x64/apps/org.mozilla.firefox.svg", "<svg real/>");
        // What a symlink becomes after extraction on Windows.
        std::fs::write(dir.join("64x64/apps/firefox.svg"), "org.mozilla.firefox.svg").unwrap();
        std::fs::write(dir.join("64x64/apps/ff.svg"), "firefox.svg").unwrap();
        std::fs::write(dir.join("64x64/apps/09BE_Epic.0.svg"), "../apps/org.mozilla.firefox.svg\n").unwrap();
        std::fs::write(dir.join("64x64/apps/dangling.svg"), "nothing.svg").unwrap();
        let index = PackIndex::build(&dir, None);
        let real = index.get("org.mozilla.firefox").unwrap().to_path_buf();
        assert_eq!(index.get("firefox").unwrap(), real);
        assert_eq!(index.get("ff").unwrap(), real, "a chain of links");
        assert_eq!(index.get("epic").unwrap(), real);
        assert!(index.get("dangling").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_simple_folder_prefers_svg_over_png_over_ico() {
        let dir = temp("simple");
        touch(&dir, "firefox.png", "png");
        touch(&dir, "firefox.svg", "<svg/>");
        touch(&dir, "Visual Studio Code.ico", "ico");
        touch(&dir, "sub/gimp.png", "png");
        touch(&dir, "readme.txt", "no");
        let index = PackIndex::build(&dir, None);
        assert!(index.get("firefox").unwrap().extension().unwrap() == "svg");
        assert!(index.get("visual-studio-code").is_some());
        assert!(index.get("gimp").is_some());
        assert_eq!(index.len(), 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn inherited_themes_fill_the_gaps_but_never_win() {
        let root = temp("inherit");
        let dark = root.join("Dark");
        touch(&dark, "index.theme", "[Icon Theme]\nInherits=Base,Missing\n");
        touch(&dark, "64x64/apps/own.svg", "<svg/>");
        touch(&dark, "16x16/apps/shared.svg", "<svg dark/>");
        touch(&root, "Base/index.theme", "[Icon Theme]\nInherits=Dark\n");
        touch(&root, "Base/64x64/apps/shared.svg", "<svg base/>");
        touch(&root, "Base/64x64/apps/base-only.svg", "<svg/>");
        let index = PackIndex::build(&dark, Some(&root));
        assert!(index.get("shared").unwrap().to_string_lossy().contains("Dark"), "the theme's own wins, though smaller");
        assert!(index.get("base-only").is_some());
        assert!(index.get("own").is_some());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn loose_matches_ignore_separators_and_use_reverse_dns_tails() {
        let dir = temp("loose");
        touch(&dir, "visual-studio-code.svg", "<svg/>");
        touch(&dir, "org.mozilla.firefox.svg", "<svg/>");
        touch(&dir, "com.obsproject.Studio.svg", "<svg/>");
        let index = PackIndex::build(&dir, None);
        assert!(index.get("visualstudiocode").is_some());
        assert!(index.get("visual-studio-code").is_some());
        assert!(index.get("firefox").is_some(), "the last word of a reverse-DNS name");
        assert!(index.get("studio").is_some());
        assert!(index.get("chrome").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn lookup_goes_by_candidate_order_then_alias_then_name() {
        let dir = temp("lookup");
        touch(&dir, "visual-studio-code.svg", "<svg/>");
        touch(&dir, "code.svg", "<svg generic/>");
        touch(&dir, "google-chrome.svg", "<svg/>");
        touch(&dir, "microsoft-edge.svg", "<svg/>");
        touch(&dir, "windows-terminal.svg", "<svg/>");
        let index = PackIndex::build(&dir, None);
        let find = |exe: &str, name: Option<&str>| {
            let c = candidates(&AppRef { id: "", exe: Some(Path::new(exe)), name });
            index.lookup(&c).map(|p| p.file_stem().unwrap().to_string_lossy().to_string())
        };
        // The alias beats the pack's own generic `code`.
        assert_eq!(find(r"C:\x\Code.exe", None).as_deref(), Some("visual-studio-code"));
        assert_eq!(find(r"C:\x\chrome.exe", None).as_deref(), Some("google-chrome"));
        assert_eq!(find(r"C:\x\msedge.exe", None).as_deref(), Some("microsoft-edge"));
        // Through the shortcut's name when the program's name says nothing.
        assert_eq!(find(r"C:\x\launcher.exe", Some("Google Chrome")).as_deref(), Some("google-chrome"));
        assert_eq!(find(r"C:\x\wt.exe", Some("Windows Terminal")).as_deref(), Some("windows-terminal"));
        assert_eq!(find(r"C:\x\unknown.exe", None), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_empty_or_missing_folder_is_an_empty_index() {
        assert!(PackIndex::build(Path::new(r"Z:\no\such\pack"), None).is_empty());
    }
}
