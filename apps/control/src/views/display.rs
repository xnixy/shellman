//! Display: a brightness slider for each screen that answers, named when there are several.

use super::kit::{self, label_font, small_font, GAP};
use egui::{Ui, Vec2};
use shellman_core::system::brightness::{BrightnessWatch, Screen};
use shellman_ui::theme::{colors, glyph};

/// The last known screens, kept while the panel is hidden, and the one being dragged.
#[derive(Default)]
pub struct DisplayView {
    pub screens: Vec<Screen>,
    dragging: Option<(String, u8)>,
}

impl DisplayView {
    pub fn take(&mut self, watch: &BrightnessWatch) {
        if let Some(screens) = watch.take() {
            self.screens = screens;
        }
    }

    pub fn show(&mut self, ui: &mut Ui, watch: Option<&BrightnessWatch>) {
        if self.screens.is_empty() {
            return;
        }
        let c = colors();
        let names = labels(&self.screens);
        kit::tile(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(GAP, 8.0);
            ui.label(egui::RichText::new("Display").font(label_font()).color(c.text));
            let mut still_dragging = false;
            for (screen, name) in self.screens.iter_mut().zip(&names) {
                if let Some(name) = name {
                    ui.add_space(-2.0);
                    ui.label(egui::RichText::new(name).font(small_font()).color(c.muted));
                }
                let dragged = self.dragging.as_ref().filter(|(id, _)| *id == screen.id).map(|(_, p)| *p);
                let shown = dragged.unwrap_or(screen.percent) as f32 / 100.0;
                let out = kit::slider(ui, egui::Id::new(("brightness", &screen.id)), shown, glyph::BRIGHTNESS);
                if let Some(v) = out.value {
                    let percent = (v * 100.0).round() as u8;
                    still_dragging = true;
                    if dragged != Some(percent) {
                        self.dragging = Some((screen.id.clone(), percent));
                        if let Some(w) = watch {
                            w.set(&screen.id, percent);
                        }
                    }
                } else if let Some(percent) = dragged {
                    screen.percent = percent;
                }
            }
            if !still_dragging {
                self.dragging = None;
            }
        });
    }
}

/// Each screen's label: none when there is only one; its name otherwise, with its display
/// number when two share a name (two of the same model).
pub fn labels(screens: &[Screen]) -> Vec<Option<String>> {
    if screens.len() < 2 {
        return vec![None; screens.len()];
    }
    screens
        .iter()
        .map(|s| {
            let twins = screens.iter().filter(|o| o.name == s.name).count() > 1;
            let number = s.id.trim_start_matches(|c: char| !c.is_ascii_digit());
            Some(if twins && !number.is_empty() { format!("{} · {number}", s.name) } else { s.name.clone() })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(id: &str, name: &str) -> Screen {
        Screen { id: id.into(), name: name.into(), percent: 50 }
    }

    #[test]
    fn one_screen_needs_no_name() {
        assert_eq!(labels(&[screen(r"\\.\DISPLAY1", "Dell")]), vec![None]);
    }

    #[test]
    fn twins_get_their_display_numbers() {
        let s = [screen(r"\\.\DISPLAY1", "XG32"), screen(r"\\.\DISPLAY2", "XG32"), screen("builtin", "Built-in display")];
        assert_eq!(
            labels(&s),
            vec![Some("XG32 · 1".into()), Some("XG32 · 2".into()), Some("Built-in display".into())]
        );
    }
}
