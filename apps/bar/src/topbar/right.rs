//! The top bar's right island, from its right end inward: the status icons (which open the
//! control centre), then the tray.

use super::{highlight, TopBar};
use crate::tray::{self, view::{self, Place}};
use shellman_core::settings::bar::{BarSettings, TrayMonitors};
use shellman_core::system::status;
use shellman_ui::theme::{colors, icon_font};

const VOLUME: char = '\u{E767}';
const MUTED: char = '\u{E74F}';
const GLOBE: char = '\u{E774}';
const LOCAL_NET: char = '\u{E839}';
const BATTERY: char = '\u{E83F}';

/// From the island's ends to the highlights inside them, and between the status and the tray.
const END: f32 = 6.0;
const GROUP_GAP: f32 = 12.0;
/// Status icons: size, space between them, and the padding inside the hover highlight.
const ICON: f32 = 16.0;
const ICON_GAP: f32 = 12.0;
const PAD: f32 = 10.0;
const STATUS_H: f32 = 30.0;

fn text_width(ui: &egui::Ui, text: &str, font: egui::FontId) -> f32 {
    ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE)).size().x
}

impl TopBar {
    /// Status and tray in `row`, the island ending at `right`. Returns the island.
    pub(super) fn right_side(&mut self, ui: &mut egui::Ui, row: egui::Rect, right: f32, place: &Place) -> egui::Rect {
        let c = colors();
        let settings = *self.settings.get_or_insert_with(BarSettings::load);
        let mid = row.center().y;

        // Status: network, volume, battery, with one soft highlight around the group. Only what
        // the watchers already sent: nothing is read here.
        if let Some(feed) = &mut self.status {
            feed.take();
        }
        let st = self.status.as_ref().map(|f| f.now.clone()).unwrap_or_default();
        let mut items: Vec<(char, Option<String>)> = Vec::new();
        items.push(match st.network_or_offline() {
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
        let small = egui::FontId::proportional(13.0);
        let widths: Vec<f32> = items
            .iter()
            .map(|(_, t)| ICON + t.as_ref().map_or(0.0, |t| 5.0 + text_width(ui, t, small.clone())))
            .collect();
        // The bell at the island's right end, the status group before it.
        let (bell, asked) = super::bell::draw(ui, mid, right - END, self.badge);
        self.bell_rect = Some(bell);
        if asked.is_some() {
            self.bell_asked = asked;
        }
        let status_right = bell.left() - 2.0;
        let inner = widths.iter().sum::<f32>() + ICON_GAP * (items.len() - 1) as f32;
        let group = egui::Rect::from_min_max(
            egui::pos2(status_right - inner - 2.0 * PAD, mid - STATUS_H / 2.0),
            egui::pos2(status_right, mid + STATUS_H / 2.0),
        );
        let resp = ui.interact(group, egui::Id::new("status"), egui::Sense::click());
        highlight(ui, group, &resp, (STATUS_H / 2.0) as u8);
        let mut x = group.left() + PAD;
        for ((glyph, text), w) in items.iter().zip(&widths) {
            ui.painter().text(egui::pos2(x + ICON / 2.0, mid), egui::Align2::CENTER_CENTER, *glyph, icon_font(ICON), c.text);
            if let Some(t) = text {
                ui.painter().text(egui::pos2(x + ICON + 5.0, mid), egui::Align2::LEFT_CENTER, t, small.clone(), c.muted);
            }
            x += w + ICON_GAP;
        }
        // Click opens the control centre (its event toggles the resident one); scroll
        // changes the volume.
        if resp.clicked() {
            shellman_core::summon::control();
        }
        if let Some((level, _)) = vol {
            let resp = resp.on_hover_text(format!("Volume {:.0}%", level * 100.0));
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if resp.hovered() && scroll != 0.0 {
                // Through the sound worker's kept endpoint, off this thread.
                if let Some(feed) = &mut self.status {
                    feed.scroll(scroll);
                }
            }
        }

        // Tray, on every monitor's bar (or the primary's), divided from the status by a faint line.
        let mut begin = group.left();
        if place.primary || settings.tray == TrayMonitors::All {
            let icons = match &self.tray {
                Some(tray) => tray.icons(),
                None => self.demo.clone(),
            };
            if !icons.is_empty() {
                let sep_x = group.left() - GROUP_GAP / 2.0;
                ui.painter().vline(sep_x, egui::Rangef::new(mid - 7.0, mid + 7.0), egui::Stroke::new(1.0_f32, c.separator));
                begin = view::draw(ui, &icons, &mut self.tray_textures, row, group.left() - GROUP_GAP, place);
            }
        } else {
            // Not drawn here any more: apps must not be sent to this bar's old places.
            tray::forget_device(place.device);
        }
        egui::Rect::from_x_y_ranges(begin - END..=right, row.y_range())
    }
}
