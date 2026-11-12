//! The sidebar: places and settings.

use super::actions::Action;
use super::tab::Loc;
use egui::{Align, Align2, CornerRadius, FontId, Layout, Rect, RichText, Sense, Ui, Vec2};
use shellman_ui::theme::{self, colors, glyph, icon_font};
use shellman_ui::widgets::{draw_text, window_drag};
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
        let c = colors();
        let current = self.tabs[self.active].loc.clone();
        // The top of the sidebar is title bar too, with the app's name in it.
        let (bar, drag) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), caption_h),
            Sense::click_and_drag(),
        );
        window_drag(ui, &drag);
        ui.painter()
            .circle_filled(egui::pos2(bar.left() + 14.0, bar.center().y), 3.5, c.accent);
        ui.painter().text(
            egui::pos2(bar.left() + 26.0, bar.center().y),
            Align2::LEFT_CENTER,
            "Files",
            FontId::proportional(12.5),
            c.muted,
        );
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
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            if nav_row(ui, glyph::SETTINGS, "Appearance", false).clicked() {
                actions.push(Action::Settings);
            }
        });
    }
}

/// A place in the sidebar. The open one gets a soft lift and a small accent bar rather than a
/// loud accent fill, so the sidebar stays quiet next to the folder.
fn nav_row(ui: &mut Ui, g: char, label: &str, active: bool) -> egui::Response {
    let c = colors();
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
    let p = ui.painter();
    if active {
        p.rect_filled(rect, CornerRadius::same(8), theme::with_alpha(c.text, 18));
        let bar = Rect::from_center_size(
            egui::pos2(rect.left() + 4.0 + 1.5, rect.center().y),
            Vec2::new(3.0, 16.0),
        );
        p.rect_filled(bar, CornerRadius::same(2), c.accent);
    } else if resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(8), theme::with_alpha(c.text, 10));
    }
    p.text(
        egui::pos2(rect.left() + 12.0, rect.center().y),
        Align2::LEFT_CENTER,
        g,
        icon_font(14.0),
        if active { c.accent_text } else { c.muted },
    );
    let text_rect = Rect::from_min_max(egui::pos2(rect.left() + 36.0, rect.top()), rect.max);
    draw_text(
        ui,
        text_rect,
        label,
        FontId::proportional(13.5),
        c.text,
        false,
    );
    resp
}

fn section_label(ui: &mut Ui, text: &str) {
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(RichText::new(text).size(11.0).color(colors().muted));
    });
    ui.add_space(2.0);
}
