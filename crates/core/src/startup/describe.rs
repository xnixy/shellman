//! What the disk says about a startup item's target: whether it is still there, which program it
//! is, and that program's own description. Touches the disk, so it runs on a worker.

use super::items::{Item, Origin};
use super::rows::{exe_of_command, Found};
use crate::fsops::link_target;
use crate::system::app_name::file_description;
use std::path::{Path, PathBuf};

/// Whether `p` is a file. A bare name (`rundll32.exe`) is looked for where Windows would look:
/// System32, the Windows folder, then PATH.
fn resolve(p: &Path) -> Option<PathBuf> {
    if p.components().count() > 1 || p.is_absolute() {
        return p.is_file().then(|| p.to_path_buf());
    }
    let root = std::env::var_os("SystemRoot").map(PathBuf::from)?;
    let mut dirs = vec![root.join("System32"), root.clone()];
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs.into_iter().map(|d| d.join(p)).find(|c| c.is_file())
}

pub fn probe(item: &Item) -> Found {
    let (exe, known) = match &item.origin {
        Origin::Run { .. } => {
            let exe = exe_of_command(&item.command, &|p| resolve(p).is_some());
            (exe, true)
        }
        Origin::Folder { .. } => {
            let path = PathBuf::from(&item.command);
            let is_link = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk"));
            if is_link {
                // A shortcut to something that is not a file (a shell folder, an app id) leaves
                // nothing to check; its own icon is still the right one to show.
                match link_target(&path) {
                    Some(t) => (Some(t), true),
                    None => (Some(path), false),
                }
            } else {
                (Some(path), true)
            }
        }
    };
    let Some(exe) = exe else { return Found::default() };
    let found = resolve(&exe);
    Found {
        description: found.as_deref().and_then(file_description),
        missing: known && found.is_none(),
        exe: Some(found.unwrap_or(exe)),
    }
}
