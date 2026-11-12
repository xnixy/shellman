//! Places that apps register for Explorer's navigation pane (iCloud Drive, OneDrive, Dropbox,
//! Google Drive, ...), worked out from registry data.
//!
//! This file is the pure half: it takes a plain description of what is in the registry and
//! produces the list. Reading the registry, and asking Windows to resolve names and known folders,
//! is `places_reg`. Nothing here names a particular app; whatever registers itself the standard
//! way shows up.
//!
//! The registrations are the ones Microsoft documents in "Integrate a Cloud Storage Provider"
//! (learn.microsoft.com/windows/win32/shell/integrate-cloud-storage), which is also what the
//! shell reads:
//! - `Desktop\NameSpace\{CLSID}` lists the entries (under HKCU and HKLM `...\Explorer`).
//! - `Software\Classes\CLSID\{CLSID}` holds the name (default value), `System.IsPinnedToNameSpaceTree`,
//!   `SortOrderIndex`, `DefaultIcon`, and `Instance\InitPropertyBag\TargetFolderPath`
//!   (or `TargetKnownFolder`).
//! - `Explorer\SyncRootManager\{provider!SID!account}` holds `DisplayNameResource`, `IconResource`
//!   and `UserSyncRoots\{SID}` whose value is the folder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One entry for the sidebar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    /// What hidden entries are remembered by: the CLSID, or `syncroot:` plus the registration's
    /// key name.
    pub id: String,
    pub name: String,
    pub target: PathBuf,
    /// The entry's own icon as registered: `path,index`, or an indirect `@dll,-id`.
    pub icon: Option<String>,
    pub order: u32,
}

/// The registry values of one CLSID's class registration. Everything is optional: a registration
/// may be missing, or partial.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClassData {
    pub name: Option<String>,
    pub pinned: Option<u32>,
    pub order: Option<u32>,
    pub icon: Option<String>,
    pub target_path: Option<String>,
    pub known_folder: Option<String>,
}

impl ClassData {
    /// Fields set here win; the rest come from `lower`. Windows shows HKCU\Software\Classes over
    /// HKLM's per value, so a per-user registration overrides without having to repeat all of it.
    fn over(self, lower: &ClassData) -> ClassData {
        ClassData {
            name: self.name.or_else(|| lower.name.clone()),
            pinned: self.pinned.or(lower.pinned),
            order: self.order.or(lower.order),
            icon: self.icon.or_else(|| lower.icon.clone()),
            target_path: self.target_path.or_else(|| lower.target_path.clone()),
            known_folder: self.known_folder.or_else(|| lower.known_folder.clone()),
        }
    }
}

/// One cloud-files sync root registration, for the current user.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncRootData {
    /// The registration's key name, `provider!SID!account`.
    pub key: String,
    pub name: Option<String>,
    pub icon: Option<String>,
    /// The folder, from `UserSyncRoots\<this user's SID>`.
    pub path: Option<String>,
}

/// Everything read from the registry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// CLSIDs listed under `Desktop\NameSpace`, per user and per machine.
    pub hkcu_namespace: Vec<String>,
    pub hklm_namespace: Vec<String>,
    /// Class registrations by CLSID (compared without regard to case).
    pub hkcu_classes: HashMap<String, ClassData>,
    pub hklm_classes: HashMap<String, ClassData>,
    pub sync_roots: Vec<SyncRootData>,
}

/// What the registry cannot say by itself: Windows resolves these.
pub trait Host {
    /// Expand `%VAR%` references.
    fn expand(&self, s: &str) -> String;
    /// The folder of a KNOWNFOLDERID such as `{...}`.
    fn known_folder(&self, id: &str) -> Option<PathBuf>;
    /// The text an indirect string `@dll,-id` points at.
    fn indirect(&self, s: &str) -> Option<String>;
    fn is_dir(&self, p: &Path) -> bool;
}

/// Order of entries with no `SortOrderIndex`, and of sync roots: after the ones that have one.
const LAST: u32 = u32::MAX;

fn key(clsid: &str) -> String {
    clsid.trim().to_ascii_uppercase()
}

/// The path as a key for finding two registrations of one folder.
fn same_folder_key(p: &Path) -> String {
    p.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_lowercase()
}

/// A name that may be an indirect resource string.
fn display_name(raw: &str, host: &dyn Host) -> Option<String> {
    let raw = raw.trim();
    let name = if raw.starts_with('@') {
        host.indirect(raw)?
    } else {
        raw.to_string()
    };
    let name = name.trim().to_string();
    (!name.is_empty()).then_some(name)
}

fn folder_name(p: &Path) -> String {
    p.file_name()
        .map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// The places to show, in order.
pub fn build(snap: &Snapshot, host: &dyn Host) -> Vec<Place> {
    let mut out: Vec<Place> = Vec::new();
    let mut seen: Vec<String> = Vec::new();

    // The same CLSID may be listed per user and per machine; take it once.
    let mut ids: Vec<&String> = Vec::new();
    for id in snap.hkcu_namespace.iter().chain(&snap.hklm_namespace) {
        if !ids.iter().any(|x| key(x) == key(id)) {
            ids.push(id);
        }
    }
    let empty = ClassData::default();
    for id in ids {
        let k = key(id);
        let lower = snap.hklm_classes.get(&k).unwrap_or(&empty);
        let class = match snap.hkcu_classes.get(&k) {
            Some(user) => user.clone().over(lower),
            None => lower.clone(),
        };
        if class.pinned != Some(1) {
            continue;
        }
        // Entries with no folder to open (WSL's "Linux", the shell's own virtual folders) end up
        // here: there is nothing for a file list to show.
        let target = class
            .target_path
            .as_deref()
            .map(|p| PathBuf::from(host.expand(p)))
            .or_else(|| class.known_folder.as_deref().and_then(|g| host.known_folder(g)));
        let Some(target) = target.filter(|t| host.is_dir(t)) else {
            continue;
        };
        let fk = same_folder_key(&target);
        if seen.contains(&fk) {
            continue;
        }
        seen.push(fk);
        let name = class
            .name
            .as_deref()
            .and_then(|n| display_name(n, host))
            .unwrap_or_else(|| folder_name(&target));
        out.push(Place {
            id: id.clone(),
            name,
            target,
            icon: class.icon.map(|i| host.expand(&i)),
            order: class.order.unwrap_or(LAST),
        });
    }

    // Sync roots next. OneDrive registers both a namespace entry and a sync root for one folder;
    // the namespace entry already has it, with its sort order.
    let mut roots: Vec<Place> = Vec::new();
    for sr in &snap.sync_roots {
        let Some(target) = sr
            .path
            .as_deref()
            .map(|p| PathBuf::from(host.expand(p)))
            .filter(|t| host.is_dir(t))
        else {
            continue;
        };
        let fk = same_folder_key(&target);
        if seen.contains(&fk) {
            continue;
        }
        seen.push(fk);
        let name = sr
            .name
            .as_deref()
            .and_then(|n| display_name(n, host))
            .unwrap_or_else(|| folder_name(&target));
        roots.push(Place {
            id: format!("syncroot:{}", sr.key),
            name,
            target,
            icon: sr.icon.as_deref().map(|i| host.expand(i)),
            order: LAST,
        });
    }
    out.extend(roots);
    out.sort_by(|a, b| {
        a.order
            .cmp(&b.order)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.id.cmp(&b.id))
    });
    out
}

/// Split `path,index` (or `@dll,-id`) at its last comma. A missing or unreadable index is 0.
pub fn split_icon(spec: &str) -> (String, i32) {
    let spec = spec.trim();
    if let Some((p, i)) = spec.rsplit_once(',') {
        if let Ok(i) = i.trim().parse::<i32>() {
            return (p.trim().trim_matches('"').to_string(), i);
        }
    }
    (spec.trim_matches('"').to_string(), 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        dirs: Vec<&'static str>,
    }

    impl Host for Fake {
        fn expand(&self, s: &str) -> String {
            s.replace("%USERPROFILE%", r"C:\Users\u")
        }
        fn known_folder(&self, id: &str) -> Option<PathBuf> {
            (id == "{KNOWN}").then(|| PathBuf::from(r"C:\Users\u\Known"))
        }
        fn indirect(&self, s: &str) -> Option<String> {
            (s == "@x.dll,-5").then(|| "Resource Name".to_string())
        }
        fn is_dir(&self, p: &Path) -> bool {
            self.dirs.iter().any(|d| same_folder_key(Path::new(d)) == same_folder_key(p))
        }
    }

    fn host() -> Fake {
        Fake {
            dirs: vec![
                r"C:\Users\u\Drive",
                r"C:\Users\u\Photos",
                r"C:\Users\u\Known",
                r"C:\Users\u\Other",
                r"C:\Users\u\OneDrive",
            ],
        }
    }

    fn pinned(name: &str, target: &str, order: u32) -> ClassData {
        ClassData {
            name: Some(name.into()),
            pinned: Some(1),
            order: Some(order),
            target_path: Some(target.into()),
            ..Default::default()
        }
    }

    fn snap(entries: &[(&str, ClassData)]) -> Snapshot {
        let mut s = Snapshot::default();
        for (id, c) in entries {
            s.hkcu_namespace.push((*id).into());
            s.hkcu_classes.insert(key(id), c.clone());
        }
        s
    }

    #[test]
    fn keeps_pinned_entries_with_a_folder_in_order() {
        let s = snap(&[
            ("{B}", pinned("Photos", r"C:\Users\u\Photos", 66)),
            ("{A}", pinned("Drive", r"C:\Users\u\Drive", 66)),
            ("{C}", pinned("First", r"C:\Users\u\Other", 10)),
        ]);
        let names: Vec<_> = build(&s, &host()).into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["First", "Drive", "Photos"]);
    }

    #[test]
    fn skips_unpinned_targetless_and_missing_folders() {
        let mut unpinned = pinned("Off", r"C:\Users\u\Drive", 1);
        unpinned.pinned = Some(0);
        let mut no_target = pinned("Linux", "", 1);
        no_target.target_path = None;
        let s = snap(&[
            ("{A}", unpinned),
            ("{B}", no_target),
            ("{C}", pinned("Gone", r"C:\Users\u\Nope", 1)),
            ("{D}", ClassData::default()),
            ("{E}", pinned("Ok", r"C:\Users\u\Drive", 1)),
        ]);
        let got = build(&s, &host());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "Ok");
    }

    #[test]
    fn user_registration_wins_over_machine_and_values_merge() {
        let mut s = Snapshot::default();
        s.hklm_namespace.push("{a}".into());
        s.hkcu_namespace.push("{A}".into());
        s.hklm_classes.insert(
            key("{A}"),
            ClassData {
                name: Some("Machine".into()),
                pinned: Some(1),
                order: Some(5),
                target_path: Some(r"C:\Users\u\Drive".into()),
                ..Default::default()
            },
        );
        s.hkcu_classes.insert(
            key("{A}"),
            ClassData {
                name: Some("User".into()),
                ..Default::default()
            },
        );
        let got = build(&s, &host());
        // One entry though listed twice; the user's name, the machine's remaining values.
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "User");
        assert_eq!(got[0].order, 5);
    }

    #[test]
    fn resolves_known_folders_variables_and_indirect_names() {
        let mut known = pinned("", "", 1);
        known.target_path = None;
        known.known_folder = Some("{KNOWN}".into());
        known.name = Some("@x.dll,-5".into());
        let s = snap(&[
            ("{A}", known),
            ("{B}", pinned("Env", r"%USERPROFILE%\Drive", 2)),
        ]);
        let got = build(&s, &host());
        assert_eq!(got[0].name, "Resource Name");
        assert_eq!(got[0].target, PathBuf::from(r"C:\Users\u\Known"));
        assert_eq!(got[1].target, PathBuf::from(r"C:\Users\u\Drive"));
    }

    #[test]
    fn an_unresolvable_indirect_name_falls_back_to_the_folder_name() {
        let s = snap(&[("{A}", pinned("@nope.dll,-1", r"C:\Users\u\Drive", 1))]);
        assert_eq!(build(&s, &host())[0].name, "Drive");
    }

    #[test]
    fn dedupes_by_folder_and_merges_sync_roots() {
        let mut s = snap(&[
            ("{A}", pinned("Drive", r"C:\Users\u\Drive", 1)),
            ("{B}", pinned("Again", r"c:\users\u\drive\", 2)),
        ]);
        s.sync_roots = vec![
            // Same folder as a namespace entry: dropped.
            SyncRootData {
                key: "OneDrive!S-1!Personal".into(),
                name: Some("OneDrive".into()),
                icon: None,
                path: Some(r"C:\Users\u\Drive".into()),
            },
            SyncRootData {
                key: "Other!S-1!x".into(),
                name: Some("@x.dll,-5".into()),
                icon: Some("%USERPROFILE%\\o.ico,0".into()),
                path: Some(r"C:\Users\u\OneDrive".into()),
            },
            // No folder on disk: dropped.
            SyncRootData {
                key: "Dead!S-1!x".into(),
                name: Some("Dead".into()),
                icon: None,
                path: Some(r"C:\Users\u\Nope".into()),
            },
        ];
        let got = build(&s, &host());
        let names: Vec<_> = got.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Drive", "Resource Name"]);
        assert_eq!(got[1].id, "syncroot:Other!S-1!x");
        assert_eq!(got[1].icon.as_deref(), Some(r"C:\Users\u\o.ico,0"));
    }

    #[test]
    fn icon_specs_split_at_the_last_comma() {
        assert_eq!(split_icon(r"C:\a b\x.ico,0"), (r"C:\a b\x.ico".into(), 0));
        assert_eq!(split_icon(r"@C:\x.dll,-101"), (r"@C:\x.dll".into(), -101));
        assert_eq!(split_icon(r#""C:\x.ico""#), (r"C:\x.ico".into(), 0));
        assert_eq!(split_icon(r"C:\x.exe,3"), (r"C:\x.exe".into(), 3));
    }
}
