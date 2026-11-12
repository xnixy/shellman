//! The machine-wide part: the Segoe UI entries under HKLM, the replacement files in the Windows
//! fonts folder, and the backup that puts Windows back.
//!
//! Nothing here deletes or edits Windows' own font files. Applying points the Segoe UI registry
//! values at renamed copies of another font, and restoring points them back. The original values
//! are saved before the first change, and a .reg file that restores them is kept beside them, so
//! Windows can be put back even without Fontman.
//!
//! Adopted from Fontman unchanged in what it writes: the same `backup.json`, `state.json`,
//! `history\<generation>\plan.json` and `restore-fonts.reg` in `%ProgramData%\Fontman`, and the
//! same `fontman-` file names, so either program can restore what the other put in place.
//! Added here: the administrator step reads its plan only from the fixed staging place, checks it
//! strictly (`guard`), and has GDI and DirectWrite check every copy in the fonts folder before
//! any registry value points at it.

use crate::fonts::catalog::OWN_PREFIX;
use crate::fonts::segoe::{self, WINDOWS_DEFAULTS};
use crate::guard;
use crate::plan::{MachinePlan, RegistryPlan, EXTRA_PREFIX};
use crate::reg;
use crate::settings::{self, dry_run, machine_dir, read_json, write_json};
use crate::system;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The FontSubstitutes entries Fontman may change.
pub const DIALOG_FONTS: &[&str] = &["MS Shell Dlg", "MS Shell Dlg 2"];
/// How many earlier choices are kept, ready to use again.
const HISTORY: usize = 6;

/// Windows' own values, saved before Fontman changed anything.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Backup {
    pub created: u64,
    pub fonts: Vec<(String, String)>,
    /// None when the entry did not exist.
    pub substitutes: Vec<(String, Option<String>)>,
}

/// One time a font was put in place.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Applied {
    pub generation: String,
    pub family: String,
    pub summary: String,
    pub applied_at: u64,
    pub registry: RegistryPlan,
    pub files: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub current: Option<Applied>,
    pub restored_at: Option<u64>,
    /// Newest first. Includes the current one.
    pub history: Vec<Applied>,
}

/// What the window and `fontman status` report.
#[derive(Clone, Debug, Default)]
pub struct Status {
    pub current: Option<Applied>,
    /// A change was made since Windows started, so it shows fully only after a restart.
    pub restart_pending: bool,
    /// Something put Segoe UI back behind Fontman's back, usually a Windows update.
    pub drift: bool,
    pub history: Vec<Applied>,
    pub backup: bool,
}

fn backup_path() -> PathBuf {
    machine_dir().join("backup.json")
}

fn state_path() -> PathBuf {
    machine_dir().join("state.json")
}

pub fn restore_reg_path() -> PathBuf {
    machine_dir().join("restore-fonts.reg")
}

fn history_dir(generation: &str) -> PathBuf {
    machine_dir().join("history").join(generation)
}

pub fn load_state() -> State {
    read_json(&state_path()).unwrap_or_default()
}

pub fn load_backup() -> Option<Backup> {
    read_json(&backup_path())
}

/// The registry and fonts folder: the real ones, or stand-ins during a dry run.
#[derive(Debug, PartialEq, Eq)]
enum Backend {
    Real,
    Dry,
}

#[derive(Default, Serialize, Deserialize)]
struct DryRegistry {
    fonts: BTreeMap<String, String>,
    substitutes: BTreeMap<String, String>,
}

impl Backend {
    /// The only way a backend is made: a dry run never gets the real one.
    fn choose(dry: bool) -> Self {
        if dry {
            Backend::Dry
        } else {
            Backend::Real
        }
    }

    fn current() -> Self {
        Self::choose(dry_run())
    }

    /// Refuse to change anything real during a dry run, whatever got us here.
    fn may_write(&self) -> Result<(), String> {
        match self {
            Backend::Real => reg::write_allowed(dry_run()),
            Backend::Dry => Ok(()),
        }
    }

    /// Whether a font file the registry names is there. A dry run's stand-in folder only holds
    /// what dry runs copied, so a file already in the real fonts folder (one Fontman installed)
    /// counts too; that is only read.
    fn has_font_file(&self, data: &str) -> bool {
        self.fonts_dir().join(data).exists()
            || (*self == Backend::Dry && crate::fonts::windows_fonts_dir().join(data).exists())
    }

    fn dry_path() -> PathBuf {
        machine_dir().join("dry-registry.json")
    }

    /// The stand-in registry starts as a copy of the real one.
    fn dry_load() -> DryRegistry {
        if let Some(registry) = read_json(&Self::dry_path()) {
            return registry;
        }
        DryRegistry {
            fonts: reg::values(reg::HKEY_LOCAL_MACHINE, reg::FONTS_KEY)
                .into_iter()
                .collect(),
            substitutes: reg::values(reg::HKEY_LOCAL_MACHINE, reg::SUBSTITUTES_KEY)
                .into_iter()
                .collect(),
        }
    }

    fn dry_save(registry: &DryRegistry) -> Result<(), String> {
        write_json(&Self::dry_path(), registry)
    }

    fn font_values(&self) -> Vec<(String, String)> {
        match self {
            Backend::Real => reg::values(reg::HKEY_LOCAL_MACHINE, reg::FONTS_KEY),
            Backend::Dry => Self::dry_load().fonts.into_iter().collect(),
        }
    }

    fn set_font(&self, name: &str, data: &str) -> Result<(), String> {
        match self {
            Backend::Real => reg::set(reg::HKEY_LOCAL_MACHINE, reg::FONTS_KEY, name, data),
            Backend::Dry => {
                let mut registry = Self::dry_load();
                registry.fonts.insert(name.into(), data.into());
                Self::dry_save(&registry)
            }
        }
    }

    fn delete_font(&self, name: &str) -> Result<(), String> {
        match self {
            Backend::Real => reg::delete(reg::HKEY_LOCAL_MACHINE, reg::FONTS_KEY, name),
            Backend::Dry => {
                let mut registry = Self::dry_load();
                registry.fonts.remove(name);
                Self::dry_save(&registry)
            }
        }
    }

    fn substitute(&self, name: &str) -> Option<String> {
        match self {
            Backend::Real => reg::get(reg::HKEY_LOCAL_MACHINE, reg::SUBSTITUTES_KEY, name),
            Backend::Dry => Self::dry_load().substitutes.get(name).cloned(),
        }
    }

    fn set_substitute(&self, name: &str, data: Option<&str>) -> Result<(), String> {
        match (self, data) {
            (Backend::Real, Some(data)) => {
                reg::set(reg::HKEY_LOCAL_MACHINE, reg::SUBSTITUTES_KEY, name, data)
            }
            (Backend::Real, None) => {
                reg::delete(reg::HKEY_LOCAL_MACHINE, reg::SUBSTITUTES_KEY, name)
            }
            (Backend::Dry, data) => {
                let mut registry = Self::dry_load();
                match data {
                    Some(data) => registry.substitutes.insert(name.into(), data.into()),
                    None => registry.substitutes.remove(name),
                };
                Self::dry_save(&registry)
            }
        }
    }

    fn fonts_dir(&self) -> PathBuf {
        match self {
            Backend::Real => crate::fonts::windows_fonts_dir(),
            Backend::Dry => machine_dir().join("dry-fonts"),
        }
    }
}

fn is_own_file(data: &str) -> bool {
    Path::new(data).file_name().is_some_and(|name| {
        name.to_string_lossy()
            .to_ascii_lowercase()
            .starts_with(OWN_PREFIX)
    })
}

/// The Segoe UI values as Windows had them: from the backup, or read now if Fontman never
/// changed anything. Values that already point at Fontman files without a backup get the
/// Windows defaults.
pub fn originals() -> Vec<(String, String)> {
    if let Some(backup) = load_backup() {
        return backup.fonts;
    }
    snapshot_fonts(&Backend::current())
}

fn snapshot_fonts(backend: &Backend) -> Vec<(String, String)> {
    originals_from(backend.font_values())
}

/// The Segoe UI values among `values` as Windows had them. A value that already points at a
/// replacement (`fontman-...`, put there by Fontman or by an earlier apply) is never taken for
/// an original: it gets Windows' default file, or is left out when there is none.
pub fn originals_from(values: Vec<(String, String)>) -> Vec<(String, String)> {
    let defaults: BTreeMap<&str, &str> = WINDOWS_DEFAULTS.iter().copied().collect();
    values
        .into_iter()
        .filter(|(name, _)| segoe::is_candidate_value(name) && !name.starts_with(EXTRA_PREFIX))
        .filter_map(|(name, data)| {
            if is_own_file(&data) {
                defaults
                    .get(name.as_str())
                    .map(|original| (name.clone(), original.to_string()))
            } else {
                Some((name, data))
            }
        })
        .collect()
}

/// The font values as they are now: the registry's, or the stand-in's in a dry run.
pub fn font_values() -> Vec<(String, String)> {
    Backend::current().font_values()
}

/// Read the backup, or take it now: the first change is never made without one.
fn ensure_backup(backend: &Backend) -> Result<Backup, String> {
    if let Some(backup) = load_backup() {
        return Ok(backup);
    }
    let substitutes = DIALOG_FONTS
        .iter()
        .map(|name| {
            let value = backend.substitute(name);
            // A substitute Fontman set without a backup goes back to Windows' own choice.
            let value = match (name, value) {
                (_, Some(value)) if !value.eq_ignore_ascii_case("Segoe UI") => Some(value),
                (&"MS Shell Dlg", _) => Some("Microsoft Sans Serif".into()),
                _ => Some("Tahoma".into()),
            };
            (name.to_string(), value)
        })
        .collect();
    let backup = Backup {
        created: system::now(),
        fonts: snapshot_fonts(backend),
        substitutes,
    };
    if backup.fonts.is_empty() {
        return Err("Windows has no Segoe UI entries to back up.".into());
    }
    write_json(&backup_path(), &backup)?;
    Ok(backup)
}

/// Make the registry match `desired`, with every value it does not mention as in the backup.
fn converge(backend: &Backend, backup: &Backup, desired: &RegistryPlan) -> Result<(), String> {
    let current: BTreeMap<String, String> = backend.font_values().into_iter().collect();
    let wanted: BTreeMap<&str, &str> = desired
        .set
        .iter()
        .map(|(name, data)| (name.as_str(), data.as_str()))
        .collect();
    for (name, original) in &backup.fonts {
        let want = wanted.get(name.as_str()).copied().unwrap_or(original);
        if current.get(name).map(String::as_str) != Some(want) {
            backend.set_font(name, want)?;
        }
    }
    for (name, data) in &desired.set {
        if !backup.fonts.iter().any(|(known, _)| known == name) {
            backend.set_font(name, data)?;
        }
    }
    let extras: BTreeMap<&str, &str> = desired
        .extras
        .iter()
        .map(|(name, data)| (name.as_str(), data.as_str()))
        .collect();
    for name in current.keys() {
        if name.starts_with(EXTRA_PREFIX) && !extras.contains_key(name.as_str()) {
            backend.delete_font(name)?;
        }
    }
    for (name, data) in &desired.extras {
        if current.get(name) != Some(data) {
            backend.set_font(name, data)?;
        }
    }
    for (name, original) in &backup.substitutes {
        let want = desired
            .substitutes
            .iter()
            .find(|(known, _)| known == name)
            .map(|(_, data)| Some(data.as_str()))
            .unwrap_or(original.as_deref());
        if backend.substitute(name).as_deref() != want {
            backend.set_substitute(name, want)?;
        }
    }
    Ok(())
}

/// Where the plan for the administrator step is written, and the only place it is read from.
pub fn plan_path() -> PathBuf {
    settings::local_dir().join("plan.json")
}

/// Put the plan built for `generation` in place. Runs as administrator (or in this process, in a
/// dry run). The plan is read from [`plan_path`] only, and its files must be in that
/// generation's staging folder.
pub fn apply_staged(generation: &str) -> Result<String, String> {
    if !guard::is_generation(generation) {
        return Err("That is not a plan this step can use.".into());
    }
    let plan: MachinePlan = read_json(&plan_path()).ok_or("The plan file could not be read.")?;
    let staged = settings::staging_dir().join(generation);
    apply(&plan, generation, &staged)
}

/// Put `plan` in place, after checking it strictly: its generation, that every file is one of
/// the replacements just built in `staged`, and that it names only known Segoe UI values.
fn apply(plan: &MachinePlan, generation: &str, staged: &Path) -> Result<String, String> {
    let backend = Backend::current();
    backend.may_write()?;
    let known: Vec<String> = originals().into_iter().map(|(name, _)| name).collect();
    guard::check_plan(plan, generation, staged, &known)?;
    let backup = ensure_backup(&backend)?;
    let fonts = backend.fonts_dir();
    std::fs::create_dir_all(&fonts).map_err(|err| format!("{}: {err}", fonts.display()))?;

    // Files first: the registry never points at a file that is not there.
    let history = history_dir(&plan.generation);
    std::fs::create_dir_all(&history).map_err(|err| format!("{}: {err}", history.display()))?;
    let mut copied = Vec::new();
    let undo = |copied: &[PathBuf]| {
        for path in copied {
            let _ = std::fs::remove_file(path);
        }
    };
    for file in &plan.files {
        let dest = fonts.join(&file.file_name);
        match copy_unless_same(&file.staged, &dest) {
            Ok(true) => copied.push(dest),
            Ok(false) => {}
            Err(err) => {
                undo(&copied);
                return Err(format!("Could not copy {}: {err}", file.file_name));
            }
        }
    }
    // The copies are where Windows will read them and only an administrator can change them: ask
    // GDI and DirectWrite about those, not the staged files, before anything points at them.
    if let Err(err) = check_copies(plan, &fonts) {
        undo(&copied);
        return Err(err);
    }
    // The history keeps the checked copies, for "Use again".
    let mut kept = plan.clone();
    for saved in kept.files.iter_mut() {
        let in_history = history.join(&saved.file_name);
        let dest = fonts.join(&saved.file_name);
        if dest != in_history {
            if let Err(err) = std::fs::copy(&dest, &in_history) {
                undo(&copied);
                return Err(format!("Could not keep {}: {err}", saved.file_name));
            }
        }
        saved.staged = in_history;
    }
    write_json(&history.join("plan.json"), &kept)?;

    if let Err(err) = converge(&backend, &backup, &plan.registry) {
        // Leave Windows as it was rather than half changed.
        let _ = converge(&backend, &backup, &RegistryPlan::default());
        return Err(err);
    }

    let applied = Applied {
        generation: plan.generation.clone(),
        family: plan.family.clone(),
        summary: plan.summary.clone(),
        applied_at: system::now(),
        registry: plan.registry.clone(),
        files: plan
            .files
            .iter()
            .map(|file| file.file_name.clone())
            .collect(),
    };
    let mut state = load_state();
    state
        .history
        .retain(|old| old.generation != applied.generation);
    state.history.insert(0, applied.clone());
    prune_history(&mut state);
    state.current = Some(applied);
    state.restored_at = None;
    write_json(&state_path(), &state)?;
    remove_old_files(
        &backend,
        &plan
            .files
            .iter()
            .map(|f| f.file_name.clone())
            .collect::<Vec<_>>(),
    );
    write_restore_reg(&restore_reg_path())?;
    reset_font_cache(&backend);
    Ok(format!(
        "{} is in place. Restart Windows to see it everywhere.",
        plan.family
    ))
}

/// Have GDI and DirectWrite check each installed copy against the Segoe UI face it is for, the
/// same check the build made of the staged file.
fn check_copies(plan: &MachinePlan, fonts: &Path) -> Result<(), String> {
    let slots = segoe::slots(&originals())?;
    for file in &plan.files {
        let slot = slots
            .iter()
            .find(|slot| slot.key == file.key && slot.value == file.value)
            .ok_or_else(|| format!("{} is not a face this PC has.", file.label))?;
        crate::plan::check(slot, &fonts.join(&file.file_name))?;
    }
    Ok(())
}

/// Put Windows' own fonts back. Runs as administrator.
pub fn restore() -> Result<String, String> {
    let backend = Backend::current();
    backend.may_write()?;
    let backup = match load_backup() {
        Some(backup) => backup,
        None => ensure_backup(&backend)?,
    };
    converge(&backend, &backup, &RegistryPlan::default())?;
    let mut state = load_state();
    state.current = None;
    state.restored_at = Some(system::now());
    write_json(&state_path(), &state)?;
    remove_old_files(&backend, &[]);
    write_restore_reg(&restore_reg_path())?;
    reset_font_cache(&backend);
    Ok("Segoe UI is back. Restart Windows to see it everywhere.".into())
}

/// The plan of an earlier choice, with its files from the history folder.
pub fn history_plan(generation: &str) -> Option<MachinePlan> {
    let plan: MachinePlan = read_json(&history_dir(generation).join("plan.json"))?;
    plan.files
        .iter()
        .all(|file| file.staged.exists())
        .then_some(plan)
}

/// Put an earlier choice back in place. Runs as administrator. Its files must be in its own
/// history folder.
pub fn reuse(generation: &str) -> Result<String, String> {
    if !guard::is_generation(generation) {
        return Err("That is not a font used before.".into());
    }
    let plan = history_plan(generation)
        .ok_or("That earlier font is no longer saved. Choose it again instead.")?;
    apply(&plan, generation, &history_dir(generation))
}

/// Copy `from` to `to`, unless `to` already holds the same bytes. Windows keeps a registered font
/// open, so copying over the file in use for "Apply again" would fail. True when it copied.
fn copy_unless_same(from: &Path, to: &Path) -> std::io::Result<bool> {
    if let (Ok(a), Ok(b)) = (std::fs::metadata(from), std::fs::metadata(to)) {
        if a.len() == b.len() && std::fs::read(from)? == std::fs::read(to)? {
            return Ok(false);
        }
    }
    std::fs::copy(from, to).map(|_| true)
}

fn prune_history(state: &mut State) {
    while state.history.len() > HISTORY {
        if let Some(old) = state.history.pop() {
            let _ = std::fs::remove_dir_all(history_dir(&old.generation));
        }
    }
}

/// Remove replacement files no longer in use. Windows keeps registered fonts open until it
/// restarts, so a file that cannot go now is removed at the next restart.
fn remove_old_files(backend: &Backend, keep: &[String]) {
    let dir = backend.fonts_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.to_ascii_lowercase().starts_with(OWN_PREFIX)
            || keep.iter().any(|k| k.eq_ignore_ascii_case(&name))
        {
            continue;
        }
        // A file Windows still has open stays until a later apply or restore finds it free. It is
        // never marked to go at the next restart: "Use again" before that restart would point the
        // registry at the same file names, and the restart would then delete fonts in use.
        let _ = std::fs::remove_file(entry.path());
    }
}

fn delete_at_restart(path: &Path) {
    use windows::core::HSTRING;
    if dry_run() {
        return;
    }
    use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT};
    unsafe {
        let _ = MoveFileExW(
            &HSTRING::from(path.as_os_str()),
            None,
            MOVEFILE_DELAY_UNTIL_REBOOT,
        );
    }
}

/// Clear Windows' font caches, so the next start reads the fonts afresh instead of trusting a
/// cache that still lists Segoe UI's old files.
fn reset_font_cache(backend: &Backend) {
    if *backend == Backend::Dry || dry_run() {
        return;
    }
    let windir = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\Windows"));
    let services = ["FontCache", "FontCache3.0.0.0"];
    let stopped: Vec<&str> = services
        .iter()
        .copied()
        .filter(|name| crate::service::stop(name))
        .collect();
    let cache = windir.join("ServiceProfiles\\LocalService\\AppData\\Local\\FontCache");
    if let Ok(entries) = std::fs::read_dir(&cache) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if path.is_file() && name.contains("fontcache") && name.ends_with(".dat") {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    // The kernel holds this one open; it goes at the next start.
    let fntcache = windir.join("System32\\FNTCACHE.DAT");
    if fntcache.exists() && std::fs::remove_file(&fntcache).is_err() {
        delete_at_restart(&fntcache);
    }
    for name in stopped {
        crate::service::start(name);
    }
}

/// What is in place now, and whether it needs a restart or was undone by Windows.
pub fn status() -> Status {
    let state = load_state();
    let backend = Backend::current();
    let boot = system::boot_time();
    let changed_at = state
        .current
        .as_ref()
        .map(|applied| applied.applied_at)
        .into_iter()
        .chain(state.restored_at)
        .max()
        .unwrap_or(0);
    // Allow a little slack between the clock and the uptime counter.
    let restart_pending = changed_at > boot + 5;
    let drift = state.current.as_ref().is_some_and(|applied| {
        let values: BTreeMap<String, String> = backend.font_values().into_iter().collect();
        applied
            .registry
            .set
            .iter()
            .chain(&applied.registry.extras)
            .any(|(name, data)| values.get(name) != Some(data) || !backend.has_font_file(data))
    });
    Status {
        current: state.current,
        restart_pending,
        drift,
        history: state.history,
        backup: load_backup().is_some(),
    }
}

/// A .reg file that puts Windows' Segoe UI entries back and removes Fontman's. Double-clicking
/// it, or `reg import`, works without Fontman, even from Safe Mode.
pub fn write_restore_reg(path: &Path) -> Result<(), String> {
    let backup = load_backup().unwrap_or_else(|| Backup {
        created: 0,
        fonts: WINDOWS_DEFAULTS
            .iter()
            .map(|(name, data)| (name.to_string(), data.to_string()))
            .collect(),
        substitutes: vec![
            ("MS Shell Dlg".into(), Some("Microsoft Sans Serif".into())),
            ("MS Shell Dlg 2".into(), Some("Tahoma".into())),
        ],
    });
    let state = load_state();
    let mut extras: Vec<String> = Backend::current()
        .font_values()
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| name.starts_with(EXTRA_PREFIX))
        .collect();
    for applied in state.history.iter().chain(&state.current) {
        for (name, _) in &applied.registry.extras {
            if !extras.contains(name) {
                extras.push(name.clone());
            }
        }
    }
    let text = restore_reg_text(&backup, &extras);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    std::fs::write(path, bytes).map_err(|err| format!("{}: {err}", path.display()))
}

fn reg_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

fn restore_reg_text(backup: &Backup, extras: &[String]) -> String {
    let mut text = String::from("Windows Registry Editor Version 5.00\r\n\r\n");
    text.push_str("; Puts back the fonts Windows had before Fontman changed them.\r\n");
    text.push_str("; Double-click this file, or run: reg import restore-fonts.reg\r\n");
    text.push_str("; Then restart Windows.\r\n\r\n");
    text.push_str(&format!("[HKEY_LOCAL_MACHINE\\{}]\r\n", reg::FONTS_KEY));
    for (name, data) in &backup.fonts {
        text.push_str(&format!("{}={}\r\n", reg_string(name), reg_string(data)));
    }
    for name in extras {
        text.push_str(&format!("{}=-\r\n", reg_string(name)));
    }
    text.push_str(&format!(
        "\r\n[HKEY_LOCAL_MACHINE\\{}]\r\n",
        reg::SUBSTITUTES_KEY
    ));
    for (name, data) in &backup.substitutes {
        match data {
            Some(data) => text.push_str(&format!("{}={}\r\n", reg_string(name), reg_string(data))),
            None => text.push_str(&format!("{}=-\r\n", reg_string(name))),
        }
    }
    text
}

/// Copy the restore file somewhere the user picked, writing it first if needed.
pub fn export_restore_reg(dest: &Path) -> Result<(), String> {
    write_restore_reg(dest)
}

/// Result of the administrator step, handed back through a file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Outcome {
    pub ok: bool,
    pub message: String,
}

pub fn write_outcome(path: &Path, result: &Result<String, String>) {
    let outcome = match result {
        Ok(message) => Outcome {
            ok: true,
            message: message.clone(),
        },
        Err(message) => Outcome {
            ok: false,
            message: message.clone(),
        },
    };
    let _ = settings::write_json(path, &outcome);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_file_escapes_and_deletes_extras() {
        let backup = Backup {
            created: 0,
            fonts: vec![("Segoe UI (TrueType)".into(), "segoeui.ttf".into())],
            substitutes: vec![("MS Shell Dlg 2".into(), Some("Tahoma".into()))],
        };
        let text = restore_reg_text(
            &backup,
            &["Fontman Segoe UI Variable Small Light (TrueType)".into()],
        );
        assert!(text.starts_with("Windows Registry Editor Version 5.00"));
        assert!(text.contains("\"Segoe UI (TrueType)\"=\"segoeui.ttf\""));
        assert!(text.contains("\"Fontman Segoe UI Variable Small Light (TrueType)\"=-"));
        assert!(text.contains("CurrentVersion\\Fonts]"));
        assert!(text.contains("\"MS Shell Dlg 2\"=\"Tahoma\""));
        assert_eq!(reg_string("a\\b\"c"), "\"a\\\\b\\\"c\"");
    }

    /// Tests that use the (dry, temporary) machine folder take turns.
    pub(crate) static FOLDER: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn replaced(gen: &str) -> Vec<(String, String)> {
        vec![
            ("Segoe UI (TrueType)".into(), format!("fontman-{gen}-segoeui-regular.ttf")),
            ("Segoe UI Bold (TrueType)".into(), format!("fontman-{gen}-segoeui-bold.ttf")),
            ("Segoe UI Variable (TrueType)".into(), format!("fontman-{gen}-segoeuivar-text-regular.ttf")),
            ("Segoe UI Emoji (TrueType)".into(), "seguiemj.ttf".into()),
            (
                "Fontman Segoe UI Variable Display Bold (TrueType)".into(),
                format!("fontman-{gen}-segoeuivar-display-bold.ttf"),
            ),
            ("Arial (TrueType)".into(), "arial.ttf".into()),
        ]
    }

    #[test]
    fn a_dry_run_never_gets_the_real_backend() {
        assert_eq!(Backend::choose(true), Backend::Dry);
        assert_eq!(Backend::choose(false), Backend::Real);
        // Tests are dry, so this is what every function here gets.
        assert_eq!(Backend::current(), Backend::Dry);
        // And the real one refuses to write while the run is dry.
        assert!(Backend::Real.may_write().is_err());
        assert!(Backend::Dry.may_write().is_ok());
    }

    #[test]
    fn replaced_values_are_never_taken_for_originals() {
        let originals = originals_from(replaced("tob9y0"));
        assert_eq!(
            originals,
            vec![
                ("Segoe UI (TrueType)".to_string(), "segoeui.ttf".to_string()),
                ("Segoe UI Bold (TrueType)".to_string(), "segoeuib.ttf".to_string()),
                ("Segoe UI Variable (TrueType)".to_string(), "SegUIVar.ttf".to_string()),
            ]
        );
        assert!(originals.iter().all(|(_, data)| !is_own_file(data)));
    }

    #[test]
    fn applying_over_replaced_values_backs_up_windows_files() {
        let _turn = FOLDER.lock().unwrap_or_else(|e| e.into_inner());
        let _ = std::fs::remove_file(backup_path());
        // The stand-in registry as a PC Fontman changed has it, with no backup left.
        let registry = DryRegistry {
            fonts: replaced("abc123").into_iter().collect(),
            substitutes: [("MS Shell Dlg".to_string(), "Segoe UI".to_string()), ("MS Shell Dlg 2".to_string(), "Segoe UI".to_string())]
                .into_iter()
                .collect(),
        };
        Backend::dry_save(&registry).unwrap();
        let backup = ensure_backup(&Backend::Dry).unwrap();
        assert!(backup.fonts.contains(&("Segoe UI (TrueType)".into(), "segoeui.ttf".into())));
        assert!(backup.fonts.iter().all(|(_, data)| !is_own_file(data)));
        assert_eq!(backup.substitutes[0], ("MS Shell Dlg".into(), Some("Microsoft Sans Serif".into())));
        assert_eq!(backup.substitutes[1], ("MS Shell Dlg 2".into(), Some("Tahoma".into())));

        // A backup that exists is the original record: a later apply never takes a new one.
        let mut marked = backup.clone();
        marked.created = 42;
        write_json(&backup_path(), &marked).unwrap();
        assert_eq!(ensure_backup(&Backend::Dry).unwrap().created, 42);
        let _ = std::fs::remove_file(backup_path());
        let _ = std::fs::remove_file(Backend::dry_path());
    }

    #[test]
    fn the_step_refuses_odd_generations_before_reading_anything() {
        assert!(apply_staged("..\\x").is_err());
        assert!(apply_staged("").is_err());
        assert!(reuse("a/b").is_err());
    }

    #[test]
    fn own_files_are_recognized_by_name() {
        assert!(is_own_file("fontman-abc-segoeui-regular.ttf"));
        assert!(is_own_file("C:\\Windows\\Fonts\\Fontman-x.ttf"));
        assert!(!is_own_file("segoeui.ttf"));
    }
}
