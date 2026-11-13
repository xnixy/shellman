//! The folders the shelf and the desktop menu use: the user's Desktop (and the common Desktop),
//! Downloads, Recent and Documents, as Windows' known folders.
//!
//! `SHELLMAN_SHELF_FOLDERS=<dir>` puts all of them under one folder instead (`<dir>\Desktop`,
//! `<dir>\Downloads`, `<dir>\Recent`, `<dir>\Documents`, and no common Desktop), so tests drop,
//! paste and create files in throwaway folders and never in the user's own.

use shellman_core::settings::desktop::DropTo;
use std::path::{Path, PathBuf};
use windows::core::GUID;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_PublicDesktop, FOLDERID_Recent,
    SHGetKnownFolderPath, KF_FLAG_DEFAULT,
};

/// Names a folder that stands in for all the known folders (see the module's note).
pub const OVERRIDE: &str = "SHELLMAN_SHELF_FOLDERS";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Folders {
    pub desktop: Option<PathBuf>,
    /// The Desktop every user shares; shown with the user's when it has anything.
    pub public_desktop: Option<PathBuf>,
    pub downloads: Option<PathBuf>,
    pub recent: Option<PathBuf>,
    pub documents: Option<PathBuf>,
}

impl Folders {
    /// Ask Windows (or the override) where the folders are.
    pub fn resolve() -> Self {
        match std::env::var_os(OVERRIDE).filter(|o| !o.is_empty()) {
            Some(root) => Self::under(Path::new(&root)),
            None => Self {
                desktop: known(&FOLDERID_Desktop),
                public_desktop: known(&FOLDERID_PublicDesktop),
                downloads: known(&FOLDERID_Downloads),
                recent: known(&FOLDERID_Recent),
                documents: known(&FOLDERID_Documents),
            },
        }
    }

    /// Every folder under `root`, made if missing; no common Desktop.
    pub fn under(root: &Path) -> Self {
        let sub = |name: &str| {
            let p = root.join(name);
            let _ = std::fs::create_dir_all(&p);
            Some(p)
        };
        Self {
            desktop: sub("Desktop"),
            public_desktop: None,
            downloads: sub("Downloads"),
            recent: sub("Recent"),
            documents: sub("Documents"),
        }
    }

    /// Where dropped files go.
    pub fn drop_target(&self, to: &DropTo) -> Option<PathBuf> {
        match to {
            DropTo::Desktop => self.desktop.clone(),
            DropTo::Downloads => self.downloads.clone(),
            DropTo::Documents => self.documents.clone(),
            DropTo::Folder(p) => Some(p.clone()),
        }
    }

    /// Every folder the shelf lists, for the folder watch.
    pub fn watched(&self) -> Vec<PathBuf> {
        [&self.desktop, &self.public_desktop, &self.downloads, &self.recent].into_iter().flatten().cloned().collect()
    }
}

fn known(id: &GUID) -> Option<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let path = p.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(p.0 as *const _));
        path.filter(|p| !p.as_os_str().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_override_keeps_everything_in_one_place() {
        let root = std::env::temp_dir().join(format!("shellman-shelf-folders-{}", std::process::id()));
        let f = Folders::under(&root);
        assert_eq!(f.desktop.as_deref(), Some(root.join("Desktop").as_path()));
        assert!(root.join("Downloads").is_dir());
        assert_eq!(f.public_desktop, None);
        assert_eq!(f.drop_target(&DropTo::Documents), Some(root.join("Documents")));
        assert_eq!(f.drop_target(&DropTo::Folder(PathBuf::from("D:\\x"))), Some(PathBuf::from("D:\\x")));
        assert_eq!(f.watched().len(), 3);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn windows_knows_the_downloads_folder() {
        assert!(known(&FOLDERID_Downloads).is_some_and(|p| p.is_absolute()));
    }
}
