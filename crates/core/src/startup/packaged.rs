//! Packaged (Store and MSIX) apps' startup tasks: what a package declares, and whether the user
//! has it on. Pure: the manifest text and the registry come in as arguments; `packaged_win`
//! finds the packages and starts a task.
//!
//! A package declares a task in its `AppxManifest.xml`, as an extension of one of its
//! applications:
//!
//! ```xml
//! <desktop:Extension Category="windows.startupTask" Executable="1Password.exe"
//!     EntryPoint="Windows.FullTrustApplication">
//!   <desktop:StartupTask TaskId="1PasswordStartup" Enabled="true" DisplayName="1Password" />
//! </desktop:Extension>
//! ```
//!
//! (`uap5:` for the same thing since 1703; `Executable` and `EntryPoint` on the extension are
//! optional and default to the application's.) A task whose entry point is
//! `Windows.FullTrustApplication` starts a desktop program with the package's identity; any other
//! entry point is a UWP app, which needs the immersive shell to come up.
//!
//! The user's choice is kept per task under
//! `HKCU\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\
//! SystemAppData\<package family>\<task id>`, value `State` (DWORD), with the numbers of the
//! public `Windows.ApplicationModel.StartupTaskState` enum: 0 Disabled, 1 DisabledByUser,
//! 2 Enabled, 3 DisabledByPolicy, 4 EnabledByPolicy. Windows writes it when the package is
//! registered (from `Enabled`) and whenever the app, Settings or Task Manager switches it.

use crate::registry::{Hive, Registry, Value};

pub const SYSTEM_APP_DATA: &str =
    "Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\SystemAppData";

const FULL_TRUST: &str = "Windows.FullTrustApplication";

/// One `windows.startupTask` extension of a manifest.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Declared {
    pub task_id: String,
    /// The `Id` of the application the extension belongs to.
    pub app_id: String,
    /// Package-relative path of the program the task starts.
    pub executable: String,
    /// `uap10:Parameters`, when the manifest gives any.
    pub parameters: String,
    /// A desktop program (`Windows.FullTrustApplication`) rather than a UWP app.
    pub full_trust: bool,
    /// The manifest's `Enabled`: the state before anyone switched it.
    pub enabled_by_default: bool,
    pub display_name: String,
}

/// The startup tasks a manifest declares, in document order. Malformed XML gives none.
pub fn parse_manifest(xml: &str) -> Vec<Declared> {
    let Ok(doc) = roxmltree::Document::parse(xml) else { return Vec::new() };
    let mut out = Vec::new();
    for app in doc.descendants().filter(|n| n.has_tag_name_local("Application")) {
        let app_id = app.attribute("Id").unwrap_or_default();
        let app_exe = app.attribute("Executable").unwrap_or_default();
        let app_entry = app.attribute("EntryPoint").unwrap_or_default();
        for ext in app.descendants().filter(|n| {
            n.has_tag_name_local("Extension") && n.attribute("Category") == Some("windows.startupTask")
        }) {
            let Some(task) = ext.children().find(|n| n.has_tag_name_local("StartupTask")) else { continue };
            let Some(task_id) = task.attribute("TaskId").filter(|t| !t.is_empty()) else { continue };
            let entry = ext.attribute("EntryPoint").unwrap_or(app_entry);
            let parameters = ext
                .attributes()
                .find(|a| a.name() == "Parameters")
                .map(|a| a.value().to_string())
                .unwrap_or_default();
            out.push(Declared {
                task_id: task_id.to_string(),
                app_id: app_id.to_string(),
                executable: ext.attribute("Executable").unwrap_or(app_exe).to_string(),
                parameters,
                full_trust: entry == FULL_TRUST,
                enabled_by_default: task.attribute("Enabled").is_some_and(|e| e.eq_ignore_ascii_case("true")),
                display_name: task.attribute("DisplayName").unwrap_or_default().to_string(),
            });
        }
    }
    out
}

trait LocalName {
    fn has_tag_name_local(&self, name: &str) -> bool;
}

impl LocalName for roxmltree::Node<'_, '_> {
    fn has_tag_name_local(&self, name: &str) -> bool {
        self.is_element() && self.tag_name().name() == name
    }
}

/// What `State` says: on or off, and a sentence saying why. `None` is a missing value, which
/// falls back to the manifest's `Enabled`.
pub fn judge(state: Option<u32>, enabled_by_default: bool) -> (bool, String) {
    match state {
        Some(0) => (false, "State 0: disabled".into()),
        Some(1) => (false, "State 1: disabled by the user".into()),
        Some(2) => (true, "State 2: enabled".into()),
        Some(3) => (false, "State 3: disabled by policy".into()),
        Some(4) => (true, "State 4: enabled by policy".into()),
        Some(n) => (false, format!("State {n}: unknown, treated as disabled")),
        None if enabled_by_default => (true, "no State value; the manifest says Enabled=\"true\"".into()),
        None => (false, "no State value; the manifest says Enabled=\"false\"".into()),
    }
}

/// The user's `State` for a package family's task, when there is one.
pub fn state(reg: &dyn Registry, family: &str, task_id: &str) -> Option<u32> {
    match reg.get(Hive::Hkcu, &format!("{SYSTEM_APP_DATA}\\{family}\\{task_id}"), "State")? {
        Value::Dword(n) => Some(n),
        _ => None,
    }
}

/// A task of an installed package, with the user's state: what the session lists and starts.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Task {
    pub family: String,
    /// The package's display name, for output.
    pub package_name: String,
    /// The package's folder, which holds the executable.
    pub install_dir: String,
    pub declared: Declared,
    pub enabled: bool,
    pub why: String,
}

impl Task {
    /// The app's user model id, `<family>!<app id>`.
    pub fn aumid(&self) -> String {
        format!("{}!{}", self.family, self.declared.app_id)
    }

    /// The program the task starts, as a full path.
    pub fn program(&self) -> String {
        let dir = self.install_dir.trim_end_matches('\\');
        let exe = self.declared.executable.trim_start_matches('\\');
        let path = format!("{dir}\\{exe}");
        if self.declared.parameters.is_empty() { path } else { format!("\"{path}\" {}", self.declared.parameters) }
    }
}

/// The tasks of one package, judged.
pub fn tasks_of(reg: &dyn Registry, family: &str, package_name: &str, install_dir: &str, manifest: &str) -> Vec<Task> {
    parse_manifest(manifest)
        .into_iter()
        .map(|declared| {
            let (enabled, why) = judge(state(reg, family, &declared.task_id), declared.enabled_by_default);
            Task {
                family: family.to_string(),
                package_name: package_name.to_string(),
                install_dir: install_dir.to_string(),
                declared,
                enabled,
                why,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::fake::Fake;

    const ONE_PASSWORD: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
         xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
         xmlns:desktop="http://schemas.microsoft.com/appx/manifest/desktop/windows10"
         xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities">
  <Applications>
    <Application Id="1Password" Executable="app\8\1Password.exe" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements DisplayName="1Password" Description="d" BackgroundColor="transparent" Square150x150Logo="a.png" Square44x44Logo="b.png" />
      <Extensions>
        <desktop:Extension Category="windows.startupTask" Executable="1Password.exe" EntryPoint="Windows.FullTrustApplication">
          <desktop:StartupTask TaskId="1PasswordStartup" Enabled="true" DisplayName="1Password" />
        </desktop:Extension>
        <uap:Extension Category="windows.protocol"><uap:Protocol Name="onepassword" /></uap:Extension>
      </Extensions>
    </Application>
  </Applications>
</Package>"#;

    // The shapes Windows Terminal (no Executable on the extension) and the Xbox app
    // (uap10:Parameters) use, and a UWP app.
    const MIXED: &str = r#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
         xmlns:uap5="http://schemas.microsoft.com/appx/manifest/uap/windows10/5"
         xmlns:uap10="http://schemas.microsoft.com/appx/manifest/uap/windows10/10">
  <Applications>
    <Application Id="App" Executable="WindowsTerminal.exe" EntryPoint="Windows.FullTrustApplication">
      <Extensions>
        <uap5:Extension Category="windows.startupTask">
          <uap5:StartupTask TaskId="StartTerminalOnLoginTask" Enabled="false" DisplayName="ms-resource:AppName" />
        </uap5:Extension>
        <uap5:Extension Category="windows.startupTask" Executable="XboxPcTray.exe" EntryPoint="Windows.FullTrustApplication" uap10:Parameters="/startupLaunch">
          <uap5:StartupTask TaskId="Tray" Enabled="true" DisplayName="XBOX" />
        </uap5:Extension>
      </Extensions>
    </Application>
    <Application Id="Uwp" Executable="Uwp.exe" EntryPoint="Uwp.App">
      <Extensions>
        <uap5:Extension Category="windows.startupTask">
          <uap5:StartupTask TaskId="UwpTask" Enabled="true" DisplayName="U" />
        </uap5:Extension>
        <uap5:Extension Category="windows.startupTask"><uap5:StartupTask Enabled="true" /></uap5:Extension>
      </Extensions>
    </Application>
  </Applications>
</Package>"#;

    #[test]
    fn reads_a_desktop_bridge_task() {
        let t = parse_manifest(ONE_PASSWORD);
        assert_eq!(
            t,
            [Declared {
                task_id: "1PasswordStartup".into(),
                app_id: "1Password".into(),
                executable: "1Password.exe".into(),
                parameters: String::new(),
                full_trust: true,
                enabled_by_default: true,
                display_name: "1Password".into(),
            }]
        );
    }

    #[test]
    fn extension_falls_back_to_its_application_and_reads_parameters() {
        let t = parse_manifest(MIXED);
        let ids: Vec<_> = t.iter().map(|d| d.task_id.as_str()).collect();
        assert_eq!(ids, ["StartTerminalOnLoginTask", "Tray", "UwpTask"], "a task without an id is skipped");
        assert_eq!((t[0].executable.as_str(), t[0].full_trust, t[0].enabled_by_default), ("WindowsTerminal.exe", true, false));
        assert_eq!((t[1].executable.as_str(), t[1].parameters.as_str()), ("XboxPcTray.exe", "/startupLaunch"));
        assert_eq!((t[2].app_id.as_str(), t[2].full_trust), ("Uwp", false));
    }

    #[test]
    fn bad_xml_declares_nothing() {
        assert!(parse_manifest("<Package><Applications>").is_empty());
        assert!(parse_manifest("").is_empty());
    }

    #[test]
    fn state_follows_the_startup_task_state_enum() {
        assert!(!judge(Some(0), true).0);
        assert!(!judge(Some(1), true).0);
        assert!(judge(Some(2), false).0);
        assert!(!judge(Some(3), true).0);
        assert!(judge(Some(4), false).0);
        assert!(!judge(Some(9), true).0);
        assert!(judge(None, true).0);
        assert!(!judge(None, false).0);
        assert!(judge(Some(1), true).1.contains("by the user"));
    }

    #[test]
    fn reads_the_users_state_from_system_app_data() {
        let reg = Fake::default();
        let key = format!("{SYSTEM_APP_DATA}\\Agilebits.1Password_amwd9z03whsfe\\1PasswordStartup");
        reg.put(Hive::Hkcu, &key, "State", Value::Dword(2));
        let tasks = tasks_of(&reg, "Agilebits.1Password_amwd9z03whsfe", "1Password", "C:\\P\\1P\\", ONE_PASSWORD);
        assert_eq!(tasks.len(), 1);
        assert!(tasks[0].enabled);
        assert_eq!(tasks[0].aumid(), "Agilebits.1Password_amwd9z03whsfe!1Password");
        assert_eq!(tasks[0].program(), "C:\\P\\1P\\1Password.exe");
        reg.put(Hive::Hkcu, &key, "State", Value::Dword(1));
        assert!(!tasks_of(&reg, "Agilebits.1Password_amwd9z03whsfe", "1Password", "C:\\P", ONE_PASSWORD)[0].enabled);
        // A State of the wrong type is as good as none.
        reg.put(Hive::Hkcu, &key, "State", Value::Text("2".into()));
        assert_eq!(state(&reg, "Agilebits.1Password_amwd9z03whsfe", "1PasswordStartup"), None);
    }
}
