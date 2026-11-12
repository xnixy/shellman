//! Asking for a file or folder through shellman's own picker (`shellman-files --pick`), without
//! waiting for it: the window keeps drawing, and `poll` is asked each frame.

use shellman_pick::{exit, FileType, Mode, Request};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

/// Names the picker program instead of the copy next to this one (for tests).
const EXE_VAR: &str = "SHELLMAN_PICKER_EXE";

pub struct Pick {
    child: Child,
    request: PathBuf,
    response: PathBuf,
}

impl Pick {
    /// Start the picker. The error says why it could not be.
    pub fn start(mode: Mode, title: &str, types: Vec<FileType>) -> Result<Self, String> {
        let exe = std::env::var_os(EXE_VAR)
            .map(PathBuf::from)
            .or_else(|| std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("shellman-files.exe"))))
            .filter(|p| p.exists())
            .ok_or("shellman-files.exe was not found next to Settings.")?;
        let request = shellman_pick::new_request_path(&std::env::temp_dir(), std::process::id(), nonce());
        let response = shellman_pick::response_path(&request);
        let _ = std::fs::remove_file(&response);
        let req = Request {
            mode,
            title: title.to_string(),
            types,
            client: "shellman-settings.exe".into(),
            host_pid: std::process::id(),
            must_exist: true,
            ..Default::default()
        };
        shellman_pick::write_json(&request, &req).map_err(|e| format!("Could not ask for a file: {e}"))?;
        let child = Command::new(exe)
            .arg("--pick")
            .arg(&request)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Could not start the file picker: {e}"))?;
        Ok(Self { child, request, response })
    }

    /// None while the picker is open; then `Some(None)` when nothing was chosen, or the choice.
    pub fn poll(&mut self) -> Option<Option<PathBuf>> {
        let status = self.child.try_wait().ok().flatten()?;
        let chosen = if status.code() == Some(exit::OK) {
            shellman_pick::read_response(&self.response).filter(|r| r.ok).and_then(|r| r.paths.into_iter().next())
        } else {
            None
        };
        let _ = std::fs::remove_file(&self.request);
        let _ = std::fs::remove_file(&self.response);
        Some(chosen)
    }
}

impl Drop for Pick {
    fn drop(&mut self) {
        // A picker this window started and no longer waits for.
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
        }
        let _ = std::fs::remove_file(&self.request);
        let _ = std::fs::remove_file(&self.response);
    }
}

fn nonce() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(1);
    N.fetch_add(1, Ordering::Relaxed)
}

/// The file types the icon chooser offers.
pub fn image_types() -> Vec<FileType> {
    vec![FileType::parse("Images", "*.png;*.svg;*.ico;*.jpg;*.jpeg;*.webp;*.bmp"), FileType::parse("All files", "*.*")]
}
