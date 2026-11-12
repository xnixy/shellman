//! The rail: a narrow pill down the left with the places as icons (their names show on hover),
//! then the pinned folders, the apps' places, and This PC with the drives. The current place is
//! lit with the accent. Its empty part moves the window, and its menu brings hidden places back.

use super::actions::Action;
use super::tab::Loc;
use egui::{Align2, CornerRadius, Id, Painter, Rect, ScrollArea, Sense, Ui, UiBuilder, Vec2};
use shellman_ui::chips::soft_colors;
use shellman_ui::theme::{self, colors, glyph, icon_font};
use shellman_ui::widgets::window_drag;
use std::path::PathBuf;

/// Above the first icon and below the last.
const PAD: f32 = 8.0;
/// One icon's square, and the room it takes in the rail.
const ICON: f32 = 40.0;
const STEP: f32 = 44.0;
/// A short line between groups.
const SEPARATOR: f32 = 13.0;

/// The places shown on the rail and on This PC.
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

/// How tall the rail wants to be for this many icons in each group (empty groups are left out,
/// with their separator).
pub(super) fn wanted_height(groups: &[usize]) -> f32 {
    let shown: Vec<usize> = groups.iter().copied().filter(|&n| n > 0).collect();
    let icons: usize = shown.iter().sum();
    PAD * 2.0 + icons as f32 * STEP + shown.len().saturating_sub(1) as f32 * SEPARATOR
}

/// One icon of the rail, taking the next step down: lit when `active`, faded when `dim`. The
/// caller draws the icon itself into the square it is given, in the colour it is given.
pub(super) fn item(
    ui: &mut Ui,
    salt: impl std::hash::Hash,
    active: bool,
    dim: bool,
    icon: impl FnOnce(&Painter, Rect, egui::Color32),
) -> egui::Response {
    let c = colors();
    let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), STEP), Sense::hover());
    let square = Rect::from_center_size(row.center(), Vec2::splat(ICON));
    let resp = ui.interact(square, Id::new(("rail", salt)), Sense::click());
    let p = ui.painter();
    let r = CornerRadius::same(14);
    let fg = if active {
        p.rect_filled(square, r, c.accent_fill);
        c.on_accent
    } else {
        if resp.hovered() {
            p.rect_filled(square, r, soft_colors(c.light).1);
        }
        c.text
    };
    let fg = if dim { theme::with_alpha(fg, 90) } else { fg };
    icon(p, square, fg);
    resp
}

/// An item with a glyph for its icon.
fn glyph_item(ui: &mut Ui, g: char, name: &str, active: bool) -> egui::Response {
    item(ui, name, active, false, |p, square, fg| {
        p.text(
            square.center(),
            Align2::CENTER_CENTER,
            g,
            icon_font(15.0),
            fg,
        );
    })
    .on_hover_text(name)
}

fn separator(ui: &mut Ui) {
    let c = colors();
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), SEPARATOR), Sense::hover());
    let line = Rect::from_center_size(r.center(), Vec2::new(20.0, 1.0));
    ui.painter()
        .rect_filled(line, 0, theme::with_alpha(c.text, 36));
}

impl super::FileMan {
    /// Draw the rail at the top of `area`; returns the pill it is on.
    pub(super) fn rail(&mut self, ui: &mut Ui, area: Rect, actions: &mut Vec<Action>) -> Rect {
        let places = quick_places();
        let groups = [
            places.len() + usize::from(self.pick.is_none()),
            self.settings.pins.len(),
            self.shown_apps().len(),
            1 + self.drives.len(),
        ];
        let pill = super::plan::rail(area, wanted_height(&groups));
        // Its own empty space: moves the window, and its menu brings hidden places back.
        let bg = ui.interact(pill, Id::new("rail-bg"), Sense::click_and_drag());
        window_drag(ui, &bg);
        self.rail_background_menu(&bg, actions);

        let current = self.tabs[self.active].loc.clone();
        let inner = pill.shrink2(Vec2::new(0.0, PAD));
        let mut child = ui.new_child(UiBuilder::new().id_salt("rail").max_rect(inner));
        child.set_clip_rect(inner);
        ScrollArea::vertical()
            .id_salt("rail-scroll")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .auto_shrink([false, false])
            .show(&mut child, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                let mut first = true;
                let mut group = |ui: &mut Ui, n: usize| {
                    if n > 0 && !std::mem::take(&mut first) {
                        separator(ui);
                    }
                };
                group(ui, groups[0]);
                for (g, name, path) in places {
                    let loc = Loc::Dir(path);
                    let resp = glyph_item(ui, g, name, current == loc);
                    if resp.clicked() {
                        actions.push(Action::Navigate(loc.clone()));
                    }
                    if resp.middle_clicked() {
                        actions.push(Action::NewTab(loc));
                    }
                }
                // A picker has no use for the bin.
                if !self.pick_hides(&Loc::Bin) {
                    let bin = glyph_item(ui, glyph::DELETE, "Recycle Bin", current == Loc::Bin);
                    if bin.clicked() {
                        actions.push(Action::Navigate(Loc::Bin));
                    }
                    if bin.middle_clicked() {
                        actions.push(Action::NewTab(Loc::Bin));
                    }
                }
                group(ui, groups[1]);
                self.rail_pins(ui, &current, actions);
                group(ui, groups[2]);
                self.rail_apps(ui, &current, actions);
                group(ui, groups[3]);
                if glyph_item(ui, glyph::PC, "This PC", current == Loc::Pc).clicked() {
                    actions.push(Action::Navigate(Loc::Pc));
                }
                for d in &self.drives {
                    let loc = Loc::Dir(d.root.clone());
                    let resp = glyph_item(ui, glyph::DRIVE, &d.title(), current == loc);
                    if resp.clicked() {
                        actions.push(Action::Navigate(loc.clone()));
                    }
                    if resp.middle_clicked() {
                        actions.push(Action::NewTab(loc));
                    }
                }
            });
        pill
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn height_counts_icons_and_the_separators_between_groups() {
        assert_eq!(wanted_height(&[8]), PAD * 2.0 + 8.0 * STEP);
        assert_eq!(
            wanted_height(&[8, 0, 0, 3]),
            PAD * 2.0 + 11.0 * STEP + SEPARATOR
        );
        assert_eq!(
            wanted_height(&[8, 2, 1, 3]),
            PAD * 2.0 + 14.0 * STEP + 3.0 * SEPARATOR
        );
    }
}
