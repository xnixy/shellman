//! Which startup items there are, in the order Explorer runs them, and whether each is enabled.
//! Pure: the registry, the Startup folders' files and environment expansion come in as arguments.

use super::approved::{self, Area};
use crate::registry::{Hive, Registry};
use std::path::PathBuf;

const CV: &str = "Software\\Microsoft\\Windows\\CurrentVersion";

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Origin {
    Run { hive: Hive, key: String },
    Folder { common: bool, path: PathBuf },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Item {
    pub origin: Origin,
    /// Short place name for output, e.g. "HKCU Run".
    pub place: String,
    pub name: String,
    /// What to start: a command line, or a file path for a Startup folder.
    pub command: String,
    pub enabled: bool,
    pub why: String,
    /// A RunOnce value: removed from the registry when it is started.
    pub once: bool,
    /// RunOnce with a `!` prefix: removed after starting instead of before.
    pub delete_after: bool,
}

impl Item {
    /// The StartupApproved key that governs this item; None for RunOnce and policy items, which
    /// have no on/off switch.
    pub fn area(&self) -> Option<Area> {
        match &self.origin {
            Origin::Folder { .. } => Some(Area::StartupFolder),
            Origin::Run { .. } if self.once => None,
            Origin::Run { key, .. } => {
                let k = key.to_ascii_lowercase();
                if k.contains("policies") {
                    None
                } else if k.contains("wow6432node") {
                    Some(Area::Run32)
                } else {
                    Some(Area::Run)
                }
            }
        }
    }

    /// Whether the item is for everyone on the machine, so its approval is kept under HKLM.
    pub fn machine_wide(&self) -> bool {
        match &self.origin {
            Origin::Run { hive, .. } => *hive == Hive::Hklm,
            Origin::Folder { common, .. } => *common,
        }
    }
}

/// A file found in a Startup folder.
pub struct FolderFile {
    pub common: bool,
    pub path: PathBuf,
}

/// Leading `*` (run even in safe mode) and `!` (delete a RunOnce value after, not before)
/// belong to the value, not to the command. Returns the command and whether `!` was there.
pub fn strip_prefix(raw: &str) -> (&str, bool) {
    let mut after = false;
    let mut rest = raw.trim_start();
    while let Some(c) = rest.chars().next().filter(|c| *c == '*' || *c == '!') {
        after |= c == '!';
        rest = &rest[1..];
    }
    (rest.trim(), after)
}

fn hive_name(h: Hive) -> &'static str {
    if h == Hive::Hkcu { "HKCU" } else { "HKLM" }
}

pub fn collect(
    reg: &dyn Registry,
    files: &[FolderFile],
    expand: &dyn Fn(&str) -> String,
    elevated: bool,
) -> Vec<Item> {
    let run = format!("{CV}\\Run");
    let run32 = "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Run".to_string();
    let once = format!("{CV}\\RunOnce");
    let policy = format!("{CV}\\Policies\\Explorer\\Run");
    let mut items = Vec::new();

    // (hive, key, place, once, area); `None` area: not subject to StartupApproved.
    let places: [(Hive, &String, &str, bool, Option<Area>); 7] = [
        (Hive::Hklm, &once, "RunOnce", true, None),
        (Hive::Hkcu, &once, "RunOnce", true, None),
        (Hive::Hklm, &policy, "Policies Run", false, None),
        (Hive::Hkcu, &policy, "Policies Run", false, None),
        (Hive::Hklm, &run, "Run", false, Some(Area::Run)),
        (Hive::Hklm, &run32, "Run (32-bit)", false, Some(Area::Run32)),
        (Hive::Hkcu, &run, "Run", false, Some(Area::Run)),
    ];
    for (hive, key, place, is_once, area) in places {
        for (name, value) in reg.list(hive, key) {
            let Some(text) = value.text() else { continue };
            let (command, after) = strip_prefix(text);
            let (mut enabled, mut why) = match area {
                Some(a) => approved::check(reg, a, &name),
                None => (true, "not subject to StartupApproved".to_string()),
            };
            // Deleting a machine-wide RunOnce value takes admin rights; without them it would
            // run again at every logon, so it is not run at all.
            if is_once && hive == Hive::Hklm && !elevated {
                (enabled, why) = (false, "machine-wide RunOnce, and the session is not elevated".to_string());
            }
            items.push(Item {
                origin: Origin::Run { hive, key: key.clone() },
                place: format!("{} {place}", hive_name(hive)),
                name,
                command: expand(command),
                enabled,
                why,
                once: is_once,
                delete_after: is_once && after,
            });
        }
    }
    for f in files {
        let Some(name) = f.path.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
        if name.eq_ignore_ascii_case("desktop.ini") {
            continue;
        }
        let (enabled, why) = approved::check(reg, Area::StartupFolder, &name);
        let dir = f.path.parent().map(PathBuf::from).unwrap_or_default();
        items.push(Item {
            origin: Origin::Folder { common: f.common, path: dir },
            place: if f.common { "Common Startup folder" } else { "Startup folder" }.to_string(),
            command: f.path.to_string_lossy().into_owned(),
            name,
            enabled,
            why,
            once: false,
            delete_after: false,
        });
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{fake::Fake, Value};

    fn text(s: &str) -> Value {
        Value::Text(s.to_string())
    }

    fn approved_key(leaf: &str) -> String {
        format!("{CV}\\Explorer\\StartupApproved\\{leaf}")
    }

    #[test]
    fn prefixes_belong_to_the_value() {
        assert_eq!(strip_prefix("!*\"C:\\a b\\x.exe\" /q"), ("\"C:\\a b\\x.exe\" /q", true));
        assert_eq!(strip_prefix("*cmd /c x"), ("cmd /c x", false));
        assert_eq!(strip_prefix("plain.exe"), ("plain.exe", false));
    }

    #[test]
    fn collects_in_explorers_order_with_state() {
        let reg = Fake::default();
        reg.put(Hive::Hkcu, &format!("{CV}\\Run"), "A", text("%X%\\a.exe"));
        reg.put(Hive::Hklm, &format!("{CV}\\Run"), "B", text("b.exe"));
        reg.put(Hive::Hkcu, &format!("{CV}\\RunOnce"), "O", text("!o.exe"));
        reg.put(Hive::Hkcu, &approved_key("Run"), "A", Value::Binary(vec![3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        let files = [FolderFile { common: false, path: PathBuf::from("C:\\S\\t.lnk") }, FolderFile { common: false, path: PathBuf::from("C:\\S\\desktop.ini") }];
        let items = collect(&reg, &files, &|s| s.replace("%X%", "C:\\x"), true);
        let names: Vec<_> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["O", "B", "A", "t.lnk"]);
        assert!(items[0].once && items[0].delete_after && items[0].enabled);
        assert!(items[1].enabled);
        assert!(!items[2].enabled);
        assert_eq!(items[2].command, "C:\\x\\a.exe");
        assert!(items[3].enabled);
    }

    #[test]
    fn only_run_and_folder_items_have_a_switch() {
        let reg = Fake::default();
        reg.put(Hive::Hkcu, &format!("{CV}\\Run"), "A", text("a.exe"));
        reg.put(Hive::Hklm, &format!("{CV}\\Run"), "B", text("b.exe"));
        reg.put(Hive::Hklm, "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Run", "C", text("c.exe"));
        reg.put(Hive::Hkcu, &format!("{CV}\\RunOnce"), "O", text("o.exe"));
        reg.put(Hive::Hkcu, &format!("{CV}\\Policies\\Explorer\\Run"), "P", text("p.exe"));
        let files = [FolderFile { common: true, path: PathBuf::from("C:\\C\\t.lnk") }];
        let items = collect(&reg, &files, &|s| s.to_string(), true);
        let by = |n: &str| items.iter().find(|i| i.name == n).unwrap();
        assert_eq!((by("A").area(), by("A").machine_wide()), (Some(Area::Run), false));
        assert_eq!((by("B").area(), by("B").machine_wide()), (Some(Area::Run), true));
        assert_eq!((by("C").area(), by("C").machine_wide()), (Some(Area::Run32), true));
        assert_eq!((by("t.lnk").area(), by("t.lnk").machine_wide()), (Some(Area::StartupFolder), true));
        assert_eq!(by("O").area(), None);
        assert_eq!(by("P").area(), None);
    }

    #[test]
    fn machine_runonce_needs_elevation() {
        let reg = Fake::default();
        reg.put(Hive::Hklm, &format!("{CV}\\RunOnce"), "M", text("m.exe"));
        assert!(!collect(&reg, &[], &|s| s.to_string(), false)[0].enabled);
        assert!(collect(&reg, &[], &|s| s.to_string(), true)[0].enabled);
    }

    #[test]
    fn folder_items_use_their_own_approval_key() {
        let reg = Fake::default();
        reg.put(Hive::Hkcu, &approved_key("StartupFolder"), "t.lnk", Value::Binary(vec![3; 12]));
        let files = [FolderFile { common: true, path: PathBuf::from("C:\\C\\t.lnk") }];
        let items = collect(&reg, &files, &|s| s.to_string(), false);
        assert!(!items[0].enabled);
        assert_eq!(items[0].place, "Common Startup folder");
    }
}
