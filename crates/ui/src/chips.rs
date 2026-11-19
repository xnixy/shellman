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
    // Glasshouse's field fill: a faint wash of the text colour, deeper under the pointer, and a
    // hairline where a chip needs an edge.
    let white = |a: u8| shows_as(Color32::from_rgba_unmultiplied(255, 255, 255, a));
    if light {
        (
            Color32::from_black_alpha(18),
            Color32::from_black_alpha(30),
            Stroke::new(1.0_f32, Color32::from_black_alpha(26)),
        )
    } else {
        (white(22), white(36), Stroke::new(1.0_f32, white(23)))
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
            // Under the pointer it leans toward the text colour (darker on light, lighter on
            // dark), so `on_accent` keeps its contrast, as the bell and calendar do.
            let fill = if hovered && !c.high_contrast {
                theme::mix(c.accent_fill, c.text, 0.12)
            } else {
                c.accent_fill
            };
            (fill, Stroke::NONE, c.on_accent)
        }
        // Glasshouse's destructive action: red text on a faint red wash, inside a hairline.
        Chip::Danger => {
            let a = if hovered { 46 } else { 22 };
            (
                shows_as(theme::with_alpha(c.danger_text, a)),
                line,
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

/// The width of one window button, as Windows draws its own.
pub const CAPTION_W: f32 = 46.0;
/// Minimize, maximize and close side by side.
pub const CAPTIONS_W: f32 = CAPTION_W * 3.0;

/// Minimize, maximize (or restore) and close, side by side across `rect` (a third each), as
/// Windows 11 draws its own: thin glyphs, a faint square wash under the pointer, and close turning
/// Windows' red with a white glyph. They stay native in every look, so a shellman window sits
/// beside other apps' windows without standing out. They act on the window themselves.
pub fn window_buttons(ui: &mut Ui, rect: Rect) {
    buttons(ui, rect, None);
}

/// The window buttons flush in the window's top right corner, as Windows places its own: square,
/// the close button's outer corner rounded with the window's `corner` radius (0 when maximized).
pub fn window_buttons_flush(ui: &mut Ui, rect: Rect, corner: f32) {
    buttons(ui, rect, Some(corner));
}

fn buttons(ui: &mut Ui, rect: Rect, flush: Option<f32>) {
    let w = rect.width() / 3.0;
    let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
    let at = |i: f32| Rect::from_min_size(egui::pos2(rect.right() - w * (i + 1.0), rect.top()), Vec2::new(w, rect.height()));
    let ctx = ui.ctx().clone();
    // Floating in an island: each a softly rounded square. Flush: square, the close button's
    // outer corner following the window's.
    let (round, close_round) = match flush {
        None => (CornerRadius::same(6), CornerRadius::same(6)),
        Some(r) => {
            let r = r.round().clamp(0.0, 255.0) as u8;
            (CornerRadius::ZERO, CornerRadius { nw: 0, ne: r, sw: 0, se: 0 })
        }
    };
    if caption(ui, at(0.0), "window-close", glyph::CHROME_CLOSE, true, "Close", close_round).clicked() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    let (g, tip) = if maximized { (glyph::RESTORE, "Restore") } else { (glyph::MAXIMIZE, "Maximize") };
    if caption(ui, at(1.0), "window-max", g, false, tip, round).clicked() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
    if caption(ui, at(2.0), "window-min", glyph::MINIMIZE, false, "Minimize", round).clicked() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }
}

/// One window button.
fn caption(ui: &mut Ui, rect: Rect, id: &str, g: char, close: bool, name: &str, round: CornerRadius) -> egui::Response {
    let c = colors();
    let resp = ui.interact(rect, egui::Id::new(id), Sense::click());
    crate::access::describe(&resp, || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    let resp = resp.on_hover_text(name);
    theme::focus_ring(ui, &resp, rect, 6.0);
    let (hovered, pressed) = (resp.hovered(), resp.is_pointer_button_down_on());
    // Windows' own close red, and its pressed shade.
    let red = if pressed { Color32::from_rgb(200, 60, 49) } else { Color32::from_rgb(196, 43, 28) };
    let (fill, fg) = match (hovered, close, c.high_contrast) {
        (true, _, true) => (c.accent_fill, c.on_accent),
        (true, true, false) => (red, Color32::WHITE),
        (true, false, false) => (if pressed { c.press_soft } else { c.hover_soft }, c.text),
        _ => (Color32::TRANSPARENT, c.text),
    };
    ui.painter().rect_filled(rect, round, fill);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, g, icon_font(10.0), fg);
    resp
}
