//! Drawing the tray icons in the top bar, right to left from where the status icons begin.

use super::{click::anchor_of, Click, TrayIcon};
use crate::topbar::highlight;
use std::collections::HashMap;

/// Icons sit on this pitch; each has a rounded highlight of the same size on hover.
const PITCH: f32 = 26.0;
const ICON: f32 = 16.0;
const RADIUS: u8 = 6;

/// Textures for the icons, re-uploaded only when an icon's picture changes.
#[derive(Default)]
pub struct TrayTextures {
    by_icon: HashMap<(isize, u32), (u64, egui::TextureHandle)>,
}

/// The top bar window drawing the tray: its monitor, and its client origin in physical pixels
/// (None before the window exists, when nothing can be reported yet).
pub struct Place<'a> {
    pub device: &'a str,
    pub primary: bool,
    pub origin: Option<(i32, i32)>,
}

/// Draw `icons` ending at `right`; returns where they begin.
pub fn draw(ui: &mut egui::Ui, icons: &[TrayIcon], tex: &mut TrayTextures, bar: egui::Rect, right: f32, at: &Place) -> f32 {
    let mut x = right;
    tex.by_icon
        .retain(|k, _| icons.iter().any(|i| (i.hwnd, i.uid) == *k));
    for icon in icons.iter().rev() {
        x -= PITCH;
        let cell = egui::Rect::from_center_size(egui::pos2(x + PITCH / 2.0, bar.center().y), egui::Vec2::splat(PITCH));
        let resp = ui.interact(cell, egui::Id::new(("tray", icon.hwnd, icon.uid)), egui::Sense::click());
        highlight(ui, cell, &resp, RADIUS);
        let rect = at.origin.map(|o| phys_rect(o, cell, ui.ctx().pixels_per_point()));
        if let Some(rect) = rect {
            super::set_rect(icon.hwnd, icon.uid, at.device, at.primary, rect);
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
        let click = if resp.double_clicked() {
            Some(Click::Double)
        } else if resp.clicked() {
            Some(Click::Left)
        } else if resp.secondary_clicked() {
            Some(Click::Right)
        } else {
            None
        };
        if resp.hovered() || click.is_some() {
            super::touch(icon.hwnd, icon.uid, at.device);
        }
        if let Some(click) = click {
            icon.click(click, rect.map(anchor_of));
        }
    }
    x
}

/// `cell` (points, in the window) as left, top, right, bottom in physical screen pixels, given
/// the window's client origin in physical pixels and its own scale. Each monitor's window has its
/// own scale (150%, 300%), so this is never done with another window's.
fn phys_rect(origin: (i32, i32), cell: egui::Rect, ppp: f32) -> [i32; 4] {
    let px = |v: f32| (v * ppp).round() as i32;
    [origin.0 + px(cell.min.x), origin.1 + px(cell.min.y), origin.0 + px(cell.max.x), origin.1 + px(cell.max.y)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rects_are_physical_per_window_scale() {
        let cell = |x: f32| egui::Rect::from_min_size(egui::pos2(x, 3.0), egui::Vec2::splat(26.0));
        // 4K at 150%, primary: a 2560 pt wide window at 0.
        assert_eq!(phys_rect((0, 0), cell(2000.0), 1.5), [3000, 5, 3039, 44]);
        // 4K at 150%, left of the primary.
        assert_eq!(phys_rect((-3840, 0), cell(2000.0), 1.5), [-840, 5, -801, 44]);
        // 4K at 300%, below and right: 26 pt is 78 px.
        assert_eq!(phys_rect((3746, 2160), cell(1000.0), 3.0), [6746, 2169, 6824, 2247]);
    }
}
