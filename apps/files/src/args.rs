//! What the command line asks Files to open.

use std::ffi::OsString;
use std::path::PathBuf;

/// The flag that opens the Recycle Bin. The dock's bin icon starts Files with it.
pub const RECYCLE_BIN: &str = "--recycle-bin";
/// The flag, followed by a request file, that runs Files as a file picker for another app.
pub const PICK: &str = "--pick";
/// The flag, followed by a file or folder, that opens the folder it is in with it selected
/// ("open file location" from the other apps).
pub const SELECT: &str = "--select";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Start {
    /// The tabs from last time.
    Saved,
    /// One tab on this folder.
    Folder(PathBuf),
    /// One tab on the folder this item is in, with the item selected once it is listed.
    Select(PathBuf),
    /// One tab on the Recycle Bin.
    RecycleBin,
    /// A picker for another app, as the request in this file asks (see `crate::pick`).
    Pick(PathBuf),
}

/// Reads the arguments after the program name. A folder that does not exist is ignored, as is
/// anything else unknown, so a stray argument never keeps the window from opening.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Start {
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == PICK {
            // The picker never falls back to browsing: with no file, the request is "" and
            // reading it fails, so the asking app gets an error and uses another dialog.
            return Start::Pick(args.next().map(PathBuf::from).unwrap_or_default());
        }
        if arg == RECYCLE_BIN {
            return Start::RecycleBin;
        }
        if arg == SELECT {
            // Not looked at here: the folder's listing finds the item, or does not. Something
            // with no folder above it (a drive, nothing at all) is not an item to select.
            match args.next().map(PathBuf::from) {
                Some(path) if path.parent().is_some_and(|d| !d.as_os_str().is_empty()) => return Start::Select(path),
                Some(path) if path.is_dir() => return Start::Folder(path),
                _ => continue,
            }
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
    fn pick_takes_the_request_file_after_it() {
        assert_eq!(
            parse_strs(&["--pick", r"C:\Temp\req.json"]),
            Start::Pick(PathBuf::from(r"C:\Temp\req.json"))
        );
        assert_eq!(parse_strs(&["--pick"]), Start::Pick(PathBuf::new()));
    }

    #[test]
    fn select_takes_the_item_after_it() {
        assert_eq!(parse_strs(&["--select", r"C:\Windows\notepad.exe"]), Start::Select(PathBuf::from(r"C:\Windows\notepad.exe")));
        assert_eq!(
            parse_strs(&["--select", r"\\wsl.localhost\Debian\etc\hosts"]),
            Start::Select(PathBuf::from(r"\\wsl.localhost\Debian\etc\hosts"))
        );
        // Nothing to select: the saved tabs, as for any stray argument.
        assert_eq!(parse_strs(&["--select"]), Start::Saved);
        assert_eq!(parse_strs(&["--select", "file-with-no-folder"]), Start::Saved);
        assert_eq!(parse_strs(&["--select", r"Z:\"]), Start::Saved);
    }

    #[test]
    fn nothing_usable_keeps_the_saved_tabs() {
        assert_eq!(parse_strs(&[]), Start::Saved);
        assert_eq!(parse_strs(&["--nope", r"Z:\does\not\exist"]), Start::Saved);
    }
}
