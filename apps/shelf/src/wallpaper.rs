//! "Change wallpaper…": pick a picture with shellman's own picker (`shellman-files --pick`, see
//! `crates/pick`) and make it Windows' wallpaper, which the desktop process already follows
//! (it hears `WM_SETTINGCHANGE` for the wallpaper and paints the new one).
//!
//! `SHELLMAN_WALLPAPER_DRYRUN=<file>` writes the chosen picture's path to that file instead of
//! setting it, for tests on a machine whose wallpaper must stay as it is.

use shellman_pick::{exit, FileType, Mode, Request};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::os::windows::ffi::OsStrExt;
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_SETDESKWALLPAPER,
};

pub const DRY_RUN: &str = "SHELLMAN_WALLPAPER_DRYRUN";
/// Names the picker program instead of the copy next to this one (for tests).
const PICKER_VAR: &str = "SHELLMAN_PICKER_EXE";

/// The file types offered.
fn image_types() -> Vec<FileType> {
    vec![
        FileType::parse("Pictures", "*.jpg;*.jpeg;*.png;*.bmp;*.gif;*.webp;*.jfif;*.heic"),
        FileType::parse("All files", "*.*"),
    ]
}

/// Ask for a picture, then set it. Blocking (the picker is open meanwhile): call it from a
/// thread of its own.
pub fn change() {
    match pick() {
        Ok(Some(path)) => match set(&path) {
            Ok(how) => crate::log::line(format!("wallpaper: {how} {}", path.display())),
            Err(e) => crate::log::line(format!("wallpaper: could not set {}: {e}", path.display())),
        },
        Ok(None) => crate::log::line("wallpaper: nothing chosen"),
        Err(e) => crate::log::line(format!("wallpaper: {e}")),
    }
}

/// Open the picker and wait for it. None when nothing was chosen.
fn pick() -> Result<Option<PathBuf>, String> {
    let exe = std::env::var_os(PICKER_VAR)
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("shellman-files.exe"))))
        .filter(|p| p.exists())
        .ok_or("shellman-files.exe was not found next to the shelf")?;
    let request = shellman_pick::new_request_path(&std::env::temp_dir(), std::process::id(), nonce());
    let response = shellman_pick::response_path(&request);
    let _ = std::fs::remove_file(&response);
    let req = Request {
        mode: Mode::Open,
        title: "Choose a wallpaper".into(),
        types: image_types(),
        client: "shellman-shelf.exe".into(),
        host_pid: std::process::id(),
        must_exist: true,
        ..Default::default()
    };
    shellman_pick::write_json(&request, &req).map_err(|e| format!("could not ask for a file: {e}"))?;
    let status = Command::new(exe)
        .arg("--pick")
        .arg(&request)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("could not start the picker: {e}"));
    let chosen = status.map(|s| {
        (s.code() == Some(exit::OK))
            .then(|| shellman_pick::read_response(&response))
            .flatten()
            .filter(|r| r.ok)
            .and_then(|r| r.paths.into_iter().next())
    });
    let _ = std::fs::remove_file(&request);
    let _ = std::fs::remove_file(&response);
    chosen
}

fn nonce() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(1);
    N.fetch_add(1, Ordering::Relaxed)
}

/// Make `path` Windows' wallpaper (saved, and every window told), or, under the dry run,
/// write it down instead.
pub fn set(path: &Path) -> Result<&'static str, String> {
    if let Some(file) = std::env::var_os(DRY_RUN).filter(|f| !f.is_empty()) {
        std::fs::write(&file, path.display().to_string()).map_err(|e| e.to_string())?;
        return Ok("dry run, would set");
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        SystemParametersInfoW(
            SPI_SETDESKWALLPAPER,
            0,
            Some(wide.as_ptr() as *mut _),
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        )
    }
    .map(|_| "set")
    .map_err(|e| e.message())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dry_run_writes_the_choice_down_and_leaves_the_wallpaper() {
        let file = std::env::temp_dir().join(format!("shellman-wallpaper-{}.txt", std::process::id()));
        std::env::set_var(DRY_RUN, &file);
        let how = set(Path::new(r"C:\pictures\dunes.jpg")).unwrap();
        std::env::remove_var(DRY_RUN);
        assert_eq!(how, "dry run, would set");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), r"C:\pictures\dunes.jpg");
        let _ = std::fs::remove_file(file);
    }
}
