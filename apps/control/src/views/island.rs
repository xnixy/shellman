//! One island of the panel: a block of controls on its own piece of glass. The block is laid out
//! with padding, and the rectangle it took is recorded, so the glass under it (set by the shell
//! from the same list, in the same frame) is exactly as big as what it holds.

use super::kit::GAP;
use egui::{Margin, Ui, Vec2};
use shellman_ui::glass::Island;

/// Corner radius of an island, in points.
pub const RADIUS: f32 = shellman_ui::theme::shape::ISLAND;
/// The space between stacked islands, in points.
pub const SPACING: f32 = 12.0;

/// Lay `add` out in a full-width island with `pad` points of padding, and record its rectangle in
/// `islands` (clipped to what is in view, when the panel scrolls).
pub fn show<R>(ui: &mut Ui, islands: &mut Vec<Island>, pad: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let out = egui::Frame::new().inner_margin(Margin::same(pad as i8)).show(ui, |ui| {
        ui.spacing_mut().item_spacing = Vec2::new(GAP, GAP);
        ui.set_width(ui.available_width());
        add(ui)
    });
    // Scrolled partly out of view, only what shows is glass.
    let visible = out.response.rect.intersect(ui.clip_rect());
    if visible.width() > 1.0 && visible.height() > 1.0 {
        islands.push(Island::new(visible, RADIUS));
    }
    out.inner
}
