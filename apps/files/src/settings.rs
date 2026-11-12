//! The settings saved between runs: accent, hidden files, and open tabs.

use serde::{Deserialize, Serialize};
use shellman_core::config;
use shellman_ui::theme;
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

/// Where the old Fileman kept its settings, read once when the new file does not exist yet.
fn legacy_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("fileman").join("settings.json"))
}

pub fn load_settings() -> Settings {
    let current = config::config_dir().map(|d| d.join("files.json"));
    if current.is_some_and(|p| p.exists()) {
        return config::load("files");
    }
    // Migration: the next save writes the new location.
    legacy_path()
        .map(|p| config::load_from(&p))
        .unwrap_or_default()
}

pub fn save_settings(s: &Settings) {
    config::save("files", s);
}
