//! The This PC page.

use super::actions::Action;
use super::sidebar::quick_places;
use super::tab::Loc;
use super::widgets::hover_outline;
use crate::fsops;
use crate::theme::{self, colors, glyph, icon};
use egui::{CornerRadius, RichText, ScrollArea, Sense, Ui, Vec2};

impl super::FileMan {
    pub(super) fn pc_page(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let c = colors();
        ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(8.0);
            ui.label(RichText::new("Places").size(18.0).strong().color(c.text));
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(12.0, 12.0);
                for (g, name, path) in quick_places() {
                    let resp = theme::card(ui, |ui| {
                        ui.set_width(150.0);
                        ui.horizontal(|ui| {
                            ui.label(icon(g, 20.0).color(c.accent_text));
                            ui.label(RichText::new(name).size(14.5).color(c.text));
                        });
                    })
                    .response
                    .interact(Sense::click());
                    hover_outline(ui, &resp);
                    if resp.clicked() {
                        actions.push(Action::Navigate(Loc::Dir(path.clone())));
                    }
                    if resp.middle_clicked() {
                        actions.push(Action::NewTab(Loc::Dir(path)));
                    }
                }
            });
            ui.add_space(18.0);
            ui.label(RichText::new("Drives").size(18.0).strong().color(c.text));
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(12.0, 12.0);
                for d in &self.drives {
                    let resp = theme::card(ui, |ui| {
                        ui.set_width(250.0);
                        ui.horizontal(|ui| {
                            ui.label(icon(glyph::DRIVE, 26.0).color(c.accent_text));
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 4.0;
                                ui.label(RichText::new(d.title()).size(14.5).color(c.text));
                                if d.total > 0 {
                                    let used = 1.0 - d.free as f32 / d.total as f32;
                                    let (bar, _) = ui
                                        .allocate_exact_size(Vec2::new(190.0, 6.0), Sense::hover());
                                    let p = ui.painter();
                                    p.rect_filled(bar, CornerRadius::same(3), c.well);
                                    let mut fill = bar;
                                    fill.set_width(bar.width() * used.clamp(0.0, 1.0));
                                    let col = if used > 0.9 { c.danger } else { c.accent };
                                    p.rect_filled(fill, CornerRadius::same(3), col);
                                    ui.label(
                                        theme::muted(format!(
                                            "{} free of {}",
                                            fsops::format_size(d.free),
                                            fsops::format_size(d.total)
                                        ))
                                        .size(12.0),
                                    );
                                }
                            });
                        });
                    })
                    .response
                    .interact(Sense::click());
                    hover_outline(ui, &resp);
                    if resp.clicked() {
                        actions.push(Action::Navigate(Loc::Dir(d.root.clone())));
                    }
                    if resp.middle_clicked() {
                        actions.push(Action::NewTab(Loc::Dir(d.root.clone())));
                    }
                }
            });
        });
    }
}
