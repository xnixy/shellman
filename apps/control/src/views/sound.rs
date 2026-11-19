//! Sound: the output volume (its glyph mutes), the outputs with the default one checked (a
//! click makes another the default), and the microphone's mute.

use super::kit::{self, chip, clipped_text, label_font, small_font, CHIP, GAP};
use egui::{Align2, Pos2, Rect, Sense, Ui, Vec2};
use shellman_core::system::audio::Sound;
use shellman_core::system::audio_watch::SoundWatch;
use shellman_ui::theme::shape::{self, MENU_ROW};
use shellman_ui::theme::{colors, glyph, icon_font};

const ROW: f32 = 28.0;

/// The last known sound state, kept while the panel is hidden, and a drag in progress.
#[derive(Default)]
pub struct SoundView {
    pub sound: Sound,
    /// The level being dragged to; readings do not move the slider meanwhile.
    dragging: Option<f32>,
    /// Whether every output is listed, or only the default one.
    expanded: bool,
}

impl SoundView {
    /// Back to listing only the default output (on hiding).
    pub fn collapse(&mut self) {
        self.expanded = false;
    }

    pub fn take(&mut self, watch: &SoundWatch) {
        if let Some(sound) = watch.take() {
            self.sound = sound;
        }
    }

    /// True when the "Sound settings" link was clicked.
    pub fn show(&mut self, ui: &mut Ui, watch: Option<&SoundWatch>) -> bool {
        let Some(volume) = self.sound.volume else {
            return false;
        };
        let mut open_settings = false;
        let c = colors();
        kit::tile(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(GAP, 8.0);
            ui.label(egui::RichText::new("Sound").font(label_font()).color(c.text));
            let shown = if volume.muted { 0.0 } else { self.dragging.unwrap_or(volume.level) };
            let g = if volume.muted { glyph::MUTED } else { glyph::VOLUME };
            let out = kit::slider(ui, egui::Id::new("volume"), shown, g, "Volume", Some(if volume.muted { "Unmute" } else { "Mute" }));
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
            ui.add_space(2.0);
            open_settings = settings_link(ui, "Sound settings");
        });
        open_settings
    }

    /// The outputs. Folded, only the default one shows, with a chevron; unfolded, every one,
    /// the default checked. Choosing one makes it the default and folds the list again.
    fn outputs(&mut self, ui: &mut Ui, watch: Option<&SoundWatch>) {
        let c = colors();
        let default = self.sound.default_output.clone();
        let mut chosen = None;
        let mut toggle = false;
        // The default first: its row (with the chevron) folds and unfolds the list.
        let mut shown: Vec<_> = self
            .sound
            .outputs
            .iter()
            .filter(|o| self.expanded || default.as_deref() == Some(o.id.as_str()))
            .collect();
        shown.sort_by_key(|o| default.as_deref() != Some(o.id.as_str()));
        for (i, o) in shown.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::click());
            let is_default = default.as_deref() == Some(o.id.as_str());
            let expanded = self.expanded;
            shellman_ui::theme::speak_selected(&resp, egui::WidgetType::SelectableLabel, is_default, || {
                output_name(&o.name, i == 0, expanded)
            });
            let back = rect.expand2(Vec2::new(4.0, 0.0));
            shellman_ui::theme::focus_ring(ui, &resp, back, MENU_ROW);
            if resp.hovered() {
                ui.painter().rect_filled(back, shape::corner(MENU_ROW), kit::row_hover());
            }
            shellman_ui::theme::contrast_outline(ui, back, MENU_ROW, resp.hovered(), false);
            if is_default && self.expanded {
                let at = Pos2::new(rect.left() + 8.0, rect.center().y);
                ui.painter().text(at, Align2::CENTER_CENTER, glyph::CHECK, icon_font(12.0), c.text);
            }
            if i == 0 {
                let g = if self.expanded { glyph::SORT_UP } else { glyph::SORT_DOWN };
                let at = Pos2::new(rect.right() - 8.0, rect.center().y);
                ui.painter().text(at, Align2::CENTER_CENTER, g, icon_font(10.0), c.muted);
            }
            let color = if is_default { c.text } else { c.muted };
            let left = rect.left() + if self.expanded { 24.0 } else { 0.0 };
            clipped_text(ui, Pos2::new(left, rect.center().y), Align2::LEFT_CENTER, &o.name, label_font(), color, rect.right() - 24.0 - left);
            if resp.clicked() {
                if is_default {
                    toggle = true;
                } else {
                    chosen = Some(o.id.clone());
                }
            }
        }
        if toggle {
            self.expanded = !self.expanded;
        }
        if let Some(id) = chosen {
            self.sound.default_output = Some(id.clone());
            self.expanded = false;
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
        // A switch (a checkbox stands in): checked while the microphone is on.
        shellman_ui::theme::speak_selected(&resp, egui::WidgetType::Checkbox, !muted, || "Microphone".to_owned());
        shellman_ui::theme::focus_ring(ui, &resp, resp.rect, CHIP / 2.0);
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

/// What a screen reader reads for an output row: its name, and for the first row (which folds
/// and unfolds the list) what a click does.
pub fn output_name(name: &str, first: bool, expanded: bool) -> String {
    match (first, expanded) {
        (true, false) => format!("{name}, show other outputs"),
        (true, true) => format!("{name}, hide other outputs"),
        _ => name.to_owned(),
    }
}

/// A small link under a view's controls that opens our settings on that view's page.
pub(super) fn settings_link(ui: &mut Ui, text: &str) -> bool {
    let c = colors();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::click());
    shellman_ui::theme::speak(&resp, egui::WidgetType::Link, || text.to_owned());
    let back = rect.expand2(Vec2::new(4.0, 0.0));
    shellman_ui::theme::focus_ring(ui, &resp, back, MENU_ROW);
    if resp.hovered() {
        ui.painter().rect_filled(back, shape::corner(MENU_ROW), kit::row_hover());
    }
    shellman_ui::theme::contrast_outline(ui, back, MENU_ROW, resp.hovered(), false);
    let color = if resp.hovered() { c.text } else { c.muted };
    let at = Pos2::new(rect.right() - 8.0, rect.center().y);
    ui.painter().text(at, Align2::CENTER_CENTER, glyph::OPEN_EXTERNAL, icon_font(10.0), color);
    kit::clipped_text(ui, Pos2::new(rect.left(), rect.center().y), Align2::LEFT_CENTER, text, small_font(), color, rect.width() - 24.0);
    resp.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_output_says_it_folds_the_list() {
        assert_eq!(output_name("Speakers", true, false), "Speakers, show other outputs");
        assert_eq!(output_name("Speakers", true, true), "Speakers, hide other outputs");
        assert_eq!(output_name("Headphones", false, true), "Headphones");
    }
}
