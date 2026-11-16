//! What Windows says about how the user needs to see things: high contrast, a Remote Desktop
//! session, the text size, light or dark. Each has a preview override, so a look can be checked
//! without changing the user's Windows settings:
//!
//! - `SHELLMAN_HIGH_CONTRAST`: as if high contrast were on;
//! - `SHELLMAN_SOLID`: a plain solid window (as with "No transparency");
//! - `SHELLMAN_REMOTE`: as if over Remote Desktop (islands without blur);
//! - `SHELLMAN_TEXT_SCALE=150`: Windows' text size, 100 to 225;
//! - `SHELLMAN_THEME=light|dark` (`SHELLMAN_FORCE_LIGHT` still means light).
//!
//! High contrast is read once and cached ([`refresh`] reads it again: `style::apply` calls it),
//! so `colors()` stays cheap.

use std::sync::atomic::{AtomicBool, Ordering};

static HIGH_CONTRAST: AtomicBool = AtomicBool::new(false);
static READ: AtomicBool = AtomicBool::new(false);

fn read_high_contrast() -> bool {
    use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
    use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};
    if std::env::var_os("SHELLMAN_HIGH_CONTRAST").is_some() {
        return true;
    }
    let mut hc = HIGHCONTRASTW { cbSize: size_of::<HIGHCONTRASTW>() as u32, ..Default::default() };
    // SAFETY: a correctly sized HIGHCONTRASTW for SPI_GETHIGHCONTRAST to fill.
    let ok = unsafe { SystemParametersInfoW(SPI_GETHIGHCONTRAST, hc.cbSize, Some(&mut hc as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) };
    ok.is_ok() && hc.dwFlags.contains(HCF_HIGHCONTRASTON)
}

/// Read the settings again (after `WM_SETTINGCHANGE`, or when restyling).
pub fn refresh() {
    HIGH_CONTRAST.store(read_high_contrast(), Ordering::Relaxed);
    READ.store(true, Ordering::Relaxed);
}

/// Windows' high contrast theme is on: Windows' own colours, solid windows, outlines.
pub fn high_contrast() -> bool {
    if !READ.load(Ordering::Relaxed) {
        refresh();
    }
    HIGH_CONTRAST.load(Ordering::Relaxed)
}

/// A Remote Desktop session (Windows composes no glass there; a see-through window shows dark
/// borders). It changes as one connects or the console takes over.
pub fn remote_session() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_REMOTESESSION};
    if std::env::var_os("SHELLMAN_REMOTE").is_some() {
        return true;
    }
    // SAFETY: no preconditions.
    unsafe { GetSystemMetrics(SM_REMOTESESSION) != 0 }
}

/// A plain solid window: high contrast, or `SHELLMAN_SOLID`. (Transparency effects off is
/// handled by the glass itself: solid fill, shape and shadow kept.) Not Remote Desktop: there the
/// islands stay, see [`no_glass`].
pub fn solid() -> bool {
    std::env::var_os("SHELLMAN_SOLID").is_some() || high_contrast()
}

/// No blur behind the islands: a solid window, or a Remote Desktop session. Over Remote Desktop
/// the window keeps the island look: islands painted nearly opaque with their hairline, the
/// window invisible around them (no Windows corners, border or shadow, which is what showed as
/// borders there).
pub fn no_glass() -> bool {
    solid() || remote_session()
}

/// Windows' "Text size" (Settings > Accessibility > Text size) as a factor, 1.0 to 2.25.
pub fn text_scale() -> f32 {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    if let Some(v) = std::env::var("SHELLMAN_TEXT_SCALE").ok().and_then(|v| v.parse::<u32>().ok()) {
        return v.clamp(100, 225) as f32 / 100.0;
    }
    let mut v: u32 = 100;
    let mut size = 4u32;
    // SAFETY: a DWORD read into a 4-byte buffer whose size is passed.
    let _ = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Accessibility"),
            w!("TextScaleFactor"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    v.clamp(100, 225) as f32 / 100.0
}

/// Make everything larger by Windows' text size. Returns the factor, for a window to grow its
/// size and minimum by.
pub fn follow_text_scale(ctx: &egui::Context) -> f32 {
    let z = text_scale();
    if (ctx.zoom_factor() - z).abs() > 0.01 {
        ctx.set_zoom_factor(z);
    }
    z
}

/// Light, dark or Windows' choice, with `SHELLMAN_THEME` / `SHELLMAN_FORCE_LIGHT` overriding.
pub fn preference() -> egui::ThemePreference {
    if std::env::var_os("SHELLMAN_FORCE_LIGHT").is_some() {
        return egui::ThemePreference::Light;
    }
    match std::env::var("SHELLMAN_THEME").as_deref() {
        Ok("light") => egui::ThemePreference::Light,
        Ok("dark") => egui::ThemePreference::Dark,
        _ => egui::ThemePreference::System,
    }
}
