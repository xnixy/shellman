//! What an app asking for a file and the Files picker say to each other.
//!
//! The asking side writes a [`Request`] as JSON to a file (in `%TEMP%`) and starts
//! `shellman-files --pick <that file>`. The picker writes a [`Response`] next to it
//! ([`response_path`]) and exits; its exit code says OK, cancelled or failed ([`exit`]).
//!
//! When the asking side wants to check the choice before the picker closes (an app's
//! `OnFileOk`, which may refuse it), the request says `confirm`. The picker then writes the
//! response and waits, still open, for a [`Verdict`] in [`verdict_path`]: `ok` closes it, `veto`
//! lets the user choose again.
//!
//! Plain data and files only: both a GUI app and a small DLL inside other apps use this crate.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The picker's exit codes.
pub mod exit {
    /// Something was chosen; the response holds it.
    pub const OK: i32 = 0;
    /// The user cancelled (or the asking app went away).
    pub const CANCELLED: i32 = 1;
    /// The picker could not do its job (an unreadable request, say). The asking side should
    /// fall back to another dialog.
    pub const FAILED: i32 = 2;
}

/// What the app wants picked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// One existing file.
    #[default]
    Open,
    /// One or more existing files.
    OpenMany,
    /// A name to save under, new or existing.
    Save,
    /// A folder.
    Folder,
}

/// One entry of the file type list: "Text files" with `*.txt` and `*.md`, say.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FileType {
    pub name: String,
    /// Patterns such as `*.txt`; `*` and `?` are wildcards.
    pub patterns: Vec<String>,
}

impl FileType {
    /// A type from the Windows way of writing one: patterns joined by `;` ("*.txt;*.md").
    pub fn parse(name: &str, spec: &str) -> Self {
        let patterns = spec
            .split(';')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect();
        Self {
            name: name.to_owned(),
            patterns,
        }
    }
}

/// What the asking app wants, as the picker reads it. Every field has a default, so a request
/// may leave out whatever it does not care about.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Request {
    pub mode: Mode,
    /// The window's title; empty for the mode's own ("Open", "Save as", "Select folder").
    pub title: String,
    /// The main button's label; empty for the mode's own.
    pub ok_label: String,
    /// The label before the name field; empty for "File name" / "Folder".
    pub file_name_label: String,
    /// Always open here, whatever was used last (the app's `SetFolder`).
    pub folder: Option<PathBuf>,
    /// Open here only when nothing was remembered for this app (`SetDefaultFolder`).
    pub default_folder: Option<PathBuf>,
    /// The name the field starts with. May hold a folder too ("C:\\notes\\todo.txt").
    pub file_name: String,
    pub types: Vec<FileType>,
    /// Which of `types` is chosen to start with, from 0.
    pub type_index: usize,
    /// Added to a saved name that has no extension ("txt", without the dot).
    pub default_ext: String,
    /// Open: the chosen files must exist.
    pub must_exist: bool,
    /// Save: ask before replacing an existing file.
    pub overwrite_prompt: bool,
    /// Open: more than one file may be chosen (the same as `Mode::OpenMany`).
    pub allow_multiple: bool,
    /// Show hidden and system items whatever the user's setting.
    pub show_hidden: bool,
    /// The asking app's window (an HWND), to centre over and stay above; 0 for none.
    pub owner: i64,
    /// Who is asking, to remember its last folder by: the program's file name, and the id the
    /// app gave its dialog when it gave one.
    pub client: String,
    /// The asking process. If it goes away, the picker cancels; 0 to not watch.
    pub host_pid: u32,
    /// Wait for a [`Verdict`] before closing (see the crate's notes).
    pub confirm: bool,
}

impl Request {
    /// Whether more than one file may be chosen.
    pub fn multiple(&self) -> bool {
        self.mode == Mode::OpenMany || (self.mode == Mode::Open && self.allow_multiple)
    }
}

/// What the picker answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Response {
    /// False when the user cancelled.
    pub ok: bool,
    /// What was chosen, full paths, in order.
    pub paths: Vec<PathBuf>,
    /// The file type chosen last, from 0.
    pub type_index: usize,
    /// The folder the picker was showing at the end.
    pub folder: Option<PathBuf>,
}

/// The asking side's answer to a confirmed response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Take it: the picker closes.
    Ok,
    /// Refused (the app said why itself): the picker stays open.
    Veto,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Ok => "ok",
            Verdict::Veto => "veto",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "ok" => Some(Verdict::Ok),
            "veto" => Some(Verdict::Veto),
            _ => None,
        }
    }
}

/// Where the response to the request at `request` goes: `x.json` gives `x.response.json`.
pub fn response_path(request: &Path) -> PathBuf {
    request.with_extension("response.json")
}

/// Where the verdict on a confirmed response goes: `x.json` gives `x.verdict`.
pub fn verdict_path(request: &Path) -> PathBuf {
    request.with_extension("verdict")
}

/// A fresh request file name in `dir`, unique to this process and call.
pub fn new_request_path(dir: &Path, pid: u32, n: u64) -> PathBuf {
    dir.join(format!("shellman-pick-{pid}-{n}.json"))
}

/// Read a request; the error says why it could not be.
pub fn read_request(path: &Path) -> Result<Request, String> {
    read_json(path)
}

/// Read a response, if one is there and whole.
pub fn read_response(path: &Path) -> Option<Response> {
    read_json(path).ok()
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let json = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    serde_json::from_slice(json).map_err(|e| format!("{}: {e}", path.display()))
}

/// Write `value` as JSON to `path`, through a temporary file and a rename, so a reader polling
/// for the file never sees half of it.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    write_whole(path, &json)
}

/// Write a verdict, whole, as [`write_json`] does.
pub fn write_verdict(path: &Path, verdict: Verdict) -> std::io::Result<()> {
    write_whole(path, verdict.as_str().as_bytes())
}

/// Read a verdict, if one is there.
pub fn read_verdict(path: &Path) -> Option<Verdict> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| Verdict::parse(&s))
}

fn write_whole(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, bytes)?;
    // Rename over an older file of the same name (a vetoed choice's response, say).
    let _ = std::fs::remove_file(path);
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shellman-pick-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn a_request_survives_the_round_trip() {
        let req = Request {
            mode: Mode::Save,
            title: "Save as".into(),
            folder: Some(PathBuf::from(r"C:\Temp")),
            file_name: "notes".into(),
            types: vec![
                FileType::parse("Text", "*.txt; *.md"),
                FileType::parse("All", "*.*"),
            ],
            type_index: 1,
            default_ext: "txt".into(),
            overwrite_prompt: true,
            owner: 0x1234,
            client: "notepad.exe".into(),
            host_pid: 42,
            confirm: true,
            ..Default::default()
        };
        let path = scratch("round.json");
        write_json(&path, &req).unwrap();
        assert_eq!(read_request(&path).unwrap(), req);
    }

    #[test]
    fn missing_fields_take_their_defaults() {
        let path = scratch("sparse.json");
        std::fs::write(&path, br#"{"mode":"folder","title":"Pick one"}"#).unwrap();
        let req = read_request(&path).unwrap();
        assert_eq!(req.mode, Mode::Folder);
        assert_eq!(req.title, "Pick one");
        assert!(req.types.is_empty() && req.folder.is_none() && !req.confirm);
        // A byte-order mark, as some editors write, is fine too.
        std::fs::write(&path, b"\xEF\xBB\xBF{\"mode\":\"open_many\"}").unwrap();
        assert!(read_request(&path).unwrap().multiple());
    }

    #[test]
    fn a_broken_request_says_why() {
        let path = scratch("broken.json");
        std::fs::write(&path, b"{ not json").unwrap();
        assert!(read_request(&path).is_err());
        assert!(read_request(&scratch("absent.json")).is_err());
    }

    #[test]
    fn the_response_and_verdict_sit_next_to_the_request() {
        let req = Path::new(r"C:\Temp\shellman-pick-7-1.json");
        assert_eq!(
            response_path(req),
            Path::new(r"C:\Temp\shellman-pick-7-1.response.json")
        );
        assert_eq!(verdict_path(req), Path::new(r"C:\Temp\shellman-pick-7-1.verdict"));
        assert_eq!(
            new_request_path(Path::new(r"C:\Temp"), 7, 1),
            Path::new(r"C:\Temp\shellman-pick-7-1.json")
        );
    }

    #[test]
    fn responses_and_verdicts_round_trip() {
        let resp = Response {
            ok: true,
            paths: vec![PathBuf::from(r"C:\a.txt"), PathBuf::from(r"C:\b.txt")],
            type_index: 2,
            folder: Some(PathBuf::from(r"C:\")),
        };
        let path = scratch("resp.json");
        write_json(&path, &resp).unwrap();
        assert_eq!(read_response(&path), Some(resp));
        let v = scratch("v.verdict");
        write_verdict(&v, Verdict::Veto).unwrap();
        assert_eq!(read_verdict(&v), Some(Verdict::Veto));
        write_verdict(&v, Verdict::Ok).unwrap();
        assert_eq!(read_verdict(&v), Some(Verdict::Ok));
        assert_eq!(Verdict::parse("maybe"), None);
    }

    #[test]
    fn types_split_on_semicolons() {
        let t = FileType::parse("Images", "*.png;*.JPG ; ;*.gif");
        assert_eq!(t.patterns, ["*.png", "*.JPG", "*.gif"]);
        assert!(FileType::parse("None", "").patterns.is_empty());
        let one = Request {
            mode: Mode::Open,
            allow_multiple: true,
            ..Default::default()
        };
        assert!(one.multiple());
        assert!(!Request::default().multiple());
    }
}
