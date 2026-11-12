//! The words on the picker: its title, the main button's label, and whether that button can be
//! pressed. Pure, so each mode and selection is tested.

use super::decide::enters_folder;
use shellman_pick::{Mode, Request};
use std::path::PathBuf;

/// The window's title: the app's, or the mode's own.
pub fn title(req: &Request) -> String {
    if !req.title.trim().is_empty() {
        return req.title.trim().to_owned();
    }
    match req.mode {
        Mode::Open => "Open",
        Mode::OpenMany => "Open files",
        Mode::Save => "Save as",
        Mode::Folder => "Select folder",
    }
    .to_owned()
}

/// The label before the name field.
pub fn name_label(req: &Request) -> String {
    let own = req.file_name_label.trim().trim_end_matches(':').trim();
    if !own.is_empty() {
        return own.to_owned();
    }
    if req.mode == Mode::Folder { "Folder" } else { "File name" }.to_owned()
}

/// The main button's label. While it would go into the selected folder it says Open, as on
/// Windows; otherwise the app's label, or the mode's.
pub fn main_label(req: &Request, selected: &[(PathBuf, bool)], typed_since_selection: bool) -> String {
    if enters_folder(req.mode, selected, typed_since_selection) {
        return "Open".into();
    }
    if !req.ok_label.trim().is_empty() {
        return req.ok_label.trim().to_owned();
    }
    match req.mode {
        Mode::Open | Mode::OpenMany => "Open",
        Mode::Save => "Save",
        Mode::Folder => "Select Folder",
    }
    .to_owned()
}

/// Whether the main button can be pressed: there must be something for it to act on.
pub fn main_enabled(
    mode: Mode,
    typed: &str,
    selected: &[(PathBuf, bool)],
    typed_since_selection: bool,
    in_folder: bool,
) -> bool {
    if enters_folder(mode, selected, typed_since_selection) {
        return true;
    }
    let typed = !typed.trim().is_empty();
    match mode {
        Mode::Open | Mode::OpenMany => typed || selected.iter().any(|(_, dir)| !dir),
        Mode::Save => typed,
        Mode::Folder => typed || in_folder || selected.iter().any(|(_, dir)| *dir),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(mode: Mode) -> Request {
        Request {
            mode,
            ..Default::default()
        }
    }

    fn file() -> Vec<(PathBuf, bool)> {
        vec![(PathBuf::from(r"C:\a.txt"), false)]
    }

    fn dir() -> Vec<(PathBuf, bool)> {
        vec![(PathBuf::from(r"C:\sub"), true)]
    }

    #[test]
    fn each_mode_names_its_button() {
        assert_eq!(main_label(&req(Mode::Open), &[], false), "Open");
        assert_eq!(main_label(&req(Mode::OpenMany), &file(), false), "Open");
        assert_eq!(main_label(&req(Mode::Save), &[], false), "Save");
        assert_eq!(main_label(&req(Mode::Folder), &dir(), false), "Select Folder");
    }

    #[test]
    fn a_selected_folder_turns_save_into_open() {
        assert_eq!(main_label(&req(Mode::Save), &dir(), false), "Open");
        // Typing a name after selecting it puts Save back.
        assert_eq!(main_label(&req(Mode::Save), &dir(), true), "Save");
    }

    #[test]
    fn the_apps_own_label_wins() {
        let mut r = req(Mode::Open);
        r.ok_label = "Import".into();
        assert_eq!(main_label(&r, &file(), false), "Import");
        // But not while it would only go into a folder.
        assert_eq!(main_label(&r, &dir(), false), "Open");
    }

    #[test]
    fn open_needs_a_file_or_a_name() {
        assert!(!main_enabled(Mode::Open, "", &[], false, true));
        assert!(main_enabled(Mode::Open, "", &file(), false, true));
        assert!(main_enabled(Mode::Open, "x.txt", &[], true, true));
        assert!(main_enabled(Mode::Open, "", &dir(), false, true));
        assert!(!main_enabled(Mode::Open, "   ", &[], true, true));
    }

    #[test]
    fn save_needs_a_name() {
        assert!(!main_enabled(Mode::Save, "", &file(), false, true));
        assert!(main_enabled(Mode::Save, "new", &[], true, true));
        assert!(main_enabled(Mode::Save, "", &dir(), false, true));
    }

    #[test]
    fn folder_mode_needs_a_folder_on_show_or_chosen() {
        assert!(main_enabled(Mode::Folder, "", &[], false, true));
        assert!(!main_enabled(Mode::Folder, "", &[], false, false));
        assert!(main_enabled(Mode::Folder, "", &dir(), false, false));
        assert!(main_enabled(Mode::Folder, r"C:\x", &[], true, false));
    }

    #[test]
    fn titles_and_labels_fall_back_to_the_modes() {
        assert_eq!(title(&req(Mode::Save)), "Save as");
        assert_eq!(title(&req(Mode::Folder)), "Select folder");
        let mut r = req(Mode::Open);
        r.title = " Open a photo ".into();
        assert_eq!(title(&r), "Open a photo");
        assert_eq!(name_label(&req(Mode::Folder)), "Folder");
        r.file_name_label = "Photo name:".into();
        assert_eq!(name_label(&r), "Photo name");
    }
}
