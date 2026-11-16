//! What Windows says about how the user needs to see things: high contrast, a Remote Desktop
//! session, the text size, light or dark. Each has a preview override, so a look can be checked
//! without changing the user's Windows settings:
//!
//! - `SHELLMAN_HIGH_CONTRAST`: as if high contrast were on;
//! - `SHELLMAN_SOLID`: a plain solid window (as with "No transparency");
//! - `SHELLMAN_NO_TRANSPARENCY`: as if Windows' transparency effects were off;
//! - `SHELLMAN_REMOTE`: as if over Remote Desktop (islands without blur);
//! - `SHELLMAN_TEXT_SCALE=150`: Windows' text size, 100 to 225;
//! - `SHELLMAN_THEME=light|dark` (`SHELLMAN_FORCE_LIGHT` still means light).
//!
//! High contrast is read once and cached ([`refresh`] reads it again: `style::apply` calls it),
//! so `colors()` stays cheap.

use std::sync::atomic::{AtomicBool, Ordering};

static HIGH_CONTRAST: AtomicBool = AtomicBool::new(false);
static TRANSPARENCY_OFF: AtomicBool = AtomicBool::new(false);
static READ: AtomicBool = AtomicBool::new(false);

/// `HKCU\...\Themes\Personalize` `EnableTransparency` = 0 (Settings › Personalization › Colors ›
/// Transparency effects off).
fn read_transparency_off() -> bool {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    if std::env::var_os("SHELLMAN_NO_TRANSPARENCY").is_some() {
        return true;
    }
    let mut v: u32 = 1;
    let mut size = 4u32;
    // SAFETY: a DWORD read into a 4-byte buffer whose size is passed.
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("EnableTransparency"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    read.is_ok() && v == 0
}

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
    TRANSPARENCY_OFF.store(read_transparency_off(), Ordering::Relaxed);
    READ.store(true, Ordering::Relaxed);
}

/// Windows' high contrast theme is on: Windows' own colours, solid windows, outlines.
pub fn high_contrast() -> bool {
    if !READ.load(Ordering::Relaxed) {
        refresh();
    }
    HIGH_CONTRAST.load(Ordering::Relaxed)
}

/// Windows' transparency effects are off: app windows are solid (see `window_style`). Cached
/// like [`high_contrast`]; the glass panel reads it again when Windows says it changed.
pub fn transparency_off() -> bool {
    if !READ.load(Ordering::Relaxed) {
        refresh();
    }
    TRANSPARENCY_OFF.load(Ordering::Relaxed)
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

/// No glass anywhere, shell surfaces included: high contrast, or `SHELLMAN_SOLID`. Not Remote
/// Desktop: there the islands stay, see [`no_glass`]. App windows go further and follow the
/// user's Window choice and transparency effects: see `window_style::window_look`. (On shell
/// surfaces transparency effects off is handled by the glass itself: solid fill, shape and
/// shadow kept.)
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

/// A window's size and minimum size (points) grown by Windows' text size `z`, as far as 90% of
/// `screen` allows (the minimum too, so a large text size never makes a window that cannot fit).
pub fn scaled_window(size: [f32; 2], min: [f32; 2], z: f32, screen: Option<[f32; 2]>) -> ([f32; 2], [f32; 2]) {
    let room = screen.map_or([f32::INFINITY; 2], |s| [s[0] * 0.9, s[1] * 0.9]);
    let fit = |v: [f32; 2]| [(v[0] * z).min(room[0]).max(v[0].min(room[0])), (v[1] * z).min(room[1]).max(v[1].min(room[1]))];
    let min = fit(min);
    let size = fit(size);
    (size, [min[0].min(size[0]), min[1].min(size[1])])
}

/// Make the window at least `min` points (from [`scaled_window`]): a window size restored from
/// an earlier run, at a smaller text size, is grown. True once the window's size is known (call
/// it each frame until then).
pub fn grow_to(ctx: &egui::Context, min: [f32; 2]) -> bool {
    let Some(inner) = ctx.input(|i| i.viewport().inner_rect) else {
        return false;
    };
    // The window's size is read in its own points, but a size asked for is taken in egui's
    // points, which the text size has zoomed.
    let want = egui::vec2(inner.width().max(min[0]), inner.height().max(min[1]));
    if want != inner.size() {
        let zoom = ctx.zoom_factor().max(0.1);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(want / zoom));
    }
    true
}

/// The primary screen's work area in points (at the system DPI), for [`scaled_window`].
pub fn work_area_points() -> Option<[f32; 2]> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::HiDpi::GetDpiForSystem;
    use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};
    let mut r = RECT::default();
    // SAFETY: SPI_GETWORKAREA fills a RECT.
    unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut r as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) }.ok()?;
    // SAFETY: no preconditions.
    let scale = unsafe { GetDpiForSystem() }.max(96) as f32 / 96.0;
    Some([(r.right - r.left) as f32 / scale, (r.bottom - r.top) as f32 / scale])
}

/// Light, dark or Windows' mode: the user's Theme choice (Settings › Appearance), with
/// `SHELLMAN_THEME` / `SHELLMAN_FORCE_LIGHT` overriding it for a preview.
pub fn preference() -> egui::ThemePreference {
    if std::env::var_os("SHELLMAN_FORCE_LIGHT").is_some() {
        return egui::ThemePreference::Light;
    }
    match std::env::var("SHELLMAN_THEME").as_deref() {
        Ok("light") => egui::ThemePreference::Light,
        Ok("dark") => egui::ThemePreference::Dark,
        _ => super::window_style::theme_choice().preference(),
    }
}

#[cfg(test)]
mod tests {
    use super::scaled_window;

    #[test]
    fn a_window_grows_with_the_text_size_within_the_screen() {
        let (size, min) = scaled_window([960.0, 720.0], [700.0, 540.0], 1.5, Some([2560.0, 1400.0]));
        assert_eq!(size, [1440.0, 1080.0]);
        assert_eq!(min, [1050.0, 810.0]);
        // A small screen: 90% of it, the minimum no larger than the size.
        let (size, min) = scaled_window([960.0, 720.0], [700.0, 540.0], 2.25, Some([1280.0, 680.0]));
        assert_eq!(size, [1152.0, 612.0]);
        assert!(min[0] <= size[0] && min[1] <= size[1]);
        // At 100% nothing changes.
        assert_eq!(scaled_window([960.0, 720.0], [700.0, 540.0], 1.0, None), ([960.0, 720.0], [700.0, 540.0]));
    }
}
