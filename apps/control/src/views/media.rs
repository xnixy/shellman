//! Now playing: the art, title and artist, and previous / play-pause / next. Hidden when no app
//! is playing anything.

use super::kit::{self, clipped_text, label_font, small_font};
use egui::{Align2, CornerRadius, Pos2, Rect, Sense, TextureHandle, Ui, Vec2};
use shellman_core::system::media::{Action, MediaWatch, NowPlaying};
use shellman_core::system::media_art::Art;
use shellman_ui::theme::{colors, glyph, icon_font, with_alpha};
use std::sync::Arc;

const ART: f32 = 48.0;

#[derive(Default)]
pub struct MediaView {
    pub now: Option<NowPlaying>,
    /// The art as a texture, with the pixels it was made from.
    texture: Option<(Arc<Art>, TextureHandle)>,
}

impl MediaView {
    pub fn take(&mut self, watch: &MediaWatch) {
        if let Some(now) = watch.take() {
            self.now = now;
        }
    }

    pub fn show(&mut self, ui: &mut Ui, watch: Option<&MediaWatch>) {
        let Some(now) = self.now.clone() else {
            self.texture = None;
            return;
        };
        let c = colors();
        let texture = self.texture_for(ui.ctx(), now.art.as_ref());
        kit::tile(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ART), Sense::hover());
            let art = Rect::from_min_size(rect.min, Vec2::splat(ART));
            match texture {
                Some(id) => {
                    egui::Image::new((id, art.size()))
                        .corner_radius(CornerRadius::same(10))
                        .paint_at(ui, art);
                }
                None => {
                    ui.painter().rect_filled(art, CornerRadius::same(10), kit::well(false));
                    ui.painter().text(art.center(), Align2::CENTER_CENTER, glyph::MUSIC, icon_font(20.0), c.muted);
                }
            }
            // Controls from the right edge: next, play-pause, previous.
            let mut x = rect.right();
            let mut asked = None;
            for (g, action, enabled) in [
                (glyph::NEXT, Action::Next, now.can_next),
                (if now.playing { glyph::PAUSE } else { glyph::PLAY }, Action::PlayPause, now.can_toggle),
                (glyph::PREVIOUS, Action::Previous, now.can_previous),
            ] {
                let size = 30.0;
                let r = Rect::from_center_size(Pos2::new(x - size / 2.0, rect.center().y), Vec2::splat(size));
                x -= size + 2.0;
                let resp = ui.interact(r, egui::Id::new(("media", g)), if enabled { Sense::click() } else { Sense::hover() });
                if enabled && resp.hovered() {
                    ui.painter().circle_filled(r.center(), size / 2.0, kit::well(false));
                }
                let fg = if enabled { c.text } else { with_alpha(c.muted, 110) };
                ui.painter().text(r.center(), Align2::CENTER_CENTER, g, icon_font(14.0), fg);
                if resp.clicked() {
                    asked = Some(action);
                }
            }
            let left = art.right() + 12.0;
            let width = x - 6.0 - left;
            let mid = rect.center().y;
            let title = if now.title.is_empty() { "Playing" } else { now.title.as_str() };
            clipped_text(ui, Pos2::new(left, mid - 1.0), Align2::LEFT_BOTTOM, title, label_font(), c.text, width);
            clipped_text(ui, Pos2::new(left, mid + 1.0), Align2::LEFT_TOP, &now.artist, small_font(), c.muted, width);
            if let (Some(action), Some(w)) = (asked, watch) {
                w.act(action);
                if action == Action::PlayPause {
                    if let Some(n) = self.now.as_mut() {
                        n.playing = !n.playing;
                    }
                }
            }
        });
    }

    /// The texture for `art`, made once per picture.
    fn texture_for(&mut self, ctx: &egui::Context, art: Option<&Arc<Art>>) -> Option<egui::TextureId> {
        let Some(art) = art else {
            self.texture = None;
            return None;
        };
        let fresh = self.texture.as_ref().is_none_or(|(had, _)| !Arc::ptr_eq(had, art));
        if fresh {
            let size = [art.side as usize, art.side as usize];
            let image = egui::ColorImage::from_rgba_unmultiplied(size, &art.rgba);
            let handle = ctx.load_texture("now-playing", image, egui::TextureOptions::LINEAR);
            self.texture = Some((art.clone(), handle));
        }
        self.texture.as_ref().map(|(_, t)| t.id())
    }
}
