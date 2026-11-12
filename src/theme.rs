//! Colors, spacing, and small widgets, ported from Blurman.
//!
//! Instead of a fixed rose, everything is derived from one accent color the user picks: the
//! Windows accent, a preset, or a custom color. Neutrals carry a faint tint of the accent, the
//! way Blurman's plum shell carries its rose. Dark and light follow the Windows app mode.

use egui::{Color32, CornerRadius, Frame, Margin, RichText, Shadow, Stroke, Theme, Ui, Vec2};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Accent presets, each strong enough to carry its text color on dark and light.
pub const PRESETS: [(&str, [u8; 3]); 9] = [
    ("Rose", [196, 54, 78]),
    ("Coral", [214, 82, 52]),
    ("Amber", [204, 132, 0]),
    ("Green", [28, 138, 76]),
    ("Teal", [0, 128, 128]),
    ("Blue", [0, 103, 192]),
    ("Indigo", [84, 82, 214]),
    ("Violet", [136, 64, 200]),
    ("Slate", [96, 108, 128]),
];

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum AccentChoice {
    /// Follows the Windows accent color and changes with it.
    System,
    Preset(usize),
    Custom([u8; 3]),
}

impl Default for AccentChoice {
    fn default() -> Self {
        AccentChoice::System
    }
}

impl AccentChoice {
    pub fn resolve(self) -> Color32 {
        match self {
            AccentChoice::System => system_accent().unwrap_or(rgb(PRESETS[0].1)),
            AccentChoice::Preset(i) => rgb(PRESETS.get(i).unwrap_or(&PRESETS[0]).1),
            AccentChoice::Custom(c) => rgb(c),
        }
    }
}

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// The Windows accent color, from the same registry value the taskbar uses (0xAABBGGRR).
pub fn system_accent() -> Option<Color32> {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let mut value: u32 = 0;
    let mut size = size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\DWM"),
            w!("AccentColor"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&mut size),
        )
    };
    status.is_ok().then(|| {
        Color32::from_rgb(value as u8, (value >> 8) as u8, (value >> 16) as u8)
    })
}

/// Linear blend from `a` to `b`. Alpha comes from `a`.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

fn with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

fn luminance(c: Color32) -> f32 {
    let ch = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
}

/// Every color that differs between dark and light, or depends on the accent.
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub struct Colors {
    pub light: bool,
    pub accent: Color32,
    /// Text drawn on a solid accent fill.
    pub on_accent: Color32,
    /// The accent, adjusted to read as text or a glyph on the page.
    pub accent_text: Color32,
    pub bg: Color32,
    pub popup: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub warn: Color32,
    pub row_hover: Color32,
    pub row_selected: Color32,
    pub hairline: Color32,
    pub well: Color32,
    pub track: Color32,
    pub danger: Color32,
    pub danger_text: Color32,
    pub card: Color32,
    pub shadow: Color32,
    pub sidebar_glass: Color32,
    pub sidebar_solid: Color32,
    pub page_glass: Color32,
}

impl Colors {
    pub fn new(light: bool, accent: Color32) -> Self {
        let on_accent = if luminance(accent) > 0.4 {
            Color32::from_rgb(20, 16, 20)
        } else {
            Color32::WHITE
        };
        // Neutral greys with a little of the accent mixed in.
        let tint = |grey: u8, amount: f32| mix(Color32::from_gray(grey), accent, amount);
        if light {
            let bg = tint(242, 0.05);
            Self {
                light,
                accent,
                on_accent,
                accent_text: mix(accent, Color32::BLACK, 0.25),
                bg,
                popup: tint(252, 0.02),
                text: tint(28, 0.06),
                muted: tint(96, 0.08),
                warn: Color32::from_rgb(160, 96, 0),
                row_hover: Color32::from_black_alpha(14),
                row_selected: with_alpha(accent, 48),
                hairline: Color32::from_black_alpha(28),
                well: Color32::from_black_alpha(12),
                track: tint(188, 0.06),
                danger: Color32::from_rgb(196, 43, 28),
                danger_text: Color32::from_rgb(150, 24, 28),
                card: with_alpha(tint(255, 0.02), 200),
                shadow: Color32::from_black_alpha(26),
                sidebar_glass: with_alpha(tint(230, 0.05), 170),
                sidebar_solid: tint(230, 0.05),
                page_glass: with_alpha(tint(248, 0.03), 120),
            }
        } else {
            Self {
                light,
                accent,
                on_accent,
                accent_text: mix(accent, Color32::WHITE, 0.4),
                bg: tint(16, 0.06),
                popup: tint(34, 0.08),
                text: tint(240, 0.03),
                muted: tint(184, 0.06),
                warn: Color32::from_rgb(230, 186, 120),
                row_hover: Color32::from_rgba_unmultiplied(255, 255, 255, 14),
                row_selected: with_alpha(accent, 64),
                hairline: Color32::from_rgba_unmultiplied(255, 255, 255, 32),
                well: Color32::from_rgba_unmultiplied(255, 255, 255, 12),
                track: tint(66, 0.1),
                danger: Color32::from_rgb(196, 43, 28),
                danger_text: Color32::from_rgb(255, 160, 164),
                card: with_alpha(tint(34, 0.08), 214),
                shadow: Color32::from_black_alpha(80),
                sidebar_glass: with_alpha(tint(10, 0.06), 178),
                sidebar_solid: tint(12, 0.06),
                page_glass: with_alpha(tint(16, 0.06), 140),
            }
        }
    }
}

static LIGHT: AtomicBool = AtomicBool::new(false);
static ACCENT: AtomicU32 = AtomicU32::new(0xC4364E);

fn accent() -> Color32 {
    let v = ACCENT.load(Ordering::Relaxed);
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// The colors for the mode and accent the window has now.
pub fn colors() -> Colors {
    Colors::new(LIGHT.load(Ordering::Relaxed), accent())
}

/// Switch the palette's mode. True when it changed.
pub fn set_light(light: bool) -> bool {
    LIGHT.swap(light, Ordering::Relaxed) != light
}

/// Switch the accent and restyle egui. True when it changed.
pub fn set_accent(ctx: &egui::Context, c: Color32) -> bool {
    let v = (c.r() as u32) << 16 | (c.g() as u32) << 8 | c.b() as u32;
    let changed = ACCENT.swap(v, Ordering::Relaxed) != v;
    if changed {
        apply(ctx);
    }
    changed
}

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

pub fn sidebar(glass: bool) -> Frame {
    let c = colors();
    Frame::new()
        .fill(if glass { c.sidebar_glass } else { c.sidebar_solid })
        .inner_margin(Margin::symmetric(10, 12))
}

pub fn page(glass: bool) -> Frame {
    let c = colors();
    Frame::new()
        .fill(if glass { c.page_glass } else { c.bg })
        .inner_margin(Margin::symmetric(12, 8))
}

/// The tab strip and toolbar: the sidebar's shade, so the content reads as the lit part.
pub fn chrome(glass: bool) -> Frame {
    let c = colors();
    Frame::new()
        .fill(if glass { c.sidebar_glass } else { c.sidebar_solid })
        .inner_margin(Margin::symmetric(10, 6))
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
    let c = colors();
    Frame::new()
        .fill(c.card)
        .stroke(Stroke::new(1.0_f32, c.hairline))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(Margin::same(14))
        .shadow(Shadow {
            offset: [0, 8],
            blur: 18,
            spread: 0,
            color: c.shadow,
        })
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

// ---- Icons: Segoe Fluent Icons on Windows 11, Segoe MDL2 Assets on Windows 10. ----

#[allow(dead_code)]
pub mod glyph {
    pub const BACK: char = '\u{E72B}';
    pub const FORWARD: char = '\u{E72A}';
    pub const UP: char = '\u{E74A}';
    pub const REFRESH: char = '\u{E72C}';
    pub const SEARCH: char = '\u{E721}';
    pub const HOME: char = '\u{E80F}';
    pub const PC: char = '\u{E7F8}';
    pub const DESKTOP: char = '\u{E7F4}';
    pub const DOCUMENTS: char = '\u{E8A5}';
    pub const DOWNLOADS: char = '\u{E896}';
    pub const PICTURES: char = '\u{EB9F}';
    pub const MUSIC: char = '\u{EC4F}';
    pub const VIDEOS: char = '\u{E8B2}';
    pub const DRIVE: char = '\u{EDA2}';
    pub const FOLDER: char = '\u{E8B7}';
    pub const FILE: char = '\u{E7C3}';
    pub const SETTINGS: char = '\u{E713}';
    pub const ADD: char = '\u{E710}';
    pub const CLOSE: char = '\u{E711}';
    pub const CHEVRON: char = '\u{E76C}';
    pub const NEW_FOLDER: char = '\u{E8F4}';
    pub const DELETE: char = '\u{E74D}';
    pub const RENAME: char = '\u{E8AC}';
    pub const COPY: char = '\u{E8C8}';
    pub const CUT: char = '\u{E8C6}';
    pub const PASTE: char = '\u{E77F}';
    pub const OPEN_EXTERNAL: char = '\u{E8A7}';
    pub const SORT_UP: char = '\u{E70E}';
    pub const SORT_DOWN: char = '\u{E70D}';
    pub const CHECK: char = '\u{E73E}';
}

pub fn icon_font(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name("icons".into()))
}

pub fn icon(c: char, size: f32) -> RichText {
    RichText::new(c.to_string()).font(icon_font(size))
}

/// egui's fonts, Segoe UI for text when present, and the icon font.
pub fn install_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontFamily};
    use std::sync::Arc;
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") {
        fonts
            .font_data
            .insert("segoe".into(), Arc::new(FontData::from_owned(bytes)));
        if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
            family.insert(0, "segoe".into());
        }
    }
    for path in [r"C:\Windows\Fonts\SegoeIcons.ttf", r"C:\Windows\Fonts\segmdl2.ttf"] {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert("icons".into(), Arc::new(FontData::from_owned(bytes)));
            fonts
                .families
                .insert(FontFamily::Name("icons".into()), vec!["icons".into()]);
            break;
        }
    }
    ctx.set_fonts(fonts);
}

/// A flat icon button, tinted on hover, that dims when it cannot be used.
pub fn icon_button(ui: &mut Ui, c: char, enabled: bool, tip: &str) -> egui::Response {
    let button = egui::Button::new(icon(c, 14.0))
        .frame(false)
        .min_size(Vec2::new(32.0, 30.0));
    ui.add_enabled(enabled, button).on_hover_text(tip)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_on_accent_contrasts() {
        for (_, c) in PRESETS {
            let a = rgb(c);
            let on = Colors::new(false, a).on_accent;
            let (la, lo) = (luminance(a), luminance(on));
            let ratio = (la.max(lo) + 0.05) / (la.min(lo) + 0.05);
            assert!(ratio >= 3.0, "{c:?} has contrast {ratio}");
        }
    }

    #[test]
    fn bright_accent_gets_dark_text() {
        let on = Colors::new(true, Color32::from_rgb(255, 220, 80)).on_accent;
        assert!(luminance(on) < 0.1);
    }
}
