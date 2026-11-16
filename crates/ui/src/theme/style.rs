//! Applying the palette to egui, and the shared frames and buttons.

use super::icons::icon;
use crate::glass::islands::shows_as;
use super::{accent, colors, mix, with_alpha, Colors};
use egui::{Color32, CornerRadius, Frame, Margin, RichText, Stroke, Theme, Ui, Vec2};

/// Give egui both looks for the current accent. egui picks one by the Windows app mode.
pub fn apply(ctx: &egui::Context) {
    super::env::refresh();
    let a = accent();
    let hc = super::env::high_contrast();
    for (theme, colors) in [
        (Theme::Dark, if hc { Colors::high_contrast() } else { Colors::new(false, a) }),
        (Theme::Light, if hc { Colors::high_contrast() } else { Colors::new(true, a) }),
    ] {
        let mut style = (*ctx.style_of(theme)).clone();
        style.visuals = shell_visuals(&colors);
        if hc {
            outline(&mut style.visuals, &colors);
        }
        style.spacing.item_spacing = Vec2::new(10.0, 8.0);
        style.spacing.button_padding = Vec2::new(12.0, 7.0);
        style.spacing.interact_size.y = 30.0;
        style.interaction.selectable_labels = false;
        ctx.set_style_of(theme, style);
    }
}

fn shell_visuals(c: &Colors) -> egui::Visuals {
    let pick = |dark: Color32, light: Color32| if c.light { light } else { dark };
    // A translucent white, as it should show over the glass (see `shows_as`).
    let white = |a: u8| shows_as(Color32::from_rgba_unmultiplied(255, 255, 255, a));
    let mut visuals = if c.light {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    visuals.window_fill = c.popup;
    visuals.panel_fill = Color32::TRANSPARENT;
    visuals.faint_bg_color = pick(
        white(14),
        Color32::from_black_alpha(10),
    );
    visuals.extreme_bg_color = pick(
        with_alpha(mix(Color32::from_gray(18), c.accent, 0.06), 170),
        Color32::from_rgba_unmultiplied(255, 255, 255, 210),
    );
    visuals.widgets.noninteractive.fg_stroke.color = c.muted;
    // What is disabled keeps a visible shape: egui draws it with the non-interactive fill, which
    // on a light page would otherwise be as pale as the page.
    visuals.widgets.noninteractive.weak_bg_fill = pick(white(10), Color32::from_black_alpha(6));
    visuals.widgets.noninteractive.bg_stroke.color = c.hairline;
    let mut idle = visuals.widgets.inactive;
    idle.bg_fill = c.track;
    // Buttons and combo boxes. On a light page a faint black would be a grey smudge, so they are
    // a lifted white with a clear edge; on a dark one a faint white that shows as intended.
    idle.weak_bg_fill = pick(white(18), Color32::from_rgba_unmultiplied(255, 255, 255, 170));
    idle.bg_stroke = Stroke::new(1.0_f32, pick(white(36), Color32::from_black_alpha(40)));
    idle.corner_radius = CornerRadius::same(8);
    idle.fg_stroke.color = c.text;
    let mut hovered = idle;
    hovered.weak_bg_fill = pick(white(30), Color32::from_rgba_unmultiplied(255, 255, 255, 235));
    hovered.bg_fill = mix(c.track, c.accent_fill, 0.25);
    let mut active = hovered;
    active.bg_fill = c.accent_fill;
    active.weak_bg_fill = c.accent_fill;
    active.fg_stroke.color = c.on_accent;
    visuals.widgets.inactive = idle;
    visuals.widgets.hovered = hovered;
    visuals.widgets.active = active;
    visuals.widgets.open = hovered;
    visuals.selection.bg_fill = pick(c.accent_fill, mix(c.accent_fill, Color32::WHITE, 0.45));
    visuals.selection.stroke = Stroke::new(1.0_f32, c.accent_text);
    visuals.hyperlink_color = c.accent_text;
    visuals.text_cursor.stroke.color = c.accent_text;
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.window_stroke = Stroke::new(1.0_f32, c.hairline);
    visuals
}

/// High contrast: every control outlined in the text colour, Windows' own pairs only.
fn outline(v: &mut egui::Visuals, c: &Colors) {
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.bg_stroke = Stroke::new(1.0_f32, c.text);
        w.weak_bg_fill = c.bg;
        w.fg_stroke.color = c.text;
    }
    v.widgets.hovered.weak_bg_fill = c.accent_fill;
    v.widgets.hovered.fg_stroke.color = c.on_accent;
    v.widgets.active.weak_bg_fill = c.accent_fill;
    v.widgets.active.fg_stroke.color = c.on_accent;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, c.text);
    v.extreme_bg_color = c.bg;
    v.faint_bg_color = c.bg;
    v.window_fill = c.bg;
    v.panel_fill = c.bg;
    v.selection.bg_fill = c.accent_fill;
    v.selection.stroke = Stroke::new(1.0_f32, c.on_accent);
    v.window_shadow = egui::epaint::Shadow::NONE;
    v.popup_shadow = egui::epaint::Shadow::NONE;
}

pub fn dialog() -> Frame {
    let c = colors();
    Frame::new()
        .fill(c.popup)
        .stroke(Stroke::new(1.0_f32, c.hairline))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(20))
}

pub fn muted(text: impl Into<String>) -> RichText {
    RichText::new(text).color(colors().muted)
}

/// The height of every pill button.
pub const PILL_H: f32 = 30.0;

/// The one thing to do in a view: an accent pill, semibold, `on_accent` text.
pub fn primary_button(text: &str) -> egui::Button<'static> {
    let c = colors();
    egui::Button::new(RichText::new(text).font(super::semibold_font(13.0)).color(c.on_accent))
        .fill(c.accent_fill)
        .stroke(if c.high_contrast { Stroke::new(1.0_f32, c.text) } else { Stroke::NONE })
        .corner_radius(CornerRadius::same((PILL_H / 2.0) as u8))
        .min_size(Vec2::new(88.0, PILL_H))
}

/// Everything else: a soft pill with a hairline.
pub fn secondary_button(text: &str) -> egui::Button<'static> {
    let c = colors();
    let soft = if c.high_contrast {
        c.bg
    } else if c.light {
        Color32::from_white_alpha(150)
    } else {
        Color32::from_white_alpha(14)
    };
    egui::Button::new(RichText::new(text).size(13.0).color(c.text))
        .fill(soft)
        .stroke(Stroke::new(1.0_f32, if c.high_contrast { c.text } else { c.hairline }))
        .corner_radius(CornerRadius::same((PILL_H / 2.0) as u8))
        .min_size(Vec2::new(80.0, PILL_H))
}

/// A destructive action (Uninstall, Delete): a red pill.
pub fn danger_button(text: &str) -> egui::Button<'static> {
    let c = colors();
    let (fill, ink) = if c.high_contrast { (c.accent_fill, c.on_accent) } else { (c.danger, Color32::WHITE) };
    egui::Button::new(RichText::new(text).font(super::semibold_font(13.0)).color(ink))
        .fill(fill)
        .stroke(if c.high_contrast { Stroke::new(1.0_f32, c.text) } else { Stroke::NONE })
        .corner_radius(CornerRadius::same((PILL_H / 2.0) as u8))
        .min_size(Vec2::new(88.0, PILL_H))
}

/// A 2 px ring around a painted control that has the keyboard focus (egui's own widgets draw
/// theirs; painted ones call this).
pub fn focus_ring(ui: &Ui, resp: &egui::Response, rect: egui::Rect, radius: f32) {
    if resp.has_focus() {
        ui.painter().rect_stroke(rect.expand(2.0), radius + 2.0, Stroke::new(2.0_f32, colors().text), egui::StrokeKind::Outside);
    }
}

/// Tell a screen reader what a painted item is: its role and name. `name` runs only while one
/// is listening (egui asks for widget info only then, or when the item gains focus), so a name
/// that costs a format or a Windows call is fine.
pub fn speak(resp: &egui::Response, role: egui::WidgetType, name: impl Fn() -> String) {
    crate::access::describe(&resp, || egui::WidgetInfo::labeled(role, resp.enabled(), name()));
}

/// [`speak`] for an item that can be chosen (a tile, a day, a row): also whether it is.
pub fn speak_selected(resp: &egui::Response, role: egui::WidgetType, selected: bool, name: impl Fn() -> String) {
    crate::access::describe(&resp, || egui::WidgetInfo::selected(role, resp.enabled(), selected, name()));
}

/// High contrast's stand-in for a hover wash and a selected fill, which it does not allow (only
/// highlight text goes on highlight): a selected item is outlined 2 px in the highlight colour, a
/// hovered one 1 px in the text colour. Draws nothing outside high contrast.
pub fn contrast_outline(ui: &Ui, rect: egui::Rect, radius: f32, hovered: bool, selected: bool) {
    let c = colors();
    if !c.high_contrast {
        return;
    }
    if selected {
        ui.painter().rect_stroke(rect, radius, Stroke::new(2.0_f32, c.row_selected), egui::StrokeKind::Inside);
    } else if hovered {
        ui.painter().rect_stroke(rect, radius, Stroke::new(1.0_f32, c.text), egui::StrokeKind::Inside);
    }
}

/// A row with `control` at its right and `body` in the rest. The control is laid out first, so
/// the body wraps before it rather than running underneath it in a narrow window.
pub fn trailing<R>(ui: &mut Ui, control: impl FnOnce(&mut Ui) -> R, body: impl FnOnce(&mut Ui)) -> R {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let r = control(ui);
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), body);
            r
        })
        .inner
    })
    .inner
}

/// A setting: title 13 and a muted detail 11.5 on the left, a switch on the right. Returns the
/// switch's response (`changed()` when flipped).
pub fn setting_row(ui: &mut Ui, title: &str, detail: &str, on: &mut bool) -> egui::Response {
    let c = colors();
    trailing(
        ui,
        |ui| crate::widgets::toggle(ui, on, title),
        |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add(egui::Label::new(RichText::new(title).size(13.0).color(c.text)).wrap());
                if !detail.is_empty() {
                    ui.add(egui::Label::new(RichText::new(detail).size(11.5).color(c.muted)).wrap());
                }
            });
        },
    )
}

/// A checkbox: a 20 rounded square, accent-filled with an `on_accent` tick when checked.
pub fn checkbox(ui: &mut Ui, checked: &mut bool, label: &str) -> egui::Response {
    let c = colors();
    let galley = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::proportional(13.0), c.text);
    let size = Vec2::new(20.0 + 10.0 + galley.size().x, 22.0_f32.max(galley.size().y));
    let (rect, mut resp) = ui.allocate_exact_size(size, egui::Sense::click());
    crate::access::describe(&resp, || egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, *checked, label));
    focus_ring(ui, &resp, rect, 6.0);
    if resp.clicked() {
        *checked = !*checked;
        resp.mark_changed();
    }
    let painter = ui.painter();
    let bx = egui::Rect::from_min_size(egui::pos2(rect.left(), rect.center().y - 10.0), Vec2::splat(20.0));
    if *checked {
        painter.rect_filled(bx, 4.0, c.accent_fill);
        if c.high_contrast {
            painter.rect_stroke(bx, 4.0, Stroke::new(1.0_f32, c.text), egui::StrokeKind::Inside);
        }
        let tick = vec![
            egui::pos2(bx.left() + 5.0, bx.center().y + 0.5),
            egui::pos2(bx.left() + 8.5, bx.center().y + 4.0),
            egui::pos2(bx.right() - 4.5, bx.center().y - 4.0),
        ];
        painter.line(tick, Stroke::new(1.6_f32, c.on_accent));
    } else {
        if resp.hovered() && !c.high_contrast {
            painter.rect_filled(bx, 4.0, c.row_hover);
        }
        let edge = if c.high_contrast { c.text } else { c.muted };
        painter.rect_stroke(bx.shrink(0.5), 4.0, Stroke::new(1.0_f32, edge), egui::StrokeKind::Inside);
    }
    painter.galley(egui::pos2(bx.right() + 10.0, rect.center().y - galley.size().y / 2.0), galley, c.text);
    resp
}

#[allow(dead_code)]
/// A flat icon button, tinted on hover, that dims when it cannot be used.
pub fn icon_button(ui: &mut Ui, c: char, enabled: bool, tip: &str) -> egui::Response {
    let button = egui::Button::new(icon(c, 14.0))
        .frame(false)
        .min_size(Vec2::new(32.0, 30.0));
    ui.add_enabled(enabled, button).on_hover_text(tip)
}
