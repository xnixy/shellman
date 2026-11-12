//! Drawing the tray icons in the top bar, right to left from where the status icons begin.

use super::{Click, TrayIcon};
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

/// Draw `icons` ending at `right`; returns where they begin.
pub fn draw(ui: &mut egui::Ui, icons: &[TrayIcon], tex: &mut TrayTextures, bar: egui::Rect, right: f32) -> f32 {
    let mut x = right;
    tex.by_icon
        .retain(|k, _| icons.iter().any(|i| (i.hwnd, i.uid) == *k));
    for icon in icons.iter().rev() {
        x -= PITCH;
        let cell = egui::Rect::from_center_size(egui::pos2(x + PITCH / 2.0, bar.center().y), egui::Vec2::splat(PITCH));
        let resp = ui.interact(cell, egui::Id::new(("tray", icon.hwnd, icon.uid)), egui::Sense::click());
        highlight(ui, cell, &resp, RADIUS);
        record_rect(ui.ctx(), icon, cell);
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

/// Tell the tray host where `cell` is on screen, in physical pixels, so apps that ask where
/// their icon is (`Shell_NotifyIconGetRect`) get an answer.
fn record_rect(ctx: &egui::Context, icon: &TrayIcon, cell: egui::Rect) {
    let Some(origin) = ctx.input(|i| i.viewport().inner_rect) else { return };
    let ppp = ctx.pixels_per_point();
    let min = (origin.min + cell.min.to_vec2()) * ppp;
    let max = (origin.min + cell.max.to_vec2()) * ppp;
    let px = |v: f32| v.round() as i32;
    super::set_rect(icon.hwnd, icon.uid, [px(min.x), px(min.y), px(max.x), px(max.y)]);
}
