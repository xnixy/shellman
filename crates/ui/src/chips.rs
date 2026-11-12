//! Small pieces drawn on islands (see `glass::islands`): chips, round icon buttons, and the
//! window buttons a frameless islands window draws for itself.

use crate::theme::{self, colors, glyph, icon_font, semibold_font};
use egui::{Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, Ui, Vec2};

/// How a chip is filled.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chip {
    /// A faint fill and a hairline: a button, or a step of a path.
    Soft,
    /// The accent, for the one that is current.
    Accent,
    /// Reddish, for what deletes.
    Danger,
    /// Nothing until hovered.
    Plain,
}

/// The height of a chip, in points.
pub const CHIP_H: f32 = 28.0;
const PAD: f32 = 12.0;
const ICON_W: f32 = 20.0;

fn font(kind: Chip) -> FontId {
    if kind == Chip::Accent {
        semibold_font(13.0)
    } else {
        FontId::proportional(13.0)
    }
}

/// How wide a chip with `label` (and an icon, if any) is.
pub fn chip_width(ui: &Ui, label: &str, icon: bool, kind: Chip) -> f32 {
    let text = ui
        .fonts(|f| f.layout_no_wrap(label.to_owned(), font(kind), Color32::WHITE))
        .size()
        .x;
    let icon = if icon { ICON_W } else { 0.0 };
    (text + icon + PAD * 2.0).ceil()
}

/// The faint fill and line of a soft chip (or a card) on glass, and its hovered fill.
pub fn soft_colors(light: bool) -> (Color32, Color32, Stroke) {
    if light {
        (
            Color32::from_white_alpha(130),
            Color32::from_white_alpha(200),
            Stroke::new(1.0_f32, Color32::from_black_alpha(16)),
        )
    } else {
        (
            Color32::from_white_alpha(13),
            Color32::from_white_alpha(26),
            Stroke::new(1.0_f32, Color32::from_white_alpha(18)),
        )
    }
}

/// A chip in `rect`: a rounded label, with a glyph before it if `icon` is given.
pub fn chip(ui: &mut Ui, rect: Rect, id: egui::Id, label: &str, icon: Option<char>, kind: Chip) -> egui::Response {
    let c = colors();
    let resp = ui.interact(rect, id, Sense::click());
    let hovered = resp.hovered();
    let (soft, soft_hover, line) = soft_colors(c.light);
    let (fill, stroke, text) = match kind {
        Chip::Soft => (if hovered { soft_hover } else { soft }, line, c.text),
        Chip::Plain => (if hovered { soft_hover } else { Color32::TRANSPARENT }, Stroke::NONE, c.text),
        Chip::Accent => {
            let fill = if hovered { theme::mix(c.accent_fill, Color32::WHITE, 0.12) } else { c.accent_fill };
            (fill, Stroke::NONE, c.on_accent)
        }
        Chip::Danger => {
            let a = if hovered { 90 } else { 56 };
            (theme::with_alpha(c.danger, a), Stroke::NONE, c.danger_text)
        }
    };
    let r = CornerRadius::same((rect.height() / 2.0).round() as u8);
    ui.painter().rect(rect, r, fill, stroke, egui::StrokeKind::Inside);
    let mut x = rect.left() + PAD;
    if let Some(g) = icon {
        ui.painter().text(egui::pos2(x + 7.0, rect.center().y), Align2::CENTER_CENTER, g, icon_font(12.0), text);
        x += ICON_W;
    }
    let galley = ui.fonts(|f| {
        f.layout(label.to_owned(), font(kind), text, (rect.right() - PAD - x).max(0.0))
    });
    let y = rect.center().y - galley.size().y / 2.0;
    ui.painter().galley(egui::pos2(x, y), galley, text);
    resp
}

/// A round icon button of `size` points centred in `rect`. `danger` turns it red when hovered
/// (a close button).
pub fn round_icon(ui: &mut Ui, rect: Rect, id: egui::Id, g: char, enabled: bool, danger: bool) -> egui::Response {
    let c = colors();
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let resp = ui.interact(rect, id, sense);
    let radius = rect.width().min(rect.height()) / 2.0;
    let mut fg = if enabled { c.text } else { theme::with_alpha(c.muted, 90) };
    if enabled && resp.hovered() {
        let fill = if danger { Color32::from_rgb(196, 43, 28) } else { soft_colors(c.light).1 };
        if danger {
            fg = Color32::WHITE;
        }
        ui.painter().circle_filled(rect.center(), radius, fill);
    }
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, g, icon_font(12.0), fg);
    resp
}

/// Minimize, maximize (or restore) and close, side by side in `rect` from its right end, each a
/// round button `rect.height()` wide. They act on the window themselves.
pub fn window_buttons(ui: &mut Ui, rect: Rect) {
    let d = rect.height();
    let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
    let at = |i: f32| Rect::from_min_size(egui::pos2(rect.right() - d * (i + 1.0), rect.top()), Vec2::splat(d));
    let size = if maximized { glyph::RESTORE } else { glyph::MAXIMIZE };
    let ctx = ui.ctx().clone();
    if round_icon(ui, at(0.0), egui::Id::new("window-close"), glyph::CHROME_CLOSE, true, true)
        .on_hover_text("Close")
        .clicked()
    {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    let tip = if maximized { "Restore" } else { "Maximize" };
    if round_icon(ui, at(1.0), egui::Id::new("window-max"), size, true, false)
        .on_hover_text(tip)
        .clicked()
    {
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
    if round_icon(ui, at(2.0), egui::Id::new("window-min"), glyph::MINIMIZE, true, false)
        .on_hover_text("Minimize")
        .clicked()
    {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }
}
