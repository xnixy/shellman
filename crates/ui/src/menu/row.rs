//! How a menu row looks and measures: an icon column, the label, and a shortcut or a chevron on
//! the right. Shellman's own rows and the shell's are drawn by the same code, so they read as
//! one menu.

use super::{Kind, Row};
use crate::theme::{self, colors, glyph, icon_font};
use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Align2, Color32, Context, CornerRadius, FontId, Frame, Margin, Rect, Stroke, Ui, Vec2};

/// Height of an ordinary row.
pub const ROW_H: f32 = 32.0;
/// The frame's inner margin.
pub(super) const PAD: f32 = 5.0;
const SEPARATOR_H: f32 = 9.0;
const NOTE_H: f32 = 28.0;
/// Left of the icon's centre, and of the label, from the row's left edge.
const ICON_X: f32 = 19.0;
const LABEL_X: f32 = 38.0;
const RIGHT_PAD: f32 = 12.0;
/// Space between the label and the shortcut or chevron.
const GAP: f32 = 28.0;
const MIN_W: f32 = 230.0;
pub(super) const MAX_W: f32 = 440.0;
const LABEL_SIZE: f32 = 14.0;
const SHORTCUT_SIZE: f32 = 12.0;

/// What sits in the icon column.
#[derive(Clone, Copy)]
pub enum Icon {
    None,
    /// A glyph from the icon font.
    Glyph(char),
    /// A picture, at this size in points.
    Image(egui::TextureId, Vec2),
}

pub(super) fn frame() -> Frame {
    let c = colors();
    Frame::new()
        .fill(c.popup)
        .stroke(Stroke::new(1.0_f32, c.hairline))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(PAD as i8))
        .shadow(egui::Shadow {
            offset: [0, 6],
            blur: 20,
            spread: 0,
            color: Color32::from_black_alpha(if c.light { 40 } else { 90 }),
        })
}

pub(super) fn height<T>(r: &Row<T>) -> f32 {
    match &r.kind {
        Kind::Separator => SEPARATOR_H,
        Kind::Note => NOTE_H,
        Kind::Picture(_, size, _) => (size.y + 6.0).max(ROW_H),
        _ => ROW_H,
    }
}

pub(super) fn content_height<T>(rows: &[Row<T>]) -> f32 {
    rows.iter().map(height).sum()
}

/// Wide enough for the widest row, within limits; longer labels are cut with an ellipsis.
pub(super) fn menu_width<T>(ctx: &Context, rows: &[Row<T>]) -> f32 {
    let measure = |text: &str, size: f32| {
        ctx.fonts(|f| f.layout_no_wrap(text.to_owned(), FontId::proportional(size), Color32::WHITE).size().x)
    };
    let widest = rows
        .iter()
        .map(|r| match &r.kind {
            Kind::Separator => 0.0,
            Kind::Picture(_, size, _) => size.x + 2.0 * 8.0,
            Kind::Note => LABEL_X + measure(&r.label, 12.5) + RIGHT_PAD,
            Kind::Submenu { .. } => LABEL_X + measure(&r.label, LABEL_SIZE) + GAP + 10.0 + RIGHT_PAD,
            Kind::Action(_) => {
                let right = if r.shortcut.is_empty() { 0.0 } else { GAP + measure(&r.shortcut, SHORTCUT_SIZE) };
                LABEL_X + measure(&r.label, LABEL_SIZE) + right + RIGHT_PAD
            }
        })
        .fold(0.0, f32::max);
    (widest + 2.0 * PAD).clamp(MIN_W, MAX_W)
}

/// Draws one row. `lit` when hovered, picked by the keyboard, or its submenu is open.
pub(super) fn paint<T>(ui: &Ui, rect: Rect, r: &Row<T>, lit: bool, open: bool) {
    let c = colors();
    let p = ui.painter();
    match &r.kind {
        Kind::Separator => {
            let y = rect.center().y;
            p.hline(rect.x_range().shrink(6.0), y, Stroke::new(1.0_f32, c.hairline));
            return;
        }
        Kind::Note => {
            let at = egui::pos2(rect.left() + LABEL_X, rect.center().y);
            let text = rect.width() - LABEL_X - RIGHT_PAD;
            label(ui, at, text, &r.label, 12.5, c.muted);
            return;
        }
        _ => {}
    }
    if lit && r.enabled {
        // The text colour, faint: never the accent, which may be black.
        let fill = theme::with_alpha(c.text, if c.light { 18 } else { 12 });
        p.rect_filled(rect.shrink2(Vec2::new(0.0, 1.0)), CornerRadius::same(6), fill);
    }
    let fg = if r.enabled { c.text } else { theme::with_alpha(c.muted, 150) };
    if let Kind::Picture(tex, size, _) = &r.kind {
        let at = Rect::from_min_size(egui::pos2(rect.left() + 8.0, rect.center().y - size.y / 2.0), *size);
        let tint = if r.enabled { Color32::WHITE } else { Color32::from_white_alpha(120) };
        egui::Image::from_texture((*tex, *size)).tint(tint).paint_at(ui, at);
        return;
    }
    let icon_at = egui::pos2(rect.left() + ICON_X, rect.center().y);
    if r.checked {
        // A checked row with its own icon gets a soft square behind it; without, a tick.
        if matches!(r.icon, Icon::None) {
            p.text(icon_at, Align2::CENTER_CENTER, glyph::CHECK, icon_font(12.0), fg);
        } else {
            let well = Rect::from_center_size(icon_at, Vec2::splat(24.0));
            p.rect_filled(well, CornerRadius::same(5), c.row_selected);
        }
    }
    match r.icon {
        Icon::None => {}
        Icon::Glyph(g) => {
            p.text(icon_at, Align2::CENTER_CENTER, g, icon_font(13.0), fg);
        }
        Icon::Image(tex, size) => {
            let tint = if r.enabled { Color32::WHITE } else { Color32::from_white_alpha(110) };
            let at = Rect::from_center_size(icon_at, size);
            egui::Image::from_texture((tex, size)).tint(tint).paint_at(ui, at);
        }
    }
    // The right side first, so the label knows how much room is left.
    let mut right_w = 0.0;
    let right_x = rect.right() - RIGHT_PAD;
    if matches!(r.kind, Kind::Submenu { .. }) {
        let col = if open { c.text } else { c.muted };
        p.text(egui::pos2(right_x, rect.center().y), Align2::RIGHT_CENTER, glyph::CHEVRON, icon_font(10.0), col);
        right_w = 10.0 + GAP;
    } else if !r.shortcut.is_empty() {
        let g = p.text(
            egui::pos2(right_x, rect.center().y),
            Align2::RIGHT_CENTER,
            &r.shortcut,
            FontId::proportional(SHORTCUT_SIZE),
            c.muted,
        );
        right_w = g.width() + GAP;
    }
    let at = egui::pos2(rect.left() + LABEL_X, rect.center().y);
    let room = rect.width() - LABEL_X - RIGHT_PAD - right_w;
    label(ui, at, room, &r.label, LABEL_SIZE, fg);
}

/// One line of text from `at` (left, vertically centred), cut with an ellipsis past `room`.
fn label(ui: &Ui, at: egui::Pos2, room: f32, text: &str, size: f32, color: Color32) {
    let mut job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat { font_id: FontId::proportional(size), color, ..Default::default() },
    );
    job.wrap = TextWrapping::truncate_at_width(room.max(10.0));
    let galley = ui.fonts(|f| f.layout_job(job));
    let pos = egui::pos2(at.x, at.y - galley.size().y / 2.0);
    ui.painter().galley(pos, galley, color);
}
