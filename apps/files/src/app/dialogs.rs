//! The modal dialogs: settings and confirm-recycle.

use super::actions::Dialog;
use egui::widgets::color_picker::{color_picker_color32, Alpha};
use egui::{Align, Align2, Color32, Id, Key, Layout, RichText, Sense, Stroke, Ui, Vec2};
use shellman_core::fsops::Op;
use shellman_ui::theme::{self, colors, glyph, icon_font, AccentChoice};
use shellman_ui::widgets::switch;

impl super::FileMan {
    /// How much of the acrylic shows through, applied as the slider moves. Without system
    /// backdrops (Windows 10) there is no glass to show, so the slider is shown but disabled.
    fn glass_slider(&mut self, ui: &mut Ui) {
        let c = colors();
        ui.label(RichText::new("Glass").size(12.0).color(c.muted));
        ui.horizontal(|ui| {
            ui.label(RichText::new("Solid").size(11.5).color(c.muted));
            ui.spacing_mut().slider_width = 240.0;
            let slider = egui::Slider::new(&mut self.settings.glass, 0.0..=1.0)
                .show_value(false)
                .trailing_fill(true);
            let resp = ui.add_enabled(self.glass, slider);
            ui.label(RichText::new("Clear").size(11.5).color(c.muted));
            if resp.changed() {
                theme::set_glass(ui.ctx(), self.settings.glass);
                self.save_pending = true;
            }
            // Shared once the slider is let go, not on every step of a drag.
            if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                self.share_look();
            }
            if !self.glass {
                resp.on_disabled_hover_text("Needs Windows 11");
            }
        });
    }

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
                                .size(16.0)
                                .strong()
                                .color(c.text),
                        );
                        ui.add_space(8.0);
                        ui.label(theme::muted("Accent color").size(12.0));
                        ui.add_space(2.0);
                        let current = self.settings.accent;
                        let picker_id = Id::new("custom-picker");
                        let mut open = ui.data(|d| d.get_temp::<bool>(picker_id)).unwrap_or(false);
                        let mut pick = None;
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                            let sys = theme::system_accent().unwrap_or(c.accent);
                            if swatch(
                                ui,
                                Swatch::System(sys),
                                current == AccentChoice::System,
                                "Windows accent",
                            )
                            .clicked()
                            {
                                pick = Some(AccentChoice::System);
                            }
                            ui.add_space(6.0);
                            for (i, (name, rgb)) in theme::PRESETS.iter().enumerate() {
                                let col = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                                if swatch(
                                    ui,
                                    Swatch::Solid(col),
                                    current == AccentChoice::Preset(i),
                                    name,
                                )
                                .clicked()
                                {
                                    pick = Some(AccentChoice::Preset(i));
                                }
                            }
                            // The eleventh swatch opens the picker, and shows the custom color once chosen.
                            let kind = match current {
                                AccentChoice::Custom(v) => {
                                    Swatch::Solid(Color32::from_rgb(v[0], v[1], v[2]))
                                }
                                _ => Swatch::Add,
                            };
                            let custom = matches!(current, AccentChoice::Custom(_));
                            if swatch(ui, kind, custom, "Custom color").clicked() {
                                open = !open;
                            }
                        });
                        ui.data_mut(|d| d.insert_temp(picker_id, open));
                        ui.add_space(2.0);
                        let label = match current {
                            AccentChoice::System => "Following Windows",
                            AccentChoice::Preset(i) => {
                                theme::PRESETS[i.min(theme::PRESETS.len() - 1)].0
                            }
                            AccentChoice::Custom(_) => "Custom",
                        };
                        ui.label(theme::muted(label).size(12.0));
                        if open {
                            ui.add_space(6.0);
                            // Live: every change restyles the app as a custom accent.
                            let mut col = c.accent;
                            if color_picker_color32(ui, &mut col, Alpha::Opaque) {
                                pick = Some(AccentChoice::Custom([col.r(), col.g(), col.b()]));
                            }
                        }
                        if let Some(p) = pick {
                            self.set_accent(p);
                        }
                        ui.add_space(14.0);
                        self.glass_slider(ui);
                        ui.add_space(14.0);
                        if switch(ui, &mut self.settings.show_hidden, "Show hidden files").changed()
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

/// What a swatch is filled with.
enum Swatch {
    Solid(Color32),
    /// Half the system accent, half neutral: it stands for "whatever Windows uses".
    System(Color32),
    /// The custom-color button, before a custom color exists.
    Add,
}

fn swatch(ui: &mut Ui, kind: Swatch, selected: bool, tip: &str) -> egui::Response {
    let c = colors();
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::click());
    let center = rect.center();
    let radius = if resp.hovered() { 13.5 } else { 12.5 };
    let p = ui.painter();
    let under = match kind {
        Swatch::Solid(col) => {
            p.circle_filled(center, radius, col);
            col
        }
        Swatch::System(col) => {
            p.add(half_disc(center, radius, true, col));
            p.add(half_disc(
                center,
                radius,
                false,
                theme::with_alpha(c.text, 40),
            ));
            col
        }
        Swatch::Add => {
            p.circle_filled(center, radius, c.control);
            p.text(
                center,
                Align2::CENTER_CENTER,
                glyph::ADD,
                icon_font(12.0),
                c.text,
            );
            c.control
        }
    };
    if selected {
        p.circle_stroke(center, 15.0, Stroke::new(2.0_f32, c.text));
        let on = theme::Colors::new(c.light, under).on_accent;
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

/// One half of a disc as a convex polygon, so two of them make a split swatch.
fn half_disc(center: egui::Pos2, radius: f32, left: bool, fill: Color32) -> egui::Shape {
    use std::f32::consts::PI;
    let start = if left { PI / 2.0 } else { -PI / 2.0 };
    let points = (0..=16)
        .map(|i| {
            let a = start + PI * i as f32 / 16.0;
            center + Vec2::new(a.cos(), a.sin()) * radius
        })
        .collect();
    egui::Shape::convex_polygon(points, fill, Stroke::NONE)
}
