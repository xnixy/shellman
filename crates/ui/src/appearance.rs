//! The look every shellman app shares: accent color and glass amount, saved once in
//! `%APPDATA%\shellman\appearance.json`. Files edits it; the bars follow it live.

use crate::fonts::{self, FontChoice};
use crate::theme::{self, AccentChoice};
use serde::{Deserialize, Serialize};

const NAME: &str = "appearance";

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub accent: AccentChoice,
    /// How much of the window's acrylic shows through, from 0 (solid) to 1 (nearly clear).
    pub glass: f32,
    /// The text font: the default, or an installed family by name.
    pub font: FontChoice,
    /// The default for every app's sidebar (see `rail`): icons only, or icons and names. Set in
    /// Settings › Appearance › Sidebars.
    pub rail_collapsed: bool,
    /// Each app's own choice, made with the fold button at the foot of its sidebar. An app with
    /// none follows `rail_collapsed`; changing that default clears them all.
    pub rails: Rails,
}

/// The apps that have a sidebar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RailApp {
    Settings,
    Files,
    Tasks,
}

/// Each app's own fold, when it has one (`true`: icons only). Saved as
/// `"rails": { "files": true }`, the apps without one left out.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Rails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tasks: Option<bool>,
}

impl Rails {
    fn slot(&mut self, app: RailApp) -> &mut Option<bool> {
        match app {
            RailApp::Settings => &mut self.settings,
            RailApp::Files => &mut self.files,
            RailApp::Tasks => &mut self.tasks,
        }
    }
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            accent: AccentChoice::System,
            glass: theme::DEFAULT_GLASS,
            font: FontChoice::Default,
            rail_collapsed: false,
            rails: Rails::default(),
        }
    }
}

impl Appearance {
    /// Whether `app`'s sidebar is folded to icons: its own choice, else the default.
    pub fn rail_folded(&self, app: RailApp) -> bool {
        let mut rails = self.rails;
        rails.slot(app).unwrap_or(self.rail_collapsed)
    }

    /// Remember `app`'s own fold. The same as the default is no choice of its own, so the app
    /// keeps following the default.
    pub fn fold_rail(&mut self, app: RailApp, folded: bool) {
        *self.rails.slot(app) = (folded != self.rail_collapsed).then_some(folded);
    }

    /// Set the default for every app, which drops each app's own choice.
    pub fn set_rail_default(&mut self, collapsed: bool) {
        self.rail_collapsed = collapsed;
        self.rails = Rails::default();
    }

    /// Flip `app`'s fold in the saved look (read fresh, so nothing another app saved is undone),
    /// save it, and return the look as saved.
    pub fn toggle_rail_saved(app: RailApp, current: Appearance) -> Appearance {
        let mut look = match file() {
            Some(path) if path.exists() => shellman_core::config::load(NAME),
            _ => current,
        };
        let folded = current.rail_folded(app);
        look.fold_rail(app, !folded);
        look.save();
        look
    }

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
        // Does nothing when the window already wears this font.
        fonts::apply(ctx, self.font);
    }
}

/// The saved font choice, read without writing anything (the default when nothing is saved).
pub fn saved_font() -> FontChoice {
    match file() {
        Some(path) if path.exists() => shellman_core::config::load::<Appearance>(NAME).font,
        _ => FontChoice::Default,
    }
}

/// Where the file lives, for apps that watch it.
pub fn file() -> Option<std::path::PathBuf> {
    shellman_core::config::config_dir().map(|d| d.join(format!("{NAME}.json")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_app_follows_the_default_until_it_folds_its_own() {
        let mut look = Appearance::default();
        assert!(!look.rail_folded(RailApp::Files));
        look.fold_rail(RailApp::Files, true);
        assert!(look.rail_folded(RailApp::Files));
        assert!(!look.rail_folded(RailApp::Tasks));
        // Unfolding it again is the default: no choice of its own is kept.
        look.fold_rail(RailApp::Files, false);
        assert_eq!(look.rails, Rails::default());
    }

    #[test]
    fn changing_the_default_resets_every_app() {
        let mut look = Appearance::default();
        look.fold_rail(RailApp::Files, true);
        look.set_rail_default(true);
        look.fold_rail(RailApp::Tasks, false);
        assert!(!look.rail_folded(RailApp::Tasks));
        look.set_rail_default(false);
        assert_eq!(look.rails, Rails::default());
        assert!(!look.rail_folded(RailApp::Files));
        assert!(!look.rail_folded(RailApp::Settings));
    }

    #[test]
    fn the_old_file_still_reads_and_the_new_one_stays_small() {
        let old: Appearance = serde_json::from_str(r#"{"glass":0.5,"rail_collapsed":true}"#).unwrap();
        assert!(old.rail_folded(RailApp::Files));
        let mut look = Appearance::default();
        look.fold_rail(RailApp::Files, true);
        let json = serde_json::to_string(&look).unwrap();
        assert!(json.contains(r#""rails":{"files":true}"#), "{json}");
    }
}
