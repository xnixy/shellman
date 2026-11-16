//! The Islands look for Firefox (`integrations/firefox`): find Firefox and its profiles, and put
//! the look into one profile, bring it up to date, or take it out again, doing what
//! `install.ps1` / `uninstall.ps1` do, in Rust. Settings › Appearance, the install wizard and the
//! uninstaller use it.
//!
//! - [`actions`]: what a button press does (loader, then profile), for Settings and the installer.
//! - [`detect`]: installations (folder and id), the profiles folder, whether Firefox holds a
//!   profile, whether a folder is writable.
//! - [`profiles`]: `profiles.ini` (read only) and each installation's own profile.
//! - [`look`]: install / update / remove in a profile, with the backup and exact restore, and
//!   the look's state there. Its `state.json` is the scripts' own, so either tool undoes the other.
//! - [`userjs`]: the marked block in `user.js`, on bytes.
//! - [`loader`]: the optional scripts' two autoconfig files in Firefox's folder (fixed text).
//!
//! The files come from [`Source`]: the `firefox` folder the installer puts in shellman's install
//! folder, or `integrations\firefox` in a source checkout.

pub mod actions;
pub mod detect;
pub mod loader;
pub mod look;
pub mod profiles;
pub mod userjs;

use std::path::{Path, PathBuf};

/// The theme's files in `<profile>\chrome\`.
pub const CHROME_FILES: [&str; 5] = ["userChrome.css", "userContent.css", "shellman-look.css", "shellman-islands.css", "shellman-popups.css"];
/// The optional scripts' folder in `<profile>\chrome\`, and its files.
pub const SCRIPTS_DIR: &str = "shellman-scripts";
pub const SCRIPT_FILES: [&str; 5] = [
    "shellman-live.sys.mjs",
    "shellman-pills.sys.mjs",
    "shellman-window.sys.mjs",
    "shellman-glass.sys.mjs",
    "shellman-watch.worker.js",
];
/// Where shellman's install folder keeps the integration (`<install>\firefox`).
pub const INSTALLED_DIR: &str = "firefox";
/// Every file of the integration the installer ships, relative to its folder. The scripts and the
/// README come too, so the folder also works with `install.ps1` / `uninstall.ps1`.
pub fn shipped_files() -> Vec<String> {
    let mut v: Vec<String> = CHROME_FILES.iter().map(|f| format!("chrome\\{f}")).collect();
    v.extend(SCRIPT_FILES.iter().map(|f| format!("chrome\\{SCRIPTS_DIR}\\{f}")));
    v.extend(["user.js", "autoconfig\\config.js", "autoconfig\\config-prefs.js", "install.ps1", "uninstall.ps1", "common.ps1", "README.md"].map(String::from));
    v
}

/// The folder the look's files are read from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub dir: PathBuf,
}

impl Source {
    /// `dir`, if every file the look installs is in it.
    pub fn at(dir: &Path) -> Option<Source> {
        let s = Source { dir: dir.to_path_buf() };
        let all = CHROME_FILES.iter().map(|f| s.chrome(f)).chain(SCRIPT_FILES.iter().map(|f| s.script(f))).chain([s.prefs()]);
        all.into_iter().all(|p| p.is_file()).then_some(s)
    }

    /// Next to the running exe (`firefox`, as installed), else `integrations\firefox` in the
    /// source checkout the exe was built in.
    pub fn find() -> Option<Source> {
        let exe = std::env::current_exe().ok()?;
        let dir = exe.parent()?;
        Source::at(&dir.join(INSTALLED_DIR))
            .or_else(|| dir.ancestors().take(4).find_map(|d| Source::at(&d.join("integrations").join("firefox"))))
    }

    /// The source in shellman's install folder `prefix`.
    pub fn installed(prefix: &Path) -> Option<Source> {
        Source::at(&prefix.join(INSTALLED_DIR))
    }

    pub fn chrome(&self, name: &str) -> PathBuf {
        self.dir.join("chrome").join(name)
    }

    pub fn script(&self, name: &str) -> PathBuf {
        self.dir.join("chrome").join(SCRIPTS_DIR).join(name)
    }

    pub fn prefs(&self) -> PathBuf {
        self.dir.join("user.js")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The repo's `install-manifest` ships exactly the integration's files, into `firefox\`.
    #[test]
    fn the_install_manifest_ships_the_integration() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        let text = std::fs::read_to_string(repo.join("install-manifest")).unwrap();
        let mut listed: Vec<(String, String)> = text
            .lines()
            .map(str::trim)
            .filter(|l| l.contains("integrations/firefox/"))
            .map(|l| {
                let (src, dest) = l.trim_start_matches('?').split_once("->").expect("a destination");
                (src.trim().to_string(), dest.trim().to_string())
            })
            .collect();
        listed.sort();
        let mut want: Vec<(String, String)> = shipped_files()
            .into_iter()
            .map(|f| (format!("integrations/firefox/{}", f.replace('\\', "/")), format!("{INSTALLED_DIR}\\{f}")))
            .collect();
        want.sort();
        assert_eq!(listed, want);
        // And each of them is in the repo.
        let src = Source::at(&repo.join("integrations").join("firefox")).expect("the repo's integration");
        for f in shipped_files() {
            assert!(src.dir.join(&f).is_file(), "{f}");
        }
    }
}
