//! The settings saved between runs: hidden files, the view, and open tabs.

use serde::{Deserialize, Serialize};
use shellman_core::config;
use std::path::PathBuf;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub show_hidden: bool,
    /// Folders open in tabs when the window last closed.
    pub tabs: Vec<PathBuf>,
    pub view: ViewMode,
    /// Folders the user pinned to the sidebar, in order.
    pub pins: Vec<PathBuf>,
    /// Ids (CLSIDs) of apps' sidebar entries the user hid. Kept here, never written to the registry.
    pub hidden_places: Vec<String>,
    /// Where the window was when it last closed, in physical pixels.
    pub window: Option<crate::placement::Rect>,
    /// Where items were last moved or copied to with Move to… and Copy to…, newest first.
    pub recent_dests: Vec<PathBuf>,
    /// Whether the details island shows on the right.
    pub details: bool,
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
