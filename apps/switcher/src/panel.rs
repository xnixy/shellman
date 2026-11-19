//! Drawing the panel: the dock's glass, one tile per window with its title and icon under the
//! preview, and the selection as Glasshouse marks a chosen tile (a deeper field fill inside a
//! hairline). Live previews are drawn by the compositor on top; under them nothing is painted.

use crate::layout::{Layout, Rect};
use shellman_core::thumbs::Thumbs;
use shellman_core::winlist::AppWindow;
use shellman_ui::theme::{colors, glyph, icon_font, shape};
use shellman_ui::widgets::draw_text;

/// Windows 11's window corner radius, which the panel's glass is cut to: DWM rounds the frosted
/// window itself, so the panel follows it rather than Glasshouse's own radii.
const RADIUS: u8 = 8;
const TILE_RADIUS: f32 = shape::TILE;
/// The icon plate in the preview's place: concentric with the tile, `INSET` outside it.
const PLATE_RADIUS: f32 = TILE_RADIUS - crate::layout::INSET;
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
            // The switcher never takes the keyboard (Alt+Tab is read by its hook): the selection
            // is the tile's selected state.
            shellman_ui::theme::speak_selected(&resp, egui::WidgetType::SelectableLabel, i == view.sel, || {
                shellman_ui::widgets::window_name(&window.title)
            });
            let tile_radius = shape::corner(TILE_RADIUS);
            if i == view.sel {
                // A deeper field fill inside a hairline. High contrast has no fills: the window
                // colour, outlined 2 px in highlight (`contrast_outline`).
                ui.painter().rect(outer, tile_radius, c.control_hover, egui::Stroke::new(1.0_f32, c.hairline), egui::StrokeKind::Inside);
                shellman_ui::theme::contrast_outline(ui, outer, TILE_RADIUS, false, true);
            } else if resp.hovered() {
                ui.painter().rect_filled(outer, tile_radius, c.row_hover);
                shellman_ui::theme::contrast_outline(ui, outer, TILE_RADIUS, true, false);
            }
            let icon = window.icon_source().and_then(|p| shellman_ui::app_icon::get(ui.ctx(), &p, true, icons));
            let preview = at(tile.preview);
            if !view.live.get(i).copied().unwrap_or(false) {
                // Minimized, or not previewable: the app's icon on a plate.
                ui.painter().rect_filled(preview, shape::corner(PLATE_RADIUS), c.field);
                if c.high_contrast {
                    ui.painter().rect_stroke(preview, PLATE_RADIUS, egui::Stroke::new(1.0_f32, c.muted), egui::StrokeKind::Inside);
                }
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
