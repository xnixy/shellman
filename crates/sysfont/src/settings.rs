//! Where the font files live, and the choices saved between runs.
//!
//! These are Fontman's own folders and file formats, kept on purpose: a PC Fontman changed is
//! taken over as it is (its backup, its record of what is in place, its "Use again" list and its
//! choices), and Fontman can still read everything written here. Settings and the window-text
//! backup are in `%APPDATA%\Fontman`, staging and added fonts in `%LOCALAPPDATA%\Fontman`, and the
//! machine-wide backup, state and history in `%ProgramData%\Fontman`.
//!
//! A dry run (`SHELLMAN_SYSFONT_DRY=1`) moves all three under one stand-in root, so it never reads
//! or writes the real folders' files: `SHELLMAN_SYSFONT_ROOT`, or `%LOCALAPPDATA%\shellman\sysfont-dry`.

use crate::metrics::TextSizes;
use crate::plan::{FaceChoice, Layers, Tune};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Set to 1 to change nothing on the PC: the registry, the fonts folder and window text are
/// stand-ins under the dry root, and no administrator step is ever started.
pub const DRY_VAR: &str = "SHELLMAN_SYSFONT_DRY";
/// The stand-in root of a dry run. Read only in a dry run.
pub const ROOT_VAR: &str = "SHELLMAN_SYSFONT_ROOT";
/// The folder name Fontman uses in each of the three places.
const FOLDER: &str = "Fontman";

/// Whether a value of the dry variable asks for a dry run: anything but empty or "0".
pub fn dry_flag(value: Option<&str>) -> bool {
    value.is_some_and(|v| !v.is_empty() && v != "0")
}

/// Dry runs change nothing on the PC. Unit tests are always dry, whatever the environment says,
/// so no test can reach the real registry, fonts folder or window text.
pub fn dry_run() -> bool {
    if cfg!(test) {
        return true;
    }
    static DRY: OnceLock<bool> = OnceLock::new();
    *DRY.get_or_init(|| dry_flag(std::env::var(DRY_VAR).ok().as_deref()))
}

/// The three folders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dirs {
    /// Settings and the window-text backup (roaming).
    pub user: PathBuf,
    /// Staging, the plan for the administrator step, and fonts added without installing them.
    pub local: PathBuf,
    /// The machine-wide backup, state and history.
    pub machine: PathBuf,
}

/// The folders for a run: under `root` when dry, Fontman's real ones otherwise.
pub fn dirs_for(dry: bool, root: &Path) -> Dirs {
    if dry {
        return Dirs {
            user: root.join("Roaming").join(FOLDER),
            local: root.join("Local").join(FOLDER),
            machine: root.join("ProgramData").join(FOLDER),
        };
    }
    let program_data = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\ProgramData"));
    Dirs {
        user: dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join(FOLDER),
        local: dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join(FOLDER),
        machine: program_data.join(FOLDER),
    }
}

/// The stand-in root of a dry run.
pub fn dry_root() -> PathBuf {
    if cfg!(test) {
        return std::env::temp_dir().join(format!("shellman-sysfont-test-{}", std::process::id()));
    }
    match std::env::var_os(ROOT_VAR).filter(|v| !v.is_empty()) {
        Some(root) => PathBuf::from(root),
        None => dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("shellman")
            .join("sysfont-dry"),
    }
}

fn dirs() -> &'static Dirs {
    static DIRS: OnceLock<Dirs> = OnceLock::new();
    DIRS.get_or_init(|| dirs_for(dry_run(), &dry_root()))
}

pub fn user_dir() -> PathBuf {
    dirs().user.clone()
}

pub fn local_dir() -> PathBuf {
    dirs().local.clone()
}

/// These describe HKLM, so they are shared by every user and written only by the administrator
/// step (or, in a dry run, in this process).
pub fn machine_dir() -> PathBuf {
    dirs().machine.clone()
}

pub fn staging_dir() -> PathBuf {
    local_dir().join("staging")
}

pub fn added_dir() -> PathBuf {
    local_dir().join("added")
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The family picked.
    pub family: Option<String>,
    pub overrides: BTreeMap<String, FaceChoice>,
    pub tune: Tune,
    pub layers: Layers,
    pub text_sizes: TextSizes,
    /// Fontman's own update check. Not used here, but kept so its file reads back the same.
    pub check_updates: bool,
    /// The choice as it was last put in place, to tell whether there are changes to apply.
    pub applied: Option<Snapshot>,
}

/// A choice as it was applied.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub family: String,
    pub overrides: BTreeMap<String, FaceChoice>,
    pub tune: Tune,
    pub layers: Layers,
    pub text_sizes: TextSizes,
}

impl Settings {
    pub fn snapshot(&self, family: &str) -> Snapshot {
        Snapshot {
            family: family.to_string(),
            overrides: self.overrides.clone(),
            tune: self.tune,
            layers: self.layers,
            text_sizes: self.text_sizes,
        }
    }

    /// Whether `family` with the current choices differs from what was last applied.
    pub fn differs_from_applied(&self, family: &str) -> bool {
        self.applied.as_ref() != Some(&self.snapshot(family))
    }
}

fn settings_path() -> PathBuf {
    user_dir().join("settings.json")
}

pub fn load() -> Settings {
    read_json(&settings_path()).unwrap_or_default()
}

pub fn save(settings: &Settings) -> Result<(), String> {
    write_json(&settings_path(), settings)
}

/// Fonts added without installing them: copies kept in the local folder, so they outlive the
/// original file.
pub fn added_files() -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(added_dir()) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_font_file(path))
        .collect();
    files.sort();
    files
}

pub fn is_font_file(path: &Path) -> bool {
    let lower = path.to_string_lossy().to_ascii_lowercase();
    [".ttf", ".otf", ".ttc", ".otc"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// Copy a font file in, after checking that it is a font that can be used. Returns the copy.
pub fn add_font_file(path: &Path) -> Result<PathBuf, String> {
    if !is_font_file(path) {
        return Err(format!(
            "{} is not a .ttf, .otf, or .ttc font file.",
            path.display()
        ));
    }
    let faces = crate::fonts::read_faces(path)?;
    if !faces.iter().any(|face| face.latin) {
        return Err(format!(
            "{} has no Latin letters, so it cannot be a UI font.",
            faces[0].family
        ));
    }
    let dir = added_dir();
    std::fs::create_dir_all(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    let name = path.file_name().ok_or("The file has no name.")?;
    let dest = dir.join(name);
    if dest != path {
        std::fs::copy(path, &dest).map_err(|err| format!("{}: {err}", dest.display()))?;
    }
    Ok(dest)
}

pub fn remove_added(path: &Path) -> Result<(), String> {
    if path.starts_with(added_dir()) {
        std::fs::remove_file(path).map_err(|err| format!("{}: {err}", path.display()))?;
    }
    Ok(())
}

pub fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Write through a temporary file, so a crash never leaves half a file.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|err| err.to_string())?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, text).map_err(|err| format!("{}: {err}", temp.display()))?;
    std::fs::rename(&temp, path).map_err(|err| format!("{}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dry_variable_reads_like_a_switch() {
        assert!(!dry_flag(None));
        assert!(!dry_flag(Some("")));
        assert!(!dry_flag(Some("0")));
        assert!(dry_flag(Some("1")));
        assert!(dry_flag(Some("yes")));
    }

    #[test]
    fn a_dry_run_keeps_every_folder_under_its_root() {
        let root = Path::new("C:\\Temp\\stand-in");
        let dry = dirs_for(true, root);
        for dir in [&dry.user, &dry.local, &dry.machine] {
            assert!(dir.starts_with(root), "{}", dir.display());
        }
        let real = dirs_for(false, root);
        for dir in [&real.user, &real.local, &real.machine] {
            assert!(!dir.starts_with(root), "{}", dir.display());
            assert!(dir.ends_with("Fontman"));
        }
    }

    #[test]
    fn tests_are_always_dry_and_rooted_in_temp() {
        assert!(dry_run());
        assert!(machine_dir().starts_with(std::env::temp_dir()));
        assert!(user_dir().starts_with(std::env::temp_dir()));
    }

    #[test]
    fn fontman_settings_read_back_the_same() {
        // A settings.json as Fontman writes it, its update check on.
        let text = r#"{"family":"SF Pro Display","overrides":{},"tune":{"match_line_height":true,"size":100,"synth_italic":true},
            "layers":{"window_text":true,"segoe":true,"variable":true,"legacy":true},
            "text_sizes":{"title":9.0,"menu":9.0,"message":9.0,"status":9.0,"icon":9.0},"check_updates":true,"applied":null}"#;
        let s: Settings = serde_json::from_str(text).unwrap();
        assert_eq!(s.family.as_deref(), Some("SF Pro Display"));
        assert!(s.check_updates);
        let again: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(again, s);
    }
}
