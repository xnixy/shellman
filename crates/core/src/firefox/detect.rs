//! Finding Firefox: its installations (folder and id), where its profiles live, and whether a
//! running Firefox holds a profile.
//!
//! Installations come from `TaskBarIDs` (HKLM and HKCU `SOFTWARE\Mozilla\Firefox\TaskBarIDs`,
//! one value per installation: the folder as the name, its id as the data), kept when the folder
//! still has `firefox.exe`. For tests, `SHELLMAN_FIREFOX_ROOT` replaces the folder holding
//! `profiles.ini` and `SHELLMAN_FIREFOX_DIR` (with `SHELLMAN_FIREFOX_ID`, default sixteen zeros)
//! the installations, so nothing reaches the real Firefox.

use super::profiles::{self, Ini, Profile};
use crate::registry::{Hive, Registry};
use std::path::{Path, PathBuf};

pub const TASKBAR_IDS: &str = r"SOFTWARE\Mozilla\Firefox\TaskBarIDs";

/// One Firefox installation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Install {
    pub dir: PathBuf,
    /// 16 hex digits, as `profiles.ini`'s `[Install<id>]` and `TaskBarIDs` spell it.
    pub id: String,
    /// Registered machine-wide (HKLM): only then can the administrator helper find it.
    pub machine: bool,
}

/// The installations in `TaskBarIDs` lists (HKLM's first), each folder once, kept when `exists`
/// says the folder has Firefox. Pure apart from `exists`.
pub fn installs_from(hklm: &[(String, String)], hkcu: &[(String, String)], exists: impl Fn(&Path) -> bool) -> Vec<Install> {
    let mut out: Vec<Install> = Vec::new();
    for (list, machine) in [(hklm, true), (hkcu, false)] {
        for (dir, id) in list {
            let dir = PathBuf::from(dir.trim_end_matches('\\'));
            if !profiles::is_install_id(id) || out.iter().any(|i| i.dir.as_os_str().eq_ignore_ascii_case(dir.as_os_str())) {
                continue;
            }
            if exists(&dir) {
                out.push(Install { dir, id: id.to_ascii_uppercase(), machine });
            }
        }
    }
    out
}

fn texts(reg: &dyn Registry, hive: Hive) -> Vec<(String, String)> {
    reg.list(hive, TASKBAR_IDS).into_iter().filter_map(|(n, v)| Some((n, v.text()?.to_string()))).collect()
}

/// The Firefox installations on this PC (or the test one).
pub fn installs(reg: &dyn Registry) -> Vec<Install> {
    if let Some(dir) = std::env::var_os("SHELLMAN_FIREFOX_DIR") {
        let id = std::env::var("SHELLMAN_FIREFOX_ID").unwrap_or_else(|_| "0".repeat(16));
        return vec![Install { dir: PathBuf::from(dir), id, machine: false }];
    }
    installs_from(&texts(reg, Hive::Hklm), &texts(reg, Hive::Hkcu), |d| d.join("firefox.exe").is_file())
}

/// The machine-wide installation with id `id`, as the administrator helper finds it: from HKLM
/// only, whose values only administrators can write.
pub fn machine_install(reg: &dyn Registry, id: &str) -> Option<PathBuf> {
    installs_from(&texts(reg, Hive::Hklm), &[], |d| d.join("firefox.exe").is_file())
        .into_iter()
        .find(|i| i.id.eq_ignore_ascii_case(id))
        .map(|i| i.dir)
}

/// The folder holding `profiles.ini`.
pub fn root() -> Option<PathBuf> {
    if let Some(r) = std::env::var_os("SHELLMAN_FIREFOX_ROOT") {
        return Some(PathBuf::from(r));
    }
    std::env::var_os("APPDATA").map(|a| Path::new(&a).join("Mozilla").join("Firefox"))
}

/// What `profiles.ini` lists, and the profile each installation uses.
#[derive(Clone, Debug, Default)]
pub struct Profiles {
    pub list: Vec<Profile>,
    ini: Ini,
    root: PathBuf,
}

impl Profiles {
    pub fn read() -> Profiles {
        let Some(root) = root() else { return Profiles::default() };
        let ini = Ini::parse(&std::fs::read_to_string(root.join("profiles.ini")).unwrap_or_default());
        Profiles { list: profiles::list(&ini, &root), ini, root }
    }

    /// The profile `install` opens by default, when profiles.ini names one that exists.
    pub fn default_for(&self, install: &Install) -> Option<PathBuf> {
        profiles::default_for(&self.ini, &self.root, &install.id).filter(|p| p.is_dir())
    }
}

/// Whether a running Firefox holds `profile`: it keeps `parent.lock` open with no sharing.
pub fn in_use(profile: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    ["parent.lock", "lock"].iter().any(|name| {
        let f = profile.join(name);
        f.exists() && std::fs::OpenOptions::new().read(true).write(true).share_mode(0).open(&f).is_err()
    })
}

/// Whether this process can write into `dir` and its `defaults\pref` (a folder under Program
/// Files needs administrator rights). Probes with a file it removes again.
pub fn writable(dir: &Path) -> bool {
    [dir.to_path_buf(), dir.join("defaults").join("pref")].iter().all(|d| {
        let probe = d.join(format!(".shellman-write-test-{}", std::process::id()));
        let ok = std::fs::write(&probe, b"").is_ok();
        let _ = std::fs::remove_file(&probe);
        ok
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kv(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn installations_come_once_machine_first_and_only_when_there() {
        let hklm = kv(&[(r"C:\Program Files\Mozilla Firefox", "308046B0AF4A39CB"), (r"C:\Gone", "1111111111111111")]);
        let hkcu = kv(&[(r"C:\Program Files\Mozilla Firefox\", "308046B0AF4A39CB"), (r"C:\Temp\ff", "9182eb444aa46955"), (r"C:\Bad", "not an id")]);
        let found = installs_from(&hklm, &hkcu, |d| !d.ends_with("Gone"));
        assert_eq!(
            found,
            vec![
                Install { dir: PathBuf::from(r"C:\Program Files\Mozilla Firefox"), id: "308046B0AF4A39CB".into(), machine: true },
                Install { dir: PathBuf::from(r"C:\Temp\ff"), id: "9182EB444AA46955".into(), machine: false },
            ]
        );
    }

    #[test]
    fn the_helper_finds_machine_installations_only() {
        use crate::registry::{fake::Fake, Value};
        let reg = Fake::default();
        let here = std::env::temp_dir().join(format!("shellman-ff-machine-{}", std::process::id()));
        std::fs::create_dir_all(&here).unwrap();
        std::fs::write(here.join("firefox.exe"), b"").unwrap();
        let dir = here.to_string_lossy().into_owned();
        reg.put(Hive::Hkcu, TASKBAR_IDS, &dir, Value::Text("ABCDEF0123456789".into()));
        assert_eq!(machine_install(&reg, "ABCDEF0123456789"), None);
        reg.put(Hive::Hklm, TASKBAR_IDS, &dir, Value::Text("ABCDEF0123456789".into()));
        assert_eq!(machine_install(&reg, "abcdef0123456789"), Some(here.clone()));
        let _ = std::fs::remove_dir_all(&here);
    }

    #[test]
    fn a_lock_held_without_sharing_means_in_use() {
        use std::os::windows::fs::OpenOptionsExt;
        let d = std::env::temp_dir().join(format!("shellman-ff-lock-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert!(!in_use(&d));
        std::fs::write(d.join("parent.lock"), b"").unwrap();
        assert!(!in_use(&d), "a lock file nobody holds");
        let held = std::fs::OpenOptions::new().read(true).write(true).share_mode(0).open(d.join("parent.lock")).unwrap();
        assert!(in_use(&d));
        drop(held);
        let _ = std::fs::remove_dir_all(&d);
    }
}
