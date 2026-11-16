//! The look every shellman app shares: accent color, glass amount, window style and theme, saved once in
//! `%APPDATA%\shellman\appearance.json`. Files edits it; the bars follow it live.

use crate::fonts::{self, FontChoice};
use crate::theme::window_style::{self, ThemeChoice, WindowStyle};
use crate::theme::{self, AccentChoice};
use serde::{Deserialize, Serialize};

const NAME: &str = "appearance";

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub accent: AccentChoice,
    /// How much of the window's acrylic shows through, from 0 (solid) to 1 (nearly clear).
    /// Applies to the Invisible and Frosted windows alike; a solid window ignores it.
    pub glass: f32,
    /// How app windows are drawn: Invisible, Frosted or No transparency (`window_style`).
    pub window: WindowStyle,
    /// Light, dark, or Windows' mode.
    pub theme: ThemeChoice,
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
            window: WindowStyle::default(),
            theme: ThemeChoice::default(),
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
    /// save it, and return the look as saved. When the file cannot be read, the fold changes in
    /// `current` only and nothing is saved (the user's look is never replaced by defaults).
    pub fn toggle_rail_saved(app: RailApp, current: Appearance) -> Appearance {
        match file() {
            Some(path) => toggle_rail_at(&path, app, current),
            None => {
                let mut look = current;
                look.fold_rail(app, !current.rail_folded(app));
                look
            }
        }
    }

    /// The saved look; `fallback` (the look in use) when it cannot be read; or `fallback` when
    /// none is saved yet (then it is saved, so the other apps see the same).
    pub fn load_or(fallback: Appearance) -> Self {
        match file() {
            Some(path) => load_or_at(&path, fallback),
            None => fallback,
        }
    }

    pub fn save(&self) {
        shellman_core::config::save(NAME, self);
    }

    /// Make the theme use this look.
    pub fn apply(&self, ctx: &egui::Context) {
        window_style::set_window_style(self.window);
        window_style::set_theme_choice(self.theme);
        // The preview overrides (`SHELLMAN_THEME`) still win; see `env::preference`.
        let want = theme::env::preference();
        if ctx.options(|o| o.theme_preference) != want {
            ctx.set_theme(want);
        }
        theme::set_light(ctx.theme() == egui::Theme::Light);
        theme::set_accent(ctx, self.accent.resolve());
        theme::set_glass(ctx, self.glass);
        // Does nothing when the window already wears this font.
        fonts::apply(ctx, self.font);
        // A window's glass panel notices a new window style on the next frame.
        ctx.request_repaint();
    }
}

/// The saved font choice, read without writing anything (the default when nothing is saved).
pub fn saved_font() -> FontChoice {
    match file() {
        Some(path) if path.exists() => shellman_core::config::load::<Appearance>(NAME).font,
        _ => FontChoice::Default,
    }
}

/// [`Appearance::toggle_rail_saved`] on the file at `path`.
fn toggle_rail_at(path: &std::path::Path, app: RailApp, current: Appearance) -> Appearance {
    let folded = current.rail_folded(app);
    let read = if path.exists() { shellman_core::config::try_load_from::<Appearance>(path) } else { Ok(current) };
    match read {
        Ok(mut look) => {
            look.fold_rail(app, !folded);
            shellman_core::config::save_to(path, &look);
            look
        }
        Err(e) => {
            eprintln!("shellman: {}: {e}; the sidebar folds here only", path.display());
            let mut look = current;
            look.fold_rail(app, !folded);
            look
        }
    }
}

/// [`Appearance::load_or`] on the file at `path`.
fn load_or_at(path: &std::path::Path, fallback: Appearance) -> Appearance {
    if !path.exists() {
        shellman_core::config::save_to(path, &fallback);
        return fallback;
    }
    shellman_core::config::try_load_from(path).unwrap_or(fallback)
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

    /// Two apps saving at once could leave the look half written; reading that must not reset
    /// the accent, glass and font in the app that reads it, nor save the defaults over it.
    #[test]
    fn an_unreadable_look_is_kept_not_replaced_by_defaults() {
        let dir = std::env::temp_dir().join(format!("shellman-appearance-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("appearance.json");
        let mine = Appearance { glass: 0.2, rail_collapsed: true, ..Appearance::default() };
        let half = br#"{"accent":"System","glass":0.2,"font":"Def"#;
        std::fs::write(&path, half).unwrap();
        // Reading it keeps the look in use.
        assert_eq!(load_or_at(&path, mine), mine);
        // Folding a sidebar folds it here, and does not write the defaults over the file.
        let folded = toggle_rail_at(&path, RailApp::Files, mine);
        assert!(!folded.rail_folded(RailApp::Files));
        assert_eq!(folded.glass, 0.2);
        assert_eq!(std::fs::read(&path).unwrap(), half);
        // A whole file is read and saved as before.
        shellman_core::config::save_to(&path, &mine);
        let saved = toggle_rail_at(&path, RailApp::Tasks, mine);
        assert_eq!(shellman_core::config::load_from::<Appearance>(&path), saved);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn window_and_theme_default_and_round_trip() {
        let old: Appearance = serde_json::from_str(r#"{"glass":0.5}"#).unwrap();
        assert_eq!((old.window, old.theme), (WindowStyle::Invisible, ThemeChoice::Windows));
        let look = Appearance { window: WindowStyle::NoTransparency, theme: ThemeChoice::Light, ..Appearance::default() };
        let json = serde_json::to_string(&look).unwrap();
        assert!(json.contains(r#""window":"solid""#) && json.contains(r#""theme":"light""#), "{json}");
        assert_eq!(serde_json::from_str::<Appearance>(&json).unwrap(), look);
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
