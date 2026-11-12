//! Drawing the tray icons in the top bar, right to left from where the status pill begins.

use super::{Click, TrayIcon};
use shellman_ui::theme::colors;
use std::collections::HashMap;

const CELL: f32 = 28.0;
const ICON: f32 = 16.0;

/// Textures for the icons, re-uploaded only when an icon's picture changes.
#[derive(Default)]
pub struct TrayTextures {
    by_icon: HashMap<(isize, u32), (u64, egui::TextureHandle)>,
}

/// Draw `icons` ending at `right`; returns where they begin.
pub fn draw(ui: &mut egui::Ui, icons: &[TrayIcon], tex: &mut TrayTextures, bar: egui::Rect, right: f32) -> f32 {
    let c = colors();
    let mut x = right;
    tex.by_icon
        .retain(|k, _| icons.iter().any(|i| (i.hwnd, i.uid) == *k));
    for icon in icons.iter().rev() {
        x -= CELL;
        let cell = egui::Rect::from_min_max(egui::pos2(x, bar.top() + 4.0), egui::pos2(x + CELL, bar.bottom() - 4.0));
        let resp = ui.interact(cell, egui::Id::new(("tray", icon.hwnd, icon.uid)), egui::Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(cell, egui::CornerRadius::same(8), c.control_hover);
        }
        if let Some(image) = &icon.image {
            let key = (icon.hwnd, icon.uid);
            let stale = tex.by_icon.get(&key).is_none_or(|(rev, _)| *rev != icon.rev);
            if stale {
                let t = ui.ctx().load_texture(format!("tray:{key:?}"), (**image).clone(), egui::TextureOptions::LINEAR);
                tex.by_icon.insert(key, (icon.rev, t));
            }
            let t = &tex.by_icon[&key].1;
            egui::Image::from_texture((t.id(), egui::Vec2::splat(ICON)))
                .paint_at(ui, egui::Rect::from_center_size(cell.center(), egui::Vec2::splat(ICON)));
        }
        let resp = if icon.tip.is_empty() { resp } else { resp.on_hover_text(&icon.tip) };
        if resp.double_clicked() {
            icon.click(Click::Double);
        } else if resp.clicked() {
            icon.click(Click::Left);
        } else if resp.secondary_clicked() {
            icon.click(Click::Right);
        }
    }
    x
}
