//! The pages of the window, one file each.

mod about;
mod accessibility;
mod apps;
mod appearance;
mod bar;
mod desktop;
mod desktop_widgets;
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
mod default_apps;
mod storage;
mod storage_rows;
mod datetime;
mod power;
mod privacy;
mod live;
mod winlink;
mod mouse;
mod slider_row;
mod sound;
mod sound_mixer;
mod sound_rows;
mod startup;
mod terminal;
mod updates;
mod updates_rows;

pub use accessibility::AccessView;
pub use apps::AppsView;
pub use desktop_widgets::WidgetsView;
pub use display::DisplayView;
pub use fonts_view::FontsView;
pub use icon_pack::IconsView;
pub use mouse::MouseView;
pub use default_apps::DefaultAppsView;
pub use storage::StorageView;
pub use datetime::DateTimeView;
pub use power::PowerView;
pub use privacy::PrivacyView;
pub use sound::SoundView;
pub use startup::StartupView;
pub use terminal::TerminalView;
pub use updates::UpdatesView;
use crate::state::State;
use shellman_ui::theme::glyph;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Appearance,
    Desktop,
    Sound,
    Display,
    Mouse,
    Power,
    DateTime,
    Storage,
    DefaultApps,
    Accessibility,
    Privacy,
    Updates,
    Apps,
    Startup,
    Fonts,
    Dock,
    TopBar,
    Launcher,
    Terminal,
    About,
}

impl Section {
    pub const ALL: [Section; 20] = [
        Section::Appearance,
        Section::Desktop,
        Section::Sound,
        Section::Display,
        Section::Mouse,
        Section::Power,
        Section::DateTime,
        Section::Storage,
        Section::DefaultApps,
        Section::Accessibility,
        Section::Privacy,
        Section::Updates,
        Section::Apps,
        Section::Startup,
        Section::Fonts,
        Section::Dock,
        Section::TopBar,
        Section::Launcher,
        Section::Terminal,
        Section::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Section::Appearance => "Appearance",
            Section::Desktop => "Desktop",
            Section::Sound => "Sound",
            Section::Display => "Display",
            Section::Mouse => "Mouse & keyboard",
            Section::Power => "Power",
            Section::DateTime => "Date, time & region",
            Section::Storage => "Storage",
            Section::DefaultApps => "Default apps",
            Section::Accessibility => "Accessibility",
            Section::Privacy => "Privacy & security",
            Section::Updates => "Windows Update",
            Section::Apps => "Apps",
            Section::Startup => "Startup apps",
            Section::Fonts => "Fonts",
            Section::Dock => "Dock",
            Section::TopBar => "Top bar",
            Section::Launcher => "Launcher",
            Section::Terminal => "Terminal",
            Section::About => "About",
        }
    }

    pub fn glyph(self) -> char {
        match self {
            Section::Appearance => '\u{E790}',
            // Segoe Fluent Icons' Photo: the wallpaper.
            Section::Desktop => glyph::PICTURES,
            Section::Sound => glyph::VOLUME,
            Section::Display => glyph::DESKTOP,
            // Segoe Fluent Icons' Mouse glyph.
            Section::Mouse => '\u{E962}',
            Section::Power => glyph::BATTERY,
            Section::DateTime => '\u{E823}',
            Section::Storage => glyph::DRIVE,
            Section::DefaultApps => '\u{E8E5}',
            // Segoe Fluent Icons' EaseOfAccess and Lock, as Windows' own Settings shows them.
            Section::Accessibility => '\u{E776}',
            Section::Privacy => '\u{E72E}',
            // Segoe Fluent Icons' Sync glyph, as Windows' own Settings shows for Windows Update.
            Section::Updates => '\u{E895}',
            Section::Apps => '\u{E71D}',
            Section::Startup => '\u{E7E8}',
            // Segoe Fluent Icons' Font glyph.
            Section::Fonts => '\u{E8D2}',
            Section::Dock => glyph::APP,
            Section::TopBar => glyph::DESKTOP,
            Section::Launcher => glyph::SEARCH,
            // Segoe Fluent Icons' command prompt, as the terminal's own tabs show.
            Section::Terminal => '\u{E756}',
            Section::About => glyph::INFO,
        }
    }

    pub fn show(self, ui: &mut egui::Ui, state: &mut State) {
        match self {
            Section::Appearance => appearance::show(ui, state),
            Section::Desktop => desktop::show(ui, state),
            Section::Sound => sound::show(ui, state),
            Section::Display => display::show(ui, state),
            Section::Mouse => mouse::show(ui, state),
            Section::Power => power::show(ui, state),
            Section::DateTime => datetime::show(ui, state),
            Section::Storage => storage::show(ui, state),
            Section::DefaultApps => default_apps::show(ui, state),
            Section::Accessibility => accessibility::show(ui, state),
            Section::Privacy => privacy::show(ui, state),
            Section::Updates => updates::show(ui, state),
            Section::Apps => apps::show(ui, state),
            Section::Startup => startup::show(ui, state),
            Section::Fonts => fonts::show(ui, state),
            Section::Dock => dock::show(ui, state),
            Section::TopBar => bar::show(ui, state),
            Section::Launcher => launcher::show(ui, state),
            Section::Terminal => terminal::show(ui, state),
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
                | Section::Desktop
                | Section::Sound
                | Section::Display
                | Section::Mouse
                | Section::Power
                | Section::DateTime
                | Section::Storage
                | Section::DefaultApps
                | Section::Accessibility
                | Section::Privacy
                | Section::Updates
                | Section::Apps
                | Section::Startup
                | Section::Fonts
                | Section::Dock
                | Section::TopBar
                | Section::Launcher
                | Section::Terminal
                | Section::About
        )
    }

    /// The name a `--page` argument and the single-instance events use.
    pub fn slug(self) -> &'static str {
        match self {
            Section::Appearance => "appearance",
            Section::Desktop => "desktop",
            Section::Sound => "sound",
            Section::Display => "display",
            Section::Mouse => "mouse",
            Section::Power => "power",
            Section::DateTime => "datetime",
            Section::Storage => "storage",
            Section::DefaultApps => "default-apps",
            Section::Accessibility => "accessibility",
            Section::Privacy => "privacy",
            Section::Updates => "windows-update",
            Section::Apps => "apps",
            Section::Startup => "startup",
            Section::Fonts => "fonts",
            Section::Dock => "dock",
            Section::TopBar => "top-bar",
            Section::Launcher => "launcher",
            Section::Terminal => "terminal",
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
