//! Opening files with one particular app, as a drop on its dock icon does: only where that is
//! well defined. A program (an app known by its exe path) gets the paths as arguments, as
//! Explorer's drop on an exe does; a Store app goes through Windows' activation with files
//! (`IApplicationActivationManager::ActivateForFile`), which works for the file types the app
//! declares. Anything else (an app known only by an AppUserModelID whose exe may be a helper
//! process) is not opened: the caller only brings its window forward.

use std::path::{Path, PathBuf};

/// How an app takes files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum With {
    /// Run the program with the paths as arguments.
    Exe(PathBuf),
    /// Activate the Store app (`<Family>!<App>`) for the files.
    Packaged(String),
}

/// How the app with this id (a path, a Store id, or another AppUserModelID) and program takes
/// files, if that is well defined.
pub fn with_for(app_id: &str, exe: Option<&Path>) -> Option<With> {
    if !app_id.contains('\\') && app_id.contains('!') {
        return Some(With::Packaged(app_id.to_owned()));
    }
    let exe = exe?;
    let is_exe = exe.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"));
    // Known by its program path: the window's own program is the app.
    (app_id.contains('\\') && is_exe).then(|| With::Exe(exe.to_path_buf()))
}

/// Open `files` with the app. Blocks (a Store app's activation can take a while), so call it
/// from a thread of its own.
pub fn open(with: &With, files: &[PathBuf]) -> Result<(), String> {
    if files.is_empty() {
        return Ok(());
    }
    allow_foreground();
    match with {
        With::Exe(exe) => {
            let mut cmd = std::process::Command::new(exe);
            cmd.args(files);
            if let Some(dir) = exe.parent() {
                cmd.current_dir(dir);
            }
            cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
        }
        With::Packaged(id) => activate_for_files(id, files),
    }
}

fn allow_foreground() {
    use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
}

fn activate_for_files(id: &str, files: &[PathBuf]) -> Result<(), String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, CLSCTX_LOCAL_SERVER,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows::Win32::UI::Shell::{
        ApplicationActivationManager, IApplicationActivationManager, ILCreateFromPathW, ILFree,
        SHCreateShellItemArrayFromIDLists,
    };
    unsafe {
        if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
            return Err("COM".into());
        }
        let pidls: Vec<*mut ITEMIDLIST> = files
            .iter()
            .map(|f| ILCreateFromPathW(&HSTRING::from(f.as_os_str())))
            .filter(|p| !p.is_null())
            .collect();
        let result = (|| {
            if pidls.is_empty() {
                return Err("no items".to_string());
            }
            let list: Vec<*const ITEMIDLIST> = pidls.iter().map(|p| *p as *const _).collect();
            let items = SHCreateShellItemArrayFromIDLists(&list).map_err(|e| e.message())?;
            let manager: IApplicationActivationManager =
                CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_INPROC_SERVER | CLSCTX_LOCAL_SERVER)
                    .map_err(|e| e.message())?;
            let verb = HSTRING::from("open");
            manager
                .ActivateForFile(&HSTRING::from(id), &items, PCWSTR(verb.as_ptr()))
                .map(|_| ())
                .map_err(|e| e.message())
        })();
        for p in pidls {
            ILFree(Some(p as *const _));
        }
        CoUninitialize();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_well_defined_apps_take_files() {
        let exe = Path::new(r"C:\Windows\System32\notepad.exe");
        assert_eq!(with_for(r"c:\windows\system32\notepad.exe", Some(exe)), Some(With::Exe(exe.to_path_buf())));
        assert_eq!(
            with_for("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App", Some(Path::new(r"C:\x\Notepad.exe"))),
            Some(With::Packaged("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App".into()))
        );
        // An AppUserModelID that is not a Store app: its exe may be a helper (Steam's is).
        assert_eq!(with_for("Valve.Steam.Client", Some(Path::new(r"C:\Steam\steamwebhelper.exe"))), None);
        // A path that is not a program, or no program known.
        assert_eq!(with_for(r"c:\docs\a.lnk", Some(Path::new(r"C:\docs\a.lnk"))), None);
        assert_eq!(with_for(r"c:\apps\x.exe", None), None);
    }
}
