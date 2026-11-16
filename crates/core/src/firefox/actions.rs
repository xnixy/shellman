//! What a button press does, put together from the parts: Install / Update (with or without the
//! scripts) and Remove for one profile, and the loader in Firefox's folder written or removed as
//! needed, directly when the folder is writable, else through the administrator helper (one UAC
//! prompt). These block (file copies, maybe a UAC prompt): call them from a worker thread.
//!
//! Order: the loader is written before the profile changes, so a declined prompt leaves the
//! profile as it was. It is removed after, and only when no profile still has the scripts; if
//! that fails the profile is done anyway and the message says the loader stays (it does nothing
//! without a profile's scripts folder).

use super::detect::{self, Install, Profiles};
use super::{loader, look, Source};
use crate::elevate::{self, ops::Op, Outcome};
use std::path::Path;

/// Where `shellman-elevate.exe` is: None for beside this exe.
pub type HelperDir<'a> = Option<&'a Path>;

fn helper(op: &Op, dir: HelperDir) -> Outcome {
    match dir {
        Some(d) => elevate::run_from(d, op),
        None => elevate::run(op),
    }
}

/// Make sure the scripts' loader for `install` is current.
pub fn ensure_loader(install: &Install, dir: HelperDir) -> Result<(), String> {
    match loader::state(&install.dir) {
        loader::State::Current => return Ok(()),
        loader::State::Foreign => {
            return Err("Firefox already has another autoconfig setup (config.js in its folder), so the scripts were not installed.".into())
        }
        _ => {}
    }
    if detect::writable(&install.dir) {
        return loader::write(&install.dir);
    }
    let op = Op::firefox_loader(true, &install.id).filter(|_| install.machine).ok_or(format!("Cannot write to {}.", install.dir.display()))?;
    match helper(&op, dir) {
        Outcome::Done if loader::state(&install.dir) == loader::State::Current => Ok(()),
        Outcome::Done => Err("The scripts' loader did not land in Firefox's folder.".into()),
        Outcome::Cancelled => Err("The scripts need administrator rights to put their loader in Firefox's folder. Nothing was changed.".into()),
        other => Err(other.message().unwrap_or_default()),
    }
}

/// Remove the loader from `ff` unless a profile other than `except` still has the scripts.
/// Returns a note when it stays.
pub fn drop_loader_if_unused(ff: &Path, except: &Path, profiles: &Profiles, installs: &[Install], dir: HelperDir) -> Option<String> {
    let used = profiles.list.iter().any(|p| !p.path.as_os_str().eq_ignore_ascii_case(except.as_os_str()) && look::uses_scripts(&p.path));
    if used || loader::state(ff) == loader::State::Absent {
        return None;
    }
    let stays = "The scripts' loader stays in Firefox's folder; it does nothing without the scripts.";
    if detect::writable(ff) {
        return loader::remove(ff).err().map(|e| format!("{stays} ({e})"));
    }
    let install = installs.iter().find(|i| i.machine && i.dir.as_os_str().eq_ignore_ascii_case(ff.as_os_str()));
    let Some(op) = install.and_then(|i| Op::firefox_loader(false, &i.id)) else { return Some(stays.into()) };
    match helper(&op, dir) {
        Outcome::Done if loader::state(ff) == loader::State::Absent => None,
        _ => Some(stays.into()),
    }
}

const RESTART: &str = "Restart Firefox to see the change.";

/// Install or update the look in `profile`; with scripts when `scripts_for` names the Firefox
/// they are for, CSS only otherwise (taking scripts installed before out).
pub fn apply(profile: &Path, src: &Source, scripts_for: Option<&Install>, profiles: &Profiles, installs: &[Install], dir: HelperDir) -> Result<String, String> {
    if detect::in_use(profile) {
        return Err(look::Refused::InUse.message());
    }
    if let Some(i) = scripts_for {
        ensure_loader(i, dir)?;
    }
    let done = look::install(profile, src, scripts_for.map(|i| i.dir.as_path())).map_err(|e| e.message())?;
    let mut msg = format!("{} {RESTART}", if done.updated { "Updated." } else { "Installed." });
    if let Some(ff) = done.scripts_dropped {
        if let Some(note) = drop_loader_if_unused(&ff, profile, profiles, installs, dir) {
            msg = format!("{msg} {note}");
        }
    }
    Ok(msg)
}

/// Remove the look from `profile`, and the loader when nothing uses it any more.
pub fn remove(profile: &Path, profiles: &Profiles, installs: &[Install], dir: HelperDir) -> Result<String, String> {
    let done = look::remove(profile).map_err(|e| e.message())?;
    if done.nothing {
        return Ok("The look was not installed in this profile.".into());
    }
    let mut msg = format!("Removed. {RESTART} Prefs Firefox already copied into prefs.js (the backdrop, vertical tabs) stay; reset them in about:config if you like.");
    if let Some(ff) = done.scripts_for {
        if let Some(note) = drop_loader_if_unused(&ff, profile, profiles, installs, dir) {
            msg = format!("{msg} {note}");
        }
    }
    Ok(msg)
}

/// The profiles that have the look.
pub fn profiles_with_look(profiles: &Profiles) -> Vec<std::path::PathBuf> {
    profiles.list.iter().filter(|p| look::read_state(&p.path).is_some()).map(|p| p.path.clone()).collect()
}
