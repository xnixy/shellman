//! Reading what is installed: the registry's font lists (read-only) and the fonts folder.

use super::catalog::{self, Family};
use shellman_core::registry::{Hive, Real, Registry};
use std::path::PathBuf;
use std::sync::OnceLock;

const KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts";
const USER_KEY: &str = r"Software\Microsoft\Windows NT\CurrentVersion\Fonts";

/// Where relative registry entries live.
pub fn fonts_dir() -> PathBuf {
    let win = std::env::var_os("windir").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    win.join("Fonts")
}

/// Every text font family on the machine (per-user installs and system ones), read once per
/// process; a font installed later shows up after a restart.
pub fn installed() -> &'static [Family] {
    static ALL: OnceLock<Vec<Family>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut entries = Vec::new();
        for (hive, key) in [(Hive::Hkcu, USER_KEY), (Hive::Hklm, KEY)] {
            for (name, value) in Real.list(hive, key) {
                if let Some(file) = value.text() {
                    entries.push((name, file.to_owned()));
                }
            }
        }
        catalog::parse(&entries, &fonts_dir())
    })
}
