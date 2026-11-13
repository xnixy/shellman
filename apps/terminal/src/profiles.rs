//! The shells a new tab can run: PowerShell 7, Windows PowerShell, Command Prompt, each WSL
//! distribution and Git Bash, whichever are installed. Found once at start from files and the
//! registry (`shellman_core::terminal::profiles`, which Settings lists too).

use crate::osc;
pub use shellman_core::terminal::profiles::{detect, Found};
use shellman_core::terminal::{quote_arg, quote_program};
use std::path::{Path, PathBuf};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Kind {
    Pwsh,
    WindowsPowerShell,
    Cmd,
    Wsl(String),
    GitBash,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Profile {
    pub name: String,
    pub kind: Kind,
    /// The program, quoted (`quote_program`).
    pub program: String,
}


/// The profiles for what was found, in the order the menu shows them.
pub fn build(found: &Found) -> Vec<Profile> {
    let mut list = Vec::new();
    if let Some(p) = &found.pwsh {
        list.push(Profile { name: "PowerShell".into(), kind: Kind::Pwsh, program: quote(p) });
    }
    list.push(Profile { name: "Windows PowerShell".into(), kind: Kind::WindowsPowerShell, program: "powershell.exe".into() });
    list.push(Profile { name: "Command Prompt".into(), kind: Kind::Cmd, program: "cmd.exe".into() });
    for d in &found.wsl {
        list.push(Profile { name: d.clone(), kind: Kind::Wsl(d.clone()), program: "wsl.exe".into() });
    }
    if let Some(p) = &found.git_bash {
        list.push(Profile { name: "Git Bash".into(), kind: Kind::GitBash, program: quote(p) });
    }
    list
}

impl Profile {
    /// The command line to run, and the directory to start in, for a tab opened from a tab whose
    /// shell reported `cwd` (or from nowhere). WSL takes the directory as `--cd` (it understands
    /// both its own paths and Windows ones); Windows shells start in it when it is a Windows
    /// path that exists, else in the home folder.
    pub fn launch(&self, cwd: Option<&str>, home: &Path) -> (String, PathBuf) {
        let windows_dir = cwd.filter(|c| osc::is_windows_path(c)).map(PathBuf::from).filter(|p| p.is_dir());
        let start = windows_dir.clone().unwrap_or_else(|| home.to_path_buf());
        let command = match &self.kind {
            Kind::Wsl(distro) => {
                let cd = match cwd {
                    Some(c) if c.starts_with('/') => c.to_owned(),
                    _ if windows_dir.is_some() => windows_dir.unwrap().display().to_string(),
                    _ => "~".to_owned(),
                };
                format!("wsl.exe -d {} --cd {}", quote_arg(distro), quote_arg(&cd))
            }
            Kind::GitBash => format!("{} -i -l", self.program),
            Kind::Pwsh => format!("{} -NoLogo", self.program),
            Kind::WindowsPowerShell => format!("{} -NoLogo", self.program),
            Kind::Cmd => self.program.clone(),
        };
        (command, start)
    }
}

/// The profile named `name`, else Windows PowerShell (always there).
pub fn pick<'a>(list: &'a [Profile], name: &str) -> &'a Profile {
    list.iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
        .or_else(|| list.iter().find(|p| p.kind == Kind::WindowsPowerShell))
        .unwrap_or(&list[0])
}

fn quote(p: &Path) -> String {
    quote_program(&p.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_has_what_was_found_in_order() {
        let found = Found {
            pwsh: Some(PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe")),
            wsl: vec!["Ubuntu".into(), "Debian".into()],
            git_bash: Some(PathBuf::from(r"C:\Program Files\Git\bin\bash.exe")),
        };
        let names: Vec<_> = build(&found).into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["PowerShell", "Windows PowerShell", "Command Prompt", "Ubuntu", "Debian", "Git Bash"]);
        let bare: Vec<_> = build(&Found::default()).into_iter().map(|p| p.name).collect();
        assert_eq!(bare, ["Windows PowerShell", "Command Prompt"]);
        // Settings lists the same names.
        assert_eq!(build(&found).into_iter().map(|p| p.name).collect::<Vec<_>>(), found.names());
    }


    #[test]
    fn launch_lines_and_directories() {
        let found = Found { pwsh: Some(PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe")), wsl: vec!["Debian".into()], git_bash: None };
        let list = build(&found);
        let home = std::env::temp_dir();
        let windows = std::env::temp_dir().display().to_string();
        let (cmd, dir) = pick(&list, "powershell").launch(Some(&windows), Path::new(r"C:\nowhere"));
        assert_eq!(cmd, r#""C:\Program Files\PowerShell\7\pwsh.exe" -NoLogo"#);
        assert_eq!(dir, PathBuf::from(&windows));
        // A POSIX directory means nothing to cmd: home.
        let (cmd, dir) = pick(&list, "Command Prompt").launch(Some("/home/me"), &home);
        assert_eq!((cmd.as_str(), dir), ("cmd.exe", home.clone()));
        // WSL takes it as --cd.
        let (cmd, _) = pick(&list, "Debian").launch(Some("/home/me/my dir"), &home);
        assert_eq!(cmd, r#"wsl.exe -d Debian --cd "/home/me/my dir""#);
        let (cmd, _) = pick(&list, "Debian").launch(None, &home);
        assert_eq!(cmd, "wsl.exe -d Debian --cd ~");
        // A directory that is gone: home.
        let (_, dir) = pick(&list, "Command Prompt").launch(Some(r"Q:\gone\away"), &home);
        assert_eq!(dir, home);
    }

    /// What Windows makes of a command line.
    fn argv(line: &str) -> Vec<String> {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{LocalFree, HLOCAL};
        use windows::Win32::UI::Shell::CommandLineToArgvW;
        let wide: Vec<u16> = line.encode_utf16().chain(std::iter::once(0)).collect();
        let mut n = 0;
        let list = unsafe { CommandLineToArgvW(PCWSTR(wide.as_ptr()), &mut n) };
        let out = (0..n as usize).map(|i| unsafe { (*list.add(i)).to_string().unwrap() }).collect();
        unsafe {
            let _ = LocalFree(Some(HLOCAL(list.cast())));
        }
        out
    }

    #[test]
    fn a_directory_cannot_add_arguments_to_a_wsl_line() {
        let found = Found { pwsh: None, wsl: vec!["Debian".into()], git_bash: None };
        let list = build(&found);
        let home = std::env::temp_dir();
        for cwd in [r#"/tmp/a" --exec calc.exe ""#, "/tmp/x\\\" --exec calc.exe", "/tmp/end\\", "/tmp/a\tb"] {
            let (cmd, _) = pick(&list, "Debian").launch(Some(cwd), &home);
            assert_eq!(argv(&cmd), ["wsl.exe", "-d", "Debian", "--cd", cwd], "{cmd}");
        }
    }

    #[test]
    fn an_unknown_default_falls_back_to_windows_powershell() {
        let list = build(&Found::default());
        assert_eq!(pick(&list, "zsh").kind, Kind::WindowsPowerShell);
        assert_eq!(pick(&list, "command prompt").kind, Kind::Cmd);
    }
}
