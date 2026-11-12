//! The look every shellman app shares: accent color and glass amount, saved once in
//! `%APPDATA%\shellman\appearance.json`. Files edits it; the bars follow it live.

use crate::theme::{self, AccentChoice};
use serde::{Deserialize, Serialize};

const NAME: &str = "appearance";

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub accent: AccentChoice,
    /// How much of the window's acrylic shows through, from 0 (solid) to 1 (nearly clear).
    pub glass: f32,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            accent: AccentChoice::System,
            glass: theme::DEFAULT_GLASS,
        }
    }
}

impl Appearance {
    /// The saved look, or `fallback` when none is saved yet (then it is saved, so the other apps
    /// see the same).
    pub fn load_or(fallback: Appearance) -> Self {
        match shellman_core::config::config_dir().map(|d| d.join(format!("{NAME}.json"))) {
            Some(path) if path.exists() => shellman_core::config::load(NAME),
            _ => {
                fallback.save();
                fallback
            }
        }
    }

    pub fn save(&self) {
        shellman_core::config::save(NAME, self);
    }

    /// Make the theme use this look.
    pub fn apply(&self, ctx: &egui::Context) {
        theme::set_accent(ctx, self.accent.resolve());
        theme::set_glass(ctx, self.glass);
    }
}

/// Where the file lives, for apps that watch it.
pub fn file() -> Option<std::path::PathBuf> {
    shellman_core::config::config_dir().map(|d| d.join(format!("{NAME}.json")))
}
