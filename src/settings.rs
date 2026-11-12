//! The settings saved between runs: accent, hidden files, and open tabs.

use crate::theme;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub accent: theme::AccentChoice,
    pub show_hidden: bool,
    /// Folders open in tabs when the window last closed.
    pub tabs: Vec<PathBuf>,
    pub view: ViewMode,
    /// How much of the window's acrylic shows through, from 0 (solid) to 1 (nearly clear).
    pub glass: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            accent: Default::default(),
            show_hidden: false,
            tabs: Vec::new(),
            view: ViewMode::default(),
            glass: theme::DEFAULT_GLASS,
        }
    }
}

/// How a folder's items are shown.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum ViewMode {
    /// Rows with columns: name, date, type, size.
    List,
    /// Tiles with large previews. The default for a fresh install.
    #[default]
    Grid,
}

fn settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("fileman").join("settings.json"))
}

pub fn load_settings() -> Settings {
    settings_path()
        .and_then(|p| std::fs::read(p).ok())
        // Editors on Windows may save with a byte-order mark, which JSON parsers reject.
        .and_then(|b| serde_json::from_slice(b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&b)).ok())
        .unwrap_or_default()
}

pub fn save_settings(s: &Settings) {
    let Some(path) = settings_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_vec_pretty(s) {
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    }
}
