//! Files between apps: dragging out and dropping in (OLE drag and drop), and the Windows
//! clipboard. Both carry the shell's own data object, so every format other apps look for is
//! there (`CF_HDROP`, the shell's item list, file names, …).
//!
//! The rules (what a drop does with which keys held, which drops make no sense) and the formats'
//! bytes are plain functions here, tested without a window or the clipboard; the COM side is in
//! `data` (data objects), `drag` (the source), `target` (the drop target) and `clip`.

pub mod clip;
pub mod data;
pub mod dib;
pub mod drag;
pub mod dropfiles;
pub mod target;

use std::path::{Component, Path, PathBuf};

pub const NONE: u32 = 0;
pub const COPY: u32 = 1;
pub const MOVE: u32 = 2;
pub const LINK: u32 = 4;

/// The key state OLE passes along (`MK_*`).
pub const MK_LBUTTON: u32 = 0x1;
pub const MK_SHIFT: u32 = 0x4;
pub const MK_CONTROL: u32 = 0x8;
pub const MK_ALT: u32 = 0x20;

/// What a drop carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Payload {
    Files(Vec<PathBuf>),
    /// A picture, as PNG bytes.
    Image(Vec<u8>),
    Text(String),
    Nothing,
}

/// What a drop or the clipboard offers, without reading the heavy parts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Offer {
    pub files: Vec<PathBuf>,
    pub image: bool,
    pub text: bool,
}

impl Offer {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && !self.image && !self.text
    }
}

/// A path's volume, as a key: `c:`, or `\\server\share`.
pub fn volume_key(p: &Path) -> Option<String> {
    match p.components().next()? {
        Component::Prefix(pre) => {
            let s = pre.as_os_str().to_string_lossy().to_lowercase();
            // `\\?\C:` and `C:` are the same volume.
            Some(s.trim_start_matches(r"\\?\").to_owned())
        }
        _ => None,
    }
}

/// A path as a key: case, the separator and a trailing separator say nothing.
pub fn path_key(p: &Path) -> String {
    p.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

/// Whether `inner` is `outer` or inside it.
pub fn is_within(inner: &Path, outer: &Path) -> bool {
    let (i, o) = (path_key(inner), path_key(outer));
    i == o || i.starts_with(&format!("{o}\\"))
}

/// A drop that makes no sense: a folder into itself or into something inside it.
pub fn refused(sources: &[PathBuf], dest: &Path) -> bool {
    sources.is_empty() || sources.iter().any(|s| is_within(dest, s))
}

/// What a drop of `sources` into the folder `dest` does, with `keys` held and the source
/// allowing `allowed`, as Explorer has it: a move on the same volume and a copy across volumes;
/// Ctrl copies, Shift moves, Ctrl+Shift or Alt makes shortcuts. Moving items into the folder
/// they are already in does nothing (a copy there makes copies beside them).
pub fn choose(sources: &[PathBuf], dest: &Path, keys: u32, allowed: u32) -> u32 {
    if refused(sources, dest) {
        return NONE;
    }
    let ctrl = keys & MK_CONTROL != 0;
    let shift = keys & MK_SHIFT != 0;
    let forced = if (ctrl && shift) || keys & MK_ALT != 0 {
        Some(LINK)
    } else if ctrl {
        Some(COPY)
    } else if shift {
        Some(MOVE)
    } else {
        None
    };
    let here = sources
        .iter()
        .all(|s| s.parent().is_some_and(|p| path_key(p) == path_key(dest)));
    let wanted = forced.unwrap_or_else(|| {
        let dv = volume_key(dest);
        if dv.is_some() && sources.iter().all(|s| volume_key(s) == dv) {
            MOVE
        } else {
            COPY
        }
    });
    if wanted == MOVE && here {
        return NONE;
    }
    if allowed & wanted != 0 {
        return wanted;
    }
    // Keys held ask for exactly that; without them, whatever the source allows will do.
    if forced.is_some() {
        return NONE;
    }
    [COPY, MOVE]
        .into_iter()
        .find(|&e| allowed & e != 0 && !(e == MOVE && here))
        .unwrap_or(NONE)
}

/// What a drop of `sources` on the Recycle Bin does: they go there, when the source lets them
/// move.
pub fn choose_bin(sources: &[PathBuf], allowed: u32) -> u32 {
    if !sources.is_empty() && allowed & MOVE != 0 {
        MOVE
    } else {
        NONE
    }
}

/// What a drop of a picture or text does: it is written as a new file, a copy.
pub fn choose_content(allowed: u32) -> u32 {
    if allowed & COPY != 0 {
        COPY
    } else {
        NONE
    }
}

/// The verb for an effect, for the drag image's line ("Move to %1").
pub fn verb(effect: u32) -> &'static str {
    match effect {
        MOVE => "Move to %1",
        COPY => "Copy to %1",
        LINK => "Create link in %1",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }
    const ALL: u32 = COPY | MOVE | LINK;

    #[test]
    fn same_volume_moves_and_other_volumes_copy() {
        let src = [p(r"C:\a\x.txt"), p(r"C:\a\y")];
        assert_eq!(choose(&src, Path::new(r"C:\b"), 0, ALL), MOVE);
        assert_eq!(choose(&src, Path::new(r"c:\B\deeper"), 0, ALL), MOVE);
        assert_eq!(choose(&src, Path::new(r"D:\b"), 0, ALL), COPY);
        assert_eq!(choose(&[p(r"\\srv\s\x")], Path::new(r"\\SRV\s\y"), 0, ALL), MOVE);
        assert_eq!(choose(&[p(r"\\srv\s\x")], Path::new(r"\\srv\other"), 0, ALL), COPY);
        // Mixed volumes: a copy.
        let mixed = [p(r"C:\a\x"), p(r"D:\x")];
        assert_eq!(choose(&mixed, Path::new(r"C:\b"), 0, ALL), COPY);
    }

    #[test]
    fn keys_force_the_effect() {
        let src = [p(r"C:\a\x.txt")];
        let dest = Path::new(r"C:\b");
        assert_eq!(choose(&src, dest, MK_CONTROL, ALL), COPY);
        assert_eq!(choose(&src, Path::new(r"D:\b"), MK_SHIFT, ALL), MOVE);
        assert_eq!(choose(&src, dest, MK_CONTROL | MK_SHIFT, ALL), LINK);
        assert_eq!(choose(&src, dest, MK_ALT, ALL), LINK);
        // Asked for, but the source does not allow it.
        assert_eq!(choose(&src, dest, MK_SHIFT, COPY), NONE);
    }

    #[test]
    fn without_keys_whatever_the_source_allows() {
        // A browser offers only a copy.
        assert_eq!(choose(&[p(r"C:\a\x")], Path::new(r"C:\b"), 0, COPY), COPY);
        assert_eq!(choose(&[p(r"C:\a\x")], Path::new(r"D:\b"), 0, MOVE), MOVE);
        assert_eq!(choose(&[p(r"C:\a\x")], Path::new(r"D:\b"), 0, LINK), NONE);
    }

    #[test]
    fn a_folder_never_goes_into_itself() {
        let src = [p(r"C:\a\folder")];
        assert_eq!(choose(&src, Path::new(r"C:\a\folder"), 0, ALL), NONE);
        assert_eq!(choose(&src, Path::new(r"C:\A\Folder\inside"), MK_CONTROL, ALL), NONE);
        // A sibling whose name only starts the same is fine.
        assert_eq!(choose(&src, Path::new(r"C:\a\folder2"), 0, ALL), MOVE);
        assert_eq!(choose(&[], Path::new(r"C:\x"), 0, ALL), NONE);
    }

    #[test]
    fn moving_into_the_same_folder_does_nothing_but_a_copy_duplicates() {
        let src = [p(r"C:\a\x.txt"), p(r"C:\a\y.txt")];
        assert_eq!(choose(&src, Path::new(r"C:\a"), 0, ALL), NONE);
        assert_eq!(choose(&src, Path::new(r"C:\a\"), MK_SHIFT, ALL), NONE);
        assert_eq!(choose(&src, Path::new(r"C:\a"), MK_CONTROL, ALL), COPY);
        // A source that only allows moving: still nothing.
        assert_eq!(choose(&src, Path::new(r"C:\a"), 0, MOVE), NONE);
    }

    #[test]
    fn the_bin_takes_what_may_move() {
        assert_eq!(choose_bin(&[p(r"C:\x")], ALL), MOVE);
        assert_eq!(choose_bin(&[p(r"C:\x")], COPY), NONE);
        assert_eq!(choose_bin(&[], ALL), NONE);
        assert_eq!(choose_content(ALL), COPY);
        assert_eq!(choose_content(MOVE), NONE);
    }

    #[test]
    fn keys_and_volumes() {
        assert_eq!(volume_key(Path::new(r"C:\x")), Some("c:".into()));
        assert_eq!(volume_key(Path::new(r"\\?\C:\x")), Some("c:".into()));
        assert_eq!(volume_key(Path::new(r"\\Srv\Share\x")), Some(r"\\srv\share".into()));
        assert_eq!(volume_key(Path::new("rel")), None);
        assert!(is_within(Path::new(r"C:\A\b\"), Path::new(r"c:\a")));
        assert!(!is_within(Path::new(r"C:\ab"), Path::new(r"c:\a")));
    }
}
