//! Painting the preview panel: on the dock's glass, one tile per window with its title under the
//! picture, and a close button on the tile the pointer is over. The live pictures are drawn by the
//! compositor on top of the window, so the close button sits in the title row, beside them.

use super::layout::Layout;
use egui::{Context, TextureId, Vec2};
use shellman_core::winlist::AppWindow;
use shellman_ui::theme::{colors, glyph, icon_font, mix, with_alpha};
use shellman_ui::widgets::draw_text;
use std::path::Path;

/// Concentric with the panel's corner, a tile's padding in.
const TILE_RADIUS: u8 = 10;
const BIG_ICON: f32 = 48.0;
const SMALL_ICON: f32 = 18.0;

/// Loads an app's icon: None until it has arrived.
pub type Icons<'a> = &'a mut dyn FnMut(&Context, &Path) -> Option<(TextureId, Vec2)>;

pub struct View<'a> {
    pub windows: &'a [AppWindow],
    pub layout: &'a Layout,
    /// Which tiles have a live picture over them; the others show the app's icon on a plate.
    pub live: &'a [bool],
    /// Glass is composed under the window; if not, the panel paints its own surface.
    pub composed: bool,
    /// The layout's points to this window's points: the monitor's scale over the scale the window
    /// is drawn at, which differ for a frame or two after a move to a monitor of another scale.
    pub k: f32,
}

/// What was clicked.
pub enum Action {
    /// Bring this window forward.
    Show(isize),
    Close(isize),
}

pub fn draw(ctx: &Context, view: &View, icons: Icons) -> Option<Action> {
    let at = |r: egui::Rect| egui::Rect::from_min_max(r.min * view.k, r.max * view.k);
    let mut action = None;
    egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |ui| {
        let c = colors();
        let full = ui.max_rect();
        if !view.composed {
            let f = shellman_ui::glass::content_frame(false, &super::style());
            ui.painter().rect(full, f.corner_radius, f.fill, f.stroke, egui::StrokeKind::Inside);
        }
        let tile_radius = egui::CornerRadius::same(TILE_RADIUS);
        // Nearly opaque shades of the glass's own tint: Windows blends the window over its glass
        // in linear light, where a faint translucent white comes out far brighter than drawn.
        let base = super::style().tint.to_opaque();
        let shade = |t: f32, a: u8| with_alpha(mix(base, c.text, t), a);
        let (hover, plate, pressable) = (shade(0.10, 200), shade(0.07, 220), shade(0.2, 230));
        for (i, (window, tile)) in view.windows.iter().zip(&view.layout.tiles).enumerate() {
            let outer = at(tile.outer);
            let over = ui.rect_contains_pointer(outer);
            let resp = ui.interact(outer, egui::Id::new(("preview tile", window.hwnd)), egui::Sense::click());
            if over {
                ui.painter().rect_filled(outer, tile_radius, hover);
            }
            let icon = window.icon_source().and_then(|p| icons(ui.ctx(), &p));
            let picture = at(tile.preview);
            if !view.live.get(i).copied().unwrap_or(false) {
                // Minimized, or one the compositor won't show: the app's icon on a plate.
                ui.painter().rect_filled(picture, tile_radius, plate);
                paint_icon(ui, icon, picture.center(), BIG_ICON.min(picture.height() * 0.6));
            }
            let label = at(tile.label);
            paint_icon(ui, icon, egui::pos2(label.left() + SMALL_ICON / 2.0, label.center().y), SMALL_ICON);
            let close = at(tile.close);
            let text = egui::Rect::from_min_max(
                egui::pos2(label.left() + SMALL_ICON + 6.0, label.top()),
                egui::pos2(close.left() - 4.0, label.bottom()),
            );
            draw_text(ui, text, &window.title, egui::FontId::proportional(12.5), c.text, false);
            // Added after the tile, so it takes the clicks over its own area.
            let mut close_clicked = false;
            if over {
                let button = ui.interact(close, egui::Id::new(("preview close", window.hwnd)), egui::Sense::click());
                if button.hovered() {
                    ui.painter().circle_filled(close.center(), close.width() / 2.0, pressable);
                }
                ui.painter().text(
                    close.center(),
                    egui::Align2::CENTER_CENTER,
                    glyph::CLOSE,
                    icon_font(10.0),
                    if button.hovered() { c.text } else { c.muted },
                );
                close_clicked = button.clicked();
            }
            if close_clicked || resp.middle_clicked() {
                action = Some(Action::Close(window.hwnd));
            } else if resp.clicked() {
                action = Some(Action::Show(window.hwnd));
            }
        }
    });
    action
}

/// An app icon of `size` points centred on `center`, or a generic glyph while it loads.
fn paint_icon(ui: &egui::Ui, icon: Option<(TextureId, Vec2)>, center: egui::Pos2, size: f32) {
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
