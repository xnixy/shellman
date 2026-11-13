//! The shells installed on this machine that a terminal tab can run: PowerShell 7, Windows
//! PowerShell, Command Prompt, each WSL distribution and Git Bash. Found from files and the
//! registry (never by running `wsl.exe`, which can take seconds to wake the WSL service). The
//! terminal builds its profiles from this; Settings lists their names.

use std::path::{Path, PathBuf};

/// What was found on this machine.
#[derive(Default, Debug)]
pub struct Found {
    pub pwsh: Option<PathBuf>,
    /// WSL distributions, the default one first.
    pub wsl: Vec<String>,
    pub git_bash: Option<PathBuf>,
}

impl Found {
    /// The profiles' names, in the order the terminal's menu shows them.
    pub fn names(&self) -> Vec<String> {
        let mut list = Vec::new();
        if self.pwsh.is_some() {
            list.push("PowerShell".to_owned());
        }
        list.push("Windows PowerShell".to_owned());
        list.push("Command Prompt".to_owned());
        list.extend(self.wsl.iter().cloned());
        if self.git_bash.is_some() {
            list.push("Git Bash".to_owned());
        }
        list
    }
}

/// The WSL distributions from the registry's `Lxss` subkeys `(guid, name)`, with the default
/// GUID first. Docker Desktop's own distributions are not shells anyone wants a tab of.
pub fn wsl_order(mut entries: Vec<(String, String)>, default: Option<&str>) -> Vec<String> {
    entries.retain(|(_, name)| !name.is_empty() && !name.starts_with("docker-desktop"));
    entries.sort_by_key(|(guid, _)| Some(guid.as_str()) != default);
    entries.into_iter().map(|(_, n)| n).collect()
}

/// Look on this machine.
pub fn detect() -> Found {
    use crate::registry::{Hive, Real, Registry};
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let pwsh = env("ProgramFiles").and_then(|pf| {
        ["7", "7-preview"].iter().map(|v| pf.join("PowerShell").join(v).join("pwsh.exe")).find(|p| p.is_file())
    });
    let git_bash = env("ProgramFiles").map(|pf| pf.join("Git").join("bin").join("bash.exe")).filter(|p| p.is_file());
    const LXSS: &str = r"Software\Microsoft\Windows\CurrentVersion\Lxss";
    let reg = Real;
    let default = reg.get(Hive::Hkcu, LXSS, "DefaultDistribution").and_then(|v| v.text().map(str::to_owned));
    let entries = reg
        .subkeys(Hive::Hkcu, LXSS)
        .into_iter()
        .filter_map(|guid| {
            let name = reg.get(Hive::Hkcu, &format!(r"{LXSS}\{guid}"), "DistributionName")?.text()?.to_owned();
            Some((guid, name))
        })
        .collect();
    let wsl = if Path::new(&format!(r"{}\System32\wsl.exe", std::env::var("SystemRoot").unwrap_or_default())).is_file() {
        wsl_order(entries, default.as_deref())
    } else {
        Vec::new()
    };
    Found { pwsh, wsl, git_bash }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wsl_default_first_and_no_docker() {
        let e = vec![
            ("{a}".to_string(), "Ubuntu".to_string()),
            ("{b}".to_string(), "docker-desktop".to_string()),
            ("{c}".to_string(), "Debian".to_string()),
        ];
        assert_eq!(wsl_order(e.clone(), Some("{c}")), ["Debian", "Ubuntu"]);
        assert_eq!(wsl_order(e, None), ["Ubuntu", "Debian"]);
    }

    #[test]
    fn names_in_the_menu_s_order() {
        let found = Found {
            pwsh: Some(PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe")),
            wsl: vec!["Ubuntu".into(), "Debian".into()],
            git_bash: Some(PathBuf::from(r"C:\Program Files\Git\bin\bash.exe")),
        };
        assert_eq!(found.names(), ["PowerShell", "Windows PowerShell", "Command Prompt", "Ubuntu", "Debian", "Git Bash"]);
        assert_eq!(Found::default().names(), ["Windows PowerShell", "Command Prompt"]);
    }
}
