//! The user's Window and Theme choices (Settings › Appearance), and what a window actually
//! looks like once Windows has had its say.
//!
//! - **Window**: Frosted (the default: the window is one sheet of real glass, see
//!   `crate::sheet`) or No transparency (a solid window). Before app windows became one sheet
//!   there was a third, Invisible (each island on its own glass, the desktop between them); a
//!   saved "invisible" now reads as Frosted, which draws the same way.
//! - **Theme**: Windows' light or dark mode, or always light, or always dark.
//!
//! The glass amount (`theme::glass`) is a separate dial: how much of the glass shows through its
//! tint. It has nothing to do in a solid window.
//!
//! The choices are saved in `appearance.json` and set here by `Appearance::apply`, so every app
//! that applies the look follows them. App windows read [`window_look`]; shell surfaces (bars,
//! dock, popups) are not windows and keep their own glass.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};

/// How an app window is drawn, as the user chose it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowStyle {
    /// The window on real glass. Saved as "frosted"; "invisible", the old default, reads as this.
    #[default]
    #[serde(alias = "invisible")]
    Frosted,
    /// A solid window: islands on a page in the theme's colour.
    #[serde(rename = "solid")]
    NoTransparency,
}

impl WindowStyle {
    pub const ALL: [WindowStyle; 2] = [WindowStyle::Frosted, WindowStyle::NoTransparency];

    pub fn label(self) -> &'static str {
        match self {
            WindowStyle::Frosted => "Frosted",
            WindowStyle::NoTransparency => "No transparency",
        }
    }
}

/// Light or dark, as the user chose it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeChoice {
    /// Windows' app mode, and changes with it.
    #[default]
    Windows,
    Light,
    Dark,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 3] = [ThemeChoice::Windows, ThemeChoice::Light, ThemeChoice::Dark];

    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::Windows => "Windows",
            ThemeChoice::Light => "Light",
            ThemeChoice::Dark => "Dark",
        }
    }

    pub fn preference(self) -> egui::ThemePreference {
        match self {
            ThemeChoice::Windows => egui::ThemePreference::System,
            ThemeChoice::Light => egui::ThemePreference::Light,
            ThemeChoice::Dark => egui::ThemePreference::Dark,
        }
    }
}

/// What an app window actually looks like.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowLook {
    /// Frosted: the window's islands (an app window's one sheet) on real glass.
    Glass,
    /// Over Remote Desktop: the islands painted nearly opaque, nothing around them, no blur.
    Bare,
    /// A plain solid window.
    Solid,
}

impl WindowLook {
    /// Whether each island is a piece of composed glass (the app paints none of them itself).
    pub fn islands_on_glass(self) -> bool {
        self == WindowLook::Glass
    }

    /// Whether the window has a composed backdrop at all.
    pub fn composed(self) -> bool {
        self == WindowLook::Glass
    }
}

/// Why the choice was not followed as made.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Overruled {
    /// High contrast is on: always a solid window.
    HighContrast,
    /// Windows' transparency effects are off: always a solid window.
    TransparencyOff,
    /// A Remote Desktop session: islands without blur.
    Remote,
}

impl Overruled {
    /// The setting's detail line.
    pub fn explain(self) -> &'static str {
        match self {
            Overruled::HighContrast => "No transparency while high contrast is on.",
            Overruled::TransparencyOff => "No transparency while Windows' transparency effects are off (Personalization › Colors).",
            Overruled::Remote => "Over Remote Desktop there is no blur: the islands are painted nearly solid.",
        }
    }
}

/// What Windows says, for [`resolve`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Conditions {
    pub high_contrast: bool,
    pub transparency_off: bool,
    pub remote: bool,
    /// `SHELLMAN_SOLID`: a solid window, as if "No transparency" were chosen.
    pub forced_solid: bool,
}

/// The look a window gets for the user's `chosen` style, and what overruled it, if anything.
/// High contrast and transparency effects off force a solid window; over Remote Desktop the
/// glass styles become islands without blur, and a solid window stays solid.
pub fn resolve(chosen: WindowStyle, w: Conditions) -> (WindowLook, Option<Overruled>) {
    if w.high_contrast {
        return (WindowLook::Solid, (chosen != WindowStyle::NoTransparency).then_some(Overruled::HighContrast));
    }
    if w.transparency_off {
        return (WindowLook::Solid, (chosen != WindowStyle::NoTransparency).then_some(Overruled::TransparencyOff));
    }
    let chosen = if w.forced_solid { WindowStyle::NoTransparency } else { chosen };
    match chosen {
        WindowStyle::NoTransparency => (WindowLook::Solid, None),
        _ if w.remote => (WindowLook::Bare, Some(Overruled::Remote)),
        WindowStyle::Frosted => (WindowLook::Glass, None),
    }
}

/// What Settings shows for the Window setting: the chip that is lit, whether the chips can be
/// used (not while Windows forces a solid window), and the detail line under them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shown {
    pub lit: WindowStyle,
    pub enabled: bool,
    pub detail: &'static str,
    /// Whether the glass amount does anything in the window as it is now.
    pub glass_applies: bool,
}

/// [`Shown`] for the user's `chosen` style under `w`.
pub fn shown(chosen: WindowStyle, w: Conditions) -> Shown {
    let (look, overruled) = resolve(chosen, w);
    let forced = matches!(overruled, Some(Overruled::HighContrast | Overruled::TransparencyOff));
    let detail = match overruled {
        Some(why) => why.explain(),
        None if w.forced_solid => "No transparency (SHELLMAN_SOLID is set).",
        None => match chosen {
            WindowStyle::Frosted => "The window is frosted glass: the desktop shows through it, blurred.",
            WindowStyle::NoTransparency => "A solid window: nothing see-through.",
        },
    };
    Shown {
        lit: if forced { WindowStyle::NoTransparency } else { chosen },
        enabled: !forced,
        detail,
        glass_applies: look.composed(),
    }
}

static WINDOW: AtomicU8 = AtomicU8::new(0);
static THEME: AtomicU8 = AtomicU8::new(0);

/// Remember the user's window style (from `Appearance::apply`).
pub fn set_window_style(style: WindowStyle) {
    let v = WindowStyle::ALL.iter().position(|s| *s == style).unwrap_or(0) as u8;
    WINDOW.store(v, Ordering::Relaxed);
}

/// The user's window style, as last applied.
pub fn window_style() -> WindowStyle {
    WindowStyle::ALL.get(WINDOW.load(Ordering::Relaxed) as usize).copied().unwrap_or_default()
}

/// Remember the user's theme (from `Appearance::apply`).
pub fn set_theme_choice(choice: ThemeChoice) {
    let v = ThemeChoice::ALL.iter().position(|s| *s == choice).unwrap_or(0) as u8;
    THEME.store(v, Ordering::Relaxed);
}

/// The user's theme, as last applied.
pub fn theme_choice() -> ThemeChoice {
    ThemeChoice::ALL.get(THEME.load(Ordering::Relaxed) as usize).copied().unwrap_or_default()
}

/// What Windows says now (high contrast and transparency cached; see `env::refresh`).
pub fn conditions() -> Conditions {
    Conditions {
        high_contrast: super::env::high_contrast(),
        transparency_off: super::env::transparency_off(),
        remote: super::env::remote_session(),
        forced_solid: std::env::var_os("SHELLMAN_SOLID").is_some(),
    }
}

/// How app windows look now: the user's style as Windows lets it be.
pub fn window_look() -> WindowLook {
    resolve(window_style(), conditions()).0
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: Conditions = Conditions { high_contrast: false, transparency_off: false, remote: false, forced_solid: false };

    #[test]
    fn each_style_as_chosen_when_windows_allows() {
        assert_eq!(resolve(WindowStyle::Frosted, PLAIN), (WindowLook::Glass, None));
        assert_eq!(resolve(WindowStyle::NoTransparency, PLAIN), (WindowLook::Solid, None));
    }

    #[test]
    fn high_contrast_and_transparency_off_force_solid_and_say_why() {
        {
            let chosen = WindowStyle::Frosted;
            let hc = Conditions { high_contrast: true, remote: true, transparency_off: true, ..PLAIN };
            assert_eq!(resolve(chosen, hc), (WindowLook::Solid, Some(Overruled::HighContrast)));
            let off = Conditions { transparency_off: true, ..PLAIN };
            assert_eq!(resolve(chosen, off), (WindowLook::Solid, Some(Overruled::TransparencyOff)));
        }
        // Already solid: nothing was overruled.
        let hc = Conditions { high_contrast: true, ..PLAIN };
        assert_eq!(resolve(WindowStyle::NoTransparency, hc), (WindowLook::Solid, None));
    }

    #[test]
    fn remote_desktop_keeps_islands_without_blur() {
        let remote = Conditions { remote: true, ..PLAIN };
        assert_eq!(resolve(WindowStyle::Frosted, remote), (WindowLook::Bare, Some(Overruled::Remote)));
        assert_eq!(resolve(WindowStyle::NoTransparency, remote), (WindowLook::Solid, None));
        // The preview override is the "No transparency" choice.
        let forced = Conditions { forced_solid: true, remote: true, ..PLAIN };
        assert_eq!(resolve(WindowStyle::Frosted, forced), (WindowLook::Solid, None));
    }

    #[test]
    fn only_glass_puts_each_island_on_glass() {
        assert!(WindowLook::Glass.islands_on_glass() && WindowLook::Glass.composed());
        assert!(!WindowLook::Bare.composed() && !WindowLook::Solid.composed());
    }

    #[test]
    fn settings_shows_the_forced_choice_and_why() {
        let hc = Conditions { high_contrast: true, ..PLAIN };
        let s = shown(WindowStyle::Frosted, hc);
        assert_eq!((s.lit, s.enabled, s.glass_applies), (WindowStyle::NoTransparency, false, false));
        assert!(s.detail.contains("high contrast"), "{}", s.detail);
        let off = shown(WindowStyle::Frosted, Conditions { transparency_off: true, ..PLAIN });
        assert!(!off.enabled && off.detail.contains("transparency effects"));
        // Over Remote Desktop the choice stays the user's; the line says there is no blur.
        let remote = shown(WindowStyle::Frosted, Conditions { remote: true, ..PLAIN });
        assert_eq!((remote.lit, remote.enabled, remote.glass_applies), (WindowStyle::Frosted, true, false));
        assert!(remote.detail.contains("Remote Desktop"));
        // As chosen: the glass amount applies to glass only.
        assert!(shown(WindowStyle::Frosted, PLAIN).glass_applies);
        assert!(!shown(WindowStyle::NoTransparency, PLAIN).glass_applies);
    }

    #[test]
    fn saved_names_are_short_and_stable() {
        assert_eq!(serde_json::to_string(&WindowStyle::NoTransparency).unwrap(), r#""solid""#);
        assert_eq!(serde_json::to_string(&WindowStyle::Frosted).unwrap(), r#""frosted""#);
        assert_eq!(serde_json::to_string(&ThemeChoice::Dark).unwrap(), r#""dark""#);
        // The old default, Invisible, comes back as Frosted, which now draws the same.
        let back: WindowStyle = serde_json::from_str(r#""invisible""#).unwrap();
        assert_eq!(back, WindowStyle::Frosted);
    }
}
