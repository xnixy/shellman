//! The top bar's right side, from the edge inward: the clock, the status icons, the tray.

use super::{highlight, TopBar};
use crate::tray::view;
use shellman_core::system::{clock, status};
use shellman_ui::theme::{colors, icon_font, semibold_font};

const VOLUME: char = '\u{E767}';
const MUTED: char = '\u{E74F}';
const GLOBE: char = '\u{E774}';
const LOCAL_NET: char = '\u{E839}';
const BATTERY: char = '\u{E83F}';

/// From the right edge to the clock, and between the three groups.
const EDGE: f32 = 14.0;
const GROUP_GAP: f32 = 14.0;
/// Status icons: size, space between them, and the padding inside the hover highlight.
const ICON: f32 = 15.0;
const ICON_GAP: f32 = 12.0;
const PAD: f32 = 6.0;
const STATUS_H: f32 = 24.0;

fn text_width(ui: &egui::Ui, text: &str, font: egui::FontId) -> f32 {
    ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE)).size().x
}

impl TopBar {
    /// Clock, status and tray, from the right edge. Returns where they begin.
    pub(super) fn right_side(&mut self, ui: &mut egui::Ui, bar: egui::Rect, with_tray: bool) -> f32 {
        let c = colors();
        let settings = *self.settings.get_or_insert_with(BarSettings::load);
        let mid = bar.center().y;

        // Clock: the time in the text colour, the date muted beside it. Segoe UI's digits are
        // all one width, so the clock does not jitter as it ticks.
        let (date, time) = clock::now_text(settings.clock.hour_cycle.pattern());
        let (time_font, date_font) = (semibold_font(13.0), egui::FontId::proportional(13.0));
        let time_w = text_width(ui, &time, time_font.clone());
        let mut left = bar.right() - EDGE - time_w;
        ui.painter().text(egui::pos2(bar.right() - EDGE, mid), egui::Align2::RIGHT_CENTER, &time, time_font, c.text);
        if settings.clock.show_date {
            let x = left - 8.0;
            ui.painter().text(egui::pos2(x, mid), egui::Align2::RIGHT_CENTER, &date, date_font.clone(), c.muted);
            left = x - text_width(ui, &date, date_font);
        }

        // Status: network, volume, battery, with one soft highlight around the group.
        let st = self.status();
        let mut items: Vec<(char, Option<String>)> = Vec::new();
        items.push(match st.network {
            status::Network::Internet => (GLOBE, None),
            status::Network::Local => (LOCAL_NET, None),
            status::Network::Offline => (GLOBE, Some("offline".into())),
        });
        let vol = st.volume.as_ref().map(|v| (v.level, v.muted));
        if let Some((_, muted)) = vol {
            items.push((if muted { MUTED } else { VOLUME }, None));
        }
        if let Some(b) = &st.battery {
            items.push((BATTERY, Some(format!("{}%", b.percent))));
        }
        let small = egui::FontId::proportional(12.0);
        let widths: Vec<f32> = items
            .iter()
            .map(|(_, t)| ICON + t.as_ref().map_or(0.0, |t| 4.0 + text_width(ui, t, small.clone())))
            .collect();
        let inner = widths.iter().sum::<f32>() + ICON_GAP * (items.len() - 1) as f32;
        let group = egui::Rect::from_center_size(
            egui::pos2(left - GROUP_GAP - (inner + 2.0 * PAD) / 2.0, mid),
            egui::vec2(inner + 2.0 * PAD, STATUS_H),
        );
        let resp = ui.interact(group, egui::Id::new("status"), egui::Sense::click());
        highlight(ui, group, &resp, 8);
        let mut x = group.left() + PAD;
        for ((glyph, text), w) in items.iter().zip(&widths) {
            ui.painter().text(egui::pos2(x + ICON / 2.0, mid), egui::Align2::CENTER_CENTER, *glyph, icon_font(ICON), c.text);
            if let Some(t) = text {
                ui.painter().text(egui::pos2(x + ICON + 4.0, mid), egui::Align2::LEFT_CENTER, t, small.clone(), c.muted);
            }
            x += w + ICON_GAP;
        }
        // Click opens the control centre (running its exe toggles the resident one); scroll
        // changes the volume.
        if resp.clicked() {
            shellman_core::sibling::start("control");
        }
        if let Some((level, _)) = vol {
            let resp = resp.on_hover_text(format!("Volume {:.0}%", level * 100.0));
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if resp.hovered() && scroll != 0.0 {
                status::set_volume(level + scroll.signum() * 0.02);
                self.status = None;
            }
        }

        // Tray, on the primary monitor's bar only, divided from the status by a faint line.
        let mut begin = group.left();
        if with_tray {
            let icons = match &self.tray {
                Some(tray) => tray.icons(),
                None => self.demo.clone(),
            };
            if !icons.is_empty() {
                let sep_x = group.left() - GROUP_GAP / 2.0;
                ui.painter().vline(sep_x, egui::Rangef::new(mid - 6.0, mid + 6.0), egui::Stroke::new(1.0_f32, c.separator));
                begin = view::draw(ui, &icons, &mut self.tray_textures, bar, group.left() - GROUP_GAP);
            }
        }
        begin
    }
}

use shellman_core::settings::bar::BarSettings;
