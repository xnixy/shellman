//! Which app handles the common file types and links right now, read with `AssocQueryStringW`.
//! Read-only on purpose: Windows protects the choice with a hash that only its own Settings can
//! write (`UserChoice`), so a program that sets it is ignored and reset. Nothing here writes the
//! registry.

use std::path::PathBuf;
use windows::core::{HSTRING, PWSTR};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{AssocQueryStringW, ASSOCF, ASSOCF_IS_PROTOCOL, ASSOCF_NOTRUNCATE, ASSOCSTR, ASSOCSTR_EXECUTABLE, ASSOCSTR_FRIENDLYAPPNAME};

/// A file type (`.txt`) or a link kind (`https`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Extension,
    Protocol,
}

/// What the page lists, in order: the key Windows is asked about and the words people use.
pub const ITEMS: [(&str, Kind, &str); 11] = [
    (".txt", Kind::Extension, "Text document"),
    (".pdf", Kind::Extension, "PDF document"),
    (".png", Kind::Extension, "PNG picture"),
    (".jpg", Kind::Extension, "JPEG picture"),
    (".mp3", Kind::Extension, "MP3 music"),
    (".mp4", Kind::Extension, "MP4 video"),
    (".zip", Kind::Extension, "ZIP archive"),
    (".html", Kind::Extension, "Web page"),
    ("http", Kind::Protocol, "Web links (http)"),
    ("https", Kind::Protocol, "Secure web links (https)"),
    ("mailto", Kind::Protocol, "Email links"),
];

/// One row: what is asked about, and what answers.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    pub key: &'static str,
    pub kind: Kind,
    pub title: &'static str,
    /// The app's name, or `None` when nothing handles it.
    pub app: Option<String>,
    /// The program, for its icon, when Windows names one.
    pub exe: Option<PathBuf>,
}

/// The line under a row's title: `.txt` or `https`, as it would be typed.
pub fn key_label(key: &str, kind: Kind) -> String {
    match kind {
        Kind::Extension => key.to_owned(),
        Kind::Protocol => format!("{key}:"),
    }
}

/// What a row says when nothing handles the type.
pub const NO_APP: &str = "No app chosen";

/// The program path Windows returned, made usable: quotes and arguments are dropped.
pub fn clean_exe(raw: &str) -> Option<PathBuf> {
    let raw = raw.trim();
    let path = match raw.strip_prefix('"') {
        Some(rest) => rest.split('"').next().unwrap_or(""),
        None => raw,
    };
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// Whether the program is Windows' "Open with" chooser, which stands in when no app is chosen.
pub fn is_picker(exe: &std::path::Path) -> bool {
    exe.file_name().is_some_and(|n| n.eq_ignore_ascii_case("openwith.exe"))
}

fn query(what: ASSOCSTR, key: &str, kind: Kind) -> Option<String> {
    let flags = ASSOCF(ASSOCF_NOTRUNCATE.0 | if kind == Kind::Protocol { ASSOCF_IS_PROTOCOL.0 } else { 0 });
    let key = HSTRING::from(key);
    let mut buf = [0u16; 520];
    let mut len = buf.len() as u32;
    // SAFETY: the buffer and its length are valid for the call.
    let hr = unsafe { AssocQueryStringW(flags, what, &key, None, Some(PWSTR(buf.as_mut_ptr())), &mut len) };
    if hr.is_err() {
        return None;
    }
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    let s = String::from_utf16_lossy(&buf[..end]);
    (!s.is_empty()).then_some(s)
}

/// Ask Windows about every item. Takes a moment (it reads the registry and app manifests), so
/// call it from a worker.
pub fn read() -> Vec<Entry> {
    // SAFETY: balanced below; the shell's association lookup wants an apartment.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    let out = ITEMS
        .iter()
        .map(|&(key, kind, title)| {
            let exe = query(ASSOCSTR_EXECUTABLE, key, kind).and_then(|p| clean_exe(&p)).filter(|p| p.exists());
            // "Pick an app" is what Windows answers when nobody has been chosen.
            if exe.as_deref().is_some_and(is_picker) {
                return Entry { key, kind, title, app: None, exe: None };
            }
            Entry { key, kind, title, app: query(ASSOCSTR_FRIENDLYAPPNAME, key, kind), exe }
        })
        .collect();
    if com {
        // SAFETY: pairs with the successful init above.
        unsafe { CoUninitialize() };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_has_what_was_asked_for() {
        let keys: Vec<&str> = ITEMS.iter().map(|i| i.0).collect();
        for k in [".txt", ".pdf", ".png", ".jpg", ".mp3", ".mp4", ".zip", ".html", "http", "https", "mailto"] {
            assert!(keys.contains(&k), "{k}");
        }
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), keys.len());
    }

    #[test]
    fn keys_read_as_typed() {
        assert_eq!(key_label(".txt", Kind::Extension), ".txt");
        assert_eq!(key_label("https", Kind::Protocol), "https:");
    }

    #[test]
    fn the_open_with_chooser_is_not_an_app() {
        assert!(is_picker(std::path::Path::new("C:\\Windows\\system32\\OpenWith.exe")));
        assert!(!is_picker(std::path::Path::new("C:\\Windows\\notepad.exe")));
    }

    #[test]
    fn exe_paths_are_cleaned() {
        assert_eq!(clean_exe("C:\\Windows\\notepad.exe"), Some(PathBuf::from("C:\\Windows\\notepad.exe")));
        assert_eq!(clean_exe("\"C:\\A B\\x.exe\" --arg"), Some(PathBuf::from("C:\\A B\\x.exe")));
        assert_eq!(clean_exe("  "), None);
    }

    /// Prints this machine's answers: `cargo test -p shellman-core default_apps -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn read_this_machine() {
        for e in read() {
            println!("{:8} {:?} {:?}", e.key, e.app, e.exe);
        }
    }
}
