//! Command-line modes. The exe is a Windows-subsystem program (no console window flashes when
//! Winlogon starts it), so a mode that prints attaches to the console it was started from.

use windows::core::w;
use windows::Win32::Foundation::GENERIC_WRITE;
use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_WRITE, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL};
use windows::Win32::System::Console::{
    AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
};

#[derive(PartialEq, Eq, Debug)]
pub enum Mode {
    Run { startup: bool },
    DryRun,
    Install,
    Uninstall,
    Help,
}

pub fn parse(args: &[String]) -> Mode {
    let has = |f: &str| args.iter().any(|a| a == f);
    if has("--dry-run") {
        Mode::DryRun
    } else if has("--install") {
        Mode::Install
    } else if has("--uninstall") {
        Mode::Uninstall
    } else if has("--help") || has("-h") || has("/?") {
        Mode::Help
    } else {
        Mode::Run { startup: !has("--no-startup") }
    }
}

/// Make `println!` reach the terminal this was started from. Redirected output already works.
pub fn attach_console() {
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        let has_out = GetStdHandle(STD_OUTPUT_HANDLE).is_ok_and(|h| !h.is_invalid() && !h.0.is_null());
        if !has_out {
            if let Ok(con) = CreateFileW(w!("CONOUT$"), GENERIC_WRITE.0, FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, None) {
                let _ = SetStdHandle(STD_OUTPUT_HANDLE, con);
                let _ = SetStdHandle(STD_ERROR_HANDLE, con);
            }
        }
    }
}

pub fn help() {
    println!(
        "shellman-session: the shell process.\n\n\
         (no arguments)  run: start and supervise the apps\n\
         --no-startup    run, without the user's startup items\n\
         --dry-run       print the apps and startup items it would start; changes nothing\n\
         --install       make this the user's shell (HKCU Winlogon Shell); see docs/PLAN.md\n\
         --uninstall     put Explorer back as the shell"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn modes() {
        assert_eq!(parse(&a(&[])), Mode::Run { startup: true });
        assert_eq!(parse(&a(&["--no-startup"])), Mode::Run { startup: false });
        assert_eq!(parse(&a(&["--dry-run"])), Mode::DryRun);
        assert_eq!(parse(&a(&["--install"])), Mode::Install);
        assert_eq!(parse(&a(&["--uninstall"])), Mode::Uninstall);
    }
}
