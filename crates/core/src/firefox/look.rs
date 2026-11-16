//! The look in one profile: install (first time with a backup), update, remove with an exact
//! restore, and its state there.
//!
//! What a profile gets, as `install.ps1` does it: the five CSS files in `<profile>\chrome\`, the
//! prefs as a marked block at the end of `user.js`, and with scripts the script files in
//! `chrome\shellman-scripts\`. The first install copies an existing `chrome\` folder and `user.js`
//! into `<profile>\shellman-islands-backup\<yyyyMMdd-HHmmss>\` and writes `state.json` there;
//! later installs update the files and keep that backup. Removal deletes exactly what was added,
//! puts back what the backup holds of it, takes the block (and a line break install had to add)
//! out of `user.js`, deletes a `chrome\` folder or `user.js` it created once empty, and deletes
//! the backup folder. Nothing runs while Firefox holds the profile.
//!
//! The loader in Firefox's folder is not written here: whoever installs scripts first makes sure
//! it is there ([`super::loader`], or the administrator helper) and passes the folder, so a
//! refused UAC prompt leaves the profile untouched.

use super::{detect, loader, userjs, Source, CHROME_FILES, SCRIPTS_DIR, SCRIPT_FILES};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const BACKUP_ROOT: &str = "shellman-islands-backup";
pub const STATE_FILE: &str = "state.json";

/// One autoconfig file as recorded: where below the Firefox folder, its SHA-256 (upper-case hex,
/// as PowerShell's `Get-FileHash`), and whether a file of another setup was backed up for it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Autoconfig {
    pub path: String,
    pub sha256: String,
    #[serde(default)]
    pub backed_up: bool,
}

/// `state.json`, as `install.ps1` writes it, plus `userJsBreak`. Fields this code does not know
/// are kept as they are.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub backup: String,
    pub had_chrome: bool,
    pub had_user_js: bool,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub installed: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub firefox_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autoconfig: Option<Vec<Autoconfig>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scripts: Option<Vec<String>>,
    /// Install added a line break after a `user.js` that did not end in one.
    #[serde(default)]
    pub user_js_break: bool,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl State {
    /// From the file's bytes; PowerShell 5 writes a byte order mark first.
    pub fn parse(bytes: &[u8]) -> Option<State> {
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
        serde_json::from_slice(bytes).ok()
    }

    pub fn has_scripts(&self) -> bool {
        self.firefox_dir.as_deref().is_some_and(|d| !d.is_empty())
    }
}

/// The look's state in a profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    NotInstalled,
    Installed { scripts: bool },
    /// Installed, but a file differs from the ones shellman has now (or is missing).
    Outdated { scripts: bool },
}

impl Status {
    pub fn installed(self) -> bool {
        !matches!(self, Status::NotInstalled)
    }
}

/// What can stop a change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Firefox has this profile open.
    InUse,
    Other(String),
}

impl Refused {
    pub fn message(&self) -> String {
        match self {
            Refused::InUse => "Firefox is open with this profile. Close Firefox, then try again.".into(),
            Refused::Other(s) => s.clone(),
        }
    }
}

impl From<String> for Refused {
    fn from(s: String) -> Refused {
        Refused::Other(s)
    }
}

fn state_path(profile: &Path) -> PathBuf {
    profile.join(BACKUP_ROOT).join(STATE_FILE)
}

pub fn read_state(profile: &Path) -> Option<State> {
    State::parse(&std::fs::read(state_path(profile)).ok()?)
}

fn write_state(profile: &Path, st: &State) -> Result<(), String> {
    let p = state_path(profile);
    let text = serde_json::to_vec_pretty(st).map_err(|e| e.to_string())?;
    std::fs::write(&p, text).map_err(|e| format!("cannot write {}: {e}", p.display()))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02X}")).collect()
}

fn same(a: &Path, b: &Path) -> bool {
    matches!((std::fs::read(a), std::fs::read(b)), (Ok(x), Ok(y)) if x == y)
}

/// The look's state in `profile`, compared with `src` when given (without it, installed is all
/// that can be said).
pub fn status(profile: &Path, src: Option<&Source>) -> Status {
    let Some(st) = read_state(profile) else { return Status::NotInstalled };
    let scripts = st.has_scripts();
    let Some(src) = src else { return Status::Installed { scripts } };
    let chrome = profile.join("chrome");
    let mut current = CHROME_FILES.iter().all(|f| same(&src.chrome(f), &chrome.join(f)));
    current &= match (std::fs::read(profile.join("user.js")), std::fs::read(src.prefs())) {
        (Ok(u), Ok(p)) => userjs::has_block(&u, &p),
        _ => false,
    };
    if scripts {
        let dir = chrome.join(SCRIPTS_DIR);
        current &= SCRIPT_FILES.iter().all(|f| same(&src.script(f), &dir.join(f)));
        current &= st.firefox_dir.as_deref().is_some_and(|d| loader::state(Path::new(d)) == loader::State::Current);
    }
    if current {
        Status::Installed { scripts }
    } else {
        Status::Outdated { scripts }
    }
}

/// Whether `profile` has the scripts' folder (with anything in it), whoever put it there.
pub fn uses_scripts(profile: &Path) -> bool {
    std::fs::read_dir(profile.join("chrome").join(SCRIPTS_DIR)).is_ok_and(|mut d| d.next().is_some())
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("cannot create {}: {e}", to.display()))?;
    let rd = std::fs::read_dir(from).map_err(|e| format!("cannot read {}: {e}", from.display()))?;
    for e in rd.flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            copy_dir(&src, &dst)?;
        } else {
            copy(&src, &dst)?;
        }
    }
    Ok(())
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::copy(from, to).map(|_| ()).map_err(|e| format!("cannot copy {} to {}: {e}", from.display(), to.display()))
}

fn remove_file(p: &Path) -> Result<(), String> {
    match std::fs::remove_file(p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("cannot remove {}: {e}", p.display())),
    }
}

/// Remove `dir` if it is there and empty.
fn remove_if_empty(dir: &Path) {
    if std::fs::read_dir(dir).is_ok_and(|mut d| d.next().is_none()) {
        let _ = std::fs::remove_dir(dir);
    }
}

/// Local time as `yyyyMMdd-HHmmss` (the backup's name) and as ISO 8601.
fn now() -> (String, String) {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    (
        format!("{:04}{:02}{:02}-{:02}{:02}{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond),
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond),
    )
}

/// `list` with `add` appended, each name once.
fn union(list: &[String], add: &[&str]) -> Vec<String> {
    let mut v = list.to_vec();
    for a in add {
        if !v.iter().any(|x| x.eq_ignore_ascii_case(a)) {
            v.push((*a).to_string());
        }
    }
    v
}

/// Take the profile's scripts out (their files, the folder once empty) and forget them in `st`.
fn drop_scripts(profile: &Path, st: &mut State) -> Result<(), String> {
    let dir = profile.join("chrome").join(SCRIPTS_DIR);
    for f in st.scripts.take().unwrap_or_default() {
        remove_file(&dir.join(f))?;
    }
    remove_if_empty(&dir);
    st.firefox_dir = None;
    st.autoconfig = None;
    Ok(())
}

/// What an install did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    /// It was there already and was brought up to date.
    pub updated: bool,
    /// The first install's backup folder.
    pub backup: PathBuf,
    /// Scripts were on and this install took them out: their Firefox folder, whose loader may now
    /// be unused.
    pub scripts_dropped: Option<PathBuf>,
}

/// Install the look into `profile`, or bring it up to date. `scripts`: the Firefox folder whose
/// loader is already in place (the scripts go in with it), or None (CSS only; scripts installed
/// before are taken out).
pub fn install(profile: &Path, src: &Source, scripts: Option<&Path>) -> Result<Installed, Refused> {
    if !profile.is_dir() {
        return Err(format!("The profile folder {} is not there.", profile.display()).into());
    }
    if detect::in_use(profile) {
        return Err(Refused::InUse);
    }
    let prefs = std::fs::read(src.prefs()).map_err(|e| format!("cannot read {}: {e}", src.prefs().display()))?;
    if let Some(ff) = scripts {
        if loader::state(ff) != loader::State::Current {
            return Err(format!("The scripts' loader is not in {}.", ff.display()).into());
        }
    }
    let chrome = profile.join("chrome");
    let user_js = profile.join("user.js");
    let root = profile.join(BACKUP_ROOT);

    let previous = read_state(profile);
    let updated = previous.is_some();
    let mut st = match previous {
        Some(st) => st,
        None => {
            let (stamp, iso) = now();
            let dir = root.join(&stamp);
            std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
            let had_chrome = chrome.is_dir();
            let had_user_js = user_js.is_file();
            if had_chrome {
                copy_dir(&chrome, &dir.join("chrome"))?;
            }
            if had_user_js {
                copy(&user_js, &dir.join("user.js"))?;
            }
            let st = State { backup: stamp, had_chrome, had_user_js, files: CHROME_FILES.map(String::from).to_vec(), installed: iso, ..Default::default() };
            // Recorded before anything changes, so a failure from here on can still be undone.
            write_state(profile, &st)?;
            st
        }
    };
    if let (Some(ff), Some(was)) = (scripts, st.firefox_dir.as_deref().filter(|d| !d.is_empty())) {
        if !Path::new(was).as_os_str().eq_ignore_ascii_case(ff.as_os_str()) {
            return Err(format!("The scripts are installed here for the Firefox in {was}. Remove the look first to switch.").into());
        }
    }

    std::fs::create_dir_all(&chrome).map_err(|e| format!("cannot create {}: {e}", chrome.display()))?;
    for f in CHROME_FILES {
        copy(&src.chrome(f), &chrome.join(f))?;
    }
    st.files = union(&st.files, &CHROME_FILES);

    let existing = std::fs::read(&user_js).ok();
    let (text, added) = userjs::with_block(existing.as_deref(), &prefs);
    std::fs::write(&user_js, text).map_err(|e| format!("cannot write {}: {e}", user_js.display()))?;
    st.user_js_break |= added;

    let mut scripts_dropped = None;
    match scripts {
        Some(ff) => {
            let dir = chrome.join(SCRIPTS_DIR);
            std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
            for f in SCRIPT_FILES {
                copy(&src.script(f), &dir.join(f))?;
            }
            st.scripts = Some(union(st.scripts.as_deref().unwrap_or_default(), &SCRIPT_FILES));
            let before = st.autoconfig.take().unwrap_or_default();
            st.autoconfig = Some(
                loader::FILES
                    .iter()
                    .map(|(rel, _)| Autoconfig {
                        path: (*rel).to_string(),
                        sha256: sha256_hex(&std::fs::read(ff.join(rel)).unwrap_or_default()),
                        backed_up: before.iter().any(|b| b.path.eq_ignore_ascii_case(rel) && b.backed_up),
                    })
                    .collect(),
            );
            st.firefox_dir = Some(ff.to_string_lossy().into_owned());
        }
        None if st.scripts.is_some() || st.has_scripts() => {
            if st.autoconfig.iter().flatten().any(|a| a.backed_up) {
                return Err(backed_up_refusal());
            }
            scripts_dropped = st.firefox_dir.clone().map(PathBuf::from);
            drop_scripts(profile, &mut st)?;
        }
        None => {}
    }
    write_state(profile, &st)?;
    Ok(Installed { updated, backup: root.join(&st.backup), scripts_dropped })
}

fn backed_up_refusal() -> Refused {
    Refused::Other(
        "The scripts were installed over another autoconfig setup, which is kept in the profile's backup. Remove them with integrations\\firefox\\uninstall.ps1, which puts that setup back.".into(),
    )
}

/// What a removal did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Removed {
    /// Nothing was installed there.
    pub nothing: bool,
    /// The Firefox folder the scripts were installed for, whose loader may now be unused.
    pub scripts_for: Option<PathBuf>,
}

/// Remove the look from `profile` and put back what it replaced.
pub fn remove(profile: &Path) -> Result<Removed, Refused> {
    let Some(mut st) = read_state(profile) else { return Ok(Removed { nothing: true, scripts_for: None }) };
    if detect::in_use(profile) {
        return Err(Refused::InUse);
    }
    if st.autoconfig.iter().flatten().any(|a| a.backed_up) {
        return Err(backed_up_refusal());
    }
    let chrome = profile.join("chrome");
    let user_js = profile.join("user.js");
    let root = profile.join(BACKUP_ROOT);
    let backup = root.join(&st.backup);

    let scripts_for = st.firefox_dir.clone().filter(|d| !d.is_empty()).map(PathBuf::from);
    drop_scripts(profile, &mut st)?;

    for f in &st.files {
        let p = chrome.join(f);
        remove_file(&p)?;
        let old = backup.join("chrome").join(f);
        if old.is_file() {
            std::fs::create_dir_all(&chrome).map_err(|e| format!("cannot create {}: {e}", chrome.display()))?;
            copy(&old, &p)?;
        }
    }
    if !st.had_chrome {
        remove_if_empty(&chrome);
    }

    if let Ok(text) = std::fs::read(&user_js) {
        let rest = userjs::without_block(&text, st.user_js_break);
        if !st.had_user_js && userjs::is_blank(&rest) {
            remove_file(&user_js)?;
        } else {
            std::fs::write(&user_js, rest).map_err(|e| format!("cannot write {}: {e}", user_js.display()))?;
        }
    }
    if st.had_user_js && !user_js.exists() && backup.join("user.js").is_file() {
        copy(&backup.join("user.js"), &user_js)?;
    }

    std::fs::remove_dir_all(&root).map_err(|e| format!("cannot remove {}: {e}", root.display()))?;
    Ok(Removed { nothing: false, scripts_for })
}

#[cfg(test)]
#[path = "look_tests.rs"]
mod tests;
