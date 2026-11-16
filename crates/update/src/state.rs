//! What the last check found, in `%LOCALAPPDATA%\shellman\update.json` (with
//! `SHELLMAN_CONFIG_DIR` set, as in tests: `<that folder>-update.json`, like the session's log).
//! `shellman-update.exe` writes it as it goes; the session reads when it last checked, and
//! Settings > About shows the result.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Nothing checked yet.
    #[default]
    Never,
    /// Under way (`phase` says which step).
    Working,
    UpToDate,
    /// A newer release was installed (`version`).
    Installed,
    /// A newer release exists and was not installed (updating automatically is off).
    Available,
    /// A newer release was ready but a fullscreen app or a file copy kept it waiting too long.
    Held,
    /// No network, a bad download, a refused signature, a failed install (`detail`).
    Failed,
    /// This copy was not installed by setup (a build folder): it is never updated.
    NotInstalled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct State {
    /// When the last check finished (Unix seconds); 0 never.
    pub last_check: u64,
    pub outcome: Outcome,
    /// The step under way while `Working`: "checking", "downloading", "waiting", "installing".
    pub phase: String,
    /// The newest release's version, when one was seen.
    pub version: String,
    /// Its release-notes page.
    pub page: String,
    /// Why, for `Failed` and `Held`.
    pub detail: String,
}

fn beside_config(suffix: &str, file: &str) -> Option<PathBuf> {
    match std::env::var_os("SHELLMAN_CONFIG_DIR").filter(|d| !d.is_empty()) {
        Some(mut dir) => {
            dir.push(suffix);
            Some(PathBuf::from(dir))
        }
        None => dirs::data_local_dir().map(|d| d.join("shellman").join(file)),
    }
}

pub fn path() -> Option<PathBuf> {
    beside_config("-update.json", "update.json")
}

/// The session's log (the same rule as `apps/session/src/log.rs`).
pub fn log_path() -> Option<PathBuf> {
    beside_config("-session.log", "session.log")
}

/// Unix seconds now.
pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl State {
    pub fn load() -> Self {
        path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    /// Written whole to a temporary file and renamed in, so a reader never sees half of it.
    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let tmp = p.with_extension(format!("{}.tmp", std::process::id()));
        if let Ok(text) = serde_json::to_string_pretty(self) {
            if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, &p).is_err() {
                let _ = std::fs::remove_file(&tmp);
            }
        }
    }

    /// One line for Settings.
    pub fn summary(&self) -> String {
        match self.outcome {
            Outcome::Never => "Not checked yet.".into(),
            Outcome::Working => match self.phase.as_str() {
                "downloading" => format!("Downloading {}…", self.version),
                "waiting" => format!("{} is ready; waiting until no app is fullscreen and no files are being copied…", self.version),
                "installing" => format!("Installing {}…", self.version),
                _ => "Checking…".into(),
            },
            Outcome::UpToDate => "shellman is up to date.".into(),
            Outcome::Installed => format!("Updated to {}. The apps restarted; the session itself switches at your next sign-in.", self.version),
            Outcome::Available => format!("{} is available. Press Check now to install it.", self.version),
            Outcome::Held => format!("{} is ready but was not installed: {}. It is tried again later.", self.version, self.detail),
            Outcome::Failed => format!("The last check failed: {}", self.detail),
            Outcome::NotInstalled => "This copy was not installed by setup, so it is not updated.".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_and_partial_files_read() {
        let s: State = serde_json::from_str(r#"{"outcome":"installed","version":"0.2.0","extra":1}"#).unwrap();
        assert_eq!((s.outcome, s.version.as_str(), s.last_check), (Outcome::Installed, "0.2.0", 0));
        assert_eq!(serde_json::from_str::<State>("{}").unwrap(), State::default());
        assert!(State { outcome: Outcome::Failed, detail: "offline".into(), ..Default::default() }.summary().contains("offline"));
    }
}
