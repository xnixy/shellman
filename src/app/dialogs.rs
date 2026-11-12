//! The modal dialogs: settings and confirm-recycle.

use super::actions::Dialog;
use crate::fsops::Op;
use crate::theme::{self, colors, glyph, icon_font, AccentChoice};
use egui::{Align, Align2, Color32, Id, Key, Layout, RichText, Sense, Stroke, Ui, Vec2};

impl super::FileMan {
    pub(super) fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        let c = colors();
        let mut keep = true;
        match &dialog {
            Dialog::ConfirmRecycle(paths) => {
                let paths = paths.clone();
                let modal = egui::Modal::new(Id::new("recycle"))
                    .frame(theme::dialog())
                    .show(ctx, |ui| {
                        ui.set_width(380.0);
                        let title = if paths.len() == 1 {
                            format!(
                                "Move \"{}\" to the Recycle Bin?",
                                paths[0].file_name().unwrap_or_default().to_string_lossy()
                            )
                        } else {
                            format!("Move {} items to the Recycle Bin?", paths.len())
                        };
                        ui.label(RichText::new(title).size(16.0).strong().color(c.text));
                        ui.label(theme::muted("You can restore them from the Recycle Bin."));
                        ui.add_space(12.0);
                        let mut result = None;
                        ui.horizontal(|ui| {
                            if ui
                                .add(theme::danger_button("Move to Recycle Bin"))
                                .clicked()
                                || ui.input(|i| i.key_pressed(Key::Enter))
                            {
                                result = Some(true);
                            }
                            if ui.button("Cancel").clicked() {
                                result = Some(false);
                            }
                        });
                        result
                    });
                let answer = modal.inner.or(modal.should_close().then_some(false));
                if let Some(yes) = answer {
                    keep = false;
                    if yes {
                        self.run_op(Op::Recycle, "Delete", paths, None);
                    }
                }
            }
            Dialog::Settings => {
                let modal = egui::Modal::new(Id::new("settings"))
                    .frame(theme::dialog())
                    .show(ctx, |ui| {
                        ui.set_width(420.0);
                        ui.label(
                            RichText::new("Appearance")
                                .size(18.0)
                                .strong()
                                .color(c.text),
                        );
                        ui.add_space(8.0);
                        ui.label(RichText::new("Accent color").color(c.text));
                        ui.add_space(2.0);
                        let current = self.settings.accent;
                        let mut pick = None;
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                            let sys = theme::system_accent().unwrap_or(c.accent);
                            if swatch(ui, sys, current == AccentChoice::System, "Windows accent")
                                .clicked()
                            {
                                pick = Some(AccentChoice::System);
                            }
                            ui.add_space(6.0);
                            for (i, (name, rgb)) in theme::PRESETS.iter().enumerate() {
                                let col = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                                if swatch(ui, col, current == AccentChoice::Preset(i), name)
                                    .clicked()
                                {
                                    pick = Some(AccentChoice::Preset(i));
                                }
                            }
                        });
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let mut rgb = match current {
                                AccentChoice::Custom(v) => v,
                                _ => [c.accent.r(), c.accent.g(), c.accent.b()],
                            };
                            if ui.color_edit_button_srgb(&mut rgb).changed() {
                                pick = Some(AccentChoice::Custom(rgb));
                            }
                            let label = match current {
                                AccentChoice::System => "Following Windows",
                                AccentChoice::Preset(i) => {
                                    theme::PRESETS[i.min(theme::PRESETS.len() - 1)].0
                                }
                                AccentChoice::Custom(_) => "Custom",
                            };
                            ui.label(theme::muted(format!(
                                "{label}  ·  click the box for any color"
                            )));
                        });
                        if let Some(p) = pick {
                            self.set_accent(p);
                        }
                        ui.add_space(14.0);
                        if ui
                            .checkbox(&mut self.settings.show_hidden, "Show hidden files")
                            .changed()
                        {
                            self.save_pending = true;
                            for t in &mut self.tabs {
                                t.dirty = true;
                            }
                        }
                        ui.add_space(14.0);
                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                            ui.add(theme::primary_button("Done")).clicked()
                        })
                        .inner
                    });
                if modal.inner || modal.should_close() {
                    keep = false;
                }
            }
        }
        if keep {
            self.dialog = Some(dialog);
        }
    }
}

fn swatch(ui: &mut Ui, col: Color32, selected: bool, tip: &str) -> egui::Response {
    let c = colors();
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::click());
    let center = rect.center();
    let p = ui.painter();
    p.circle_filled(center, if resp.hovered() { 13.5 } else { 12.5 }, col);
    if selected {
        p.circle_stroke(center, 15.0, Stroke::new(2.0_f32, c.text));
        let on = theme::Colors::new(c.light, col).on_accent;
        p.text(
            center,
            Align2::CENTER_CENTER,
            glyph::CHECK,
            icon_font(11.0),
            on,
        );
    }
    resp.on_hover_text(tip)
}
