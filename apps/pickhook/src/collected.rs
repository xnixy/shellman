//! What the app told its dialog before showing it, and whether the picker can honour all of it.
//! Pure: no COM here, so every option and combination is tested.

use shellman_pick::{FileType, Mode, Request};
use std::path::PathBuf;

/// The `FOS_*` option bits this file looks at (shobjidl.h).
pub mod fos {
    pub const OVERWRITEPROMPT: u32 = 0x2;
    pub const PICKFOLDERS: u32 = 0x20;
    pub const ALLOWMULTISELECT: u32 = 0x200;
    pub const FILEMUSTEXIST: u32 = 0x1000;
    pub const CREATEPROMPT: u32 = 0x2000;
    pub const FORCESHOWHIDDEN: u32 = 0x1000_0000;
}

/// Everything the app set, as plain data.
#[derive(Clone, Debug, Default)]
pub struct Collected {
    /// A save dialog (else open).
    pub save: bool,
    /// The dialog's `FOS_*` options, as the real dialog reports them at `Show` (so its own
    /// defaults are in).
    pub options: u32,
    pub title: String,
    pub ok_label: String,
    pub file_name_label: String,
    pub file_name: String,
    pub default_ext: String,
    pub folder: Option<PathBuf>,
    pub default_folder: Option<PathBuf>,
    pub types: Vec<FileType>,
    /// As the app gave it: from 1, 0 when it gave none.
    pub type_index: u32,
    /// The program's file name, and the dialog id it set, if any.
    pub client: String,
    /// What the app used that the picker cannot do (custom controls, an item filter...). Any
    /// entry means the real dialog is shown instead.
    pub unsupported: Vec<&'static str>,
}

/// Why the picker cannot stand in, when it cannot.
pub fn blocker(c: &Collected) -> Option<&'static str> {
    if let Some(first) = c.unsupported.first() {
        return Some(first);
    }
    let o = c.options;
    if o & fos::PICKFOLDERS != 0 && c.save {
        return Some("a save dialog that picks folders");
    }
    if o & fos::PICKFOLDERS != 0 && o & fos::ALLOWMULTISELECT != 0 {
        return Some("picking several folders");
    }
    if o & fos::CREATEPROMPT != 0 && !c.save {
        return Some("offering to create a missing file");
    }
    None
}

/// The request for the picker, or why there can be none.
pub fn request(c: &Collected) -> Result<Request, &'static str> {
    if let Some(why) = blocker(c) {
        return Err(why);
    }
    let o = c.options;
    let mode = if c.save {
        Mode::Save
    } else if o & fos::PICKFOLDERS != 0 {
        Mode::Folder
    } else if o & fos::ALLOWMULTISELECT != 0 {
        Mode::OpenMany
    } else {
        Mode::Open
    };
    Ok(Request {
        mode,
        title: c.title.clone(),
        ok_label: c.ok_label.clone(),
        file_name_label: c.file_name_label.clone(),
        folder: c.folder.clone(),
        default_folder: c.default_folder.clone(),
        file_name: c.file_name.clone(),
        types: if mode == Mode::Folder { Vec::new() } else { c.types.clone() },
        type_index: (c.type_index.max(1) - 1) as usize,
        default_ext: c.default_ext.trim_start_matches('.').to_owned(),
        must_exist: !c.save && o & fos::FILEMUSTEXIST != 0,
        overwrite_prompt: c.save && o & fos::OVERWRITEPROMPT != 0,
        allow_multiple: mode == Mode::OpenMany,
        show_hidden: o & fos::FORCESHOWHIDDEN != 0,
        client: c.client.clone(),
        ..Default::default()
    })
}

/// Who is asking, to remember its folder by: `notepad.exe`, or `notepad.exe|{guid}` when the
/// app gave its dialog an id (`SetClientGuid`), as Windows keeps one folder per id.
pub fn client(exe: &str, guid: Option<&str>) -> String {
    let name = exe
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(exe)
        .to_lowercase();
    match guid {
        Some(g) => format!("{name}|{g}"),
        None => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(options: u32) -> Collected {
        Collected {
            options,
            ..Default::default()
        }
    }

    #[test]
    fn options_choose_the_mode() {
        assert_eq!(request(&open(fos::FILEMUSTEXIST)).unwrap().mode, Mode::Open);
        let many = request(&open(fos::ALLOWMULTISELECT)).unwrap();
        assert_eq!(many.mode, Mode::OpenMany);
        assert!(many.multiple());
        assert_eq!(request(&open(fos::PICKFOLDERS)).unwrap().mode, Mode::Folder);
        let save = Collected {
            save: true,
            options: fos::OVERWRITEPROMPT,
            ..Default::default()
        };
        let r = request(&save).unwrap();
        assert_eq!(r.mode, Mode::Save);
        assert!(r.overwrite_prompt && !r.must_exist);
    }

    #[test]
    fn flags_carry_over() {
        let r = request(&open(fos::FILEMUSTEXIST | fos::FORCESHOWHIDDEN)).unwrap();
        assert!(r.must_exist && r.show_hidden);
        // Overwrite asks only in save mode; must-exist only in open mode.
        let r = request(&open(fos::OVERWRITEPROMPT)).unwrap();
        assert!(!r.overwrite_prompt);
    }

    #[test]
    fn what_the_picker_cannot_do_falls_back() {
        assert!(request(&open(fos::PICKFOLDERS | fos::ALLOWMULTISELECT)).is_err());
        assert!(request(&open(fos::CREATEPROMPT)).is_err());
        let folders_save = Collected {
            save: true,
            options: fos::PICKFOLDERS,
            ..Default::default()
        };
        assert!(request(&folders_save).is_err());
        let custom = Collected {
            unsupported: vec!["custom controls"],
            ..Default::default()
        };
        assert_eq!(request(&custom).unwrap_err(), "custom controls");
    }

    #[test]
    fn types_index_and_extension_are_translated() {
        let c = Collected {
            types: vec![
                FileType::parse("Text", "*.txt"),
                FileType::parse("All", "*.*"),
            ],
            type_index: 2,
            default_ext: ".txt".into(),
            save: true,
            ..Default::default()
        };
        let r = request(&c).unwrap();
        assert_eq!(r.type_index, 1);
        assert_eq!(r.default_ext, "txt");
        assert_eq!(r.types.len(), 2);
        // No index given: the first type.
        assert_eq!(request(&Collected { type_index: 0, ..c.clone() }).unwrap().type_index, 0);
        // A folder picker shows no types.
        let folder = Collected {
            options: fos::PICKFOLDERS,
            save: false,
            ..c
        };
        assert!(request(&folder).unwrap().types.is_empty());
    }

    #[test]
    fn the_client_is_the_program_and_its_dialog_id() {
        assert_eq!(client(r"C:\Windows\System32\Notepad.exe", None), "notepad.exe");
        assert_eq!(
            client(r"C:\x\App.exe", Some("{1234}")),
            "app.exe|{1234}"
        );
    }
}
