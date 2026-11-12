//! Colors, spacing, and small widgets, ported from Blurman.
//!
//! Instead of a fixed rose, everything is derived from one accent color the user picks: the
//! Windows accent, a preset, or a custom color. Neutrals carry a faint tint of the accent, the
//! way Blurman's plum shell carries its rose. Dark and light follow the Windows app mode.

mod icons;
mod style;

pub use icons::{glyph, icon, icon_font, install_fonts};
pub use style::{apply, danger_button, dialog, muted, primary_button};

use egui::Color32;
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
    status
        .is_ok()
        .then(|| Color32::from_rgb(value as u8, (value >> 8) as u8, (value >> 16) as u8))
}

/// Linear blend from `a` to `b`. Alpha comes from `a`.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

pub fn with_alpha(c: Color32, a: u8) -> Color32 {
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
    /// The one tint over the whole window's glass, so every part of the shell reads as one surface.
    pub shell: Color32,
    /// The sheet the folder sits on.
    pub sheet: Color32,
    /// Pills: path bar, search, cards, hovered rows.
    pub control: Color32,
    pub control_hover: Color32,
    /// The active tab.
    pub control_active: Color32,
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
                muted: tint(110, 0.08),
                warn: Color32::from_rgb(160, 96, 0),
                row_hover: Color32::from_black_alpha(14),
                row_selected: with_alpha(accent, 48),
                hairline: Color32::from_black_alpha(16),
                well: Color32::from_black_alpha(12),
                track: tint(188, 0.06),
                danger: Color32::from_rgb(196, 43, 28),
                danger_text: Color32::from_rgb(150, 24, 28),
                card: with_alpha(tint(255, 0.02), 200),
                shadow: Color32::from_black_alpha(20),
                sidebar_glass: with_alpha(tint(230, 0.05), 170),
                sidebar_solid: tint(230, 0.05),
                page_glass: with_alpha(tint(248, 0.03), 120),
                shell: with_alpha(tint(238, 0.06), 150),
                sheet: with_alpha(tint(255, 0.01), 236),
                control: with_alpha(tint(255, 0.02), 150),
                control_hover: with_alpha(tint(255, 0.03), 210),
                control_active: with_alpha(tint(255, 0.0), 245),
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
                muted: tint(150, 0.08),
                warn: Color32::from_rgb(230, 186, 120),
                row_hover: Color32::from_rgba_unmultiplied(255, 255, 255, 14),
                row_selected: with_alpha(accent, 64),
                hairline: Color32::from_rgba_unmultiplied(255, 255, 255, 16),
                well: Color32::from_rgba_unmultiplied(255, 255, 255, 12),
                track: tint(66, 0.1),
                danger: Color32::from_rgb(196, 43, 28),
                danger_text: Color32::from_rgb(255, 160, 164),
                card: with_alpha(tint(34, 0.08), 214),
                shadow: Color32::from_black_alpha(60),
                sidebar_glass: with_alpha(tint(10, 0.06), 178),
                sidebar_solid: tint(12, 0.06),
                page_glass: with_alpha(tint(16, 0.06), 140),
                shell: with_alpha(tint(10, 0.06), 150),
                sheet: with_alpha(tint(30, 0.06), 240),
                // Deep tinted fills, not translucent white: over glass, white lifts toward grey.
                control: with_alpha(tint(34, 0.07), 200),
                control_hover: with_alpha(tint(44, 0.09), 215),
                control_active: with_alpha(tint(44, 0.08), 240),
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
        style::apply(ctx);
    }
    changed
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
