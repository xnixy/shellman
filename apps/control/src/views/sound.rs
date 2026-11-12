//! Sound: the output volume (its glyph mutes), the outputs with the default one checked (a
//! click makes another the default), and the microphone's mute.

use super::kit::{self, chip, clipped_text, label_font, small_font, CHIP, GAP};
use egui::{Align2, CornerRadius, Pos2, Rect, Sense, Ui, Vec2};
use shellman_core::system::audio::Sound;
use shellman_core::system::audio_watch::SoundWatch;
use shellman_ui::theme::{colors, glyph, icon_font, with_alpha};

const ROW: f32 = 28.0;

/// The last known sound state, kept while the panel is hidden, and a drag in progress.
#[derive(Default)]
pub struct SoundView {
    pub sound: Sound,
    /// The level being dragged to; readings do not move the slider meanwhile.
    dragging: Option<f32>,
}

impl SoundView {
    pub fn take(&mut self, watch: &SoundWatch) {
        if let Some(sound) = watch.take() {
            self.sound = sound;
        }
    }

    pub fn show(&mut self, ui: &mut Ui, watch: Option<&SoundWatch>) {
        let Some(volume) = self.sound.volume else {
            return;
        };
        let c = colors();
        kit::tile(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(GAP, 8.0);
            ui.label(egui::RichText::new("Sound").font(label_font()).color(c.text));
            let shown = if volume.muted { 0.0 } else { self.dragging.unwrap_or(volume.level) };
            let g = if volume.muted { glyph::MUTED } else { glyph::VOLUME };
            let out = kit::slider(ui, egui::Id::new("volume"), shown, g);
            if let Some(level) = out.value {
                if self.dragging != Some(level) {
                    self.dragging = Some(level);
                    if let Some(w) = watch {
                        if volume.muted {
                            w.set_muted(false);
                        }
                        w.set_volume(level);
                    }
                }
            }
            if !out.dragging {
                if let Some(level) = self.dragging.take() {
                    // Shown until the next reading, which follows the write.
                    self.sound.volume = Some(shellman_core::system::audio::Volume { level, muted: false });
                }
            }
            if out.glyph_clicked {
                if let Some(w) = watch {
                    w.set_muted(!volume.muted);
                }
                if let Some(v) = self.sound.volume.as_mut() {
                    v.muted = !v.muted;
                }
            }
            if self.sound.outputs.len() > 1 {
                ui.add_space(2.0);
                self.outputs(ui, watch);
            }
            if let Some(muted) = self.sound.mic_muted {
                ui.add_space(2.0);
                self.microphone(ui, muted, watch);
            }
        });
    }

    /// One row per output; the default one carries a check.
    fn outputs(&mut self, ui: &mut Ui, watch: Option<&SoundWatch>) {
        let c = colors();
        let mut chosen = None;
        for o in &self.sound.outputs {
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::click());
            let is_default = self.sound.default_output.as_deref() == Some(o.id.as_str());
            if resp.hovered() {
                ui.painter().rect_filled(rect.expand2(Vec2::new(4.0, 0.0)), CornerRadius::same(8), with_alpha(c.text, 18));
            }
            if is_default {
                let at = Pos2::new(rect.left() + 8.0, rect.center().y);
                ui.painter().text(at, Align2::CENTER_CENTER, glyph::CHECK, icon_font(12.0), c.text);
            }
            let color = if is_default { c.text } else { c.muted };
            let left = rect.left() + 24.0;
            clipped_text(ui, Pos2::new(left, rect.center().y), Align2::LEFT_CENTER, &o.name, label_font(), color, rect.right() - left);
            if resp.clicked() && !is_default {
                chosen = Some(o.id.clone());
            }
        }
        if let Some(id) = chosen {
            self.sound.default_output = Some(id.clone());
            if let Some(w) = watch {
                w.set_default_output(id);
            }
        }
    }

    /// The microphone: a chip that mutes it, and its state.
    fn microphone(&mut self, ui: &mut Ui, muted: bool, watch: Option<&SoundWatch>) {
        let c = colors();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), CHIP), Sense::hover());
        let center = Pos2::new(rect.left() + CHIP / 2.0, rect.center().y);
        let resp = ui.interact(Rect::from_center_size(center, Vec2::splat(CHIP)), egui::Id::new("mic"), Sense::click());
        let g = if muted { glyph::MICROPHONE_OFF } else { glyph::MICROPHONE };
        chip(ui, center, g, !muted, resp.hovered());
        let left = rect.left() + CHIP + 10.0;
        let width = rect.right() - left;
        clipped_text(ui, Pos2::new(left, rect.center().y - 1.0), Align2::LEFT_BOTTOM, "Microphone", label_font(), c.text, width);
        let state = if muted { "Muted" } else { "On" };
        clipped_text(ui, Pos2::new(left, rect.center().y + 1.0), Align2::LEFT_TOP, state, small_font(), c.muted, width);
        if resp.clicked() {
            self.sound.mic_muted = Some(!muted);
            if let Some(w) = watch {
                w.set_mic_muted(!muted);
            }
        }
    }
}
