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

/// Where a new tab is asked to start.
#[derive(Clone, Copy, Debug)]
pub enum Dir<'a> {
    /// Nowhere in particular: home.
    None,
    /// A folder the user chose (`-d`, the folder a start was made in, a handed-over launch).
    Given(&'a Path),
    /// The directory the shown tab's shell reported (OSC 7 / 9;9): output, so anyone's. Used
    /// only when it is a drive-letter path on a local drive, or (for a WSL tab) a POSIX path
    /// reported by a tab running that same distribution (`distro`).
    Reported { path: &'a str, distro: Option<&'a str> },
}

impl Profile {
    /// The WSL distribution this profile runs, if it is one.
    pub fn distro(&self) -> Option<&str> {
        match &self.kind {
            Kind::Wsl(d) => Some(d),
            _ => None,
        }
    }

    /// The command line to run, and the directory to start in, for a tab asked to start at
    /// `dir`. WSL takes the directory as `--cd` (it understands both its own paths and Windows
    /// ones); Windows shells start in a Windows one, else in the home folder.
    ///
    /// Nothing is looked up here (this runs on the UI thread): a reported path is checked by
    /// its syntax and its drive's type only (`local.rs`), never probed, so a path on another
    /// computer can neither stall the window nor be contacted; a folder that turns out not to
    /// be there makes the start fall back to home (`app::spawn`).
    pub fn launch(&self, dir: Dir, home: &Path) -> (String, PathBuf) {
        let windows_dir = match dir {
            Dir::Given(p) => Some(p.to_path_buf()).filter(|p| osc::is_windows_path(&p.to_string_lossy())),
            Dir::Reported { path, .. } => crate::local::drive_path(path).filter(|p| crate::local::on_local_drive(p)),
            Dir::None => None,
        };
        let start = windows_dir.clone().unwrap_or_else(|| home.to_path_buf());
        let command = match &self.kind {
            Kind::Wsl(distro) => {
                let posix = match dir {
                    Dir::Given(p) => p.to_str().filter(|s| s.starts_with('/')),
                    Dir::Reported { path, distro: from } if from == Some(distro.as_str()) && path.starts_with('/') && osc::reported_cwd_ok(path) => Some(path),
                    _ => None,
                };
                let cd = match (posix, &windows_dir) {
                    (Some(p), _) => p.to_owned(),
                    (None, Some(w)) => w.display().to_string(),
                    (None, None) => "~".to_owned(),
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
        let reported = |path| Dir::Reported { path, distro: None };
        let (cmd, dir) = pick(&list, "powershell").launch(reported(&windows), Path::new(r"C:\nowhere"));
        assert_eq!(cmd, r#""C:\Program Files\PowerShell\7\pwsh.exe" -NoLogo"#);
        assert_eq!(dir, PathBuf::from(&windows));
        // A POSIX directory means nothing to cmd: home.
        let (cmd, dir) = pick(&list, "Command Prompt").launch(reported("/home/me"), &home);
        assert_eq!((cmd.as_str(), dir), ("cmd.exe", home.clone()));
        // WSL takes its own as --cd.
        let debian = |path| Dir::Reported { path, distro: Some("Debian") };
        let (cmd, _) = pick(&list, "Debian").launch(debian("/home/me/my dir"), &home);
        assert_eq!(cmd, r#"wsl.exe -d Debian --cd "/home/me/my dir""#);
        let (cmd, _) = pick(&list, "Debian").launch(Dir::None, &home);
        assert_eq!(cmd, "wsl.exe -d Debian --cd ~");
        // A folder the user gave.
        let (cmd, _) = pick(&list, "Debian").launch(Dir::Given(Path::new("/srv")), &home);
        assert_eq!(cmd, "wsl.exe -d Debian --cd /srv");
    }

    #[test]
    fn reported_directories_must_be_local_and_are_never_probed() {
        let found = Found { pwsh: None, wsl: vec!["Debian".into(), "Ubuntu".into()], git_bash: None };
        let list = build(&found);
        let home = std::env::temp_dir();
        // UNC and device paths (syntax only: no network is touched): home, and not in the line.
        for path in [r"\\attacker.invalid\share", r"\\?\C:\x", r"\\.\pipe\x", r"//attacker.invalid/share"] {
            let (cmd, dir) = pick(&list, "Command Prompt").launch(Dir::Reported { path, distro: None }, &home);
            assert_eq!((cmd.as_str(), &dir), ("cmd.exe", &home), "{path}");
            let (cmd, dir) = pick(&list, "Debian").launch(Dir::Reported { path, distro: Some("Ubuntu") }, &home);
            assert_eq!((cmd.as_str(), &dir), ("wsl.exe -d Debian --cd ~", &home), "{path}");
        }
        // Another distribution's directory (or a Windows shell's POSIX claim) is not this one's.
        let (cmd, _) = pick(&list, "Debian").launch(Dir::Reported { path: "/home/u", distro: Some("Ubuntu") }, &home);
        assert_eq!(cmd, "wsl.exe -d Debian --cd ~");
        let (cmd, _) = pick(&list, "Debian").launch(Dir::Reported { path: "/home/u", distro: None }, &home);
        assert_eq!(cmd, "wsl.exe -d Debian --cd ~");
        // A local path is taken as it is, without looking whether it is there (the start falls
        // back to home when it is not).
        let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        let gone = format!(r"{system}\shellman-test-not-here-{}", std::process::id());
        let (_, dir) = pick(&list, "Command Prompt").launch(Dir::Reported { path: &gone, distro: None }, &home);
        assert_eq!(dir, PathBuf::from(&gone));
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
            let (cmd, _) = pick(&list, "Debian").launch(Dir::Given(Path::new(cwd)), &home);
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
