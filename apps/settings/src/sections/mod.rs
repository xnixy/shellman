//! The pages of the window, one file each.

mod about;
mod appearance;
mod bar;
mod display;
mod display_map;
mod dock;
mod launcher;
mod sound;
mod sound_mixer;
mod sound_rows;

pub use display::DisplayView;
pub use sound::SoundView;
use crate::state::State;
use shellman_ui::theme::glyph;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Appearance,
    Sound,
    Display,
    Dock,
    TopBar,
    Launcher,
    About,
}

impl Section {
    pub const ALL: [Section; 7] = [
        Section::Appearance,
        Section::Sound,
        Section::Display,
        Section::Dock,
        Section::TopBar,
        Section::Launcher,
        Section::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Section::Appearance => "Appearance",
            Section::Sound => "Sound",
            Section::Display => "Display",
            Section::Dock => "Dock",
            Section::TopBar => "Top bar",
            Section::Launcher => "Launcher",
            Section::About => "About",
        }
    }

    pub fn glyph(self) -> char {
        match self {
            Section::Appearance => '\u{E790}',
            Section::Sound => glyph::VOLUME,
            Section::Display => glyph::DESKTOP,
            Section::Dock => glyph::APP,
            Section::TopBar => glyph::DESKTOP,
            Section::Launcher => glyph::SEARCH,
            Section::About => glyph::INFO,
        }
    }

    pub fn show(self, ui: &mut egui::Ui, state: &mut State) {
        match self {
            Section::Appearance => appearance::show(ui, state),
            Section::Sound => sound::show(ui, state),
            Section::Display => display::show(ui, state),
            Section::Dock => dock::show(ui, state),
            Section::TopBar => bar::show(ui, state),
            Section::Launcher => launcher::show(ui, state),
            Section::About => about::show(ui, state),
        }
    }
}

impl Section {
    /// The name a `--page` argument and the single-instance events use.
    pub fn slug(self) -> &'static str {
        match self {
            Section::Appearance => "appearance",
            Section::Sound => "sound",
            Section::Display => "display",
            Section::Dock => "dock",
            Section::TopBar => "top-bar",
            Section::Launcher => "launcher",
            Section::About => "about",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Section> {
        Section::ALL.into_iter().find(|s| s.slug() == slug)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_is_found_by_its_slug() {
        for s in Section::ALL {
            assert_eq!(Section::from_slug(s.slug()), Some(s));
        }
        assert_eq!(Section::from_slug("nope"), None);
    }
}
