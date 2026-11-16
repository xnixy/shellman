//! Small pieces drawn on islands (see `glass::islands`): chips, round icon buttons, and the
//! window buttons a frameless islands window draws for itself.

use crate::glass::islands::shows_as;
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

/// The faint fill and line of a soft chip (or a card) on glass, and its hovered fill. Given as
/// they should show (see `glass::islands::shows_as`).
/// In high contrast both fills are the window colour and the line is the text colour: a white
/// wash is not one of Windows' colours, and hover is shown by an outline instead.
pub fn soft_colors(light: bool) -> (Color32, Color32, Stroke) {
    let c = colors();
    if c.high_contrast {
        return (c.bg, c.bg, Stroke::new(1.0_f32, c.text));
    }
    let white = |a: u8| shows_as(Color32::from_rgba_unmultiplied(255, 255, 255, a));
    if light {
        (
            white(110),
            white(170),
            Stroke::new(1.0_f32, Color32::from_black_alpha(18)),
        )
    } else {
        (white(14), white(28), Stroke::new(1.0_f32, white(26)))
    }
}

/// A chip in `rect`: a rounded label, with a glyph before it if `icon` is given.
pub fn chip(
    ui: &mut Ui,
    rect: Rect,
    id: egui::Id,
    label: &str,
    icon: Option<char>,
    kind: Chip,
) -> egui::Response {
    let c = colors();
    let resp = ui.interact(rect, id, Sense::click());
    // A chip is an action ("Today", "Clear all", "Send"); the accent is its look, not a state.
    crate::access::describe(&resp, || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    theme::focus_ring(ui, &resp, rect, rect.height() / 2.0);
    let hovered = resp.hovered();
    let (soft, soft_hover, line) = soft_colors(c.light);
    let (fill, stroke, text) = match kind {
        Chip::Soft => (if hovered { soft_hover } else { soft }, line, c.text),
        Chip::Plain => (
            if hovered {
                soft_hover
            } else {
                Color32::TRANSPARENT
            },
            Stroke::NONE,
            c.text,
        ),
        Chip::Accent => {
            let fill = if hovered {
                theme::mix(c.accent_fill, Color32::WHITE, 0.12)
            } else {
                c.accent_fill
            };
            (fill, Stroke::NONE, c.on_accent)
        }
        Chip::Danger => {
            let a = if hovered { 90 } else { 56 };
            (
                shows_as(theme::with_alpha(c.danger, a)),
                Stroke::NONE,
                c.danger_text,
            )
        }
    };
    let r = CornerRadius::same((rect.height() / 2.0).round() as u8);
    ui.painter()
        .rect(rect, r, fill, stroke, egui::StrokeKind::Inside);
    if hovered && kind != Chip::Accent {
        theme::contrast_outline(ui, rect, rect.height() / 2.0, true, false);
    }
    let mut x = rect.left() + PAD;
    if let Some(g) = icon {
        ui.painter().text(
            egui::pos2(x + 7.0, rect.center().y),
            Align2::CENTER_CENTER,
            g,
            icon_font(12.0),
            text,
        );
        x += ICON_W;
    }
    let galley = ui.fonts(|f| {
        f.layout(
            label.to_owned(),
            font(kind),
            text,
            (rect.right() - PAD - x).max(0.0),
        )
    });
    let y = rect.center().y - galley.size().y / 2.0;
    ui.painter().galley(egui::pos2(x, y), galley, text);
    resp
}

/// A round icon button centred in `rect`. `danger` turns it red when hovered (a close button).
/// `name` is what a screen reader calls it (the caller shows its own tooltip).
pub fn round_icon(
    ui: &mut Ui,
    rect: Rect,
    id: egui::Id,
    g: char,
    enabled: bool,
    danger: bool,
    name: &str,
) -> egui::Response {
    let c = colors();
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let resp = ui.interact(rect, id, sense);
    crate::access::describe(&resp, || egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name));
    let radius = rect.width().min(rect.height()) / 2.0;
    theme::focus_ring(ui, &resp, rect, radius);
    let mut fg = if enabled {
        c.text
    } else {
        theme::with_alpha(c.muted, 90)
    };
    if enabled && resp.hovered() {
        let fill = if danger {
            Color32::from_rgb(196, 43, 28)
        } else {
            soft_colors(c.light).1
        };
        if danger {
            fg = Color32::WHITE;
        }
        // High contrast has no red: a hovered close button is highlight under highlight text.
        let fill = if c.high_contrast && danger {
            fg = c.on_accent;
            c.accent_fill
        } else {
            fill
        };
        ui.painter().circle_filled(rect.center(), radius, fill);
        if c.high_contrast && !danger {
            ui.painter().circle_stroke(rect.center(), radius - 0.5, Stroke::new(1.0_f32, c.text));
        }
    }
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, g, icon_font(11.0), fg);
    resp
}

/// Minimize, maximize (or restore) and close, side by side in `rect` from its right end, each a
/// round button `rect.height()` wide. They act on the window themselves.
pub fn window_buttons(ui: &mut Ui, rect: Rect) {
    let d = rect.height();
    let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
    let at = |i: f32| {
        Rect::from_min_size(
            egui::pos2(rect.right() - d * (i + 1.0), rect.top()),
            Vec2::splat(d),
        )
    };
    let size = if maximized {
        glyph::RESTORE
    } else {
        glyph::MAXIMIZE
    };
    let ctx = ui.ctx().clone();
    if round_icon(
        ui,
        at(0.0),
        egui::Id::new("window-close"),
        glyph::CHROME_CLOSE,
        true,
        true,
        "Close",
    )
    .on_hover_text("Close")
    .clicked()
    {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    let tip = if maximized { "Restore" } else { "Maximize" };
    if round_icon(ui, at(1.0), egui::Id::new("window-max"), size, true, false, tip)
        .on_hover_text(tip)
        .clicked()
    {
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
    if round_icon(
        ui,
        at(2.0),
        egui::Id::new("window-min"),
        glyph::MINIMIZE,
        true,
        false,
        "Minimize",
    )
    .on_hover_text("Minimize")
    .clicked()
    {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }
}
