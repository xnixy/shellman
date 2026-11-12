//! The sidebar: places and drives.

use super::actions::Action;
use super::tab::Loc;
use egui::Ui;
use shellman_ui::nav::{nav_row, section_label, sidebar_title};
use shellman_ui::theme::glyph;
use std::path::PathBuf;

/// The places shown in the sidebar and on This PC.
pub(super) fn quick_places() -> Vec<(char, &'static str, PathBuf)> {
    [
        (glyph::HOME, "Home", dirs::home_dir()),
        (glyph::DESKTOP, "Desktop", dirs::desktop_dir()),
        (glyph::DOWNLOADS, "Downloads", dirs::download_dir()),
        (glyph::DOCUMENTS, "Documents", dirs::document_dir()),
        (glyph::PICTURES, "Pictures", dirs::picture_dir()),
        (glyph::MUSIC, "Music", dirs::audio_dir()),
        (glyph::VIDEOS, "Videos", dirs::video_dir()),
    ]
    .into_iter()
    .filter_map(|(g, n, p)| p.map(|p| (g, n, p)))
    .collect()
}

impl super::FileMan {
    pub(super) fn sidebar(&mut self, ui: &mut Ui, caption_h: f32, actions: &mut Vec<Action>) {
        let current = self.tabs[self.active].loc.clone();
        // The top of the sidebar is title bar too, with the app's name in it.
        sidebar_title(ui, caption_h, "Files");
        ui.add_space(2.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        section_label(ui, "Places");
        for (g, name, path) in quick_places() {
            let loc = Loc::Dir(path);
            let resp = nav_row(ui, g, name, current == loc);
            if resp.clicked() {
                actions.push(Action::Navigate(loc.clone()));
            }
            if resp.middle_clicked() {
                actions.push(Action::NewTab(loc));
            }
        }
        let bin = nav_row(ui, glyph::DELETE, "Recycle Bin", current == Loc::Bin);
        if bin.clicked() {
            actions.push(Action::Navigate(Loc::Bin));
        }
        if bin.middle_clicked() {
            actions.push(Action::NewTab(Loc::Bin));
        }
        section_label(ui, "Drives");
        if nav_row(ui, glyph::PC, "This PC", current == Loc::Pc).clicked() {
            actions.push(Action::Navigate(Loc::Pc));
        }
        for d in &self.drives {
            let loc = Loc::Dir(d.root.clone());
            let resp = nav_row(ui, glyph::DRIVE, &d.title(), current == loc);
            if resp.clicked() {
                actions.push(Action::Navigate(loc.clone()));
            }
            if resp.middle_clicked() {
                actions.push(Action::NewTab(loc));
            }
        }
    }
}
