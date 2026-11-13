//! "Open Terminal here". Kept to this one function, because shellman's own terminal will take
//! its place.
//!
//! What starts a terminal without Explorer (measured on 26300 with shellman as the shell):
//! Windows Terminal is a Store app, and `shell:AppsFolder\…` needs Explorer, but its app alias
//! (`%LOCALAPPDATA%\Microsoft\WindowsApps\wt.exe`, a reparse point the system resolves itself)
//! starts it with `-d <folder>`. If the alias is missing (Terminal not installed, or its alias
//! turned off), the activation manager starts the package directly (`apps_index::activate`),
//! and if that fails too, PowerShell opens in the folder.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};

/// Windows Terminal's app id.
const TERMINAL: &str = "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App";
/// CREATE_NEW_CONSOLE, for the PowerShell fallback.
const NEW_CONSOLE: u32 = 0x10;

/// Where the app alias lives.
fn alias() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Microsoft\\WindowsApps\\wt.exe");
    p.exists().then_some(p)
}

/// Windows Terminal's arguments to start in `dir`. A trailing backslash would escape the quote
/// Windows puts around the folder, so a drive root keeps a dot after it.
pub fn args(dir: &Path) -> Vec<String> {
    let mut d = dir.display().to_string();
    if d.ends_with('\\') {
        d.push('.');
    }
    vec!["-d".into(), d]
}

/// Open a terminal in `dir`. Says how it was done, or why it could not be.
pub fn open_here(dir: &Path) -> Result<&'static str, String> {
    // The user asked for it: let it come to the front.
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    if let Some(wt) = alias() {
        if Command::new(wt).args(args(dir)).current_dir(dir).spawn().is_ok() {
            return Ok("Windows Terminal (alias)");
        }
    }
    let quoted = args(dir).iter().map(|a| format!("\"{a}\"")).collect::<Vec<_>>().join(" ");
    if shellman_core::apps_index::activate::activate_with(TERMINAL, &quoted) {
        return Ok("Windows Terminal (activation)");
    }
    Command::new("powershell.exe")
        .arg("-NoExit")
        .current_dir(dir)
        .creation_flags(NEW_CONSOLE)
        .spawn()
        .map(|_| "PowerShell")
        .map_err(|e| format!("no terminal would start: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_are_passed_with_d() {
        assert_eq!(args(Path::new(r"C:\Users\a\Desktop")), ["-d", r"C:\Users\a\Desktop"]);
        assert_eq!(args(Path::new(r"D:\")), ["-d", r"D:\."]);
    }
}
