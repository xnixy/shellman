//! The few Windows calls the installer makes itself: a console for the command-line modes,
//! signing out, and running the uninstall from a copy outside the install folder.

use std::path::{Path, PathBuf};
use windows::core::w;
use windows::Win32::Foundation::GENERIC_WRITE;
use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows::Win32::System::Console::{
    AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
};
use windows::Win32::System::Shutdown::{ExitWindowsEx, EWX_LOGOFF, SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MAJOR_APPLICATION};

/// Make `println!` reach the terminal this was started from (the exe is a window program).
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

/// Sign the user out (Windows asks apps with unsaved work).
pub fn sign_out() {
    unsafe {
        let _ = ExitWindowsEx(EWX_LOGOFF, SHTDN_REASON_MAJOR_APPLICATION | SHTDN_REASON_FLAG_PLANNED);
    }
}

fn inside(path: &Path, dir: &Path) -> bool {
    let n = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().trim_start_matches("\\\\?\\").to_ascii_lowercase();
    let (p, d) = (n(path), n(dir));
    p.starts_with(&format!("{}\\", d.trim_end_matches('\\')))
}

/// An uninstall started from the install folder cannot delete its own exe: copy it to a folder
/// in `%TEMP%` and run the copy with the same arguments (plus `--relaunched`). Returns whether
/// it did (then this process should end).
pub fn relaunch_outside(prefix: &Path, args: &[String]) -> Result<bool, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    if !inside(&me, prefix) {
        return Ok(false);
    }
    let dir = std::env::temp_dir().join(format!("shellman-uninstall-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let copy = dir.join("shellman-installer.exe");
    std::fs::copy(&me, &copy).map_err(|e| format!("cannot copy the uninstaller to {}: {e}", copy.display()))?;
    let mut a: Vec<String> = args.to_vec();
    a.push("--relaunched".into());
    if !a.iter().any(|x| x == "--prefix") {
        a.push("--prefix".into());
        a.push(prefix.display().to_string());
    }
    std::process::Command::new(&copy).args(&a).spawn().map_err(|e| format!("cannot start {}: {e}", copy.display()))?;
    Ok(true)
}

/// For the copy in `%TEMP%`: wait (up to 5 s) for the installer in the install folder that
/// started it to end, so the uninstall neither closes it nor finds its exe in use.
pub fn wait_for_stub(prefix: &Path) {
    let stub = prefix.join("shellman-installer.exe").to_string_lossy().to_ascii_lowercase();
    for _ in 0..50 {
        let running = shellman_setup::sys::processes().iter().any(|(_, image)| image.trim_start_matches("\\\\?\\").to_ascii_lowercase() == stub);
        if !running {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// For the copy in `%TEMP%`: remove its folder once this process has ended.
pub fn remove_self_later() {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from)) else { return };
    let name = dir.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    // Only the folder `relaunch_outside` made.
    if !name.starts_with("shellman-uninstall-") || !inside(&dir, &std::env::temp_dir()) {
        return;
    }
    let _ = std::process::Command::new("cmd.exe")
        .raw_arg(format!("/d /c ping -n 3 127.0.0.1 >nul & rd /s /q \"{}\"", dir.display()))
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}
