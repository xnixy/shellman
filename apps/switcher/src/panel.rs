//! Drawing the panel: the dock's glass, one tile per window with its title and icon under the
//! preview, and the selection in the accent color. Live previews are drawn by the compositor on
//! top; under them nothing is painted.

use crate::layout::{Layout, Rect};
use shellman_core::thumbs::Thumbs;
use shellman_core::winlist::AppWindow;
use shellman_ui::theme::{colors, glyph, icon_font};
use shellman_ui::widgets::draw_text;

/// Windows 11's window corner radius, which the panel's glass is cut to.
const RADIUS: u8 = 8;
const TILE_RADIUS: u8 = 6;
const BIG_ICON: f32 = 48.0;
const SMALL_ICON: f32 = 18.0;

pub struct View<'a> {
    pub windows: &'a [AppWindow],
    pub layout: &'a Layout,
    pub sel: usize,
    /// Which tiles have a live preview over them.
    pub live: &'a [bool],
    pub glass: bool,
    /// Pixels per point of the panel's monitor, which the layout and previews were made for.
    pub scale: f32,
}

/// Draw the panel; returns the tile clicked, if any.
pub fn draw(ctx: &egui::Context, view: &View, icons: &mut Thumbs) -> Option<usize> {
    // The layout is in the monitor's points; until the window library has caught up with a move
    // to a monitor of another scale, its points differ.
    let k = view.scale / ctx.pixels_per_point();
    if (k - 1.0).abs() > 0.001 {
        ctx.request_repaint();
    }
    let at = |r: Rect| egui::Rect::from_min_size(egui::pos2(r.x * k, r.y * k), egui::vec2(r.w * k, r.h * k));
    let mut clicked = None;
    egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |ui| {
        let c = colors();
        let full = ui.max_rect();
        let radius = egui::CornerRadius::same(RADIUS);
        if view.glass {
            ui.painter().rect_filled(full, radius, c.shell);
            ui.painter().rect_filled(full, radius, c.sheet);
        } else {
            ui.painter().rect_filled(full, radius, c.bg);
        }
        ui.painter().rect_stroke(full.shrink(0.5), radius, egui::Stroke::new(1.0_f32, c.hairline), egui::StrokeKind::Inside);
        for (i, (window, tile)) in view.windows.iter().zip(&view.layout.tiles).enumerate() {
            let outer = at(tile.outer);
            let resp = ui.interact(outer, egui::Id::new(("tile", window.hwnd)), egui::Sense::click());
            let tile_radius = egui::CornerRadius::same(TILE_RADIUS);
            if i == view.sel {
                ui.painter().rect_filled(outer, tile_radius, c.control_hover);
                ui.painter().rect_stroke(outer, tile_radius, egui::Stroke::new(2.0_f32, c.accent_fill), egui::StrokeKind::Inside);
            } else if resp.hovered() {
                ui.painter().rect_filled(outer, tile_radius, c.control);
            }
            let icon = window.icon_source().and_then(|p| shellman_ui::app_icon::get(ui.ctx(), &p, true, icons));
            let preview = at(tile.preview);
            if !view.live.get(i).copied().unwrap_or(false) {
                // Minimized, or not previewable: the app's icon on a plate.
                ui.painter().rect_filled(preview, tile_radius, c.control);
                paint_icon(ui, icon, preview.center(), BIG_ICON.min(preview.height() * 0.6));
            }
            let label = at(tile.label);
            let icon_center = egui::pos2(label.left() + SMALL_ICON / 2.0, label.center().y);
            paint_icon(ui, icon, icon_center, SMALL_ICON);
            let text = egui::Rect::from_min_max(egui::pos2(label.left() + SMALL_ICON + 6.0, label.top()), label.max);
            draw_text(ui, text, &window.title, egui::FontId::proportional(12.5), c.text, false);
            if resp.clicked() {
                clicked = Some(i);
            }
        }
    });
    clicked
}

/// An app icon of `size` points centred on `center`, or a generic glyph while it loads.
fn paint_icon(ui: &egui::Ui, icon: Option<(egui::TextureId, egui::Vec2)>, center: egui::Pos2, size: f32) {
    match icon {
        Some((id, px)) => {
            let scale = (size / px.x).min(size / px.y);
            egui::Image::from_texture((id, px)).paint_at(ui, egui::Rect::from_center_size(center, px * scale));
        }
        None => {
            let c = colors();
            ui.painter().text(center, egui::Align2::CENTER_CENTER, glyph::APP, icon_font(size * 0.6), c.muted);
        }
    }
}
