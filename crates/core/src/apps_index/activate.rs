//! Starting a Store app when the shell cannot.
//!
//! `ShellExecute` on `shell:AppsFolder\<Family>!<App>` hands the launch to Explorer; with no
//! Explorer in the session (shellman as the shell, after a clean sign-in) it fails at once with
//! 0x80040154. Windows' activation manager needs no Explorer and starts packaged Win32 apps
//! (Terminal, Notepad, Photos) in well under a second. Immersive apps still need the modern-apps
//! host (see `app_host`); without it the call blocks for about 45 s and fails, which is why it
//! always runs on a thread of its own.

use windows::core::HSTRING;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, CLSCTX_LOCAL_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::UI::Shell::{ApplicationActivationManager, IApplicationActivationManager, AO_NONE};
use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};

const APPS_FOLDER: &str = "shell:AppsFolder\\";

/// The Store app id in `shell:AppsFolder\<Family>!<App>`; None for anything else (a classic
/// app's entry there is a path or a plain id, which the shell opens without Explorer).
pub fn packaged_id(target: &str) -> Option<&str> {
    let prefix = target.get(..APPS_FOLDER.len())?;
    let id = &target[APPS_FOLDER.len()..];
    (prefix.eq_ignore_ascii_case(APPS_FOLDER) && id.contains('!') && !id.contains('\\')).then_some(id)
}

/// What to do when `ShellExecute` refused `target`: a Store app is started through the
/// activation manager, in the background. True when that was begun.
pub fn retry(target: &str) -> bool {
    let Some(id) = packaged_id(target) else { return false };
    let id = id.to_owned();
    std::thread::spawn(move || activate(&id));
    true
}

/// Blocking: see the module's note.
fn activate(id: &str) -> bool {
    unsafe {
        // A thread of our own, so this is its first and only COM use.
        if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
            return false;
        }
        // The user just asked for this app: let it come to the front.
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        let started = CoCreateInstance::<_, IApplicationActivationManager>(
            &ApplicationActivationManager,
            None,
            CLSCTX_INPROC_SERVER | CLSCTX_LOCAL_SERVER,
        )
        .and_then(|manager| manager.ActivateApplication(&HSTRING::from(id), None, AO_NONE))
        .is_ok();
        CoUninitialize();
        started
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "opens a Notepad window on this machine"]
    fn starts_a_store_app() {
        assert!(activate("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App"));
    }

    #[test]
    fn only_store_apps_in_the_apps_folder() {
        assert_eq!(
            packaged_id("shell:AppsFolder\\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"),
            Some("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App")
        );
        assert_eq!(packaged_id("SHELL:appsfolder\\Pkg_1!App"), Some("Pkg_1!App"));
        assert_eq!(packaged_id("shell:AppsFolder\\Chrome"), None);
        assert_eq!(packaged_id("shell:AppsFolder\\{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\cmd.exe"), None);
        assert_eq!(packaged_id("C:\\odd!name.exe"), None);
        assert_eq!(packaged_id("ms-settings:display"), None);
        assert_eq!(packaged_id(""), None);
    }
}
