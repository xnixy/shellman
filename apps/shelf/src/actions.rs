//! What the desktop menu and the shelf do: new items, paste, moving dropped files in, and
//! opening things. Anything that touches the disk or waits runs on a thread of its own, and goes
//! through the shell's file operation (`fsops::shell_op`) where files move, so Windows shows its
//! progress and conflict dialogs and the move can be undone.

use shellman_core::fsops::{self, Op};
use shellman_core::transfer::{self, clip, Payload, COPY, LINK, MOVE};
use std::path::{Path, PathBuf};

/// What a "New" entry makes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum New {
    Folder,
    Text,
    Note,
}

impl New {
    /// The name it gets (numbered when taken).
    pub fn name(self) -> &'static str {
        match self {
            New::Folder => "New folder",
            New::Text => "New text document.txt",
            New::Note => "New note.md",
        }
    }
}

/// Make a new item in `dir`; returns its path.
pub fn create(dir: &Path, what: New) -> Result<PathBuf, String> {
    match what {
        New::Folder => fsops::create_new_dir(dir, what.name()).map_err(|e| e.to_string()),
        New::Text | New::Note => fsops::create_new_file(dir, what.name(), b"").map_err(|e| e.to_string()),
    }
}

/// The operation that brings `paths` into `dir` with `effect` (a drop's or a paste's): None
/// when there is nothing to do (moving items into the folder they are in).
pub fn plan(paths: &[PathBuf], dir: &Path, effect: u32) -> Option<Op> {
    if transfer::refused(paths, dir) {
        return None;
    }
    let here = paths.iter().all(|p| p.parent().is_some_and(|q| transfer::path_key(q) == transfer::path_key(dir)));
    match effect {
        MOVE if here => None,
        MOVE => Some(Op::Move),
        COPY if here => Some(Op::Duplicate),
        COPY => Some(Op::Copy),
        _ => None,
    }
}

/// Bring files into `dir`: move, copy, or make shortcuts, as `effect` says. Blocking.
pub fn bring(paths: Vec<PathBuf>, dir: &Path, effect: u32) {
    if effect == LINK {
        for p in &paths {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let link = fsops::unique_child(dir, &format!("{name} - Shortcut.lnk"));
            if let Err(e) = fsops::make_link(p, &link) {
                crate::log::line(format!("shortcut to {}: {e}", p.display()));
            }
        }
        return;
    }
    let Some(op) = plan(&paths, dir, effect) else { return };
    match fsops::shell_op(0, op, &paths, Some(dir)) {
        Ok(done) => crate::log::line(format!("{op:?} {} item(s) into {} (all done: {done})", paths.len(), dir.display())),
        Err(e) => crate::log::line(format!("{op:?} into {}: {e}", dir.display())),
    }
}

/// A new file in `dir` holding a dropped or pasted picture or text, named after `name`
/// (numbered when taken), never over an existing item.
pub fn write_new(dir: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    fsops::create_new_file(dir, name, bytes).map_err(|e| e.to_string())
}

/// What a drop brought, carried out. Blocking.
pub fn dropped(payload: Payload, dir: &Path, effect: u32) {
    let r = match payload {
        Payload::Files(paths) => {
            bring(paths, dir, effect);
            Ok(())
        }
        Payload::Image(png) => write_new(dir, "Dropped image.png", &png).map(|_| ()),
        Payload::Text(t) => write_new(dir, "Dropped text.txt", t.as_bytes()).map(|_| ()),
        Payload::Nothing => Ok(()),
    };
    if let Err(e) = r {
        crate::log::line(format!("drop into {}: {e}", dir.display()));
    }
}

/// Paste the clipboard into `dir`: files (moved after a cut, then the clipboard is emptied, as
/// Explorer does; copied otherwise), or a picture as a new PNG, or text as a new .txt. Blocking.
pub fn paste(dir: &Path) {
    let (payload, effect, seq) = clip::payload();
    match payload {
        Payload::Files(paths) => {
            let cut = effect & MOVE != 0 && effect & COPY == 0;
            bring(paths, dir, if cut { MOVE } else { COPY });
            if cut {
                clip::clear_if(seq);
            }
        }
        Payload::Image(png) => log_err(write_new(dir, "Pasted image.png", &png)),
        Payload::Text(t) => log_err(write_new(dir, "Pasted text.txt", t.as_bytes())),
        Payload::Nothing => crate::log::line("paste: nothing to paste"),
    }
}

fn log_err(r: Result<PathBuf, String>) {
    if let Err(e) = r {
        crate::log::line(format!("paste: {e}"));
    }
}

/// Open a file with its app, or a folder in Files.
pub fn open(path: &Path) {
    if path.is_dir() {
        open_in_files(path);
    } else {
        fsops::open(path);
    }
}

/// Show a folder in shellman's Files.
pub fn open_in_files(dir: &Path) {
    if !shellman_core::sibling::start_with("files", &[&dir.display().to_string()]) {
        crate::log::line(format!("could not start Files for {}", dir.display()));
    }
}

/// Open shellman Settings on a page (`desktop`, `display`).
pub fn settings(page: &str) {
    if !shellman_core::sibling::start_with("settings", &["--page", page]) {
        crate::log::line(format!("could not start Settings on {page}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shellman-shelf-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn new_items_are_numbered_when_taken() {
        let dir = temp("new");
        let a = create(&dir, New::Folder).unwrap();
        let b = create(&dir, New::Folder).unwrap();
        assert_eq!(a.file_name().unwrap(), "New folder");
        assert_eq!(b.file_name().unwrap(), "New folder (2)");
        assert!(b.is_dir());
        let t = create(&dir, New::Text).unwrap();
        let t2 = create(&dir, New::Text).unwrap();
        assert_eq!(t2.file_name().unwrap(), "New text document (2).txt");
        assert!(t.is_file());
        assert_eq!(create(&dir, New::Note).unwrap().file_name().unwrap(), "New note.md");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn plans_follow_the_effect() {
        let dir = Path::new(r"C:\Users\a\Desktop");
        let away = [PathBuf::from(r"C:\Users\a\Downloads\x.zip")];
        let here = [PathBuf::from(r"C:\Users\a\Desktop\x.zip")];
        assert_eq!(plan(&away, dir, MOVE), Some(Op::Move));
        assert_eq!(plan(&away, dir, COPY), Some(Op::Copy));
        assert_eq!(plan(&here, dir, MOVE), None);
        assert_eq!(plan(&here, dir, COPY), Some(Op::Duplicate));
        // The Desktop into itself, or nothing at all.
        assert_eq!(plan(&[PathBuf::from(r"C:\Users\a\Desktop")], dir, MOVE), None);
        assert_eq!(plan(&[], dir, MOVE), None);
    }

    #[test]
    fn dropped_pictures_and_text_become_files() {
        let dir = temp("drop");
        dropped(Payload::Text("hello".into()), &dir, COPY);
        dropped(Payload::Text("again".into()), &dir, COPY);
        assert_eq!(std::fs::read_to_string(dir.join("Dropped text.txt")).unwrap(), "hello");
        assert_eq!(std::fs::read_to_string(dir.join("Dropped text (2).txt")).unwrap(), "again");
        let _ = std::fs::remove_dir_all(dir);
    }
}
