//! The classic programs: one per key under the Uninstall keys, read as Windows' own list does.
//! Pure: the registry is the trait, and the disk and environment variables are closures.

use super::app::{App, Classic, Kind};
use super::format;
use crate::registry::{Hive, Registry, Value};
use std::path::{Path, PathBuf};

const UNINSTALL: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";
const UNINSTALL_32: &str = "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall";

/// Where the keys are, in the order the list is built (so a double keeps its 64-bit entry).
pub const PLACES: [(&str, Hive, &str); 3] = [("hklm", Hive::Hklm, UNINSTALL), ("hklm32", Hive::Hklm, UNINSTALL_32), ("hkcu", Hive::Hkcu, UNINSTALL)];

/// ReleaseType values that mark a patch or an update, not a program.
const UPDATE_KINDS: [&str; 6] = ["security update", "update rollup", "hotfix", "service pack", "update", "critical update"];

fn text(values: &[(String, Value)], name: &str) -> Option<String> {
    let v = values.iter().find(|(n, _)| n.eq_ignore_ascii_case(name))?.1.text()?.trim().to_string();
    (!v.is_empty()).then_some(v)
}

fn number(values: &[(String, Value)], name: &str) -> Option<u64> {
    match &values.iter().find(|(n, _)| n.eq_ignore_ascii_case(name))?.1 {
        Value::Dword(n) => Some(u64::from(*n)),
        Value::Text(s) | Value::Expand(s) => s.trim().parse().ok(),
        Value::Binary(_) => None,
    }
}

fn flag(values: &[(String, Value)], name: &str) -> bool {
    number(values, name) == Some(1)
}

/// The file a DisplayIcon names ("C:\\x\\app.exe,0", possibly quoted), when it is a program or an
/// icon file that exists: the shell draws its image for us.
pub fn icon_path(display_icon: &str, expand: &dyn Fn(&str) -> String, exists: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    let mut s = expand(display_icon.trim());
    if let Some(rest) = s.strip_prefix('"') {
        s = rest.split('"').next().unwrap_or_default().to_string();
    } else if let Some((path, index)) = s.rsplit_once(',') {
        if index.trim().trim_start_matches('-').bytes().all(|b| b.is_ascii_digit()) {
            s = path.to_string();
        }
    }
    let path = PathBuf::from(s.trim());
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    (matches!(ext.as_str(), "exe" | "ico") && exists(&path)).then_some(path)
}

/// One key as a program, or None for what Windows' list leaves out.
fn read(tag: &str, key_name: &str, values: &[(String, Value)], expand: &dyn Fn(&str) -> String, exists: &dyn Fn(&Path) -> bool) -> Option<App> {
    let name = text(values, "DisplayName")?;
    if flag(values, "SystemComponent") || text(values, "ParentKeyName").is_some() || text(values, "ParentDisplayName").is_some() {
        return None;
    }
    if text(values, "ReleaseType").is_some_and(|r| UPDATE_KINDS.contains(&r.to_lowercase().as_str())) {
        return None;
    }
    let msi = (flag(values, "WindowsInstaller") && key_name.starts_with('{') && key_name.ends_with('}')).then(|| key_name.to_string());
    let classic = Classic {
        uninstall: text(values, "UninstallString").or_else(|| text(values, "QuietUninstallString")).map(|s| expand(&s)),
        modify: text(values, "ModifyPath").map(|s| expand(&s)),
        msi,
        no_remove: flag(values, "NoRemove"),
        no_modify: flag(values, "NoModify"),
    };
    let date = match values.iter().find(|(n, _)| n.eq_ignore_ascii_case("InstallDate")).map(|(_, v)| v) {
        Some(Value::Dword(n)) => format::parse_install_date(&n.to_string()),
        Some(v) => v.text().and_then(format::parse_install_date),
        None => None,
    };
    Some(App {
        id: format!("{tag}|{key_name}"),
        name,
        version: text(values, "DisplayVersion").unwrap_or_default(),
        publisher: text(values, "Publisher").unwrap_or_default(),
        size_kb: number(values, "EstimatedSize").filter(|n| *n > 0),
        date,
        icon: text(values, "DisplayIcon").and_then(|i| icon_path(&i, expand, exists)),
        kind: Kind::Classic(classic),
    })
}

/// Every classic program of the three Uninstall keys.
pub fn collect(reg: &dyn Registry, expand: &dyn Fn(&str) -> String, exists: &dyn Fn(&Path) -> bool) -> Vec<App> {
    let mut out = Vec::new();
    for (tag, hive, root) in PLACES {
        for key_name in reg.subkeys(hive, root) {
            let values = reg.list(hive, &format!("{root}\\{key_name}"));
            out.extend(read(tag, &key_name, &values, expand, exists));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::fake::Fake;

    fn put(reg: &Fake, hive: Hive, root: &str, key: &str, vals: &[(&str, Value)]) {
        for (n, v) in vals {
            reg.put(hive, &format!("{root}\\{key}"), n, v.clone());
        }
    }

    fn s(v: &str) -> Value {
        Value::Text(v.to_string())
    }

    fn all(reg: &Fake) -> Vec<App> {
        collect(reg, &|s| s.to_string(), &|_| true)
    }

    #[test]
    fn a_program_is_read_with_its_details() {
        let reg = Fake::default();
        put(
            &reg,
            Hive::Hklm,
            UNINSTALL,
            "{ABC-1}",
            &[
                ("DisplayName", s("Acme Tool")),
                ("DisplayVersion", s("2.1")),
                ("Publisher", s("Acme")),
                ("InstallDate", s("20240315")),
                ("EstimatedSize", Value::Dword(2048)),
                ("DisplayIcon", s(r#""C:\Acme\tool.exe",0"#)),
                ("UninstallString", s("MsiExec.exe /I{ABC-1}")),
                ("WindowsInstaller", Value::Dword(1)),
            ],
        );
        let apps = all(&reg);
        assert_eq!(apps.len(), 1);
        let a = &apps[0];
        assert_eq!((a.name.as_str(), a.version.as_str(), a.publisher.as_str()), ("Acme Tool", "2.1", "Acme"));
        assert_eq!((a.size_kb, a.date), (Some(2048), Some(20240315)));
        assert_eq!(a.icon.as_deref(), Some(Path::new(r"C:\Acme\tool.exe")));
        let Kind::Classic(c) = &a.kind else { panic!() };
        assert_eq!(c.msi.as_deref(), Some("{abc-1}"), "the fake keeps keys in lower case");
    }

    #[test]
    fn what_windows_leaves_out_is_left_out() {
        let reg = Fake::default();
        let name = ("DisplayName", s("X"));
        put(&reg, Hive::Hklm, UNINSTALL, "nameless", &[("Publisher", s("P"))]);
        put(&reg, Hive::Hklm, UNINSTALL, "system", &[name.clone(), ("SystemComponent", Value::Dword(1))]);
        put(&reg, Hive::Hklm, UNINSTALL, "patch", &[name.clone(), ("ParentKeyName", s("Office"))]);
        put(&reg, Hive::Hklm, UNINSTALL, "patch2", &[name.clone(), ("ParentDisplayName", s("Office"))]);
        put(&reg, Hive::Hklm, UNINSTALL, "update", &[name.clone(), ("ReleaseType", s("Security Update"))]);
        put(&reg, Hive::Hklm, UNINSTALL, "blank", &[("DisplayName", s("  "))]);
        put(&reg, Hive::Hklm, UNINSTALL, "real", &[name.clone(), ("SystemComponent", Value::Dword(0))]);
        let ids: Vec<_> = all(&reg).into_iter().map(|a| a.id).collect();
        assert_eq!(ids, vec!["hklm|real"]);
    }

    #[test]
    fn all_three_keys_are_read() {
        let reg = Fake::default();
        put(&reg, Hive::Hklm, UNINSTALL, "a", &[("DisplayName", s("A"))]);
        put(&reg, Hive::Hklm, UNINSTALL_32, "b", &[("DisplayName", s("B"))]);
        put(&reg, Hive::Hkcu, UNINSTALL, "c", &[("DisplayName", s("C"))]);
        let ids: Vec<_> = all(&reg).into_iter().map(|a| a.id).collect();
        assert_eq!(ids, vec!["hklm|a", "hklm32|b", "hkcu|c"]);
    }

    #[test]
    fn quiet_uninstall_is_only_a_fallback() {
        let reg = Fake::default();
        put(&reg, Hive::Hklm, UNINSTALL, "a", &[("DisplayName", s("A")), ("UninstallString", s("u.exe")), ("QuietUninstallString", s("u.exe /S"))]);
        put(&reg, Hive::Hklm, UNINSTALL, "b", &[("DisplayName", s("B")), ("QuietUninstallString", s("q.exe /S"))]);
        let apps = all(&reg);
        let un = |a: &App| match &a.kind {
            Kind::Classic(c) => c.uninstall.clone(),
            _ => None,
        };
        assert_eq!(un(&apps[0]).as_deref(), Some("u.exe"));
        assert_eq!(un(&apps[1]).as_deref(), Some("q.exe /S"));
    }

    #[test]
    fn icons_come_from_programs_and_icon_files_that_exist() {
        let id = |s: &str| s.to_string();
        let yes = |_: &Path| true;
        assert_eq!(icon_path(r"C:\a\x.exe,0", &id, &yes), Some(PathBuf::from(r"C:\a\x.exe")));
        assert_eq!(icon_path(r"C:\a b\x.exe,-12", &id, &yes), Some(PathBuf::from(r"C:\a b\x.exe")));
        assert_eq!(icon_path(r#""C:\a b\x.ico""#, &id, &yes), Some(PathBuf::from(r"C:\a b\x.ico")));
        assert_eq!(icon_path(r"C:\a\x.dll,3", &id, &yes), None, "a library's first image is not its icon");
        assert_eq!(icon_path(r"C:\a\x.exe", &id, &|_| false), None);
        assert_eq!(icon_path("", &id, &yes), None);
        let env = |s: &str| s.replace("%PF%", r"C:\Program Files");
        assert_eq!(icon_path(r"%PF%\x.exe,0", &env, &yes), Some(PathBuf::from(r"C:\Program Files\x.exe")));
    }
}
