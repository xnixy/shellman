//! The volume mixer: one row per app that has played sound on the default output, with its icon,
//! its name, a volume and a mute. Sessions come and go as apps start and stop playing.

use super::sound_rows::{level_slider, mute_button, ROW_H};
use crate::page;
use egui::{Align, CornerRadius, FontId, Id, Layout, Sense, UiBuilder, Vec2};
use shellman_core::system::sound_page::SoundPage;
use shellman_core::system::sound_sessions::AppSession;
use shellman_core::thumbs::Thumbs;
use shellman_ui::theme::{self, colors, glyph, icon_font};
use shellman_ui::widgets::draw_text;

const ICON: f32 = 26.0;
const NAME_W: f32 = 150.0;

pub fn show(ui: &mut egui::Ui, handle: &SoundPage, sessions: &[AppSession], thumbs: &mut Thumbs) {
    page::label(ui, "Volume mixer");
    if sessions.is_empty() {
        ui.label(theme::muted("Apps appear here once they play sound on the default output."));
        return;
    }
    ui.spacing_mut().item_spacing.y = 4.0;
    for s in sessions {
        row(ui, handle, s, thumbs);
    }
}

fn row(ui: &mut egui::Ui, handle: &SoundPage, s: &AppSession, thumbs: &mut Thumbs) {
    let c = colors();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(10), c.control);

    let icon = egui::Rect::from_center_size(egui::pos2(rect.left() + 14.0 + ICON / 2.0, rect.center().y), Vec2::splat(ICON));
    let image = s.exe.as_ref().and_then(|p| shellman_ui::app_icon::get(ui.ctx(), p, true, thumbs));
    match image {
        Some((id, size)) => {
            let scale = (ICON / size.x).min(ICON / size.y);
            egui::Image::from_texture((id, size)).paint_at(ui, egui::Rect::from_center_size(icon.center(), size * scale));
        }
        None => {
            let g = if s.system { glyph::VOLUME } else { glyph::APP };
            ui.painter().text(icon.center(), egui::Align2::CENTER_CENTER, g, icon_font(18.0), c.muted);
        }
    }

    let name = egui::Rect::from_min_max(
        egui::pos2(icon.right() + 10.0, rect.top()),
        egui::pos2(icon.right() + 10.0 + NAME_W, rect.bottom()),
    );
    // An app that is not playing right now is dimmer than one that is.
    let color = if s.active { c.text } else { c.muted };
    draw_text(ui, name, &s.name, FontId::proportional(14.0), color, false);

    let controls = egui::Rect::from_min_max(egui::pos2(name.right() + 4.0, rect.top()), egui::pos2(rect.right() - 8.0, rect.bottom()));
    let mut child = ui.new_child(UiBuilder::new().max_rect(controls).layout(Layout::left_to_right(Align::Center)));
    child.spacing_mut().item_spacing.x = 6.0;
    if mute_button(&mut child, s.muted).clicked() {
        handle.set_session_muted(&s.key, !s.muted);
    }
    if let Some(v) = level_slider(&mut child, Id::new(("session", &s.key)), s.volume, s.muted, 16.0) {
        handle.set_session_volume(&s.key, v);
    }
}
