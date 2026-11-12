//! Where the picker opens, and the last folder remembered for each app that asked.

use serde::{Deserialize, Serialize};
use shellman_pick::Request;
use std::path::{Path, PathBuf};

/// How many apps' folders are kept; the least recently used goes first.
const KEEP: usize = 64;

/// The folder to open on, and the name the field starts with. In order: the folder the app
/// insists on; the folder in the name it suggests (`C:\notes\todo.txt`); the last folder used
/// by this app; the app's default; and finally `fallback` (home). Folders that are gone are
/// skipped.
pub fn start(
    req: &Request,
    remembered: Option<&Path>,
    fallback: Option<PathBuf>,
    is_dir: &dyn Fn(&Path) -> bool,
) -> (Option<PathBuf>, String) {
    let mut name = req.file_name.trim().to_owned();
    let mut from_name = None;
    let typed = Path::new(&name);
    if typed.is_absolute() {
        if let (Some(parent), Some(file)) = (typed.parent(), typed.file_name()) {
            if is_dir(parent) {
                from_name = Some(parent.to_path_buf());
                name = file.to_string_lossy().into_owned();
            }
        }
    }
    let candidates = [
        req.folder.clone(),
        from_name,
        remembered.map(Path::to_path_buf),
        req.default_folder.clone(),
    ];
    let folder = candidates
        .into_iter()
        .flatten()
        .find(|d| is_dir(d))
        .or(fallback);
    (folder, name)
}

/// The last folder each app picked in, by who asked (see `Request::client`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Remembered {
    /// Most recent first.
    folders: Vec<(String, PathBuf)>,
}

impl Remembered {
    pub fn get(&self, client: &str) -> Option<&Path> {
        let key = client.to_lowercase();
        self.folders
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, p)| p.as_path())
    }

    /// Remember `folder` for `client` (nothing for an unnamed client).
    pub fn set(&mut self, client: &str, folder: &Path) {
        let key = client.trim().to_lowercase();
        if key.is_empty() {
            return;
        }
        self.folders.retain(|(k, _)| *k != key);
        self.folders.insert(0, (key, folder.to_path_buf()));
        self.folders.truncate(KEEP);
    }

    pub fn load() -> Self {
        shellman_core::config::load(FILE)
    }

    pub fn save(&self) {
        shellman_core::config::save(FILE, self);
    }
}

/// The settings file's name, beside Files' own.
const FILE: &str = "files-pick";

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(p: &Path) -> bool {
        let s = p.to_string_lossy().to_lowercase();
        [r"c:\set", r"c:\named", r"c:\last", r"c:\default"].contains(&s.as_str())
    }

    fn home() -> Option<PathBuf> {
        Some(PathBuf::from(r"C:\home"))
    }

    #[test]
    fn the_apps_folder_wins_then_the_name_then_the_last_one() {
        let mut req = Request {
            folder: Some(PathBuf::from(r"C:\set")),
            default_folder: Some(PathBuf::from(r"C:\default")),
            file_name: r"C:\named\todo.txt".into(),
            ..Default::default()
        };
        let last = Path::new(r"C:\last");
        assert_eq!(start(&req, Some(last), home(), &dirs).0, Some(PathBuf::from(r"C:\set")));
        req.folder = None;
        let (folder, name) = start(&req, Some(last), home(), &dirs);
        assert_eq!(folder, Some(PathBuf::from(r"C:\named")));
        assert_eq!(name, "todo.txt");
        req.file_name = "todo.txt".into();
        assert_eq!(start(&req, Some(last), home(), &dirs).0, Some(PathBuf::from(r"C:\last")));
        assert_eq!(start(&req, None, home(), &dirs).0, Some(PathBuf::from(r"C:\default")));
        req.default_folder = None;
        assert_eq!(start(&req, None, home(), &dirs).0, home());
    }

    #[test]
    fn folders_that_are_gone_are_skipped() {
        let req = Request {
            folder: Some(PathBuf::from(r"C:\gone")),
            file_name: r"C:\gone\x.txt".into(),
            ..Default::default()
        };
        let (folder, name) = start(&req, Some(Path::new(r"C:\last")), home(), &dirs);
        assert_eq!(folder, Some(PathBuf::from(r"C:\last")));
        // The name keeps its folder, since that could not be opened.
        assert_eq!(name, r"C:\gone\x.txt");
    }

    #[test]
    fn remembered_folders_are_per_app_and_most_recent_first() {
        let mut r = Remembered::default();
        r.set("Notepad.exe", Path::new(r"C:\a"));
        r.set("paint.exe", Path::new(r"C:\b"));
        r.set("notepad.exe", Path::new(r"C:\c"));
        r.set("", Path::new(r"C:\ignored"));
        assert_eq!(r.get("NOTEPAD.EXE"), Some(Path::new(r"C:\c")));
        assert_eq!(r.get("paint.exe"), Some(Path::new(r"C:\b")));
        assert_eq!(r.folders.len(), 2);
        assert_eq!(r.folders[0].0, "notepad.exe");
        for i in 0..100 {
            r.set(&format!("app{i}.exe"), Path::new(r"C:\x"));
        }
        assert_eq!(r.folders.len(), KEEP);
        assert_eq!(r.get("notepad.exe"), None);
    }
}
