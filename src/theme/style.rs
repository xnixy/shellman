//! Applying the palette to egui, and the shared frames and buttons.

use super::icons::icon;
use super::{accent, colors, mix, with_alpha, Colors};
use egui::{Color32, CornerRadius, Frame, Margin, RichText, Stroke, Theme, Ui, Vec2};

/// Give egui both looks for the current accent. egui picks one by the Windows app mode.
pub fn apply(ctx: &egui::Context) {
    let a = accent();
    for (theme, colors) in [
        (Theme::Dark, Colors::new(false, a)),
        (Theme::Light, Colors::new(true, a)),
    ] {
        let mut style = (*ctx.style_of(theme)).clone();
        style.visuals = shell_visuals(&colors);
        style.spacing.item_spacing = Vec2::new(10.0, 8.0);
        style.spacing.button_padding = Vec2::new(12.0, 7.0);
        style.spacing.interact_size.y = 30.0;
        style.interaction.selectable_labels = false;
        ctx.set_style_of(theme, style);
    }
}

fn shell_visuals(c: &Colors) -> egui::Visuals {
    let pick = |dark: Color32, light: Color32| if c.light { light } else { dark };
    let mut visuals = if c.light {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    visuals.window_fill = c.popup;
    visuals.panel_fill = Color32::TRANSPARENT;
    visuals.faint_bg_color = pick(
        Color32::from_rgba_unmultiplied(255, 255, 255, 14),
        Color32::from_black_alpha(10),
    );
    visuals.extreme_bg_color = pick(
        with_alpha(mix(Color32::from_gray(18), c.accent, 0.06), 170),
        Color32::from_rgba_unmultiplied(255, 255, 255, 210),
    );
    visuals.widgets.noninteractive.fg_stroke.color = c.muted;
    visuals.widgets.noninteractive.bg_stroke.color = c.hairline;
    let mut idle = visuals.widgets.inactive;
    idle.bg_fill = c.track;
    idle.weak_bg_fill = pick(
        Color32::from_rgba_unmultiplied(255, 255, 255, 18),
        Color32::from_black_alpha(14),
    );
    idle.bg_stroke = Stroke::new(
        1.0_f32,
        pick(
            Color32::from_rgba_unmultiplied(255, 255, 255, 36),
            Color32::from_black_alpha(32),
        ),
    );
    idle.corner_radius = CornerRadius::same(8);
    idle.fg_stroke.color = c.text;
    let mut hovered = idle;
    hovered.weak_bg_fill = pick(
        Color32::from_rgba_unmultiplied(255, 255, 255, 30),
        Color32::from_black_alpha(26),
    );
    hovered.bg_fill = mix(c.track, c.accent, 0.25);
    let mut active = hovered;
    active.bg_fill = c.accent;
    active.weak_bg_fill = c.accent;
    active.fg_stroke.color = c.on_accent;
    visuals.widgets.inactive = idle;
    visuals.widgets.hovered = hovered;
    visuals.widgets.active = active;
    visuals.widgets.open = hovered;
    visuals.selection.bg_fill = pick(c.accent, mix(c.accent, Color32::WHITE, 0.45));
    visuals.selection.stroke = Stroke::new(1.0_f32, c.accent_text);
    visuals.hyperlink_color = c.accent_text;
    visuals.text_cursor.stroke.color = c.accent_text;
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.window_stroke = Stroke::new(1.0_f32, c.hairline);
    visuals
}

pub fn dialog() -> Frame {
    let c = colors();
    Frame::new()
        .fill(c.popup)
        .stroke(Stroke::new(1.0_f32, c.hairline))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(20))
}

pub fn card<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    Frame::new()
        .fill(colors().control)
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(14))
        .show(ui, add_contents)
}

pub fn muted(text: impl Into<String>) -> RichText {
    RichText::new(text).color(colors().muted)
}

pub fn primary_button(text: &str) -> egui::Button<'static> {
    let c = colors();
    egui::Button::new(RichText::new(text).strong().color(c.on_accent))
        .fill(c.accent)
        .corner_radius(CornerRadius::same(10))
        .min_size(Vec2::new(110.0, 32.0))
}

pub fn danger_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).strong().color(Color32::WHITE))
        .fill(colors().danger)
        .corner_radius(CornerRadius::same(10))
        .min_size(Vec2::new(110.0, 32.0))
}

#[allow(dead_code)]
/// A flat icon button, tinted on hover, that dims when it cannot be used.
pub fn icon_button(ui: &mut Ui, c: char, enabled: bool, tip: &str) -> egui::Response {
    let button = egui::Button::new(icon(c, 14.0))
        .frame(false)
        .min_size(Vec2::new(32.0, 30.0));
    ui.add_enabled(enabled, button).on_hover_text(tip)
}
