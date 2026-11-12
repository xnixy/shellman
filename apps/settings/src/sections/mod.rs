//! The pages of the window, one file each.

mod about;
mod appearance;
mod bar;
mod dock;
mod launcher;

use crate::state::State;
use shellman_ui::theme::glyph;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Appearance,
    Dock,
    TopBar,
    Launcher,
    About,
}

impl Section {
    pub const ALL: [Section; 5] = [
        Section::Appearance,
        Section::Dock,
        Section::TopBar,
        Section::Launcher,
        Section::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Section::Appearance => "Appearance",
            Section::Dock => "Dock",
            Section::TopBar => "Top bar",
            Section::Launcher => "Launcher",
            Section::About => "About",
        }
    }

    pub fn glyph(self) -> char {
        match self {
            Section::Appearance => '\u{E790}',
            Section::Dock => glyph::APP,
            Section::TopBar => glyph::DESKTOP,
            Section::Launcher => glyph::SEARCH,
            Section::About => glyph::INFO,
        }
    }

    pub fn show(self, ui: &mut egui::Ui, state: &mut State) {
        match self {
            Section::Appearance => appearance::show(ui, state),
            Section::Dock => dock::show(ui, state),
            Section::TopBar => bar::show(ui, state),
            Section::Launcher => launcher::show(ui, state),
            Section::About => about::show(ui),
        }
    }
}
