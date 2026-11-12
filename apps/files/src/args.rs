//! What the command line asks Files to open.

use std::ffi::OsString;
use std::path::PathBuf;

/// The flag that opens the Recycle Bin. The dock's bin icon starts Files with it.
pub const RECYCLE_BIN: &str = "--recycle-bin";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Start {
    /// The tabs from last time.
    Saved,
    /// One tab on this folder.
    Folder(PathBuf),
    /// One tab on the Recycle Bin.
    RecycleBin,
}

/// Reads the arguments after the program name. A folder that does not exist is ignored, as is
/// anything else unknown, so a stray argument never keeps the window from opening.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Start {
    for arg in args {
        if arg == RECYCLE_BIN {
            return Start::RecycleBin;
        }
        let path = PathBuf::from(arg);
        if path.is_dir() {
            return Start::Folder(path);
        }
    }
    Start::Saved
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> Start {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn flag_opens_the_bin() {
        assert_eq!(parse_strs(&["--recycle-bin"]), Start::RecycleBin);
        assert_eq!(parse_strs(&["--unknown", "--recycle-bin"]), Start::RecycleBin);
    }

    #[test]
    fn folder_opens_that_folder() {
        let dir = std::env::temp_dir();
        assert_eq!(parse(vec![dir.clone().into_os_string()]), Start::Folder(dir));
    }

    #[test]
    fn nothing_usable_keeps_the_saved_tabs() {
        assert_eq!(parse_strs(&[]), Start::Saved);
        assert_eq!(parse_strs(&["--nope", r"Z:\does\not\exist"]), Start::Saved);
    }
}
