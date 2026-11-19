//! Colors, spacing, and small widgets, ported from Blurman.
//!
//! Instead of a fixed rose, everything is derived from one accent color the user picks: the
//! Windows accent, a preset, or a custom color. Neutrals carry a faint tint of the accent, the
//! way Blurman's plum shell carries its rose. Dark and light follow the Windows app mode.

pub mod env;
pub mod shape;
pub mod window_style;
mod icons;
mod style;

pub use icons::{glyph, icon, icon_font, install_fonts, light_font, semibold_font};
pub(crate) use icons::mark_text_font_installed;
pub use style::{apply, checkbox, contrast_outline, danger_button, dialog, focus_ring, group_heading, muted, primary_button, secondary_button, setting_row, speak, speak_selected, trailing};

use egui::Color32;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Accent presets, each strong enough to carry its text color on dark and light.
/// Saved by index, so a new one only ever goes at the end.
pub const PRESETS: [(&str, [u8; 3]); 10] = [
    ("Rose", [196, 54, 78]),
    ("Coral", [214, 82, 52]),
    ("Amber", [204, 132, 0]),
    ("Green", [28, 138, 76]),
    ("Teal", [0, 128, 128]),
    ("Blue", [0, 103, 192]),
    ("Indigo", [84, 82, 214]),
    ("Violet", [136, 64, 200]),
    ("Slate", [96, 108, 128]),
    // Glasshouse's own blue (#2b59c3); on dark it shows as its soft #8fb4ff-like tone.
    ("Glasshouse", [43, 89, 195]),
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

/// WCAG contrast ratio between two colors, 1 to 21.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Least contrast an accent fill needs against the surface it sits on.
pub const FILL_CONTRAST: f32 = 1.6;
/// Least contrast accent-coloured text or a glyph needs.
pub const TEXT_CONTRAST: f32 = 3.0;

/// `color` when it stands out from `surface` by at least `min`; otherwise `fallback` (the text
/// colour), so a black accent on dark or a white one on light never makes a state vanish.
pub fn legible(color: Color32, surface: Color32, fallback: Color32, min: f32) -> Color32 {
    if contrast(color, surface) >= min {
        color
    } else {
        fallback
    }
}

/// Every color that differs between dark and light, or depends on the accent.
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub struct Colors {
    pub light: bool,
    /// Windows' high contrast theme: Windows' own colours, every island and control outlined in
    /// the text colour, no tints or washes; a selected row is a highlight outline.
    pub high_contrast: bool,
    /// The accent as picked. Neutrals are tinted with it and the app icons are drawn from it.
    pub accent: Color32,
    /// The accent where it fills something or marks a state (selection, toggles, primary buttons,
    /// markers): the accent itself, or the text colour when it would hardly show on the page.
    pub accent_fill: Color32,
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
    /// The top bar's tint over its glass: dark or light, following the glass amount.
    pub bar: Color32,
    /// The top bar's 1 px line along its lower edge.
    pub bar_edge: Color32,
    /// Hover and pressed highlights on bar items: the text colour, faint.
    pub hover_soft: Color32,
    pub press_soft: Color32,
    /// A thin divider between groups of bar items.
    pub separator: Color32,
    /// The fill of a field, a quiet button, a chosen sidebar row or tab: the text colour, faint.
    pub field: Color32,
    /// The 1 px light along the top edge of a raised surface (a menu, a dialog).
    pub rim: Color32,
    /// The one main button of a view: near-black on light, near-white on dark (not the accent).
    pub primary: Color32,
    /// Text on [`Colors::primary`].
    pub on_primary: Color32,
}

/// The Glasshouse neutrals, untinted: the page and its text in light and dark.
pub const LIGHT_PAGE: Color32 = Color32::from_rgb(0xf4, 0xf4, 0xf1);
pub const LIGHT_TEXT: Color32 = Color32::from_rgb(0x1b, 0x1c, 0x1f);
pub const DARK_PAGE: Color32 = Color32::from_rgb(0x07, 0x08, 0x0a);
pub const DARK_TEXT: Color32 = Color32::from_rgb(0xf4, 0xf5, 0xf7);

/// The accent a dark page shows: the picked one lifted toward white, the soft blue of
/// Glasshouse's dark accent (#8fb4ff) for its own blue, carrying dark text.
pub fn dark_accent(accent: Color32) -> Color32 {
    mix(accent, Color32::WHITE, 0.45)
}

/// White or near-black, whichever stands out more on `fill`.
fn ink_on(fill: Color32) -> Color32 {
    let dark = Color32::from_rgb(7, 18, 43);
    if contrast(Color32::WHITE, fill) >= contrast(dark, fill) {
        Color32::WHITE
    } else {
        dark
    }
}

impl Colors {
    /// Glasshouse: a warm off-white or near-black page, hairlines instead of shadows, faint text
    /// fills for fields and chosen rows, a neutral main button, and the picked accent for what is
    /// on or chosen (lifted to a soft tone on dark).
    pub fn new(light: bool, accent: Color32) -> Self {
        // How much of the window's acrylic shows through: 0 solid, 1 nearly clear.
        let g = glass();
        let see =
            |solid: u8, clear: u8| (solid as f32 + (clear as f32 - solid as f32) * g).round() as u8;
        let (bg, text, text_base) = if light {
            (LIGHT_PAGE, LIGHT_TEXT, mix(accent, Color32::BLACK, 0.25))
        } else {
            (DARK_PAGE, DARK_TEXT, mix(accent, Color32::WHITE, 0.4))
        };
        let shown = if light { accent } else { dark_accent(accent) };
        let accent_fill = legible(shown, bg, text, FILL_CONTRAST);
        // A selected row is a wash of the accent under ordinary text: of the text colour instead
        // where the accent fell back, or is so light (a yellow on dark) that light text on its
        // wash would fade.
        let fell_back = accent_fill != shown || (!light && luminance(accent_fill) > 0.62);
        let accent_text = legible(text_base, bg, text, TEXT_CONTRAST);
        let on_accent = ink_on(accent_fill);
        // Everything below is painted by egui on a see-through window, where a faint light colour
        // shows far lighter than its alpha says (`shows_as`); dark ones barely change. So every
        // translucent fill goes through it: a 5 % white card stays a faint wash, not a grey slab.
        use crate::glass::islands::shows_as;
        let faint = |a: u8| shows_as(with_alpha(text, a));
        if light {
            Self {
                light,
                high_contrast: false,
                accent,
                accent_fill,
                on_accent,
                accent_text,
                bg,
                popup: Color32::WHITE,
                text,
                // Small grey text over a see-through window renders lighter than its colour.
                muted: Color32::from_rgb(0x55, 0x57, 0x5c),
                warn: Color32::from_rgb(0x7a, 0x4f, 0x00),
                row_hover: faint(16),
                row_selected: if fell_back { faint(24) } else { shows_as(with_alpha(accent_fill, 46)) },
                hairline: Color32::from_black_alpha(26),
                well: faint(14),
                track: Color32::from_rgb(0xd9, 0xd9, 0xd4),
                danger: Color32::from_rgb(0xb3, 0x26, 0x1e),
                danger_text: Color32::from_rgb(0xb3, 0x26, 0x1e),
                card: shows_as(Color32::from_white_alpha(160)),
                shadow: Color32::from_black_alpha(20),
                sidebar_glass: with_alpha(LIGHT_PAGE, 150),
                sidebar_solid: LIGHT_PAGE,
                page_glass: shows_as(Color32::from_white_alpha(90)),
                shell: shell(true, accent, g),
                // Translucent, so the window's acrylic shows through the sheet as well.
                sheet: with_alpha(Color32::WHITE, see(240, 60)),
                control: faint(18),
                control_hover: faint(30),
                control_active: shows_as(Color32::from_white_alpha(235)),
                bar: with_alpha(Color32::from_rgb(250, 250, 248), bar_alpha(0.72, 0.62, g)),
                bar_edge: Color32::from_black_alpha(22),
                hover_soft: faint(20),
                press_soft: faint(32),
                separator: Color32::from_black_alpha(26),
                field: faint(18),
                rim: Color32::from_white_alpha(180),
                primary: LIGHT_TEXT,
                on_primary: Color32::WHITE,
            }
        } else {
            Self {
                light,
                high_contrast: false,
                accent,
                accent_fill,
                on_accent,
                accent_text,
                bg,
                popup: Color32::from_rgb(0x14, 0x17, 0x1c),
                text,
                muted: Color32::from_rgb(0xa4, 0xa8, 0xb0),
                warn: Color32::from_rgb(0xf5, 0xc2, 0x6b),
                row_hover: faint(16),
                row_selected: if fell_back { faint(30) } else { shows_as(with_alpha(accent_fill, 58)) },
                hairline: shows_as(Color32::from_white_alpha(23)),
                well: faint(14),
                track: Color32::from_rgb(0x2a, 0x2e, 0x36),
                danger: Color32::from_rgb(0xd9, 0x47, 0x3d),
                // Glasshouse's soft red: it has to read as danger on dark glass.
                danger_text: Color32::from_rgb(0xff, 0x8f, 0x85),
                card: faint(13),
                shadow: Color32::from_black_alpha(60),
                sidebar_glass: with_alpha(DARK_PAGE, 160),
                sidebar_solid: DARK_PAGE,
                page_glass: with_alpha(DARK_PAGE, 90),
                shell: shell(false, accent, g),
                // Translucent, so the window's acrylic shows through the sheet as well.
                sheet: with_alpha(Color32::from_rgb(0x10, 0x12, 0x16), see(240, 40)),
                // Faint text fills read as one material over glass at every glass amount.
                control: faint(23),
                control_hover: faint(36),
                control_active: faint(44),
                bar: with_alpha(Color32::from_rgb(0x14, 0x17, 0x1c), bar_alpha(0.66, 0.54, g)),
                bar_edge: shows_as(Color32::from_white_alpha(18)),
                hover_soft: faint(20),
                press_soft: faint(34),
                separator: shows_as(Color32::from_white_alpha(26)),
                field: faint(23),
                rim: Color32::from_white_alpha(41),
                primary: DARK_TEXT,
                on_primary: DARK_PAGE,
            }
        }
    }
}

impl Colors {
    /// Windows' high contrast theme: its own colours throughout (window, text, highlight, grey
    /// text, hyperlink), no tints or washes. Highlight only ever pairs with highlight text.
    pub fn high_contrast() -> Self {
        use windows::Win32::Graphics::Gdi::{GetSysColor, COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_HOTLIGHT, COLOR_WINDOW, COLOR_WINDOWTEXT};
        let sys = |i| {
            // SAFETY: a plain read of a system colour (0x00BBGGRR).
            let c = unsafe { GetSysColor(i) };
            Color32::from_rgb((c & 0xff) as u8, (c >> 8 & 0xff) as u8, (c >> 16 & 0xff) as u8)
        };
        let (window, text, highlight, on_highlight, grey, link) =
            (sys(COLOR_WINDOW), sys(COLOR_WINDOWTEXT), sys(COLOR_HIGHLIGHT), sys(COLOR_HIGHLIGHTTEXT), sys(COLOR_GRAYTEXT), sys(COLOR_HOTLIGHT));
        let none = Color32::TRANSPARENT;
        Self {
            light: luminance(window) > 0.5,
            high_contrast: true,
            accent: highlight,
            accent_fill: highlight,
            on_accent: on_highlight,
            accent_text: link,
            bg: window,
            popup: window,
            text,
            muted: grey,
            warn: text,
            // No hover wash: ordinary text on a highlight wash is not a pair Windows allows.
            row_hover: none,
            // Painted as an outline by the rows that know (`row_selected` is the outline colour).
            row_selected: highlight,
            hairline: text,
            well: window,
            track: grey,
            danger: highlight,
            danger_text: text,
            card: window,
            shadow: none,
            sidebar_glass: window,
            sidebar_solid: window,
            page_glass: window,
            shell: window,
            sheet: window,
            control: window,
            // No hover wash: rows and buttons put ordinary text on it, and highlight only pairs
            // with highlight text. Hovered things are outlined instead where they can be.
            control_hover: window,
            control_active: highlight,
            bar: window,
            bar_edge: text,
            hover_soft: none,
            press_soft: none,
            separator: text,
            field: window,
            rim: none,
            primary: highlight,
            on_primary: on_highlight,
        }
    }
}

/// The one tint over a glass window's acrylic (`Colors::shell`), for a mode, accent and glass
/// amount. The floating glass (`glass::Style`) lays the same tint over its backdrop, so the two
/// materials hide the same amount at every glass setting.
pub fn shell(light: bool, accent: Color32, glass: f32) -> Color32 {
    let page = if light { LIGHT_PAGE } else { DARK_PAGE };
    let alpha = (225.0 + (110.0 - 225.0) * glass.clamp(0.0, 1.0)).round() as u8;
    with_alpha(mix(page, accent, NEUTRAL_TINT), alpha)
}

/// How much of the accent the glass tints carry: barely any, Glasshouse's neutrals are untinted.
pub const NEUTRAL_TINT: f32 = 0.02;

/// The even tint over a Frosted window's one acrylic backdrop, between its islands: the page
/// colour ([`LIGHT_PAGE`], [`DARK_PAGE`]) with a trace of the accent. Its alpha follows the glass
/// amount the way [`shell`] does, and at the default amount it is the look's 140 (dark) / 120
/// (light).
pub fn frosted_tint(light: bool, accent: Color32, glass: f32) -> Color32 {
    let shell_alpha = 225.0 + (110.0 - 225.0) * glass.clamp(0.0, 1.0);
    let at_default = 225.0 + (110.0 - 225.0) * DEFAULT_GLASS;
    let (page, alpha) = if light { (LIGHT_PAGE, 120.0) } else { (DARK_PAGE, 140.0) };
    let alpha = (alpha * shell_alpha / at_default).round().clamp(0.0, 255.0) as u8;
    with_alpha(mix(page, accent, NEUTRAL_TINT), alpha)
}

/// Opacity (0..=255) of the top bar's tint: `solid` with no glass, `clear` with all of it.
fn bar_alpha(solid: f32, clear: f32, glass: f32) -> u8 {
    ((solid + (clear - solid) * glass) * 255.0).round() as u8
}

static LIGHT: AtomicBool = AtomicBool::new(false);
static ACCENT: AtomicU32 = AtomicU32::new(0xC4364E);
/// The glass amount, as `f32` bits.
static GLASS: AtomicU32 = AtomicU32::new(DEFAULT_GLASS.to_bits());

/// The glass amount a fresh install starts with.
pub const DEFAULT_GLASS: f32 = 0.72;

/// How much of the backdrop shows through, from 0 (solid) to 1 (nearly clear).
pub fn glass() -> f32 {
    f32::from_bits(GLASS.load(Ordering::Relaxed))
}

/// Set how much of the acrylic shows through the window, from 0 (solid) to 1 (nearly clear).
pub fn set_glass(ctx: &egui::Context, amount: f32) {
    GLASS.store(amount.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    ctx.request_repaint();
}

fn accent() -> Color32 {
    let v = ACCENT.load(Ordering::Relaxed);
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// The colors for the mode and accent the window has now (Windows' own in high contrast).
pub fn colors() -> Colors {
    if env::high_contrast() {
        return Colors::high_contrast();
    }
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
    fn frosted_tint_is_the_look_at_the_default_amount_and_follows_the_dial() {
        let accent = Color32::from_rgb(0, 103, 192);
        assert_eq!(frosted_tint(false, accent, DEFAULT_GLASS).a(), 140);
        assert_eq!(frosted_tint(true, accent, DEFAULT_GLASS).a(), 120);
        // Clearer glass, a fainter tint; more solid, a stronger one.
        assert!(frosted_tint(false, accent, 1.0).a() < 140 && frosted_tint(false, accent, 0.0).a() > 140);
        let [r, g, b, _] = frosted_tint(false, accent, 0.5).to_srgba_unmultiplied();
        let page = mix(DARK_PAGE, accent, NEUTRAL_TINT);
        assert!(r.abs_diff(page.r()) <= 1 && g.abs_diff(page.g()) <= 1 && b.abs_diff(page.b()) <= 1);
    }

    #[test]
    fn text_on_accent_contrasts() {
        for (_, c) in PRESETS {
            let colors = Colors::new(false, rgb(c));
            let (a, on) = (colors.accent_fill, colors.on_accent);
            let (la, lo) = (luminance(a), luminance(on));
            let ratio = (la.max(lo) + 0.05) / (la.min(lo) + 0.05);
            assert!(ratio >= 3.0, "{c:?} has contrast {ratio}");
        }
    }

    #[test]
    fn bright_accent_gets_dark_text() {
        let on = Colors::new(false, Color32::from_rgb(255, 220, 80)).on_accent;
        assert!(luminance(on) < 0.1);
    }

    #[test]
    fn black_accent_on_dark_and_white_on_light_stay_legible() {
        // White on light falls back to the text colour; black on dark is lifted to a grey that
        // shows (see `dark_accent`), so it needs no fallback.
        assert_eq!(Colors::new(true, Color32::WHITE).accent_fill, LIGHT_TEXT);
        for (light, accent) in [(false, Color32::BLACK), (true, Color32::WHITE)] {
            let c = Colors::new(light, accent);
            assert!(contrast(c.accent_fill, c.bg) >= FILL_CONTRAST);
            assert!(contrast(c.accent_text, c.bg) >= TEXT_CONTRAST);
            assert!(contrast(c.on_accent, c.accent_fill) >= 3.0);
            assert_eq!(c.accent, accent, "the picked accent stays for tints and icons");
            assert!(c.row_selected.a() > 0);
        }
    }

    /// Every accent, the presets and the extremes, in both modes: text and grey text read on the
    /// page, the popup and a card; the accent fill stands out; its ink reads on it; the main
    /// button's text reads on it.
    #[test]
    fn every_surface_reads_in_both_modes() {
        let mut accents: Vec<Color32> = PRESETS.iter().map(|(_, c)| rgb(*c)).collect();
        accents.extend([Color32::BLACK, Color32::WHITE, Color32::from_rgb(255, 220, 80), Color32::from_gray(128)]);
        for light in [false, true] {
            for &a in &accents {
                let c = Colors::new(light, a);
                // A card is translucent: judge it over the page it sits on.
                let card = over(c.card, c.bg);
                for surface in [c.bg, c.popup, card] {
                    assert!(contrast(c.text, surface) >= 7.0, "text {a:?} light={light}");
                    assert!(contrast(c.muted, surface) >= 4.5, "muted {a:?} light={light}");
                }
                assert!(contrast(c.accent_fill, c.bg) >= FILL_CONTRAST, "fill {a:?} light={light}");
                assert!(contrast(c.accent_text, c.bg) >= TEXT_CONTRAST, "accent text {a:?} light={light}");
                // A mid accent (a green, a grey) cannot give either ink 4.5:1; 3:1 is the floor, for the
                // semibold labels and glyphs that go on it.
                assert!(contrast(c.on_accent, c.accent_fill) >= 3.0, "on accent {a:?} light={light}");
                assert!(contrast(c.on_primary, c.primary) >= 7.0, "primary light={light}");
                assert!(contrast(c.danger_text, c.bg) >= 4.5, "danger light={light}");
            }
        }
    }

    /// `top` laid over the opaque `under`.
    fn over(top: Color32, under: Color32) -> Color32 {
        let a = top.a() as f32 / 255.0;
        let [r, g, b, _] = top.to_srgba_unmultiplied();
        let l = |t: u8, u: u8| (t as f32 * a + u as f32 * (1.0 - a)).round() as u8;
        Color32::from_rgb(l(r, under.r()), l(g, under.g()), l(b, under.b()))
    }

    #[test]
    fn accents_with_contrast_are_unchanged() {
        for light in [false, true] {
            for (_, p) in PRESETS {
                let a = rgb(p);
                let c = Colors::new(light, a);
                let shown = if light { a } else { dark_accent(a) };
                assert_eq!(c.accent_fill, shown, "{p:?} light={light}");
                let base = if light {
                    mix(a, Color32::BLACK, 0.25)
                } else {
                    mix(a, Color32::WHITE, 0.4)
                };
                assert_eq!(c.accent_text, base, "{p:?} light={light}");
            }
        }
    }

    #[test]
    fn mid_grey_stands_out_either_way() {
        let g = Color32::from_gray(128);
        assert_eq!(Colors::new(true, g).accent_fill, g);
        assert_eq!(Colors::new(false, g).accent_fill, dark_accent(g));
    }
}
