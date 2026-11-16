//! Firefox's `profiles.ini`, read only: the profiles it lists and the one an installation uses.
//!
//! Which profile is "the default" depends on the Firefox installation. Since Firefox 67 each
//! installation has an `[Install<id>]` section naming its own profile, where `<id>` is the
//! installation's 16-hex-digit id (the same id `TaskBarIDs` maps its folder to). The old
//! `Default=1` flag on a `[Profile…]` section is what Firefox before 67 used and is often stale
//! (it can point at an empty profile no installation opens), so it is never used here: with no
//! `[Install<id>]` section for the installation, nothing is preselected.

use std::path::{Path, PathBuf};

/// The sections of an ini file in order, each with its `key=value` lines.
#[derive(Clone, Debug, Default)]
pub struct Ini {
    sections: Vec<(String, Vec<(String, String)>)>,
}

impl Ini {
    pub fn parse(text: &str) -> Ini {
        let mut ini = Ini::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                ini.sections.push((name.trim().to_string(), Vec::new()));
            } else if let (Some((k, v)), Some(last)) = (line.split_once('='), ini.sections.last_mut()) {
                last.1.push((k.trim().to_string(), v.trim().to_string()));
            }
        }
        ini
    }

    /// The value of `key` in the section named `section` (names compare case-insensitively).
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections
            .iter()
            .filter(|(s, _)| s.eq_ignore_ascii_case(section))
            .flat_map(|(_, kv)| kv.iter())
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
}

/// One profile from `profiles.ini`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub path: PathBuf,
}

/// `Path=` as a folder: below `root` when relative (the default), `/` turned into `\`.
fn resolve(root: &Path, path: &str, relative: bool) -> PathBuf {
    let p = path.replace('/', "\\");
    if relative {
        root.join(p)
    } else {
        PathBuf::from(p)
    }
}

/// Every `[Profile…]` section, in file order. `root` is the folder holding `profiles.ini`.
pub fn list(ini: &Ini, root: &Path) -> Vec<Profile> {
    ini.sections
        .iter()
        .filter(|(s, _)| s.len() > 7 && s[..7].eq_ignore_ascii_case("profile"))
        .filter_map(|(_, kv)| {
            let get = |key: &str| kv.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str());
            let path = get("Path").filter(|p| !p.is_empty())?;
            let relative = get("IsRelative") != Some("0");
            let path = resolve(root, path, relative);
            let name = get("Name").map(str::to_string).unwrap_or_else(|| path.file_name().unwrap_or_default().to_string_lossy().into_owned());
            Some(Profile { name, path })
        })
        .collect()
}

/// The profile the installation with id `install_id` uses: its `[Install<id>]` section's
/// `Default=` (a path relative to `root` unless it is absolute).
pub fn default_for(ini: &Ini, root: &Path, install_id: &str) -> Option<PathBuf> {
    let d = ini.get(&format!("Install{install_id}"), "Default").filter(|d| !d.is_empty())?;
    let absolute = d.len() > 2 && (d.as_bytes()[1] == b':' || d.starts_with("\\\\"));
    Some(resolve(root, d, !absolute))
}

/// Whether `s` is spelled like an installation id: 16 hex digits.
pub fn is_install_id(s: &str) -> bool {
    s.len() == 16 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This PC's file on 15 Nov: two installations, and a stale `Default=1` on an empty profile.
    const SAMPLE: &str = "[General]\r\nStartWithLastProfile=1\r\nVersion=2\r\n\r\n[BackgroundTasksProfiles]\r\nMozillaBackgroundTask-308046B0AF4A39CB-defaultagent=mnop3456.MozillaBackgroundTask-308046B0AF4A39CB-defaultagent\r\n\r\n[Install9182EB444AA46955]\r\nDefault=Profiles/efgh5678.default-release-1\r\nLocked=1\r\n\r\n[Profile0]\r\nName=default-release\r\nIsRelative=1\r\nPath=Profiles/abcd1234.default-release\r\n\r\n[Profile2]\r\nName=default-release-1\r\nIsRelative=1\r\nPath=Profiles/efgh5678.default-release-1\r\n\r\n[Profile1]\r\nName=default\r\nIsRelative=1\r\nPath=Profiles/ijkl9012.default\r\nDefault=1\r\n\r\n[Install308046B0AF4A39CB]\r\nDefault=Profiles/abcd1234.default-release\r\nLocked=1\r\n\r\n[Profile3]\r\nName=elsewhere\r\nIsRelative=0\r\nPath=C:\\Temp\\fftest\\prof\r\n";

    #[test]
    fn profiles_are_listed_in_file_order_with_their_folders() {
        let root = Path::new(r"C:\Users\u\AppData\Roaming\Mozilla\Firefox");
        let ps = list(&Ini::parse(SAMPLE), root);
        let names: Vec<&str> = ps.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["default-release", "default-release-1", "default", "elsewhere"]);
        assert_eq!(ps[0].path, root.join(r"Profiles\abcd1234.default-release"));
        assert_eq!(ps[3].path, PathBuf::from(r"C:\Temp\fftest\prof"));
    }

    #[test]
    fn the_default_is_the_installations_own_never_the_stale_flag() {
        let root = Path::new(r"C:\ff");
        let ini = Ini::parse(SAMPLE);
        assert_eq!(default_for(&ini, root, "308046B0AF4A39CB"), Some(root.join(r"Profiles\abcd1234.default-release")));
        assert_eq!(default_for(&ini, root, "308046b0af4a39cb"), Some(root.join(r"Profiles\abcd1234.default-release")));
        assert_eq!(default_for(&ini, root, "9182EB444AA46955"), Some(root.join(r"Profiles\efgh5678.default-release-1")));
        // An installation profiles.ini does not know: nothing, not the Default=1 profile.
        assert_eq!(default_for(&ini, root, "0000000000000000"), None);
    }

    #[test]
    fn install_ids_are_sixteen_hex_digits() {
        assert!(is_install_id("308046B0AF4A39CB"));
        for no in ["", "308046B0AF4A39C", "308046B0AF4A39CBX", "308046B0AF4A39CG", "..\\..\\x"] {
            assert!(!is_install_id(no), "{no}");
        }
    }
}
