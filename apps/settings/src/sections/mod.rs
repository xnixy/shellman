//! The pages of the window, one file each.

mod about;
mod apps;
mod appearance;
mod bar;
mod display;
mod display_map;
mod icon_pack;
mod dock;
mod fonts;
mod fonts_list;
mod fonts_load;
mod fonts_logic;
mod fonts_preview;
mod fonts_tabs;
mod fonts_view;
mod fonts_worker;
mod launcher;
mod datetime;
mod power;
mod mouse;
mod slider_row;
mod sound;
mod sound_mixer;
mod sound_rows;
mod startup;

pub use apps::AppsView;
pub use display::DisplayView;
pub use fonts_view::FontsView;
pub use icon_pack::IconsView;
pub use mouse::MouseView;
pub use datetime::DateTimeView;
pub use power::PowerView;
pub use sound::SoundView;
pub use startup::StartupView;
use crate::state::State;
use shellman_ui::theme::glyph;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Appearance,
    Sound,
    Display,
    Mouse,
    Power,
    DateTime,
    Apps,
    Startup,
    Fonts,
    Dock,
    TopBar,
    Launcher,
    About,
}

impl Section {
    pub const ALL: [Section; 13] = [
        Section::Appearance,
        Section::Sound,
        Section::Display,
        Section::Mouse,
        Section::Power,
        Section::DateTime,
        Section::Apps,
        Section::Startup,
        Section::Fonts,
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
            Section::Mouse => "Mouse & keyboard",
            Section::Power => "Power",
            Section::DateTime => "Date, time & region",
            Section::Apps => "Apps",
            Section::Startup => "Startup apps",
            Section::Fonts => "Fonts",
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
            // Segoe Fluent Icons' Mouse glyph.
            Section::Mouse => '\u{E962}',
            Section::Power => glyph::BATTERY,
            Section::DateTime => '\u{E823}',
            Section::Apps => '\u{E71D}',
            Section::Startup => '\u{E7E8}',
            // Segoe Fluent Icons' Font glyph.
            Section::Fonts => '\u{E8D2}',
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
            Section::Mouse => mouse::show(ui, state),
            Section::Power => power::show(ui, state),
            Section::DateTime => datetime::show(ui, state),
            Section::Apps => apps::show(ui, state),
            Section::Startup => startup::show(ui, state),
            Section::Fonts => fonts::show(ui, state),
            Section::Dock => dock::show(ui, state),
            Section::TopBar => bar::show(ui, state),
            Section::Launcher => launcher::show(ui, state),
            Section::About => about::show(ui, state),
        }
    }
}

impl Section {
    /// Whether the page lays itself out in `page::group`s. Any other page is put on one island
    /// by the window, so a new page needs no groups to look right.
    pub fn grouped(self) -> bool {
        matches!(
            self,
            Section::Appearance
                | Section::Sound
                | Section::Display
                | Section::Mouse
                | Section::Power
                | Section::DateTime
                | Section::Apps
                | Section::Startup
                | Section::Fonts
                | Section::Dock
                | Section::TopBar
                | Section::Launcher
                | Section::About
        )
    }

    /// The name a `--page` argument and the single-instance events use.
    pub fn slug(self) -> &'static str {
        match self {
            Section::Appearance => "appearance",
            Section::Sound => "sound",
            Section::Display => "display",
            Section::Mouse => "mouse",
            Section::Power => "power",
            Section::DateTime => "datetime",
            Section::Apps => "apps",
            Section::Startup => "startup",
            Section::Fonts => "fonts",
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
